//! Gamepad navigation with LineXinBar's dead zones and repeat rhythm.

use crate::steam_hid::{Buttons, SteamPad};
use gilrs::{Axis, Button, EventType, Gilrs, GilrsBuilder};
use std::time::Duration;

pub const POLL_INTERVAL: Duration = Duration::from_millis(8);
pub const INITIAL_REPEAT_DELAY: Duration = Duration::from_millis(350);
pub const REPEAT_INTERVAL: Duration = Duration::from_millis(90);
const STICK_ENGAGE: f32 = 0.55;
const STICK_RELEASE: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Left,
    Right,
    Up,
    Down,
    Accept,
    /// Start — Accept everywhere except over the on-screen keyboard, where it
    /// is the one press that finishes typing: Enter, and the board away.
    ///
    /// It is folded into [`Action::Accept`] on its way in whenever the board is
    /// not up (see `Application::apply_action`), because that is the only place
    /// the two differ and nothing behind the board should have to know there
    /// are two. A field being filled in ends with Enter and then with the
    /// keyboard gone, which on a pad is two presses at opposite ends of the
    /// board, and Start is the button every console has already taught for it.
    Submit,
    Back,
    ToggleKeyboard,
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];
    fn index(self) -> usize {
        self as usize
    }
    fn action(self) -> Action {
        match self {
            Self::Left => Action::Left,
            Self::Right => Action::Right,
            Self::Up => Action::Up,
            Self::Down => Action::Down,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Held {
    down: bool,
    next: Option<Duration>,
}

pub struct Controller {
    gilrs: Option<Gilrs>,
    steam: SteamPad,
    held: [Held; 4],
    // Keep the two backends separate.  In particular, a centred Steam frame
    // must replace the previous Steam position instead of being compared with
    // it and losing because zero is closer to rest.
    standard_stick: (f32, f32),
    steam_stick: (f32, f32),
}

impl Controller {
    pub fn new(enabled: bool) -> Self {
        let gilrs = if enabled {
            match GilrsBuilder::new().with_force_feedback(false).build() {
                Ok(gilrs) => Some(gilrs),
                Err(error) => {
                    tracing::warn!(%error, "standard gamepad input unavailable");
                    None
                }
            }
        } else {
            None
        };
        Self {
            gilrs,
            steam: SteamPad::new(enabled),
            held: [Held::default(); 4],
            standard_stick: (0.0, 0.0),
            steam_stick: (0.0, 0.0),
        }
    }

    pub fn poll(&mut self, now: Duration) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut digital = [false; 4];
        let mut standard_stick = (0.0, 0.0);

        if let Some(gilrs) = self.gilrs.as_mut() {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::Connected => tracing::info!(
                        name = gilrs.gamepad(event.id).name(),
                        "controller connected"
                    ),
                    EventType::Disconnected => tracing::info!("controller disconnected"),
                    EventType::ButtonPressed(button, code) => {
                        if let Some(action) = button_action(button, code.into_u32()) {
                            actions.push(action);
                        }
                    }
                    _ => {}
                }
            }
            for (_, gamepad) in gilrs.gamepads() {
                digital[Direction::Left.index()] |= gamepad.is_pressed(Button::DPadLeft);
                digital[Direction::Right.index()] |= gamepad.is_pressed(Button::DPadRight);
                digital[Direction::Up.index()] |= gamepad.is_pressed(Button::DPadUp);
                digital[Direction::Down.index()] |= gamepad.is_pressed(Button::DPadDown);
                standard_stick = merge_sticks(
                    standard_stick,
                    (
                        gamepad.value(Axis::LeftStickX),
                        gamepad.value(Axis::LeftStickY),
                    ),
                );
            }
        }
        // Rebuilding this from the connected pads' cached state on every poll
        // also drops an unplugged standard pad's final axis value.
        self.standard_stick = standard_stick;

        let steam_frame = self.steam.poll(now);
        // `None` means there is no speaking Steam pad.  `Some` can contain an
        // explicitly centred stick.  Both have to clear an older contribution.
        self.steam_stick = reported_steam_stick(steam_frame.as_ref());
        if let Some(frame) = steam_frame {
            digital[Direction::Left.index()] |= frame.held.has(Buttons::LEFT);
            digital[Direction::Right.index()] |= frame.held.has(Buttons::RIGHT);
            digital[Direction::Up.index()] |= frame.held.has(Buttons::UP);
            digital[Direction::Down.index()] |= frame.held.has(Buttons::DOWN);
            for (button, action) in [
                (Buttons::A, Action::Accept),
                // The pad's own Start, and the same button as every other
                // pad's: Accept, except over the board. See [`Action::Submit`].
                (Buttons::MENU, Action::Submit),
                (Buttons::B, Action::Back),
                (Buttons::Y, Action::ToggleKeyboard),
                (Buttons::L1, Action::Previous),
                (Buttons::R1, Action::Next),
            ] {
                if frame.pressed.has(button) {
                    actions.push(action);
                }
            }
        }

        // Either backend may drive either axis; the position further from rest
        // wins for this poll only.  No merged position is carried to the next.
        let stick = merge_sticks(self.standard_stick, self.steam_stick);

        update_axis(&mut digital, Direction::Left, stick.0, -1.0, &self.held);
        update_axis(&mut digital, Direction::Right, stick.0, 1.0, &self.held);
        update_axis(&mut digital, Direction::Up, stick.1, 1.0, &self.held);
        update_axis(&mut digital, Direction::Down, stick.1, -1.0, &self.held);

        for direction in Direction::ALL {
            let state = &mut self.held[direction.index()];
            if digital[direction.index()] {
                if !state.down {
                    state.down = true;
                    state.next = Some(now + INITIAL_REPEAT_DELAY);
                    actions.push(direction.action());
                } else if state.next.is_some_and(|deadline| now >= deadline) {
                    state.next = Some(now + REPEAT_INTERVAL);
                    actions.push(direction.action());
                }
            } else {
                *state = Held::default();
            }
        }
        actions
    }
}

