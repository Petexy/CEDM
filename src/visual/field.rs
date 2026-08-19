//! A drawing, measured into the material it is drawn in.
//!
//! Every mark this greeter draws itself ships as a *measurement of its shape*
//! rather than as a picture of one: alpha carries how far each pixel is from the
//! nearest edge of the mark, negative inside it, and `glyph_material` in
//! shaders.wgsl builds a bead of water out of that. The drawing is a silhouette
//! and nothing else — no rim, no gradient, no sheen, no shadow.
//!
//! This is `lxb-desktop`'s glyph language, and it is here for the reason
//! everything else in this program is shaped the way it is: the shell hands over
//! to this login screen and back again in a few seconds, and ten of the thirteen
//! drawings in `assets/glyphs` are the shell's own files. A mark painted flat
//! beside the same mark made of water is the seam this project exists to avoid.
//!
//! # Making one
//!
//! 1. Draw the **shape**: the body, and the openings taken out of it by one
//!    `<mask id="pierced">`. Pure `#ffffff`; nothing samples the colour.
//! 2. Say so in the header comment with [`SHAPE_MARK`]. In the drawing rather
//!    than in a list here, because a file that paints nothing *is* a shape and a
//!    list would be a second place for that to be true or false.
//! 3. Leave a **margin**. The shader draws the mark's own shadow on the flat
//!    space beside it and can only draw it where the quad reaches, so a mark
//!    running out to its cell edge has its shadow end in a straight cut. Two of
//!    the drawing's thirty-two units, which
//!    `every_glyph_ships_as_the_shape_of_itself` holds every one of them to.
//!
//! Three ways a shape goes wrong, all of them geometry rather than shading:
//!
//! - **A part narrower than the wall is deep** never gets a flat face and comes
//!   out melted rather than beaded.
//! - **Two bodies overlapping with no gap** cannot be told from one body: a
//!   distance field has no idea which of them it is measuring. Cut the seam as a
//!   real crevice in the mask — white behind, the front swollen in black, then
//!   the front in white again — or move the two apart, which is what
//!   `user-switch.svg` does.
//! - **A hole erases whatever stands in it**, so cut the opening before laying
//!   down the thing inside it.

/// The marker a drawing carries in its header comment to say it is a shape.
///
/// `lxb`, and not this crate's own prefix, because it is the shell's language
/// and the same string in the same place in the same files: ten of these
/// drawings are `lxb-desktop`'s, copied whole, and a marker that differed
/// between the two repositories would have to be edited on the way across.
pub const SHAPE_MARK: &str = "lxb:shape";

/// Whether this drawing is a shape to be measured rather than a picture to be
/// sampled. See [`SHAPE_MARK`].
pub fn is_shape(drawing: &[u8]) -> bool {
    drawing
        .windows(SHAPE_MARK.len())
        .any(|window| window == SHAPE_MARK.as_bytes())
}

/// Half the range the field spans, as a fraction of the cell, and how much finer
/// than the cell a shape is measured before the field is reduced to it.
///
/// `GLYPH_SDF_RANGE` in shaders.wgsl undoes the first; the two are one number.
/// The field is eight bits of alpha, so range and precision trade against each
/// other: a quarter of the cell end to end is wider than any wall wants and
/// still resolves a fraction of a pixel at the size a mark is drawn.
///
/// The second is the shell's four. What a supersample buys is not resolution in
/// the stored field but *where its zero is*: the coverage it measures is binary,
/// so the boundary lands on the finer grid and the reduction averages it down.
/// At two, an edge is quantised to half a cell texel — which on the clock, drawn
/// at nearly the size of its own cell, is a third of a screen pixel of wobble,
/// and a specular of the forty-second power turns that into a row of dashes
/// along every straight stem. It was on screen before it was anywhere else.
pub const SDF_RANGE: f32 = 0.125;
pub(super) const SDF_SUPERSAMPLE: u32 = 4;

/// How deep the slab of a measured shape is, as a share of the square it is drawn
/// in.
///
/// `lxb-desktop`'s `GLYPH_DEPTH`, and the same number for the same reason: it is
/// what a wall of water looks like on a mark of this material, and the shell's
/// own marks are four seconds away on either side of this screen. It is also the
/// number a drawing has to be checked against by eye — a part of a mark thinner
/// than twice this never gets a flat face and comes out as a wire rather than a
/// bead, which is why the clock is set in the bold face and why the shell took
/// this from a tenth to what it is when the Settings cog's teeth dissolved.
pub const DEPTH: f32 = 0.075;

