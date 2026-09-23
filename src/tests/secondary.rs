//! Windows opened by a process that already has one on screen.
//!
//! A dialog declares its parent, and niri floats it for that. Plenty of dialogs never do: a
//! browser extension's popout, most of Steam. What they still cannot hide is that the process
//! asking for them already has a window up, and that is what `is-secondary` matches.

use insta::assert_snapshot;
use niri_config::Config;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;
use crate::layout::LayoutElement as _;
use crate::utils::{is_mapped, with_toplevel_role};

/// The size a window draws at when left to choose.
const NATURAL: (u16, u16) = (100, 100);

/// The size a toolkit draws at when asked for `asked`: that size, or where a dimension is 0,
/// the one it already has. GTK does exactly this, and it is why a window once given a tile's
/// size keeps it when later told to choose its own.
fn toolkit_size(asked: (i32, i32), current: (u16, u16)) -> (u16, u16) {
    let pick = |asked: i32, current: u16| {
        if asked == 0 {
            current
        } else {
            u16::try_from(asked).unwrap()
        }
    };
    (pick(asked.0, current.0), pick(asked.1, current.1))
}

/// Maps a window for `id` the way a toolkit would, and returns its surface.
fn map_window(f: &mut Fixture, id: ClientId, app_id: &str, title: &str) -> WlSurface {
    map_window_drawn(f, id, app_id, title).0
}

/// Like `map_window`, also returning the size the window drew itself at.
fn map_window_drawn(
    f: &mut Fixture,
    id: ClientId,
    app_id: &str,
    title: &str,
) -> (WlSurface, (u16, u16)) {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_app_id(app_id);
    window.set_title(title);
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    let asked = window.configures_received.last().unwrap().1.size;
    let (w, h) = toolkit_size(asked, NATURAL);
    window.attach_new_buffer();
    window.set_size(w, h);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    (surface, (w, h))
}

/// The size of the placed window titled `title`.
fn size_of(f: &mut Fixture, title: &str) -> Option<(i32, i32)> {
    f.niri().layout.windows().find_map(|(_, mapped)| {
        let t = with_toplevel_role(mapped.toplevel(), |role| role.title.clone());
        (t.as_deref() == Some(title)).then(|| {
            let size = mapped.size();
            (size.w, size.h)
        })
    })
}

/// One line per window with a buffer: where it ended up, keyed by title.
///
/// `held` is a window that has committed a buffer but has not been placed yet.
fn placement(f: &mut Fixture) -> String {
    let niri = f.niri();
    let mut lines = Vec::new();

    for (_, mapped) in niri.layout.windows() {
        let title = with_toplevel_role(mapped.toplevel(), |role| role.title.clone());
        let place = if mapped.is_floating() {
            "floating"
        } else {
            "tiled"
        };
        lines.push(format!("{}: {place}", title.unwrap_or_default()));
    }

    for (surface, unmapped) in &niri.unmapped_windows {
        if is_mapped(surface) {
            let title = with_toplevel_role(unmapped.toplevel(), |role| role.title.clone());
            lines.push(format!("{}: held", title.unwrap_or_default()));
        }
    }

    lines.sort();
    lines.join("\n")
}

fn fixture(config: &str) -> Fixture {
    let config = Config::parse_mem(config).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    f
}

const FLOAT_SECONDARY: &str = r##"
window-rule {
    match is-secondary=true
    open-floating true
}
"##;

