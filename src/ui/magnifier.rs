//! A screen magnifier, drawn by the compositor rather than by a client.
//!
//! This has to be here rather than in a program of its own. A client could copy the screen
//! sixty times a second and redraw it enlarged in a window, but it would trail a frame behind
//! what it is magnifying, capture itself into a feedback loop, pay a full read-back every
//! frame, and take part in layout and focus like any other window. The compositor already has
//! the finished picture, so magnifying is a transform rather than a copy.
//!
//! The output's elements are rendered once into an offscreen texture and that single texture
//! is scaled about the pointer. Scaling each element separately is not an option — the render
//! element type would have to contain itself — and this way the picture magnifies whole:
//! windows, panels and cursor alike, which is the point. A magnifier that left the shell's own
//! bar at its original size would be no use to the person who needed it.
//!
//! The pointer stays under the same spot on the glass as the world grows around it, which is
//! how a magnifying glass behaves and means the mouse never jumps when zooming.

use anyhow::Context as _;
use smithay::backend::renderer::element::utils::RescaleRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Scale, Size};

use crate::animation::{Animation, Clock};
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenRenderElement};
use smithay::backend::renderer::element::RenderElement;

/// The scaled picture, ready to be pushed as the output's only element.
pub type MagnifiedRenderElement = RescaleRenderElement<OffscreenRenderElement>;

#[derive(Debug)]
pub struct Magnifier {
    /// Where the level is now, animating towards `target`.
    anim: Option<Animation>,
    /// The level the last request asked for. 1.0 is unmagnified.
    target: f64,
    /// The level being rendered, kept so an interrupted animation continues from it.
    current: f64,
    clock: Clock,
    config: niri_config::Magnifier,
    buffer: OffscreenBuffer,
}

impl Magnifier {
    pub fn new(clock: Clock, config: niri_config::Magnifier) -> Self {
        Self {
            anim: None,
            target: 1.,
            current: 1.,
            clock,
            config,
            buffer: OffscreenBuffer::default(),
        }
    }

    pub fn update_config(&mut self, config: niri_config::Magnifier) {
        self.config = config;
    }

    /// The level being rendered this frame.
    pub fn level(&self) -> f64 {
        match &self.anim {
            Some(anim) => anim.value(),
            None => self.current,
        }
    }

    /// Whether anything is magnified, allowing for the float never landing exactly on 1.
    pub fn is_active(&self) -> bool {
        !self.config.off && self.level() > 1.001
    }

    fn animate_to(&mut self, target: f64) {
        let from = self.level();
        self.current = target;
        self.target = target;
        if (from - target).abs() < f64::EPSILON {
            self.anim = None;
            return;
        }
        self.anim = Some(Animation::new(
            self.clock.clone(),
            from,
            target,
            0.,
            self.config.animation.0,
        ));
    }

    pub fn zoom_in(&mut self) {
        let target = (self.target * self.config.step.0).min(self.config.max.0);
        self.animate_to(target);
    }

    pub fn zoom_out(&mut self) {
        // Never below 1: magnification is a magnifier, and shrinking the desktop below its own
        // size would leave undefined space around it with nothing to put there.
        let target = (self.target / self.config.step.0).max(1.);
        self.animate_to(target);
    }

    pub fn reset(&mut self) {
        self.animate_to(1.);
    }

    pub fn advance_animations(&mut self) {
        if let Some(anim) = &self.anim {
            if anim.is_done() {
                self.current = self.target;
                self.anim = None;
            }
        }
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.anim.as_ref().is_some_and(|anim| !anim.is_done())
    }

    /// Renders `elements` into a texture and returns it scaled about `origin`.
    ///
    /// `origin` is the pointer in output-local logical coordinates: the one point that must not
    /// move while the picture grows around it.
    /// Renders `elements` into a texture and returns the magnified view of them.
    ///
    /// The view is a window of the output, `level` times smaller than the output itself,
    /// centred on the pointer and **clamped to the output's bounds**. Clamping is what makes
    /// the corners reachable: scaling about the pointer instead would put the extreme edges
    /// one pointer-pixel from the screen edge, so the top row and the outer columns could
    /// never actually be brought into view.
    ///
    /// It also means the magnifier never shows anything outside the output, so there is no
    /// empty margin to explain away at the edges.
    pub fn magnify<E: RenderElement<GlesRenderer>>(
        &self,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
        pointer: Point<f64, Logical>,
        output_size: Size<f64, Logical>,
        elements: &[E],
    ) -> anyhow::Result<MagnifiedRenderElement> {
        let level = self.level();

        let (elem, _sync, _data) = self
            .buffer
            .render(renderer, scale, elements)
            .context("error rendering the magnifier's offscreen")?;

        // What part of the output the view shows, and where it starts.
        let view: Size<f64, Logical> = Size::from((output_size.w / level, output_size.h / level));
        let top_left = Point::from((
            (pointer.x - view.w / 2.).clamp(0., (output_size.w - view.w).max(0.)),
            (pointer.y - view.h / 2.).clamp(0., (output_size.h - view.h).max(0.)),
        ));

        // The offscreen sits at the origin of whatever it encompassed, so its own offset is
        // kept and the view's corner subtracted from it. Scaling then happens about the
        // output's corner rather than about the pointer, which is what the clamp above assumes.
        let offset = elem.offset();
        let elem = elem.with_offset(offset - top_left);

        Ok(RescaleRenderElement::from_element(
            elem,
            Point::from((0, 0)),
            level,
        ))
    }
}
