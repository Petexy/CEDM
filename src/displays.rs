//! The displays the greeter is drawn on, and where each one is on the surface.
//!
//! CEDM is a Wayland client with one surface, and the compositor that hands it
//! that surface — Cage, until the seat-owning broker on the roadmap exists —
//! extends it across every connected output: one buffer as wide as the whole
//! output layout, with the seam between two monitors somewhere in the middle of
//! it. Drawn as a single screen, that is a wallpaper stretched over two panels
//! of different shapes, a column pinned to the outer edge of the left-hand one,
//! and the clock in the gap between them.
//!
//! So the surface is cut back into the displays it was made of, and each one is
//! composed on its own. That is also what the handover needs: LineXinBar gives
//! every output its own layer surface and evaluates the wallpaper against that
//! output's own size, so a greeter that evaluated one wallpaper across all of
//! them would hand over two different pictures — the seamless frame is only
//! seamless per display.
//!
//! Nothing here talks to Wayland. It is handed what the compositor said about
//! each output and answers with rectangles, which is what makes the one thing
//! that can go wrong here — a layout that does not describe this surface —
//! something a test can ask about.

/// One display, and where it is on the surface, in the physical pixels the
/// interface is laid out in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    pub rect: [f32; 4],
}

impl Display {
    /// The whole surface as one display: what a single-monitor machine has,
    /// and what the greeter falls back to whenever the outputs it was told
    /// about do not describe the surface it was given.
    pub const fn whole(width: f32, height: f32) -> Self {
        Self {
            rect: [0.0, 0.0, width, height],
        }
    }

    pub const fn width(self) -> f32 {
        self.rect[2]
    }

    pub const fn height(self) -> f32 {
        self.rect[3]
    }
}

/// One output as the compositor advertises it: which output it is, where it is
/// in the layout, and how many pixels its current mode has. All three exactly
/// as winit reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// The connector this output is on — `DP-1`, `HDMI-A-1`, `eDP-1` — as
    /// the compositor names it.
    ///
    /// Carried because a layout does not name each output once. winit seeds
    /// its monitor list from the outputs bound while the connection is being
    /// set up, and then appends every one of them a second time as those same
    /// outputs announce themselves through the ordinary event path, so each
    /// screen arrives here twice, standing exactly on itself. By geometry
    /// alone that is indistinguishable from a mirrored pair, and it was
    /// answered as one: a login screen laid out across a whole desk of
    /// monitors as though it were a single very wide one.
    ///
    /// It is also the name LineXinBar files a display's own settings under,
    /// which is what makes a screen here the same screen there.
    ///
    /// `None` from a compositor too old to name its outputs. Nothing can be
    /// told apart then, and a surface whose outputs cannot be told apart is
    /// left whole.
    pub name: Option<String>,
    pub position: (i32, i32),
    pub size: (u32, u32),
}

/// How far the outputs' own bounding box may miss the surface and still be
/// believed to describe it.
///
/// A couple of pixels, for the rounding a compositor does when it turns a
/// layout in logical coordinates into a buffer in device ones. Anything looser
/// would start accepting layouts that are not this surface at all — a nested
/// development window is a few hundred pixels inside one — and every one of
/// those would be answered by drawing the login column somewhere off screen.
const FIT: f32 = 2.0;

/// How many displays the greeter will compose separately.
///
/// A whole login screen is built and drawn for each of them, and the frame is
/// bounded like everything else on this side of a login. Eight is more screens
/// than a machine showing a login screen has; a seat with more than that gets
/// the surface as one display, which is what the greeter did before it could
/// count them.
pub const MAX: usize = 8;

