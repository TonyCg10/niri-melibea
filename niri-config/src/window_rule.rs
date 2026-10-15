use niri_ipc::ColumnDisplay;

use crate::appearance::{
    BackgroundEffect, BackgroundEffectRule, BlockOutFrom, BorderRule, CornerRadius, ShadowRule,
    TabIndicatorRule,
};
use crate::layout::DefaultPresetSize;
use crate::utils::{MergeWith, RegexEq};
use crate::FloatOrInt;

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowRule {
    #[knuffel(children(name = "match"))]
    pub matches: Vec<Match>,
    #[knuffel(children(name = "exclude"))]
    pub excludes: Vec<Match>,

    // Rules applied at initial configure.
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child)]
    pub default_window_height: Option<DefaultPresetSize>,
    #[knuffel(child, unwrap(argument))]
    pub open_on_output: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_on_workspace: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized_to_edges: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_fullscreen: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_floating: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_focused: Option<bool>,
    /// How long to keep a new window off screen while it finishes saying what it is.
    ///
    /// Some windows only become recognizable after they map: a browser extension's popout maps
    /// under the browser's generic title and renames itself a few frames later. Placed at once,
    /// it lands in the strip under the rules for an ordinary browser window, and moves after.
    ///
    /// With this set, the window waits instead, drawn nowhere. It is placed as soon as its rules
    /// change — the late title arriving — or when the time runs out, whichever comes first, so
    /// it opens once, in the right place, at the right size.
    #[knuffel(child, unwrap(argument))]
    pub settle_ms: Option<u16>,

    // Rules applied dynamically.
    /// Whether the window belongs in the floating layer.
    ///
    /// Unlike `open_floating`, this is resolved every time the rules are, so it still catches a
    /// window that only becomes recognizable after it maps — one that opens under a generic
    /// title and renames itself a moment later, say. The window moves layer when the answer
    /// changes; leaving this unset keeps hands off, so a manual toggle stays put.
    #[knuffel(child, unwrap(argument))]
    pub floating: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub min_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub min_height: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_height: Option<u16>,

    #[knuffel(child, default)]
    pub focus_ring: BorderRule,
    #[knuffel(child, default)]
    pub border: BorderRule,
    #[knuffel(child, default)]
    pub shadow: ShadowRule,
    #[knuffel(child, default)]
    pub tab_indicator: TabIndicatorRule,
    #[knuffel(child, unwrap(argument))]
    pub draw_border_with_background: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child, unwrap(argument))]
    pub scale: Option<FloatOrInt<0, 1>>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, unwrap(argument))]
    pub clip_to_geometry: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub baba_is_float: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub block_out_from: Option<BlockOutFrom>,
    #[knuffel(child, unwrap(argument))]
    pub variable_refresh_rate: Option<bool>,
    #[knuffel(child, unwrap(argument, str))]
    pub default_column_display: Option<ColumnDisplay>,
    #[knuffel(child)]
    pub default_floating_position: Option<FloatingPosition>,
    #[knuffel(child, unwrap(argument))]
    pub scroll_factor: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub tiled_state: Option<bool>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
    #[knuffel(child, default)]
    pub popups: PopupsRule,
}

/// Rules for popup surfaces.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct PopupsRule {
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
}

/// Resolved popup-specific rules.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ResolvedPopupsRules {
    /// Extra opacity to draw popups with.
    pub opacity: Option<f32>,

    /// Corner radius to assume the popups have.
    pub geometry_corner_radius: Option<CornerRadius>,

    /// Background effect configuration for popups.
    pub background_effect: BackgroundEffect,
}

impl MergeWith<PopupsRule> for ResolvedPopupsRules {
    fn merge_with(&mut self, part: &PopupsRule) {
        if let Some(x) = part.opacity {
            self.opacity = Some(x);
        }
        if let Some(x) = part.geometry_corner_radius {
            self.geometry_corner_radius = Some(x);
        }
        self.background_effect.merge_with(&part.background_effect);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Match {
    #[knuffel(property, str)]
    pub app_id: Option<RegexEq>,
    #[knuffel(property, str)]
    pub title: Option<RegexEq>,
    #[knuffel(property)]
    pub is_active: Option<bool>,
    #[knuffel(property)]
    pub is_focused: Option<bool>,
    #[knuffel(property)]
    pub is_active_in_column: Option<bool>,
    #[knuffel(property)]
    pub is_floating: Option<bool>,
    #[knuffel(property)]
    pub is_window_cast_target: Option<bool>,
    #[knuffel(property)]
    pub is_urgent: Option<bool>,
    /// Whether this window was opened by another one: a dialog, a file chooser, a preferences
    /// window. Matched from the toplevel's parent, so it is known before the window maps and
    /// can therefore decide how it opens.
    ///
    /// Note that a program opening a second ordinary window — a browser's "open in window",
    /// say — is not a child in this sense. It has no parent toplevel, and nothing about it
    /// distinguishes it from the first window except its title.
    #[knuffel(property)]
    pub is_transient: Option<bool>,
    /// Whether the program that opened this window already had a window on screen when it did.
    ///
    /// This is the child-window test for programs that never declare a parent: a browser
    /// extension's popout, most of Steam. They cannot hide which process asked for the window,
    /// and a process asking for a second one is, far more often than not, asking for a dialog.
    ///
    /// It is decided once, when the window opens, and does not change after: closing the first
    /// window does not turn the dialog into a main window.
    ///
    /// The cost is that a deliberate second main window — a new browser window, a second editor
    /// window — is secondary too. Those are the exceptions to write down, and there are far
    /// fewer of them than there are programs with dialogs.
    #[knuffel(property)]
    pub is_secondary: Option<bool>,
    #[knuffel(property)]
    pub at_startup: Option<bool>,
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct FloatingPosition {
    #[knuffel(property)]
    pub x: FloatOrInt<-65535, 65535>,
    #[knuffel(property)]
    pub y: FloatOrInt<-65535, 65535>,
    #[knuffel(property, default)]
    pub relative_to: RelativeTo,
}

#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTo {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}
