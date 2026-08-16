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
    path.is_file().then_some(path)
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
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut raw = vec![0; reader.output_buffer_size()?];
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