/// Measure a drawing into the field the shader reads, at `size` square.
///
/// `lxb-desktop`'s `icons::builtin_distance_field`. The drawing is rasterised
/// four times over on each side and the coverage thresholded at half a pixel,
/// which is where a shape's edge is; everything after that is
/// [`distance_field`].
pub fn of_drawing(drawing: &[u8], size: u32) -> Option<Vec<u8>> {
    let fine = size.checked_mul(SDF_SUPERSAMPLE)?;
    let coverage = super::rasterise_svg(drawing, fine)?;
    let inside: Vec<bool> = coverage.chunks_exact(4).map(|px| px[3] >= 128).collect();
    distance_field(&inside, fine, size)
}

/// The same measurement, taken of coverage somebody else rasterised.
///
/// `inside` is a `fine`-by-`fine` grid of whether each pixel is within the
/// shape, and `fine` must be [`SDF_SUPERSAMPLE`] times `size`. The other caller
/// is [`super::letters`]: the clock's characters are cut out of the bundled face
/// and measured here, so a letter and a mark are the same kind of thing to the
/// shader and there is one transform rather than two.
///
/// `lxb-desktop`'s `icons::distance_field`, carried across with the transform
/// below. It is the encoding half of one number shared with the shader — see
/// [`SDF_RANGE`].
pub(super) fn distance_field(inside: &[bool], fine: u32, size: u32) -> Option<Vec<u8>> {
    if inside.len() != (fine as usize).pow(2) || fine != size.checked_mul(SDF_SUPERSAMPLE)? {
        return None;
    }

    // Two transforms: how far each pixel outside the shape is from it, and how
    // far each pixel inside it is from getting out. Their difference is the
    // signed field, and it crosses zero on the boundary between the two.
    let out = euclidean_distance(inside, fine, false);
    let within = euclidean_distance(inside, fine, true);

    let block = SDF_SUPERSAMPLE as usize;
    let cell = size as usize;
    let mut rgba = vec![255u8; cell * cell * 4];
    for y in 0..cell {
        for x in 0..cell {
            // The mean over the block the output pixel covers. A distance field
            // is smooth, so averaging it is a reduction rather than the aliasing
            // the same average would be on a picture.
            let mut sum = 0.0f32;
            for dy in 0..block {
                for dx in 0..block {
                    let i = (y * block + dy) * fine as usize + x * block + dx;
                    sum += out[i] - within[i];
                }
            }
            let fine_px = sum / (block * block) as f32;
            // Into fractions of the cell, then into the stored range.
            let cell_fraction = fine_px / fine as f32;
            let stored = 0.5 + cell_fraction / (2.0 * SDF_RANGE);
            rgba[(y * cell + x) * 4 + 3] = (stored.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Some(rgba)
}

/// Exact Euclidean distance to the nearest pixel of the given kind, by
/// Felzenszwalb and Huttenlocher's two-pass transform: the lower envelope of
/// one parabola per seed, taken along the columns and then along the rows.
///
/// Exact rather than the usual chamfer approximation because the error in a
/// chamfer field is largest along the diagonals, and a wall computed from it has
/// visible flats at forty-five degrees.
fn euclidean_distance(inside: &[bool], size: u32, seed_outside: bool) -> Vec<f32> {
    let n = size as usize;
    let far = f32::MAX / 4.0;
    let mut grid: Vec<f32> = inside
        .iter()
        .map(|&i| if i == seed_outside { far } else { 0.0 })
        .collect();

    let mut line = vec![0.0f32; n];
    for x in 0..n {
        for y in 0..n {
            line[y] = grid[y * n + x];
        }
        let done = envelope(&line);
        for y in 0..n {
            grid[y * n + x] = done[y];
        }
    }
    for y in 0..n {
        let done = envelope(&grid[y * n..(y + 1) * n]);
        grid[y * n..(y + 1) * n].copy_from_slice(&done);
    }
    grid.iter().map(|d| d.max(0.0).sqrt()).collect()
}

/// The lower envelope of the parabolas `f[q] + (x - q)^2`, sampled back onto the
/// same grid. One dimension of the transform above.
fn envelope(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut out = vec![0.0f32; n];
    if n == 0 {
        return out;
    }
    let mut vertex = vec![0usize; n];
    let mut cross = vec![0.0f32; n + 1];
    let mut k = 0usize;
    cross[0] = f32::MIN;
    cross[1] = f32::MAX;
    let sq = |v: usize| (v * v) as f32;

    for q in 1..n {
        loop {
            let s = ((f[q] + sq(q)) - (f[vertex[k]] + sq(vertex[k])))
                / (2.0 * q as f32 - 2.0 * vertex[k] as f32);
            if s <= cross[k] && k > 0 {
                k -= 1;
            } else {
                k += 1;
                vertex[k] = q;
                cross[k] = s;
                cross[k + 1] = f32::MAX;
                break;
            }
        }
    }

    k = 0;
    for (q, slot) in out.iter_mut().enumerate() {
        while cross[k + 1] < q as f32 {
            k += 1;
        }
        *slot = (q as f32 - vertex[k] as f32).powi(2) + f[vertex[k]];
    }
    out
}
