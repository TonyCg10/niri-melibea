//! Resolution of action-scoped window transition hints.
//!
//! A shell that draws minimized-window bubbles knows where a bubble is, but it owns neither
//! surfaces nor layout. Melibea forwards that rectangle to niri as an ephemeral, action-scoped
//! hint. This module turns such a hint into something the layout can animate toward, or rejects
//! it in favour of the ordinary compositor-local transition.
//!
//! Three rules keep this safe:
//!
//! - The hint is never stored. It is resolved against live state at the moment the action is
//!   handled, so it cannot go stale after a shell crash.
//! - Resolution never fails an action. An unusable anchor degrades to [`ResolvedTransition::
//!   Generic`]; authoritative minimized state changes either way.
//! - The shell supplies output-local logical coordinates only. niri retains output topology,
//!   transforms, scale, clipping, and conversion into its own render spaces.

use niri_ipc::{BubbleAnchor, WindowTransition};
use smithay::utils::{Logical, Point, Rectangle, Size};

/// How the layout should present one already-committed lifecycle change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedTransition {
    /// Travel between the window geometry and a rectangle in workspace-local logical coordinates.
    Anchored(Rectangle<f64, Logical>),
    /// Commit the state change with no spatial, scale, or opacity animation.
    Disabled,
    /// Use the ordinary compositor-local transition.
    Generic,
}

impl ResolvedTransition {
    /// The anchor rectangle, when this transition travels to or from one.
    pub fn anchor(self) -> Option<Rectangle<f64, Logical>> {
        match self {
            Self::Anchored(rect) => Some(rect),
            Self::Disabled | Self::Generic => None,
        }
    }

    /// Whether all motion is suppressed for this action.
    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }
}

/// Live output facts an anchor is resolved against.
///
/// These are read at action time rather than remembered, because an output can be reconfigured or
/// unplugged between a shell publishing a bubble and niri handling the action.
#[derive(Debug, Clone, Copy)]
pub struct OutputContext<'a> {
    /// Name of the output the affected window currently occupies.
    pub window_output: &'a str,
    /// Logical size of that output.
    pub output_size: Size<f64, Logical>,
    /// Where the target workspace renders inside that output.
    ///
    /// This already accounts for overview zoom and workspace-switch offset, so an anchor stays
    /// correct while the overview is open or a workspace switch is animating.
    pub workspace_geo: Rectangle<f64, Logical>,
}

/// Resolves one transition hint against live compositor state.
///
/// Returns [`ResolvedTransition::Generic`] whenever the hint cannot be honoured. That is a normal
/// outcome, not an error: the caller has already committed, or is about to commit, the
/// authoritative state change.
pub fn resolve(
    transition: Option<&WindowTransition>,
    context: OutputContext<'_>,
) -> ResolvedTransition {
    match transition {
        None => ResolvedTransition::Generic,
        Some(WindowTransition::Disabled) => ResolvedTransition::Disabled,
        Some(WindowTransition::Anchored { anchor }) => resolve_anchor(anchor, context)
            .map_or(ResolvedTransition::Generic, ResolvedTransition::Anchored),
    }
}