fn reported_steam_stick(frame: Option<&crate::steam_hid::Frame>) -> (f32, f32) {
    frame.map(|frame| frame.left_stick).unwrap_or_default()
}

/// Merge simultaneous controller positions without retaining either backend's
/// result beyond the poll in which it was reported.
fn merge_sticks(standard: (f32, f32), steam: (f32, f32)) -> (f32, f32) {
    let further = |standard: f32, steam: f32| {
        if steam.abs() > standard.abs() {
            steam
        } else {
            standard
        }
    };
    (further(standard.0, steam.0), further(standard.1, steam.1))
}

fn update_axis(
    digital: &mut [bool; 4],
    direction: Direction,
    value: f32,
    sign: f32,
    held: &[Held; 4],
) {
    let directed = value * sign;
    let threshold = if held[direction.index()].down {
        STICK_RELEASE
    } else {
        STICK_ENGAGE
    };
    digital[direction.index()] |= directed >= threshold;
}

fn button_action(button: Button, raw: u32) -> Option<Action> {
    match button {
        Button::South => Some(Action::Accept),
        // Start — Xbox Menu, the PlayStation Options button, Steam Deck's own
        // `≡`. Accept, like `A`, everywhere but over the on-screen keyboard,
        // where it is Enter and the way out of the board in one press. See
        // [`Action::Submit`].
        Button::Start => Some(Action::Submit),
        Button::East => Some(Action::Back),
        Button::North => Some(Action::ToggleKeyboard),
        Button::LeftTrigger => Some(Action::Previous),
        Button::RightTrigger => Some(Action::Next),
        Button::Unknown => match raw & 0xffff {
            // `BTN_SOUTH`, and `BTN_TRIGGER` for a pad numbered as a joystick.
            0x130 | 0x120 => Some(Action::Accept),
            // `BTN_START`.
            0x13b => Some(Action::Submit),
            0x131 | 0x121 => Some(Action::Back),
            0x134 | 0x123 => Some(Action::ToggleKeyboard),
            0x136 => Some(Action::Previous),
            0x137 => Some(Action::Next),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_waits_then_keeps_the_shells_rhythm() {
        let mut held = [Held::default(); 4];
        let mut digital = [false; 4];
        update_axis(&mut digital, Direction::Right, 0.7, 1.0, &held);
        assert!(digital[Direction::Right.index()]);
        held[Direction::Right.index()].down = true;
        digital = [false; 4];
        update_axis(&mut digital, Direction::Right, 0.4, 1.0, &held);
        assert!(
            digital[Direction::Right.index()],
            "hysteresis keeps a held stick engaged"
        );
    }

    #[test]
    fn centred_steam_frame_clears_the_previous_position() {
        let moved = crate::steam_hid::Frame {
            left_stick: (0.84, -0.71),
            ..crate::steam_hid::Frame::default()
        };
        let centred = crate::steam_hid::Frame::default();

        assert_eq!(reported_steam_stick(Some(&moved)), (0.84, -0.71));
        assert_eq!(reported_steam_stick(Some(&centred)), (0.0, 0.0));
        assert_eq!(reported_steam_stick(None), (0.0, 0.0));
    }

    #[test]
    fn backends_are_merged_fresh_axis_by_axis() {
        assert_eq!(merge_sticks((0.75, 0.20), (0.10, -0.80)), (0.75, -0.80));
        assert_eq!(merge_sticks((0.75, 0.20), (0.0, 0.0)), (0.75, 0.20));
        assert_eq!(merge_sticks((0.0, 0.0), (0.0, 0.0)), (0.0, 0.0));
    }
}
