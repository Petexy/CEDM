//! The letters of the clock, measured into the material the marks are drawn in.
//!
//! The time on the right of the login screen is the one piece of *type* here
//! that is not drawn through the text pipeline. It is drawn the way
//! `lxb-desktop` draws the clock in the start screen's corner, and for the same
//! reason: the shell hands over to this greeter and back again within a few
//! seconds, and a clock made of flat coverage beside a shell clock made of
//! water would be the seam this whole project is built to avoid.
//!
//! So each character the time can contain is cut out of the bundled bold Roboto
//! once, measured into a signed distance field, and drawn as one quad per
//! letter with a depth and the full gloss — see `glyph_material` in
//! shaders.wgsl, which is the shell's own and shades a bead of water out of the
//! field.
//!
//! **Eleven characters, and no more.** `crate::clock::Now::time` is always
//! `HH:MM` in twenty-four hour form, so the whole alphabet of it is the ten
//! digits and a colon. The *date* under the clock cannot follow and is not meant
//! to: it is words, in nine languages, in Latin, Cyrillic, Devanagari and Han —
//! a cell per codepoint is not a text renderer, and this greeter would need a
//! thousand of them for Chinese alone.

// The measurement is the marks' own: `field` holds the transform, the range it
// is encoded in and the four it is supersampled by, and it is argued for there.
// A letter and a mark are the same kind of thing to the shader, and there is one
// transform rather than two.
use super::field::{distance_field, SDF_SUPERSAMPLE};
use super::CELL;

/// The square of the text one letter's cell covers, as a multiple of the type's
/// size, and where the middle of that square sits above the baseline.
///
/// One square for every character rather than a tight box each, which is what
/// keeps the run looking like one object: the shader's bevel is a fixed fraction
/// of the *quad*, so a colon in a box its own size would be modelled twice as
/// deeply as the digits either side of it. The square is centred on each
/// character's own advance, so the letters keep the spacing the face gives them.
///
/// An em covers a digit and a colon of this face with margin to spare for the
/// shadow, which `every_letter_of_the_clock_is_a_shape_in_its_cell` measures.
pub const LETTER_BOX: f32 = 1.0;
pub const LETTER_MIDDLE: f32 = 0.35;

/// Where the baseline of a run sits below the top of its box, as a multiple of
/// the type's size.
///
/// The clock lays its own letters out now, so nothing forces this to agree with
/// the text pipeline — and it has to, or the time moves the day it is edited and
/// the date under it no longer belongs to it. It is the number `cosmic-text`
/// arrives at for the line height this greeter draws every run at: the face's
/// ascent and descent, centred in a box of 1.25 times the type.
/// `the_clock_sits_on_the_line_the_text_pipeline_would_have_put_it_on` holds it
/// to the shaping.
pub const BASELINE: f32 = 0.9668;

/// The size the letters are cut at: an em of type across the supersampled grid.
const FIELD_SIZE: f32 = (CELL * SDF_SUPERSAMPLE) as f32 / LETTER_BOX;

/// The size the advances are measured at. Large, so a face's own quantisation is
/// noise against it; the answer is a ratio either way.
const ADVANCE_SIZE: f32 = 1000.0;

/// Every character the clock can be written in, in the cell each is measured
/// into.
///
/// The order is the order of [`super::LETTER_SLOT`]: the ten digits by value,
/// then the colon.
pub const SET: [char; 11] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':'];

/// The cell one of them is in, or `None` for a character the clock cannot
/// contain — which is what stops anything else in this greeter being drawn out
/// of this alphabet.
pub fn slot(letter: char) -> Option<u32> {
    SET.iter()
        .position(|c| *c == letter)
        .map(|index| super::LETTER_SLOT + index as u32)
}

/// One of the clock's characters, ready to be drawn: the cell holding the
/// measurement of its shape, and how far the pen moves after it as a multiple of
/// the type's size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Letter {
    pub cell: u32,
    pub advance: f32,
}

/// The letters of one time, in order, or `None` if it holds a character the
/// clock's own alphabet does not.
///
/// `None` rather than the letters it does know: a time with a character missing
/// out of the middle of it would be a *wrong* time, and the greeter would rather
/// show none. It cannot happen while [`crate::clock::Now::time`] is what fills
/// it, and `the_clock_is_drawn_out_of_the_alphabet_it_ships` is what says so.
pub fn run(time: &str) -> Option<Vec<Letter>> {
    time.chars()
        .map(|letter| {
            Some(Letter {
                cell: slot(letter)?,
                advance: *advances().get(&letter)?,
            })
        })
        .collect()
}

