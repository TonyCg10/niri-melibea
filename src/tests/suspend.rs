//! The per-window suspended flag: set over IPC, reported back in window records.
//!
//! Actions go through `State::run_ipc_action`, the path `niri msg action` takes, so the semantic
//! result a client would read is the one observed here.

use niri_config::Config;
use niri_ipc::{Action, WindowActionResult, WindowActionStatus};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface as ServerSurface;
use smithay::utils::{Logical, Point};
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;
use crate::window::Mapped;

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

/// The window niri knows as `id`.
fn mapped(f: &mut Fixture, id: u64) -> &Mapped {
    f.niri()
        .layout
        .windows()
        .map(|(_, mapped)| mapped)
        .find(|mapped| mapped.id().get() == id)
        .expect("the window is in the layout")
}

/// The server-side toplevel surface of the window niri knows as `id`.
fn server_surface(f: &mut Fixture, id: u64) -> ServerSurface {
    mapped(f, id).toplevel().wl_surface().clone()
}

/// The centre of the area where the pointer hits the window niri knows as `id`, found by probing
/// the output on an 8 px grid.
fn window_centre(f: &mut Fixture, id: u64) -> Point<f64, Logical> {
    let surface = server_surface(f, id);
    let mut hits = (0..1080)
        .step_by(8)
        .flat_map(|y| (0..1920).step_by(8).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let pos = Point::from((f64::from(x), f64::from(y)));
            f.niri()
                .contents_under(pos)
                .window
                .is_some_and(|(window, _)| window.toplevel().unwrap().wl_surface() == &surface)
        })
        .peekable();
    let first = *hits.peek().expect("the window is somewhere on the output");
    let (min, max) = hits.fold((first, first), |(min, max), (x, y)| {
        ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y)))
    });
    Point::from((f64::from(min.0 + max.0) / 2., f64::from(min.1 + max.1) / 2.))
}

fn is_focused(f: &mut Fixture, id: u64) -> bool {
    mapped(f, id).is_focused()
}

fn focus_window(f: &mut Fixture, id: u64) {
    f.niri_state().run_ipc_action(Action::FocusWindow { id });
    f.niri_state().update_keyboard_focus();
}

#[test]
fn a_suspended_window_gets_no_keyboard_focus_but_stays_focused_in_ipc() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let id = only_window_id(&mut f);
    let surface = server_surface(&mut f, id);
    f.niri_state().update_keyboard_focus();
    assert_eq!(f.niri().keyboard_focus.surface(), Some(&surface));

    set_suspended(&mut f, id, true);
    assert_eq!(f.niri().keyboard_focus.surface(), None);
    assert!(is_focused(&mut f, id), "IPC must keep naming the window");

    set_suspended(&mut f, id, false);
    assert_eq!(f.niri().keyboard_focus.surface(), Some(&surface));
    assert!(is_focused(&mut f, id));
}

#[test]
fn pointer_over_a_suspended_window_finds_no_surface() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let id = only_window_id(&mut f);

    let centre = window_centre(&mut f, id);

    let contents = f.niri().contents_under(centre);
    assert!(
        contents.surface.is_some(),
        "an awake window yields its surface"
    );

    set_suspended(&mut f, id, true);
    let contents = f.niri().contents_under(centre);
    assert!(contents.surface.is_none());
    let (window, _) = contents.window.expect("the tile is still hit");
    assert_eq!(
        window.toplevel().unwrap().wl_surface(),
        &server_surface(&mut f, id)
    );
}

#[test]
fn focus_leaves_and_returns_to_a_suspended_window() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let suspended = only_window_id(&mut f);
    set_suspended(&mut f, suspended, true);

    map_window(&mut f, client);
    let other = f
        .niri()
        .layout
        .windows()
        .map(|(_, mapped)| mapped.id().get())
        .find(|&id| id != suspended)
        .unwrap();
    focus_window(&mut f, other);
    let other_surface = server_surface(&mut f, other);
    assert_eq!(f.niri().keyboard_focus.surface(), Some(&other_surface));
    assert!(!is_focused(&mut f, suspended));
    assert!(is_focused(&mut f, other));

    focus_window(&mut f, suspended);
    assert!(is_focused(&mut f, suspended));
    assert!(!is_focused(&mut f, other));
    assert_eq!(f.niri().keyboard_focus.surface(), None);
}

#[test]
fn a_suspended_window_is_not_focused_while_the_overview_is_open() {
    let mut f = fixture();
    let client = f.add_client();
    map_window(&mut f, client);
    let id = only_window_id(&mut f);
    set_suspended(&mut f, id, true);
    assert!(is_focused(&mut f, id));

    f.niri_state().run_ipc_action(Action::OpenOverview {});
    f.niri_state().update_keyboard_focus();
    assert!(f.niri().keyboard_focus.is_overview());
    assert!(!is_focused(&mut f, id));

    f.niri_state().run_ipc_action(Action::CloseOverview {});
    f.niri_state().update_keyboard_focus();
    assert_eq!(f.niri().keyboard_focus.surface(), None);
    assert!(is_focused(&mut f, id));
}
