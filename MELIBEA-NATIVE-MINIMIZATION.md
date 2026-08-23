# Melibea native minimization spike

This branch is a disposable GPL-3.0-or-later niri experiment for Melibea. It is
based on niri 26.04 commit `8ed0da44d974c32c6877d2f4630c314da0717ecb` and is
kept separate from the MIT Melibea repository.

Do not install this build as the main compositor yet. Test it nested first.

## Proven behavior

- `Layout` can retain a removed `Tile<Mapped>` outside every workspace.
- Normal window iteration, rendering, focus and overview do not see the held
  tile.
- Surface lookup and commit handling still find the mapped window.
- Restore prefers the original workspace and falls back to the active workspace
  when a dynamic workspace disappeared.
- Restore returns to the original output only while that output and workspace
  still exist; after output removal it deterministically uses the active output
  and workspace.
- Restore retains nearest-first relationships rather than relying on fragile
  absolute geometry. Tiled windows prefer their surviving original column and
  vertical neighbors, then neighboring columns; floating windows recover their
  stack relation. A raw index is only the final fallback.
- Tiled/floating state, width and full-width state survive the held period.
- Fullscreen state survives minimize and restore.
- Client exit removes minimized state without residue.
- A root toplevel and its separately mapped transient descendants are retained
  as one family and exposed as one public minimized item. Minimizing any family
  member retains the root and full descendant subtree.
- A transient mapped while its parent is minimized joins the existing family
  before it can render. Parent changes deterministically split a detached
  subtree into its own family or merge it into another minimized family.
- Restore returns the family root first and its descendants afterwards. If the
  root exits while minimized, a surviving member is promoted without losing
  the family.
- Targeted close of a public minimized item sends close to the full family.
- Minimizing a family stops fixed screencasts targeting every member and clears
  dynamic casts through niri's existing target-stop path. Minimized windows are
  absent from both window-cast lookup iterators.
- Targeted minimize, restore, and close requests return a semantic
  `WindowActionResult` instead of the request-only `Handled` acknowledgement.
  Minimize and restore report `Applied`, `AlreadyInRequestedState`,
  `WindowNotFound`, or `Blocked`; close reports `CloseRequested` because final
  surface destruction remains client-controlled and must be observed through
  the authoritative event snapshot. A minimized transient resolves to its
  public family-root id.
- Minimization snapshots every visible member of the resolved transient family
  and starts niri's existing close visual before the tiles leave their
  workspaces. Restore first reestablishes authoritative layout state and then
  starts the existing open visual for every restored member. Both directions
  are compositor-local and use the configured niri animation timing; neither
  delays or owns the minimized lifecycle.

Experimental actions:

```sh
niri msg action minimize-window
niri msg action minimize-window --id WINDOW_ID
niri msg action restore-window --id WINDOW_ID
```

Experimental state IPC:

```sh
niri msg minimized-windows
niri msg --json minimized-windows
niri msg --json event-stream
```

`MinimizedWindows` returns a dedicated ordered record with no fabricated
workspace or layout geometry. `MinimizedWindowsChanged` sends a complete
replacement snapshot on event-stream connection and after every material
change.

For the three lifecycle actions, `niri msg --json action ...` returns the
semantic payload directly. For example:

```json
{"requested_id":42,"window_id":42,"status":"Applied"}
```

## Nested evidence

On 2026-08-15, a nested winit niri opened Kitty id 2. After minimize,
`niri msg -j windows` returned `[]` while the Kitty PID remained alive. Restore
returned the same id, metadata and focus. A second minimize followed by
targeted close terminated Kitty, removed the held state and left the nested
compositor healthy.

A second nested cycle live-validated the state bridge. Initially both
`minimized-windows` and Melibea's read-only `minimized` diagnostic were empty.
After minimizing Kitty id 2, visible `windows` returned `[]`, the dedicated
query returned the same id, PID, app id and title, and Melibea exposed it as
`bubble=0`. The Kitty PID remained alive. Restore returned id 2 to workspace 1
and both minimized projections became empty. While Melibea held one persistent
event-stream connection, the same cycle advanced its registry from revision 1
with one bubble to revision 2 with none.

A third nested cycle exercised multiple surfaces entirely through Melibea's
minimal wire client. Kitty ids 2 and 3 were minimized in that order and exposed
as bubbles 0 and 1. Restoring id 2 left id 3 as bubble 0 and returned only id 2
to the visible layout. Targeted close then terminated id 3 while it remained
minimized and left the registry empty. Minimizing the focused window without an
explicit id also passed. The equivalent layout order and independent-removal
sequence is covered by a unit test.