/// How far the pen moves after each character, in ems.
///
/// Measured once, from the bundled bold face alone — the same face the cells are
/// cut from, and deliberately not the renderer's own font database, which has
/// the machine's fonts under these six and could answer with the metrics of some
/// other Roboto.
///
/// A run is laid out by adding these up, which is exactly what shaping answers:
/// no pair of characters in this alphabet kerns, and
/// `a_run_of_the_clock_is_as_wide_as_the_same_letters_shaped` holds it to that.
fn advances() -> &'static std::collections::HashMap<char, f32> {
    static ADVANCES: std::sync::OnceLock<std::collections::HashMap<char, f32>> =
        std::sync::OnceLock::new();
    ADVANCES.get_or_init(|| {
        let mut fonts = bold_face();
        let mut out = std::collections::HashMap::new();
        for letter in SET {
            match shaped(&mut fonts, letter, ADVANCE_SIZE) {
                Some(glyph) => {
                    out.insert(letter, glyph.w / ADVANCE_SIZE);
                }
                None => tracing::warn!(%letter, "no advance for one of the clock's characters"),
            }
        }
        out
    })
}

/// The bundled bold Roboto and nothing else.
fn bold_face() -> glyphon::FontSystem {
    let mut database = glyphon::cosmic_text::fontdb::Database::new();
    database.load_font_data(super::UI_FONT_BOLD.to_vec());
    glyphon::FontSystem::new_with_locale_and_db("en-US".to_string(), database)
}

