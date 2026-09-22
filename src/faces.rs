//! The picture an account is known by, read the way a greeter is allowed to.
//!
//! There is a copy of every user's chosen avatar at
//! `/var/lib/AccountsService/icons/<name>`, put there by `accounts-daemon` and
//! left world-readable on purpose — that directory exists so that a login
//! screen, which runs as nobody in particular, can show a face without being
//! given a way into anyone's home. GDM reads it, and so does every other
//! display manager that shows one. It is not a privilege the greeter has; it is
//! a copy the system published.
//!
//! Which is why nothing here goes near `~/.face`. That file is the *source* the
//! daemon copied from, it sits inside a home directory the greeter has no
//! business in — often unreadable anyway, `0700` on a good many distributions —
//! and reaching for it would cross exactly the boundary this project draws
//! around per-user configuration everywhere else. If a machine has an avatar
//! that never reached AccountsService, the answer is that the account has no
//! picture here, and the initial stands in for it.

use std::path::{Path, PathBuf};

/// Where the daemon publishes them.
const PUBLISHED: &str = "/var/lib/AccountsService/icons";

/// The most of one of these files that will ever be read.
///
/// They are 256-pixel portraits and the ones this has met are around a hundred
/// kilobytes. The cap is not about them: it is that a greeter reads this before
/// anyone has authenticated, and every buffer on that side of the login is
/// bounded whether or not the thing filling it is trusted.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The most decoded picture this will hold at once.
///
/// The bound above is on the *file*, and a PNG's file size says nothing about
/// how large the picture inside it is: fifty-seven bytes is enough to write a
/// header claiming 32768 by 32768, and a decoder asked how much room that
/// needs answers four gigabytes. Nothing in the file has to back the claim up,
/// because the claim is in the header and the header is read first — so the
/// allocation happens before there is any image data to contradict it, and a
/// greeter that makes it dies of it in front of a login screen it was in the
/// middle of drawing.
///
/// Four megapixels, which is sixteen times a 512-pixel portrait and more than
/// anything that is ever drawn here: the picture ends up in a circle about a
/// hundred and twenty pixels across. A photograph too large for this keeps its
/// account's initial, which is the same answer as an account with no picture
/// at all.
///
/// It also keeps [`fit`] honest. That function addresses a source pixel as
/// `(y * width + x) * 4` in `u32`, and a picture with more pixels than fit in
/// that arithmetic would wrap it — so the cap that stops the allocation is the
/// same cap that stops the index.
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;

/// And the longest either side of one may be.
///
/// Redundant against the byte cap for any ordinary shape and deliberately kept
/// anyway: a picture one pixel tall and a hundred million wide is a shape no
/// camera produces and every bound should refuse on sight, rather than by
/// arithmetic that happens to come out the right way.
const MAX_SIDE: u32 = 8192;

/// Where `name`'s published avatar is, if there is one.
pub fn published(name: &str) -> Option<PathBuf> {
    published_in(Path::new(PUBLISHED), name)
}

fn published_in(directory: &Path, name: &str) -> Option<PathBuf> {
    // A login name is one path component and never a path. Anything with a
    // separator or a traversal in it is not an account this greeter enumerated,
    // and joining it would leave the directory the system published.
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return None;
    }
    let path = directory.join(name);
    // Opened rather than stat'ed, on the terms every other pre-login read is
    // opened on: this directory belongs to `accounts-daemon` rather than to an
    // account, so the risk is smaller than it is for a published look, but
    // "there is a picture here" is a question that should be answered by the
    // same door the picture is read through. See [`crate::reading`].
    crate::reading::open(&path, crate::reading::Owner::Anyone).map(|_| path)
}