#[test]
fn a_second_window_from_the_same_process_floats() {
    let mut f = fixture(FLOAT_SECONDARY);
    let id = f.add_client();

    map_window(&mut f, id, "app", "main");
    map_window(&mut f, id, "app", "dialog");

    assert_snapshot!(placement(&mut f), @r"
    dialog: floating
    main: tiled
    ");
}

#[test]
fn another_programs_first_window_is_not_secondary() {
    let mut f = fixture(FLOAT_SECONDARY);
    let browser = f.add_client();
    let terminal = f.add_client();

    map_window(&mut f, browser, "browser", "browser");
    map_window(&mut f, terminal, "terminal", "terminal");

    assert_snapshot!(placement(&mut f), @r"
    browser: tiled
    terminal: tiled
    ");
}

#[test]
fn programs_sharing_one_connection_are_told_apart_by_app_id() {
    // Every X11 program reaches niri through xwayland-satellite's single connection, so the
    // connection alone would make the first window of the second X11 program look like a
    // dialog of the first. What still differs is the app id the satellite takes from
    // WM_CLASS.
    let mut f = fixture(FLOAT_SECONDARY);
    let satellite = f.add_client();

    map_window(&mut f, satellite, "steam", "Steam");
    map_window(&mut f, satellite, "gimp", "GIMP");

    assert_snapshot!(placement(&mut f), @r"
    GIMP: tiled
    Steam: tiled
    ");
}

#[test]
fn a_second_instance_of_the_same_program_is_not_secondary() {
    // Two terminals started separately are two processes with one app id between them. The
    // second is a new instance, not a dialog of the first.
    let mut f = fixture(FLOAT_SECONDARY);
    let first = f.add_client();
    let second = f.add_client();

    map_window(&mut f, first, "kitty", "one");
    map_window(&mut f, second, "kitty", "two");

    assert_snapshot!(placement(&mut f), @r"
    one: tiled
    two: tiled
    ");
}

/// The configuration this whole file exists for: secondary windows float unless they are a
/// browser's own ordinary windows, and the ones that have no parent wait long enough to be told
/// apart. The wait is long here so the tests never race it; the deadline has its own test.
const ZEN: &str = r##"
window-rule {
    match is-secondary=true is-transient=false
    settle-ms 60000
}

window-rule {
    match is-secondary=true
    open-floating true
}

window-rule {
    match app-id="^zen$" is-secondary=true
    exclude title="^Extension: "
    open-floating false
}
"##;

#[test]
fn a_popout_that_names_itself_late_opens_floating_without_touching_the_strip() {
    let mut f = fixture(ZEN);
    let id = f.add_client();

    map_window(&mut f, id, "zen", "New Tab — Zen Browser");
    // Measured on the real thing: the popout maps under the browser's generic title and renames
    // itself 38 ms later.
    let popout = map_window(&mut f, id, "zen", "Zen Browser");

    // Not in the strip and not floating either: nowhere yet.
    assert_snapshot!(placement(&mut f), @r"
    New Tab — Zen Browser: tiled
    Zen Browser: held
    ");

    // The late title is what it was waiting for: placed the moment it arrives.
    f.client(id)
        .window(&popout)
        .set_title("Extension: (Bitwarden Password Manager)");
    f.double_roundtrip(id);

    assert_snapshot!(placement(&mut f), @r"
    Extension: (Bitwarden Password Manager): floating
    New Tab — Zen Browser: tiled
    ");
}

#[test]
fn a_window_whose_rules_never_change_is_placed_when_its_time_is_up() {
    // A new browser window: secondary, no parent, and nothing about it changes, so the only
    // thing that can end the wait is the deadline. Short here, and waited out for real, since
    // the timer is the thing under test.
    let config = ZEN.replace("settle-ms 60000", "settle-ms 100");
    let mut f = fixture(&config);
    let id = f.add_client();

    map_window(&mut f, id, "zen", "New Tab — Zen Browser");
    map_window(&mut f, id, "zen", "Zen Browser");

    assert_snapshot!(placement(&mut f), @r"
    New Tab — Zen Browser: tiled
    Zen Browser: held
    ");

    std::thread::sleep(std::time::Duration::from_millis(300));
    f.double_roundtrip(id);

    assert_snapshot!(placement(&mut f), @r"
    New Tab — Zen Browser: tiled
    Zen Browser: tiled
    ");
}

#[test]
fn a_window_closed_while_held_is_forgotten() {
    let config = ZEN.replace("settle-ms 60000", "settle-ms 100");
    let mut f = fixture(&config);
    let id = f.add_client();

    map_window(&mut f, id, "zen", "New Tab — Zen Browser");
    let popout = map_window(&mut f, id, "zen", "Zen Browser");

    let window = f.client(id).window(&popout);
    window.xdg_toplevel.destroy();
    window.xdg_surface.destroy();
    window.surface.destroy();
    f.double_roundtrip(id);

    // Its timer still fires, and has to find nothing to place.
    std::thread::sleep(std::time::Duration::from_millis(300));
    f.double_roundtrip(id);

    assert_snapshot!(placement(&mut f), @"New Tab — Zen Browser: tiled");
    assert!(f.niri().unmapped_windows.is_empty());
}

#[test]
fn a_window_that_drops_its_buffer_while_held_can_open_again() {
    let config = ZEN.replace("settle-ms 60000", "settle-ms 100");
    let mut f = fixture(&config);
    let id = f.add_client();

    map_window(&mut f, id, "zen", "New Tab — Zen Browser");
    let popout = map_window(&mut f, id, "zen", "Zen Browser");

    // For the client this is an unmap: niri never showed the window, but the client had given
    // it a buffer, so it now starts over with a bufferless commit and waits to be configured
    // again, as the protocol requires.
    let window = f.client(id).window(&popout);
    window.attach_null();
    window.commit();
    f.double_roundtrip(id);

    // Unmapping discards everything the toplevel had said about itself, so it says it again.
    let window = f.client(id).window(&popout);
    window.set_app_id("zen");
    window.set_title("Zen Browser");
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&popout);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    std::thread::sleep(std::time::Duration::from_millis(300));
    f.double_roundtrip(id);

    assert_snapshot!(placement(&mut f), @r"
    New Tab — Zen Browser: tiled
    Zen Browser: tiled
    ");
}

#[test]
fn a_parent_declared_after_the_initial_configure_still_counts() {
    // Some clients name the parent only after their first commit. The rules were resolved at
    // that commit, without it, and have to be resolved again when it arrives or `is-transient`
    // rules miss the window entirely. Checked through open-focused, since that is read from the
    // rules alone; open-floating would hide the bug by asking the toplevel for its parent live.
    let config = r##"
        window-rule {
            match is-transient=true
            open-focused false
        }
    "##;
    let mut f = fixture(config);
    let id = f.add_client();

    let main = map_window(&mut f, id, "app", "main");

    let window = f.client(id).create_window();
    let dialog = window.surface.clone();
    window.set_app_id("app");
    window.set_title("dialog");
    window.commit();
    f.roundtrip(id);

    let parent = f.client(id).window(&main).xdg_toplevel.clone();
    f.client(id).window(&dialog).set_parent(Some(&parent));
    f.roundtrip(id);

    let window = f.client(id).window(&dialog);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let focused = f
        .niri()
        .layout
        .focus()
        .map(|mapped| with_toplevel_role(mapped.toplevel(), |role| role.title.clone()));
    assert_eq!(focused, Some(Some(String::from("main"))));
}

#[test]
fn a_popout_floats_at_the_size_it_chose_not_at_a_tiles() {
    let mut f = fixture(ZEN);
    let id = f.add_client();

    map_window(&mut f, id, "zen", "New Tab — Zen Browser");
    let (popout, drawn_at) = map_window_drawn(&mut f, id, "zen", "Zen Browser");

    let before = f.client(id).window(&popout).configures_received.len();
    f.client(id)
        .window(&popout)
        .set_title("Extension: (Bitwarden Password Manager)");
    f.double_roundtrip(id);

    // Answer anything niri sent in the meantime the way a toolkit does: at once, keeping its
    // size where it was left to choose.
    let window = f.client(id).window(&popout);
    if window.configures_received.len() > before {
        let asked = window.configures_received.last().unwrap().1.size;
        let (w, h) = toolkit_size(asked, drawn_at);
        window.set_size(w, h);
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }

    assert_eq!(
        size_of(&mut f, "Extension: (Bitwarden Password Manager)"),
        Some((100, 100))
    );
}
