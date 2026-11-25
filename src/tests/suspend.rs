//! The per-window suspended flag: set over IPC, reported back in window records.
//!
//! Actions go through `State::run_ipc_action`, the path `niri msg action` takes, so the semantic
//! result a client would read is the one observed here.

use niri_config::Config;
use niri_ipc::{Action, WindowActionResult, WindowActionStatus};
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;

fn fixture() -> Fixture {
    let mut f = Fixture::with_config(Config::default());
    f.add_output(1, (1920, 1080));
    f
}

/// Maps a window for `id` and returns its surface.
fn map_window(f: &mut Fixture, id: ClientId) -> WlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    surface
}

impl Fixture {
    fn ipc_window(&mut self, id: u64) -> Option<niri_ipc::Window> {
        self.niri().ipc_server.as_ref()?.window_record(id)
    }

    fn ipc_minimized_window(&mut self, id: u64) -> Option<niri_ipc::MinimizedWindow> {
        self.niri().ipc_server.as_ref()?.minimized_record(id)
    }
}

/// The id niri gave the one mapped window.
fn only_window_id(f: &mut Fixture) -> u64 {
    let ids: Vec<_> = f
        .niri()
        .layout
        .windows()
        .map(|(_, mapped)| mapped.id().get())
        .collect();
    assert_eq!(ids.len(), 1, "expected exactly one window");
    ids[0]
}

fn set_suspended(f: &mut Fixture, id: u64, suspended: bool) -> WindowActionResult {
    f.niri_state()
        .run_ipc_action(Action::SetWindowSuspended { id, suspended })
        .expect("the action answers with a window result")
}

#[test]
fn suspending_an_unknown_window_is_not_found() {
    let mut f = fixture();
    let result = set_suspended(&mut f, 9999, true);
    assert_eq!(result.status, WindowActionStatus::WindowNotFound);
    assert_eq!(result.requested_id, Some(9999));
    assert_eq!(result.window_id, None);
}

#[test]
fn suspending_twice_reports_already_suspended() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let id = only_window_id(&mut f);

    assert_eq!(
        set_suspended(&mut f, id, true).status,
        WindowActionStatus::Applied
    );
    assert_eq!(
        set_suspended(&mut f, id, true).status,
        WindowActionStatus::AlreadyInRequestedState
    );
    assert_eq!(
        set_suspended(&mut f, id, false).status,
        WindowActionStatus::Applied
    );
}

#[test]
fn ipc_reports_the_suspended_flag() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let id = only_window_id(&mut f);

    f.niri_state().ipc_refresh_layout();
    assert!(!f.ipc_window(id).unwrap().is_suspended);

    set_suspended(&mut f, id, true);
    f.niri_state().ipc_refresh_layout();
    assert!(
        f.ipc_window(id).unwrap().is_suspended,
        "a flag change alone must reach the IPC window record"
    );

    f.niri_state()
        .run_ipc_action(Action::MinimizeWindow {
            id: Some(id),
            transition: None,
        })
        .unwrap();
    f.niri_state().ipc_refresh_layout();
    assert!(f.ipc_window(id).is_none());
    assert!(f.ipc_minimized_window(id).unwrap().is_suspended);
}