/// Projects an output-local anchor into workspace-local logical coordinates.
///
/// `None` means the anchor is unusable and the caller must fall back to generic motion.
fn resolve_anchor(
    anchor: &BubbleAnchor,
    context: OutputContext<'_>,
) -> Option<Rectangle<f64, Logical>> {
    if !anchor.is_well_formed() {
        return None;
    }

    // The first implementation deliberately refuses cross-output travel. A bubble on one output
    // and a window on another describe a trajectory that would have to cross an unspecified
    // topology, so this degrades rather than guessing.
    if anchor.output != context.window_output {
        return None;
    }

    let output = Rectangle::from_size(context.output_size);
    if output.size.w <= 0. || output.size.h <= 0. {
        return None;
    }

    let rect = Rectangle::new(
        Point::from((anchor.x, anchor.y)),
        Size::from((anchor.width, anchor.height)),
    );

    // A hot-unplugged or reconfigured output can leave a shell describing a rectangle that no
    // longer exists. Require real overlap rather than exact containment, so a bubble sitting
    // partly past the edge still animates.
    rect.intersection(output)?;

    // Reproject into the workspace's own space. `workspace_geo` carries overview zoom and any
    // in-flight workspace switch, so this stays correct in both.
    Some(Rectangle::new(
        rect.loc - context.workspace_geo.loc,
        rect.size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor() -> BubbleAnchor {
        BubbleAnchor {
            output: String::from("DP-1"),
            x: 1874.,
            y: 9.,
            width: 22.,
            height: 22.,
        }
    }

    fn context() -> OutputContext<'static> {
        OutputContext {
            window_output: "DP-1",
            output_size: Size::from((1920., 1080.)),
            workspace_geo: Rectangle::from_size(Size::from((1920., 1080.))),
        }
    }

    #[test]
    fn an_omitted_transition_asks_for_generic_motion() {
        assert_eq!(resolve(None, context()), ResolvedTransition::Generic);
    }

    #[test]
    fn disabled_motion_survives_resolution() {
        assert_eq!(
            resolve(Some(&WindowTransition::Disabled), context()),
            ResolvedTransition::Disabled
        );
    }

    #[test]
    fn a_usable_anchor_resolves_to_workspace_local_coordinates() {
        let transition = WindowTransition::Anchored { anchor: anchor() };
        let resolved = resolve(Some(&transition), context());
        assert_eq!(
            resolved,
            ResolvedTransition::Anchored(Rectangle::new(
                Point::from((1874., 9.)),
                Size::from((22., 22.))
            ))
        );
    }

    #[test]
    fn a_workspace_offset_is_subtracted_from_the_anchor() {
        // A workspace switch or the overview shifts where the workspace renders inside the
        // output. The anchor is still output-local, so the offset must come back out.
        let transition = WindowTransition::Anchored { anchor: anchor() };
        let context = OutputContext {
            workspace_geo: Rectangle::new(Point::from((0., -1080.)), Size::from((1920., 1080.))),
            ..context()
        };
        assert_eq!(
            resolve(Some(&transition), context),
            ResolvedTransition::Anchored(Rectangle::new(
                Point::from((1874., 1089.)),
                Size::from((22., 22.))
            ))
        );
    }

    #[test]
    fn a_different_output_degrades_to_generic_motion() {
        let transition = WindowTransition::Anchored {
            anchor: BubbleAnchor {
                output: String::from("HDMI-A-1"),
                ..anchor()
            },
        };
        assert_eq!(
            resolve(Some(&transition), context()),
            ResolvedTransition::Generic
        );
    }

    #[test]
    fn a_malformed_anchor_degrades_to_generic_motion() {
        for bad in [
            BubbleAnchor {
                width: 0.,
                ..anchor()
            },
            BubbleAnchor {
                height: f64::NAN,
                ..anchor()
            },
            BubbleAnchor {
                x: f64::INFINITY,
                ..anchor()
            },
            BubbleAnchor {
                output: String::new(),
                ..anchor()
            },
        ] {
            let transition = WindowTransition::Anchored { anchor: bad };
            assert_eq!(
                resolve(Some(&transition), context()),
                ResolvedTransition::Generic
            );
        }
    }

    #[test]
    fn an_anchor_entirely_outside_the_output_degrades_to_generic_motion() {
        // This is the hot-unplug and stale-geometry case: the output shrank, so the rectangle the
        // shell described no longer exists anywhere on it.
        let transition = WindowTransition::Anchored {
            anchor: BubbleAnchor {
                x: 4000.,
                ..anchor()
            },
        };
        assert_eq!(
            resolve(Some(&transition), context()),
            ResolvedTransition::Generic
        );
    }

    #[test]
    fn a_partly_offscreen_anchor_still_animates() {
        // A bubble hugging the right edge may extend past it. That is ordinary shell layout, not
        // a stale anchor, so it must not degrade.
        let transition = WindowTransition::Anchored {
            anchor: BubbleAnchor {
                x: 1910.,
                ..anchor()
            },
        };
        assert_eq!(
            resolve(Some(&transition), context()),
            ResolvedTransition::Anchored(Rectangle::new(
                Point::from((1910., 9.)),
                Size::from((22., 22.))
            ))
        );
    }

    #[test]
    fn a_degenerate_output_degrades_to_generic_motion() {
        let transition = WindowTransition::Anchored { anchor: anchor() };
        let context = OutputContext {
            output_size: Size::from((0., 0.)),
            ..context()
        };
        assert_eq!(
            resolve(Some(&transition), context),
            ResolvedTransition::Generic
        );
    }
}