/// Cut a surface into the displays it spans.
///
/// The compositor's layout is only believed when it accounts for this surface
/// exactly: every output a rectangle of its own, no two overlapping, and their
/// bounding box the size of the surface itself. Anything else — a nested window
/// on a desktop, a mirrored pair of outputs, a mode that has been announced but
/// not yet applied, a layout that arrived a configure early — is answered with
/// the whole surface as one display, which is what the greeter did before it
/// could count displays at all and is never wrong on screen, only wide.
///
/// The display nearest the layout's top-left corner comes first. Nothing in the
/// interface depends on the order, because every display is composed the same
/// way; it is here so that "the first display" means the same thing twice in a
/// row, which is what the frame `--shot` writes and every log line needs.
pub fn split(width: f32, height: f32, monitors: &[Monitor]) -> Vec<Display> {
    let whole = || vec![Display::whole(width, height)];
    if !(width > 0.0 && height > 0.0) {
        return whole();
    }
    // One entry per output, however many times the layout named it. See
    // [`Monitor::name`]: the list arrives with every output in it twice, and a
    // screen counted twice is a screen standing on itself, which every rule
    // below would read as a mirrored pair and answer by drawing nothing at all
    // where the second screen is.
    //
    // An output the compositor did not name cannot be told from any other, so
    // the first unnamed one is the only one kept and the surface is left whole
    // rather than cut somewhere unverifiable.
    let mut seen: Vec<&str> = Vec::with_capacity(monitors.len());
    let mut anonymous = false;
    let mut distinct: Vec<Monitor> = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        match monitor.name.as_deref() {
            Some(name) if !seen.contains(&name) => seen.push(name),
            None if !anonymous => anonymous = true,
            _ => continue,
        }
        distinct.push(monitor.clone());
    }
    let monitors = distinct.as_slice();
    // An output with no current mode reports a size of zero. It is a display
    // the compositor is not driving yet rather than a display of no size, and
    // it cannot be placed — so nothing here can be trusted to be the layout.
    if monitors.iter().any(|monitor| {
        monitor.size.0 == 0
            || monitor.size.1 == 0
            || monitor.size.0 > 32768
            || monitor.size.1 > 32768
    }) {
        return whole();
    }
    if monitors.len() < 2 || monitors.len() > MAX {
        return whole();
    }

    let mut rects = monitors
        .iter()
        .map(|monitor| {
            [
                monitor.position.0 as f32,
                monitor.position.1 as f32,
                monitor.size.0 as f32,
                monitor.size.1 as f32,
            ]
        })
        .collect::<Vec<_>>();

    let left = rects.iter().fold(f32::MAX, |left, r| left.min(r[0]));
    let top = rects.iter().fold(f32::MAX, |top, r| top.min(r[1]));
    let right = rects
        .iter()
        .fold(f32::MIN, |right, r| right.max(r[0] + r[2]));
    let bottom = rects
        .iter()
        .fold(f32::MIN, |bottom, r| bottom.max(r[1] + r[3]));
    if (right - left - width).abs() > FIT || (bottom - top - height).abs() > FIT {
        return whole();
    }

    // Two outputs standing on the same pixels are a mirrored pair, whatever
    // else they are. The surface is one of them and the layout does not say
    // which, and a login column drawn once per output would be drawn twice in
    // the same place — two columns of glass in one, at twice the brightness.
    for (index, rect) in rects.iter().enumerate() {
        if rects[index + 1..]
            .iter()
            .any(|other| overlap(*rect, *other))
        {
            return whole();
        }
    }

    // Reading order, which on the row of monitors a desk or a console actually
    // has is left to right.
    rects.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    rects
        .into_iter()
        .map(|[x, y, w, h]| {
            // Inside the surface, whatever the couple of pixels of rounding
            // above allowed: these become the bounds a display is laid out in,
            // and the one thing every one of them must be is on the screen.
            let x = (x - left).clamp(0.0, width);
            let y = (y - top).clamp(0.0, height);
            Display {
                rect: [x, y, w.min(width - x), h.min(height - y)],
            }
        })
        .collect()
}