/// Shape one character on its own and answer with the glyph it came out as.
///
/// `None` for a character the face has no glyph for, which for this alphabet
/// would mean the bundled face was no longer Roboto.
fn shaped(
    fonts: &mut glyphon::FontSystem,
    letter: char,
    size: f32,
) -> Option<glyphon::cosmic_text::LayoutGlyph> {
    let mut buffer = glyphon::Buffer::new(fonts, glyphon::Metrics::new(size, size * 1.25));
    buffer.set_size(None, None);
    let attrs = glyphon::Attrs::new()
        .family(glyphon::Family::Name(super::UI_FONT))
        .weight(glyphon::Weight::BOLD);
    buffer.set_text(
        &letter.to_string(),
        &attrs,
        glyphon::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(fonts, false);
    buffer
        .layout_runs()
        .next()?
        .glyphs
        .first()
        .cloned()
        .filter(|glyph| glyph.glyph_id != 0)
}

/// Cut every character of the clock out of the bundled face and measure each
/// into the cell the shader shades a shape out of.
///
/// One cell of `RGBA` per character, in [`SET`]'s order, ready for the atlas to
/// write at [`slot`]. Colour is left white throughout: nothing samples it, and
/// white is what a multiply expects if anything ever does.
///
/// Eleven exact distance transforms over a 1024-square grid, which is the whole
/// cost of this file and the reason they are taken on threads rather than one
/// after another while the login screen has nothing on it yet. In chunks rather
/// than all at once, because each transform holds three grids of its own, and
/// eleven of those at the same time is a hundred megabytes for a login screen to
/// be carrying while it draws its first frame.
pub fn fields() -> Vec<(u32, Vec<u8>)> {
    let mut fonts = bold_face();
    let mut swash = glyphon::SwashCache::new();
    let fine = CELL * SDF_SUPERSAMPLE;

    // The coverage of every letter first, on this thread: the face is not
    // shareable and rasterising is the cheap half anyway.
    let mut coverage: Vec<(u32, Vec<bool>)> = Vec::new();
    for letter in SET {
        let Some(cell) = slot(letter) else {
            continue;
        };
        let Some(glyph) = shaped(&mut fonts, letter, FIELD_SIZE) else {
            tracing::warn!(%letter, "the bundled face has no such character");
            continue;
        };
        // The pen at the origin, so what the mask's placement is measured from
        // is the letter's own baseline and nothing else.
        let physical = glyph.physical((0.0, 0.0), 1.0);
        let Some(image) = swash.get_image_uncached(&mut fonts, physical.cache_key) else {
            tracing::warn!(%letter, "the face would not rasterise a character");
            continue;
        };
        if image.content != glyphon::cosmic_text::SwashContent::Mask {
            tracing::warn!(%letter, "a character came back as something other than coverage");
            continue;
        }

        // Where the letter's square sits in the same pixels the mask is in: the
        // pen is at zero, the baseline is at zero, and up is negative.
        let side = LETTER_BOX * FIELD_SIZE;
        let left = (glyph.w * 0.5 - side * 0.5).round() as i32;
        let top = (-(LETTER_MIDDLE * FIELD_SIZE) - side * 0.5).round() as i32;

        let mut inside = vec![false; (fine * fine) as usize];
        for row in 0..image.placement.height {
            for column in 0..image.placement.width {
                // Coverage of half a pixel or more is the letter, which is where
                // a distance field's zero belongs: the transform measures a
                // shape, and a shape's edge is where it covers half a pixel.
                if image.data[(row * image.placement.width + column) as usize] < 128 {
                    continue;
                }
                let x = image.placement.left + column as i32 - left;
                let y = -image.placement.top + row as i32 - top;
                if x < 0 || y < 0 || x >= fine as i32 || y >= fine as i32 {
                    // A letter that does not fit its own square would be drawn
                    // with a straight cut down it. The test holds the box big
                    // enough for this face; this is what stops a bad one
                    // corrupting the cell beside it instead of being visible.
                    tracing::warn!(%letter, "a character reaches outside its cell");
                    continue;
                }
                inside[(y as u32 * fine + x as u32) as usize] = true;
            }
        }
        coverage.push((cell, inside));
    }

    // And the transforms in parallel, because they are the whole cost and they
    // are eleven separate problems. As many at a time as the machine has cores
    // and no more: one core does them one after another, which is what it would
    // have done anyway.
    let at_once = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let mut fields = Vec::new();
    for chunk in coverage.chunks(at_once) {
        fields.extend(std::thread::scope(|scope| {
            let workers: Vec<_> = chunk
                .iter()
                .map(|(cell, inside)| scope.spawn(|| (*cell, distance_field(inside, fine, CELL))))
                .collect();
            workers
                .into_iter()
                .filter_map(|worker| worker.join().ok())
                .filter_map(|(cell, field)| Some((cell, field?)))
                .collect::<Vec<_>>()
        }));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::super::field::SDF_RANGE;
    use super::*;

    /// Every character the clock can be written in ships as a measurement of its
    /// own shape, inside its own cell, with room round it for the shadow.
    ///
    /// `lxb-desktop`'s guard over the same kind of cell, with one difference:
    /// the ink share. A *mark* has to be a mark on a space and covers at least a
    /// twentieth of its cell; a colon is two dots in an em and covers a
    /// fiftieth, and that is right. What matters here is that it is there, that
    /// it is inside its cell, and that the field is a distance — a chamfer
    /// approximation or a blurred silhouette would each produce a bevel that is
    /// visibly not a bevel, which is the sort of thing that gets noticed on
    /// screen and nowhere else.
    #[test]
    fn every_letter_of_the_clock_is_a_shape_in_its_cell() {
        let fields = fields();
        let cells: Vec<u32> = fields.iter().map(|(cell, _)| *cell).collect();
        let expected: Vec<u32> = SET.iter().filter_map(|letter| slot(*letter)).collect();
        assert_eq!(cells, expected, "the clock's own alphabet, in order");
        assert!(
            cells
                .iter()
                .all(|cell| *cell < super::super::ATLAS_COLUMNS * super::super::ATLAS_ROWS),
            "a letter is outside the atlas",
        );

        let size = CELL as usize;
        for (cell, field) in &fields {
            assert_eq!(field.len(), size * size * 4);
            let at = |x: usize, y: usize| {
                let stored = f32::from(field[(y * size + x) * 4 + 3]) / 255.0;
                (stored - 0.5) * 2.0 * SDF_RANGE * size as f32
            };

            // Signed: some of the cell is letter and some of it is air. A
            // character that came out one way throughout is a mask that landed
            // outside its cell, or a face that drew nothing.
            let inside = (0..size * size)
                .filter(|i| at(i % size, i / size) < 0.0)
                .count();
            let share = inside as f32 / (size * size) as f32;
            assert!(
                (0.005..0.40).contains(&share),
                "cell {cell} is {share:.3} letter, which is not a letter on a space",
            );

            // The margin the shadow is drawn in. It can only be drawn where the
            // quad reaches, so a letter running out to the edge of its cell
            // would have its shadow end in a straight cut.
            let edge = size / 20;
            for i in 0..size {
                for (x, y) in [
                    (i, edge),
                    (i, size - 1 - edge),
                    (edge, i),
                    (size - 1 - edge, i),
                ] {
                    assert!(
                        at(x.min(size - 1), y.min(size - 1)) > 0.0,
                        "cell {cell} reaches its own edge at {x},{y}",
                    );
                }
            }

            // And it is a distance: one pixel of travel can only ever be one
            // pixel of distance. The stored range saturates far from the edge,
            // which can only make a step smaller, never larger.
            for y in 1..size - 1 {
                for x in 1..size - 1 {
                    let step = (at(x, y) - at(x + 1, y))
                        .abs()
                        .max((at(x, y) - at(x, y + 1)).abs());
                    assert!(step <= 1.35, "cell {cell} steps {step} at {x},{y}");
                }
            }
        }

        // No two the same. Two digits that measured alike would be a clock that
        // shows the wrong hour and looks perfectly well drawn doing it.
        for (index, (cell, field)) in fields.iter().enumerate() {
            for (other, second) in &fields[index + 1..] {
                assert_ne!(field, second, "cells {cell} and {other} measure the same");
            }
        }
    }

    /// A run laid out by adding up the advances is exactly as wide as the same
    /// letters shaped.
    ///
    /// Which is what allows the clock to lay itself out: the alternative is
    /// shaping on the thread that has a frame due. It holds because no pair of
    /// characters in this alphabet kerns — if a future face changed that, the
    /// time would drift off the centre of the half it is drawn in, so it is
    /// checked rather than assumed.
    #[test]
    fn a_run_of_the_clock_is_as_wide_as_the_same_letters_shaped() {
        let mut fonts = bold_face();
        let size = 100.0;
        for time in ["20:38", "00:00", "09:05", "23:59", "11:11"] {
            let summed: f32 = run(time)
                .expect("the clock's own alphabet")
                .iter()
                .map(|letter| letter.advance * size)
                .sum();
            let mut buffer =
                glyphon::Buffer::new(&mut fonts, glyphon::Metrics::new(size, size * 1.25));
            buffer.set_size(None, None);
            let attrs = glyphon::Attrs::new()
                .family(glyphon::Family::Name(super::super::UI_FONT))
                .weight(glyphon::Weight::BOLD);
            buffer.set_text(time, &attrs, glyphon::Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut fonts, false);
            let shaped: f32 = buffer
                .layout_runs()
                .map(|line| line.line_w)
                .fold(0.0, f32::max);
            assert!(
                (summed - shaped).abs() < 0.05,
                "{time:?} adds up to {summed} and shapes to {shaped}",
            );
        }
    }

    /// The clock stands on the line the text pipeline would have put it on.
    ///
    /// Nothing forces the two to agree now that the hour is drawn as quads — and
    /// they have to, or the hour moves the day this is edited and the date under
    /// it stops belonging to it.
    #[test]
    fn the_clock_sits_on_the_line_the_text_pipeline_would_have_put_it_on() {
        let mut fonts = bold_face();
        for size in [64.0f32, 132.0, 200.0] {
            let mut buffer =
                glyphon::Buffer::new(&mut fonts, glyphon::Metrics::new(size, size * 1.25));
            buffer.set_size(None, None);
            let attrs = glyphon::Attrs::new()
                .family(glyphon::Family::Name(super::super::UI_FONT))
                .weight(glyphon::Weight::BOLD);
            buffer.set_text("20:38", &attrs, glyphon::Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut fonts, false);
            let baseline = buffer.layout_runs().next().expect("one line").line_y / size;
            assert!(
                (baseline - BASELINE).abs() < 1e-3,
                "at {size} the pipeline's baseline is {baseline}, the clock's is {BASELINE}",
            );
        }
    }

    /// Every time this greeter can show is written in the alphabet it ships, and
    /// nothing else asks that alphabet for a letter.
    ///
    /// The first half is what makes [`run`] refusing an unknown character safe:
    /// `Now::time` is `HH:MM` and cannot produce one. The second is the rule that
    /// keeps this from growing into a text renderer.
    #[test]
    fn the_clock_is_drawn_out_of_the_alphabet_it_ships() {
        for hour in 0..24u8 {
            for minute in 0..60u8 {
                let time = format!("{hour:02}:{minute:02}");
                assert!(
                    run(&time).is_some(),
                    "{time} is not in the alphabet the clock ships",
                );
            }
        }
        for letter in ['a', 'Z', ' ', '.', '日', 'п'] {
            assert!(slot(letter).is_none(), "{letter} has a cell of its own");
        }
    }
}
