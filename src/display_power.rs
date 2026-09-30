//! Dimming the login screen's displays and switching them off, and hearing
//! the power button — through LineXinBar's compositor.
//!
//! The compositor the greeter runs under is LineXinBar's, started with this
//! program as its shell, and it offers its shell `lxb_shell_v1`: the private
//! protocol the session's own shell speaks to it. Three of its messages are
//! the whole of what a login screen needs for [`crate::idle`]:
//!
//! * `set_output_power`, which draws the compositor's dimming sheet over a
//!   display, or fades it to black and switches it off at the connector — the
//!   panel's backlight out, a monitor asleep — and brings it back. The same
//!   sheet and the same fades the session uses, so the two screens rest alike.
//! * `power_button`, sent instead of the key itself, which is how this screen
//!   answers the one power-button setting logind cannot: the Power menu.
//!
//! On its own connection, beside winit's, for the reason [`crate::gamma`] has
//! one: winit does not lend out its registry. The connection is read without
//! ever blocking, once a pass of the greeter's loop, so nothing waits on it.
//! Under any other compositor there is no such global, nothing here is made,
//! and the screen simply stays lit — which is how it always was.
//!
//! The protocol's XML is LineXinBar's own, copied into `protocols/` — see
//! `vendor/linexinbar/ORIGIN.md`.

use std::os::fd::AsFd;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};

use crate::idle::Screens;

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    clippy::all
)]
mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/lxb-shell-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/lxb-shell-v1.xml");
}

use protocol::lxb_shell_v1::{self, LxbShellV1};

/// The first version with display power and the power button.
const POWER_SINCE: u32 = 45;

/// The connection, and what was bound on it.
pub struct DisplayPower {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

struct State {
    shell: LxbShellV1,
    /// Every display, by its registry name so one unplugged can be let go.
    outputs: Vec<(u32, wl_output::WlOutput)>,
    /// What the displays were last set to, for one plugged in later.
    screens: Screens,
    /// Power button edges heard since the last pump: `true` for down.
    presses: Vec<bool>,
}

impl DisplayPower {
    /// Bind the compositor's shell protocol, or `None` where this compositor
    /// has not got it — any other compositor, or one older than the version
    /// that has display power.
    pub fn connect() -> Option<Self> {
        let connection = Connection::connect_to_env().ok()?;
        let (globals, queue) = registry_queue_init::<State>(&connection).ok()?;
        let handle = queue.handle();
        let shell: LxbShellV1 = match globals.bind(&handle, POWER_SINCE..=POWER_SINCE, ()) {
            Ok(shell) => shell,
            Err(error) => {
                tracing::info!(%error, "this compositor cannot dim or switch off the login screen");
                return None;
            }
        };
        let mut advertised = Vec::new();
        globals.contents().with_list(|list| {
            for global in list {
                if global.interface == wl_output::WlOutput::interface().name {
                    advertised.push((global.name, global.version));
                }
            }
        });
        let outputs = advertised
            .into_iter()
            .map(|(name, version)| {
                let output: wl_output::WlOutput =
                    globals.registry().bind(name, version.min(4), &handle, ());
                (name, output)
            })
            .collect();
        tracing::info!("the login screen can dim and switch off its displays");
        Some(Self {
            connection,
            queue,
            state: State {
                shell,
                outputs,
                screens: Screens::Awake,
                presses: Vec::new(),
            },
        })
    }

    /// Dim every display, switch every one off, or bring them all back.
    pub fn set(&mut self, screens: Screens) {
        self.state.screens = screens;
        for (_, output) in &self.state.outputs {
            self.state.shell.set_output_power(output, power(screens));
        }
        let _ = self.connection.flush();
    }

    /// Read whatever has arrived, without waiting, and hand back the power
    /// button's edges since last time.
    pub fn pump(&mut self) -> Vec<bool> {
        let _ = self.connection.flush();
        if let Some(guard) = self.queue.prepare_read() {
            let mut ready = [libc::pollfd {
                fd: std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd().as_fd()),
                events: libc::POLLIN,
                revents: 0,
            }];
            // SAFETY: one pollfd, for a descriptor the guard keeps open, and no
            // wait at all.
            let readable = unsafe { libc::poll(ready.as_mut_ptr(), 1, 0) } > 0
                && ready[0].revents & libc::POLLIN != 0;
            if readable {
                let _ = guard.read();
            }
        }
        let _ = self.queue.dispatch_pending(&mut self.state);
        std::mem::take(&mut self.state.presses)
    }
}

fn power(screens: Screens) -> lxb_shell_v1::DisplayPower {
    match screens {
        Screens::Awake => lxb_shell_v1::DisplayPower::On,
        Screens::Dim => lxb_shell_v1::DisplayPower::Dim,
        Screens::Off => lxb_shell_v1::DisplayPower::Off,
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            // A display plugged in while the screen is dark goes dark with the
            // rest rather than lighting the room.
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == wl_output::WlOutput::interface().name => {
                let output: wl_output::WlOutput = registry.bind(name, version.min(4), handle, ());
                if state.screens != Screens::Awake {
                    state.shell.set_output_power(&output, power(state.screens));
                }
                state.outputs.push((name, output));
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|(bound, output)| {
                    let gone = *bound == name;
                    if gone && output.version() >= 3 {
                        output.release();
                    }
                    !gone
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Everything the compositor tells its shell but the power button is about a
/// session this screen is not.
impl Dispatch<LxbShellV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &LxbShellV1,
        event: lxb_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let lxb_shell_v1::Event::PowerButton { state: key } = event {
            state
                .presses
                .push(matches!(key, WEnum::Value(lxb_shell_v1::KeyState::Pressed)));
        }
    }
}
