//! Soft side lights that say a binding mode is active.
//!
//! Modes that persist until you leave them need something on screen saying so, or the same
//! key quietly means two different things depending on state nobody can see. A notification
//! is the wrong shape for that: it announces once, competes for attention, and then leaves
//! exactly when the information is still true.
//!
//! So this is a state indicator rather than a message. Two lit arcs hug the left and right
//! edges, centred vertically, fading inward the way a shadow does. They appear when a mode is
//! entered, stay for as long as it lasts, and fade out on the way back. Nothing appears where
//! windows live, and there is no text to read.
//!
//! It is drawn by the compositor rather than published for a shell to render, so it works the
//! same with Noctalia, with Celestina, or with no shell at all.

use std::rc::Rc;

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::Uniform;
use smithay::output::Output;
use smithay::utils::Point;

use crate::animation::{Animation, Clock};
use crate::niri_render_elements;
use crate::render_helpers::shader_element::ShaderRenderElement;
use crate::render_helpers::shaders::ProgramType;
use crate::render_helpers::RenderTarget;
use crate::utils::output_size;

niri_render_elements! {
    BindingModeIndicatorRenderElement => {
        Glow = ShaderRenderElement,
    }
}

#[derive(Debug)]
enum State {
    Hidden,
    /// Fading in or out. The animation runs 0 to 1 in both directions; `to` says which.
    Fading { anim: Animation, to: f64 },
    Shown,
}

#[derive(Debug)]
pub struct BindingModeIndicator {
    state: State,
    clock: Clock,
    config: niri_config::BindingModeIndicator,
}

impl BindingModeIndicator {
    pub fn new(clock: Clock, config: niri_config::BindingModeIndicator) -> Self {
        Self {
            state: State::Hidden,
            clock,
            config,
        }
    }

    pub fn update_config(&mut self, config: niri_config::BindingModeIndicator) {
        self.config = config;
    }

    /// Whether a mode is active, driving the fade in either direction.
    pub fn set_active(&mut self, active: bool) {
        // Start from whatever is on screen, so entering a mode while the previous one is still
        // fading out continues from that brightness instead of restarting from nothing.
        let current = self.value();
        let target = if active { 1. } else { 0. };
        if (current - target).abs() < f64::EPSILON {
            self.state = if active { State::Shown } else { State::Hidden };
            return;
        }

        self.state = State::Fading {
            anim: Animation::new(self.clock.clone(), current, target, 0., self.config.animation.0),
            to: target,
        };
    }

    /// How lit the glow is right now, in `0..=1`.
    fn value(&self) -> f64 {
        match &self.state {
            State::Hidden => 0.,
            State::Shown => 1.,
            State::Fading { anim, .. } => anim.clamped_value().clamp(0., 1.),
        }
    }

    pub fn advance_animations(&mut self) {
        if let State::Fading { anim, to } = &self.state {
            if anim.is_done() {
                self.state = if *to == 1. { State::Shown } else { State::Hidden };
            }
        }
    }

    pub fn are_animations_ongoing(&self) -> bool {
        matches!(&self.state, State::Fading { anim, .. } if !anim.is_done())
    }

    pub fn render(
        &self,
        output: &Output,
        target: RenderTarget,
    ) -> Option<BindingModeIndicatorRenderElement> {
        if self.config.off {
            return None;
        }

        // Deliberately absent from screenshots and screen recordings. The glow describes the
        // state of this session's keyboard, which is not part of what the screen is showing.
        if target != RenderTarget::Output {
            return None;
        }

        let value = self.value();
        if value <= 0. {
            return None;
        }

        let size = output_size(output);
        let scale = output.current_scale().fractional_scale();

        let color = self.config.color.to_array_premul();
        let uniforms = Rc::from(
            [
                Uniform::new("glow_color", color),
                Uniform::new("reach", self.config.reach.0 as f32),
                // Written as a proportion of the output's height so one setting looks the same
                // on a 4K panel and a 1080p one side by side.
                Uniform::new(
                    "spread",
                    (size.h * self.config.spread.0 * 0.5) as f32,
                ),
            ]
            .as_slice(),
        );

        let mut elem = ShaderRenderElement::empty(ProgramType::BindingModeGlow, Kind::Unspecified);
        elem.update(
            size,
            None,
            scale as f32,
            (value * self.config.opacity.0) as f32,
            uniforms,
            Default::default(),
        );
        let elem = elem.with_location(Point::from((0., 0.)));

        Some(BindingModeIndicatorRenderElement::Glow(elem))
    }
}