On 2026-08-17, the combined Celestina plus Melibea nested build was explicitly
shut down while an editor remained minimized. The nested socket disappeared,
the retained client exited, niri returned success, and the host niri and
Melibea service remained healthy. Restarting the nest began with an empty
minimized snapshot and restored Celestina's layer surfaces.

Later on 2026-08-17, the current debug build was tested in a disposable winit
niri nested inside the real Celestina session. A GTK 4 client mapped root id 2
and transient child id 3. Minimizing the child returned `Applied` with public
family id 2, removed both from visible `Windows`, and produced exactly one
`MinimizedWindow`; Melibea projected it as bubble 0. Repeating minimize returned
`AlreadyInRequestedState`, an invalid restore returned `WindowNotFound`, restore
through Melibea returned both root and child, and closing the minimized family
returned `CloseRequested` before the event stream removed it.

A second GTK cycle minimized root id 4 before creating transient child id 5.
The late child joined the retained family without entering visible `Windows`;
restoring id 4 revealed both toplevels. Locking only the nested compositor with
swaylock made minimize return `Blocked` and left both windows visible. The
temporary lock, client, event reader, compositor and socket were then removed;
the real niri and Melibea processes remained healthy. The deployed real-session
niri had the same modified version label but an older binary hash and rejected
the new requests, so no current-protocol result is attributed to that stale
process.

Later in the same disposable nest, animations were slowed by a factor of five
to exercise the new transition path. A GTK window id 2 minimized and restored
with `Applied` responses while moving atomically between visible `Windows` and
the native minimized snapshot. An immediate minimize followed by restore also
ended with id 2 visible and no minimized residue, showing that animation
completion does not gate lifecycle commands. The same cycle passed after the
window entered fullscreen: its id and `942 x 1016` fullscreen geometry survived
minimize and restore. The temporary client, compositor, config, and socket were
removed afterwards; the real Celestina niri and Melibea processes remained
alive.

The next disposable nest exercised Melibea protocol v1 rather than direct niri
commands. A persistent subscriber first received empty snapshot revision 1.
Minimizing GTK id 2 through Melibea returned `applied` and emitted an ordered
add at revision 2; restore emitted removal at revision 3; a second minimize
emitted revision 4; targeted close returned `close_requested` before the
authoritative removal at revision 5 and final client exit. The daemon, client,
socket, test compositor, and temporary configuration were then removed without
touching the real session.

## Known gaps

- Normal `Windows` IPC reports minimize and restore as close and open. Native
  consumers must use the dedicated minimized query and event snapshot.
- The current minimize/restore visual is compositor-local and deliberately
  reuses niri's generic close/open effects. A geometry-aware trajectory to and
  from Celestina's real bubble anchor remains pending until the versioned shell
  contract supplies that anchor; niri must not guess a shell coordinate.
- Celestina does not yet render the protocol-v1 projection as bubbles. That is
  presentation work in the separate shell repository, not missing compositor
  lifecycle authority.
- Popup surfaces render only through their retained parent tile, so they
  disappear with it. The separately mapped transient-family policy is now
  covered by layout tests and a nested live GTK probe.
- Output removal and fullscreen are layout-tested but still need optional live
  multi-output/fullscreen confirmation. The one-output winit nest cannot
  visually exercise output removal.

## Validation

```sh
cargo check --all-targets
cargo test -p niri-ipc
cargo test -p niri-ipc window_action_result_has_explicit_wire_status
cargo test minimized_window --lib
cargo test multiple_minimized_windows_keep_order_and_restore_independently --lib
cargo test restore_falls_back --lib
cargo test restore_returns_to_original_output_when_it_still_exists --lib
cargo test restore_falls_back_to_active_output_when_original_output_was_removed --lib
cargo test minimize_restore_preserves_fullscreen_state --lib
cargo test restore_reinserts_tiled_window --lib
cargo test restore_uses_closest_surviving_original_column_neighbor --lib
cargo test restore_preserves_floating_stack_relation --lib
cargo test client_close_removes_minimized_state --lib
cargo test minimizing_transient_child_holds_one_root_family --lib
cargo test new_transient_joins_minimized_parent_family --lib
cargo test closing_minimized_family_root_promotes_descendant --lib
cargo test closing_minimized_descendant_keeps_family_root --lib
cargo test minimized_transient_reparenting_splits_and_merges_families --lib
cargo test -p niri-config converts_experimental_minimization_actions_from_ipc
cargo test --lib
```

With Rust 1.97, strict clippy on the unmodified 26.04 base has existing
`useless_borrows_in_formatting` and `for_kv_map` failures. The spike is clean
when only those inherited lints are allowed:

```sh
cargo clippy --all-targets -- -D warnings \
  -A clippy::useless_borrows_in_formatting \
  -A clippy::for_kv_map
```