fn overlap([ax, ay, aw, ah]: [f32; 4], [bx, by, bw, bh]: [f32; 4]) -> bool {
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// An output nothing else in the layout is.
    ///
    /// A fresh name every time, because identity is what tells one screen from
    /// another here: two outputs standing on the same pixels are a mirrored
    /// pair and two entries for one output are a miscount, and the only thing
    /// that separates those two cases is whether the compositor gave them the
    /// same name.
    fn monitor(x: i32, y: i32, w: u32, h: u32) -> Monitor {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Monitor {
            name: Some(format!("TEST-OUT-{}", NEXT.fetch_add(1, Ordering::Relaxed))),
            position: (x, y),
            size: (w, h),
        }
    }

    /// The same outputs over again, as the layout actually arrives.
    fn twice(monitors: &[Monitor]) -> Vec<Monitor> {
        monitors.iter().chain(monitors).cloned().collect()
    }

    /// The machine this was written on: a 2560×1440 panel with a 1920×1080 one
    /// beside it, which Cage extends into one 4480×1440 surface.
    #[test]
    fn a_row_of_monitors_is_cut_where_the_monitors_are() {
        let displays = split(
            4480.0,
            1440.0,
            &[monitor(0, 0, 2560, 1440), monitor(2560, 0, 1920, 1080)],
        );
        assert_eq!(
            displays,
            vec![
                Display {
                    rect: [0.0, 0.0, 2560.0, 1440.0]
                },
                Display {
                    rect: [2560.0, 0.0, 1920.0, 1080.0]
                },
            ]
        );
    }

    /// The layout as it actually arrives: every output in it twice.
    ///
    /// This is the desk above — a 2560×1440 panel beside a second one, which
    /// Cage extends into one 5120×1440 surface — and it is what the greeter is
    /// handed on that machine. Cut by geometry alone it is four screens, two
    /// pairs of them standing on each other, and the answer to a mirrored pair
    /// is the whole surface as one display: one login screen stretched across
    /// two monitors, which is what this is here to keep fixed.
    #[test]
    fn an_output_the_layout_names_twice_is_one_display() {
        let desk = [monitor(0, 0, 2560, 1440), monitor(2560, 0, 2560, 1440)];
        let displays = split(5120.0, 1440.0, &twice(&desk));
        assert_eq!(
            displays,
            vec![
                Display {
                    rect: [0.0, 0.0, 2560.0, 1440.0]
                },
                Display {
                    rect: [2560.0, 0.0, 2560.0, 1440.0]
                },
            ]
        );
        // And one screen named twice is one screen, not a mirrored pair.
        assert_eq!(
            split(1920.0, 1080.0, &twice(&[monitor(0, 0, 1920, 1080)])),
            vec![Display::whole(1920.0, 1080.0)]
        );
        // The count that decides how many screens are composed separately is
        // the count of screens, so a desk of eight of them is still cut up
        // when the layout names all sixteen.
        let row = (0..MAX as i32)
            .map(|index| monitor(index * 1920, 0, 1920, 1080))
            .collect::<Vec<_>>();
        assert_eq!(split(MAX as f32 * 1920.0, 1080.0, &twice(&row)).len(), MAX);
    }

    /// The layout's own origin is not the surface's. A monitor to the left of
    /// the one at the origin puts negative coordinates in the layout, and the
    /// buffer still starts at zero.
    #[test]
    fn the_layout_is_placed_against_the_surfaces_own_corner() {
        let displays = split(
            3840.0,
            1080.0,
            &[monitor(0, 0, 1920, 1080), monitor(-1920, 0, 1920, 1080)],
        );
        assert_eq!(
            displays,
            vec![
                Display {
                    rect: [0.0, 0.0, 1920.0, 1080.0]
                },
                Display {
                    rect: [1920.0, 0.0, 1920.0, 1080.0]
                },
            ]
        );
    }

    /// A nested window on a desktop is handed the desktop's outputs and a
    /// surface a few hundred pixels across. Cutting that up would put the
    /// column off the edge of the window; it is one display, as it looks.
    #[test]
    fn a_window_that_is_not_the_layout_stays_one_display() {
        let displays = split(
            1280.0,
            720.0,
            &[monitor(0, 0, 2560, 1440), monitor(2560, 0, 1920, 1080)],
        );
        assert_eq!(displays, vec![Display::whole(1280.0, 720.0)]);
    }

    /// Mirrored outputs stand on the same pixels, so cutting by output would
    /// draw two columns of glass in one place rather than one on each screen.
    #[test]
    fn mirrored_outputs_stay_one_display() {
        let displays = split(
            1920.0,
            1080.0,
            &[monitor(0, 0, 1920, 1080), monitor(0, 0, 1920, 1080)],
        );
        assert_eq!(displays, vec![Display::whole(1920.0, 1080.0)]);
    }

    /// An output the compositor has not given a mode yet has no size, and a
    /// layout with one in it does not describe the surface.
    #[test]
    fn an_output_without_a_mode_stays_one_display() {
        let displays = split(
            1920.0,
            1080.0,
            &[monitor(0, 0, 1920, 1080), monitor(1920, 0, 0, 0)],
        );
        assert_eq!(displays, vec![Display::whole(1920.0, 1080.0)]);
    }

    /// One monitor is the ordinary machine, and it is the whole surface
    /// whatever the layout says about where it starts.
    #[test]
    fn one_monitor_is_the_whole_surface() {
        assert_eq!(
            split(1920.0, 1080.0, &[monitor(1080, 200, 1920, 1080)]),
            vec![Display::whole(1920.0, 1080.0)]
        );
        assert_eq!(
            split(1920.0, 1080.0, &[]),
            vec![Display::whole(1920.0, 1080.0)]
        );
    }

    /// Monitors of different heights leave a strip of the surface that is not
    /// on any display. It belongs to no display and is drawn by none of them —
    /// the alternative is a wallpaper the taller screen shows a slice of.
    #[test]
    fn a_shorter_monitor_leaves_the_strip_beneath_it_to_nobody() {
        let displays = split(
            4480.0,
            1440.0,
            &[monitor(0, 0, 2560, 1440), monitor(2560, 0, 1920, 1080)],
        );
        let covered = displays
            .iter()
            .map(|display| display.width() * display.height())
            .sum::<f32>();
        assert!(covered < 4480.0 * 1440.0);
    }

    /// Every display the greeter is given has to be inside the surface, and
    /// the couple of pixels of rounding allowed in the fit cannot leak out of
    /// one: these rectangles are what the interface is laid out in.
    #[test]
    fn every_display_lands_inside_the_surface() {
        for surface in [(3838.0, 1080.0), (3840.0, 1080.0), (3842.0, 1080.0)] {
            let displays = split(
                surface.0,
                surface.1,
                &[monitor(0, 0, 1920, 1080), monitor(1920, 0, 1920, 1080)],
            );
            assert_eq!(displays.len(), 2, "{surface:?} was not cut in two");
            for display in displays {
                let [x, y, w, h] = display.rect;
                assert!(
                    x >= 0.0 && y >= 0.0 && x + w <= surface.0 && y + h <= surface.1,
                    "{:?} escapes a {surface:?} surface",
                    display.rect
                );
            }
        }
    }
}