/// Read one and hand back `size` by `size` straight RGBA, ready for a cell of
/// the atlas.
///
/// `None` for anything that is not a PNG this can decode. The daemon does not
/// transcode what it copies, so a machine whose avatar was set from a JPEG has
/// a JPEG here — that account keeps its initial, which is the same answer as
/// having no picture at all and a great deal better than a decoder pulled in to
/// run over a file before anybody has logged in.
pub fn load(path: &Path, size: u32) -> Option<Vec<u8>> {
    let file = crate::reading::open(path, crate::reading::Owner::Anyone)?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    // The decoder's own allowance for the buffers it keeps for itself. It does
    // not cover the one below — that one is this program's — but a header can
    // ask this side for room too, and one budget said out loud is better than
    // two, one of which is a default in somebody else's crate.
    decoder.set_limits(png::Limits {
        bytes: MAX_DECODED_BYTES,
    });
    let mut reader = decoder.read_info().ok()?;

    // Everything that decides how large the allocation below is, checked
    // before it is made. This is the whole point of the function: past this
    // block the picture is one that fits, and before it the only thing known
    // about the picture is what its own header says about itself.
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || pixels * 4 > MAX_DECODED_BYTES as u64
    {
        tracing::debug!(
            ?path,
            width,
            height,
            "ignoring a picture larger than a login screen will decode"
        );
        return None;
    }
    let wanted = reader.output_buffer_size()?;
    if wanted > MAX_DECODED_BYTES {
        tracing::debug!(
            ?path,
            wanted,
            "ignoring a picture that asks for too much room"
        );
        return None;
    }

    let mut raw = vec![0; wanted];
    let info = reader.next_frame(&mut raw).ok()?;
    let rgba = to_rgba(&raw[..info.buffer_size()], info.color_type)?;
    Some(fit(&rgba, info.width, info.height, size))
}

fn to_rgba(raw: &[u8], colour: png::ColorType) -> Option<Vec<u8>> {
    let opaque = |channels: usize, spread: bool| {
        let mut rgba = Vec::with_capacity(raw.len() / channels * 4);
        for pixel in raw.chunks_exact(channels) {
            let (r, g, b) = if spread {
                (pixel[0], pixel[0], pixel[0])
            } else {
                (pixel[0], pixel[1], pixel[2])
            };
            let alpha = if channels % 2 == 0 {
                pixel[channels - 1]
            } else {
                255
            };
            rgba.extend_from_slice(&[r, g, b, alpha]);
        }
        rgba
    };
    match colour {
        png::ColorType::Rgba => Some(raw.to_vec()),
        png::ColorType::Rgb => Some(opaque(3, false)),
        png::ColorType::Grayscale => Some(opaque(1, true)),
        png::ColorType::GrayscaleAlpha => Some(opaque(2, true)),
        // `normalize_to_color8` expands a palette, so reaching here means the
        // decoder handed back something this was not told about.
        png::ColorType::Indexed => None,
    }
}

/// Centre-crop to a square and resample to `size`.
///
/// Cropped rather than squeezed, because the shape it is going into is a circle
/// and a face squeezed into one is worse than a face with its corners missing —
/// portraits are framed on the middle, which is what survives the crop.
///
/// The resample is a box filter over the source pixels each destination pixel
/// covers, which is the right one *here* for one reason: this only ever shrinks.
/// A 256-pixel portrait into a 128-pixel cell drops three quarters of it, and
/// picking the nearest source pixel instead is how an avatar comes out looking
/// like it was photographed through a screen door.
fn fit(rgba: &[u8], width: u32, height: u32, size: u32) -> Vec<u8> {
    let mut out = vec![0_u8; (size * size * 4) as usize];
    if width == 0 || height == 0 || size == 0 {
        return out;
    }
    let side = width.min(height);
    let left = (width - side) / 2;
    let top = (height - side) / 2;
    let at = |x: u32, y: u32| ((y * width + x) * 4) as usize;

    for y in 0..size {
        // The band of source rows this destination row stands for, never empty
        // even when the source is smaller than the cell.
        let y0 = top + y * side / size;
        let y1 = (top + (y + 1) * side / size).max(y0 + 1).min(top + side);
        for x in 0..size {
            let x0 = left + x * side / size;
            let x1 = (left + (x + 1) * side / size).max(x0 + 1).min(left + side);
            let mut sum = [0_u32; 4];
            let mut taken = 0_u32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let offset = at(sx, sy);
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += rgba[offset + channel] as u32;
                    }
                    taken += 1;
                }
            }
            let offset = ((y * size + x) * 4) as usize;
            for (channel, total) in sum.iter().enumerate() {
                out[offset + channel] = (total / taken.max(1)) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&pixel(x, y));
            }
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&raw).unwrap();
        }
        out
    }

    fn written(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn scratch(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cedm-faces-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_published_avatar_is_found_by_the_account_name() {
        let directory = scratch("published");
        written(&directory, "alex", b"anything");
        assert_eq!(
            published_in(&directory, "alex"),
            Some(directory.join("alex"))
        );
        assert_eq!(published_in(&directory, "nobody"), None);
    }

    /// The name comes from `/etc/passwd` and is one component by construction,
    /// but this joins it onto a path, and a function that joins a name onto a
    /// path should be the one that refuses a name that is not one.
    #[test]
    fn a_name_that_is_a_path_is_not_an_account() {
        let directory = scratch("traversal");
        std::fs::create_dir_all(directory.join("sub")).unwrap();
        written(&directory, "outside", b"anything");
        written(&directory.join("sub"), "inside", b"anything");
        assert_eq!(published_in(&directory.join("sub"), "../outside"), None);
        assert_eq!(published_in(&directory, "sub/inside"), None);
        assert_eq!(published_in(&directory, ""), None);
        assert_eq!(published_in(&directory, ".hidden"), None);
    }

    /// A PNG header, and nothing behind it.
    ///
    /// This is how the dangerous file is written: the whole attack is in the
    /// `IHDR`, so there is no image data to produce and none is produced. The
    /// empty `IDAT` is only there because `read_info` reads up to the first
    /// one before it will answer.
    fn header_only(width: u32, height: u32) -> Vec<u8> {
        fn crc(bytes: &[u8]) -> u32 {
            let mut crc = 0xffff_ffff_u32;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xedb8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let mut named = kind.to_vec();
            named.extend_from_slice(body);
            out.extend_from_slice(&named);
            out.extend_from_slice(&crc(&named).to_be_bytes());
        }
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        // Eight bits a channel, colour type 6 — RGBA, the widest there is.
        header.extend_from_slice(&[8, 6, 0, 0, 0]);

        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &[]);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    /// Fifty-seven bytes that used to take the login screen down.
    ///
    /// A PNG's header names its own size and the decoder is asked how much room
    /// that needs; nothing in the file has to back the claim up, because the
    /// claim is read first. 32768 by 32768 in RGBA is four gigabytes, and a
    /// `vec![0; four gigabytes]` in a process that cannot have it is an abort —
    /// `memory allocation of 4294967296 bytes failed` — in the middle of drawing
    /// a login screen, before anybody has signed in, for every account whose
    /// picture that file is.
    ///
    /// The route in is real: `accounts-daemon` copies what an account sets as
    /// its own icon, `org.freedesktop.accounts.change-own-user-data` is allowed
    /// to any account without authentication on an ordinary machine, and the
    /// daemon bounds the *file* rather than decoding it.
    ///
    /// The assertion is that this costs nothing. No allocation is made here at
    /// all, because nothing past the header is read — which is also why this
    /// test is safe to run beside every other one.
    #[test]
    fn a_picture_that_claims_to_be_enormous_is_refused_before_it_is_believed() {
        let directory = scratch("enormous");
        let bomb = header_only(32_768, 32_768);
        assert!(bomb.len() < 128, "the whole file is a header");
        let path = written(&directory, "alex", &bomb);
        assert_eq!(load(&path, 128), None);

        // The bound is on the picture rather than on its shape, so a strip is
        // refused for being long and a square for being large.
        for (width, height) in [
            (u32::MAX, 1),
            (1, u32::MAX),
            (MAX_SIDE + 1, 4),
            (4, MAX_SIDE + 1),
            (4096, 4096),
            (0, 0),
        ] {
            let path = written(&directory, "alex", &header_only(width, height));
            assert_eq!(load(&path, 128), None, "{width}x{height}");
        }

        // And a picture of a size somebody might really have keeps working.
        let path = written(&directory, "alex", &png(512, 512, |_, _| [90, 90, 90, 255]));
        assert_eq!(load(&path, 128).map(|rgba| rgba.len()), Some(128 * 128 * 4));
    }

    /// Nothing the greeter opens before a login may be able to wait forever.
    ///
    /// A pipe is the shape that does it: `open` on one with no writer blocks
    /// until somebody opens the other end, which nobody ever does. This
    /// directory belongs to `accounts-daemon` rather than to an account, so it
    /// is not the way in that `look::published_in`'s is — it is the same door,
    /// and the same door is the point.
    #[test]
    fn a_pipe_where_a_picture_should_be_is_refused_rather_than_waited_on() {
        use std::sync::mpsc;
        use std::time::Duration;

        let directory = scratch("pipe");
        let path = directory.join("alex");
        let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: `name` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0);

        let (answer, answered) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = answer.send((published_in(&directory, "alex"), load(&path, 128)));
        });
        assert_eq!(
            answered.recv_timeout(Duration::from_secs(5)),
            Ok((None, None)),
            "a pipe has to come back, and has to come back refusing"
        );
    }

    #[test]
    fn a_portrait_arrives_square_at_the_size_asked_for() {
        let directory = scratch("load");
        // Wider than tall, with the middle third — the part a crop keeps —
        // filled in a colour the edges do not use.
        let path = written(
            &directory,
            "alex",
            &png(120, 60, |x, _| {
                if (30..90).contains(&x) {
                    [200, 40, 60, 255]
                } else {
                    [10, 10, 10, 255]
                }
            }),
        );
        let rgba = load(&path, 32).expect("a PNG this can decode");
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Every pixel is from the middle, because the crop took the middle.
        for pixel in rgba.chunks_exact(4) {
            assert_eq!(pixel, [200, 40, 60, 255]);
        }
    }

    #[test]
    fn shrinking_averages_rather_than_picking_one_pixel_in_four() {
        let directory = scratch("resample");
        // A checkerboard of black and white: sampling it lands on one or the
        // other, averaging it lands halfway between.
        let path = written(
            &directory,
            "alex",
            &png(64, 64, |x, y| {
                if (x + y) % 2 == 0 {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            }),
        );
        let rgba = load(&path, 16).unwrap();
        for pixel in rgba.chunks_exact(4) {
            assert!(
                (100..=155).contains(&pixel[0]),
                "a checkerboard averaged to {}, which is a sample and not a mean",
                pixel[0]
            );
        }
    }

    #[test]
    fn what_it_cannot_decode_is_an_account_with_no_picture() {
        let directory = scratch("undecodable");
        let jpeg = written(&directory, "alex", &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J']);
        assert_eq!(load(&jpeg, 32), None);
        let empty = written(&directory, "sam", b"");
        assert_eq!(load(&empty, 32), None);
        assert_eq!(load(&directory.join("absent"), 32), None);
    }

    #[test]
    fn greyscale_and_opaque_sources_come_back_as_rgba() {
        let directory = scratch("grey");
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 8, 8);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[128; 64]).unwrap();
        }
        let path = written(&directory, "alex", &out);
        let rgba = load(&path, 4).unwrap();
        assert_eq!(rgba.len(), 4 * 4 * 4);
        for pixel in rgba.chunks_exact(4) {
            assert_eq!(pixel, [128, 128, 128, 255]);
        }
    }
}
