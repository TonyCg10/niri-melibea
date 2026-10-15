use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Context as _;
use glam::{Mat3, Vec2};
use smithay::backend::renderer::element::utils::{
    Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::backend::renderer::element::{Element as _, Kind, RenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, Uniform};
use smithay::backend::renderer::Texture;
use smithay::utils::{Logical, Point, Rectangle, Scale, Size};

use crate::animation::Animation;
use crate::niri_render_elements;
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenData, OffscreenRenderElement};
use crate::render_helpers::shader_element::ShaderRenderElement;
use crate::render_helpers::shaders::{mat3_uniform, ProgramType, Shaders};

#[derive(Debug)]
pub struct OpenAnimation {
    anim: Animation,

    /// Where the window should travel from, in the same space as the render `location`.
    ///
    /// `None` keeps niri's ordinary in-place open visual. `Some` starts the window at a
    /// shell-owned bubble rectangle and grows it into its real tile geometry.
    travel_from: Option<Rectangle<f64, Logical>>,
    random_seed: f32,
    buffer: OffscreenBuffer,
}

niri_render_elements! {
    OpeningWindowRenderElement => {
        Offscreen = RelocateRenderElement<RescaleRenderElement<OffscreenRenderElement>>,
        Shader = ShaderRenderElement,
    }
}

impl OpenAnimation {
    pub fn new(anim: Animation) -> Self {
        Self::with_travel(anim, None)
    }

    /// Creates an open animation that grows out of `travel_from`.
    pub fn with_travel(anim: Animation, travel_from: Option<Rectangle<f64, Logical>>) -> Self {
        Self {
            anim,
            travel_from,
            random_seed: fastrand::f32(),
            buffer: OffscreenBuffer::default(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.anim.is_done()
    }

    /// Whether the window grows out of a bubble elsewhere rather than appearing in place.
    pub fn travels(&self) -> bool {
        self.travel_from.is_some()
    }

    // We can't depend on view_rect here, because the result of window opening can be snapshot and
    // then rendered elsewhere.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        renderer: &mut GlesRenderer,
        elements: &[impl RenderElement<GlesRenderer>],
        geo_size: Size<f64, Logical>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        // The scale rule's factor, which only the travelling path applies itself. See
        // `travel_transform()`.
        render_scale: f64,
    ) -> anyhow::Result<(OpeningWindowRenderElement, OffscreenData)> {
        let progress = self.anim.value();
        let clamped_progress = self.anim.clamped_value().clamp(0., 1.);

        let (elem, _sync_point, mut data) = self
            .buffer
            .render(renderer, scale, elements)
            .context("error rendering to offscreen buffer")?;

        // An anchored restore grows out of the shell's bubble rectangle. Like the anchored close,
        // it skips the open shader, whose effect assumes a window appearing in place.
        if let Some(origin) = self.travel_from {
            let elem = elem.with_alpha(clamped_progress as f32 * alpha);

            let center = geo_size.to_point().downscale(2.);
            let (current_center, factor) =
                travel_transform(origin, geo_size, location, clamped_progress, render_scale);

            let elem = RescaleRenderElement::from_element(
                elem,
                center.to_physical_precise_round(scale),
                factor,
            );

            let location = current_center - center;
            let elem = RelocateRenderElement::from_element(
                elem,
                location.to_physical_precise_round(scale),
                Relocate::Relative,
            );

            return Ok((elem.into(), data));
        }

        if Shaders::get(renderer).program(ProgramType::Open).is_some() {
            // OffscreenBuffer renders with Transform::Normal and the scale that we passed, so we
            // can assume that below.
            let offset = elem.offset();
            let texture = elem.texture();
            let texture_size = elem.logical_size();

            let mut area = Rectangle::new(location + offset, texture_size);

            // Expand the area a bit to allow for more varied effects.
            let mut target_size = area.size.upscale(1.5);
            target_size.w = f64::max(area.size.w + 1000., target_size.w);
            target_size.h = f64::max(area.size.h + 1000., target_size.h);
            let diff = (target_size.to_point() - area.size.to_point()).downscale(2.);
            let diff = diff.to_physical_precise_round(scale).to_logical(scale);
            area.loc -= diff;
            area.size += diff.upscale(2.).to_size();

            let area_loc = Vec2::new(area.loc.x as f32, area.loc.y as f32);
            let area_size = Vec2::new(area.size.w as f32, area.size.h as f32);

            let geo_loc = Vec2::new(location.x as f32, location.y as f32);
            let geo_size = Vec2::new(geo_size.w as f32, geo_size.h as f32);

            let input_to_geo = Mat3::from_scale(area_size / geo_size)
                * Mat3::from_translation((area_loc - geo_loc) / area_size);

            let tex_scale = Vec2::new(scale.x as f32, scale.y as f32);
            let tex_loc = Vec2::new(offset.x as f32, offset.y as f32);
            let tex_size = Vec2::new(texture.width() as f32, texture.height() as f32) / tex_scale;

            let geo_to_tex =
                Mat3::from_translation(-tex_loc / tex_size) * Mat3::from_scale(geo_size / tex_size);

            let elem = ShaderRenderElement::new(
                ProgramType::Open,
                area.size,
                None,
                scale.x as f32,
                alpha,
                Rc::new([
                    mat3_uniform("niri_input_to_geo", input_to_geo),
                    Uniform::new("niri_geo_size", geo_size.to_array()),
                    mat3_uniform("niri_geo_to_tex", geo_to_tex),
                    Uniform::new("niri_progress", progress as f32),
                    Uniform::new("niri_clamped_progress", clamped_progress as f32),
                    Uniform::new("niri_random_seed", self.random_seed),
                ]),
                HashMap::from([(String::from("niri_tex"), texture.clone())]),
                Kind::Unspecified,
            )
            .with_location(area.loc);

            // We're drawing the shader, not the offscreen itself.
            data.id = elem.id().clone();

            return Ok((elem.into(), data));
        }

        let elem = elem.with_alpha(clamped_progress as f32 * alpha);

        let center = geo_size.to_point().downscale(2.);
        let elem = RescaleRenderElement::from_element(
            elem,
            center.to_physical_precise_round(scale),
            (progress / 2. + 0.5).max(0.),
        );

        let elem = RelocateRenderElement::from_element(
            elem,
            location.to_physical_precise_round(scale),
            Relocate::Relative,
        );

        Ok((elem.into(), data))
    }
}

/// Where an anchored restore draws the tile at `progress`: the centre it is drawn about and the
/// factor it is drawn at.
///
/// The factor runs from fitting the bubble to `render_scale` rather than to 1, so the window lands
/// on exactly the size the scale rule draws it at once the animation is over. Shrinking the
/// travelling window about its slot's centre from outside instead would pull the whole path
/// towards the slot.
fn travel_transform(
    from: Rectangle<f64, Logical>,
    geo_size: Size<f64, Logical>,
    location: Point<f64, Logical>,
    progress: f64,
    render_scale: f64,
) -> (Point<f64, Logical>, f64) {
    let end_center = location + geo_size.to_point().downscale(2.);
    let start_center = from.loc + from.size.to_point().downscale(2.);
    let current_center = start_center + (end_center - start_center).upscale(progress);

    let grow = f64::min(from.size.w / geo_size.w, from.size.h / geo_size.h);
    let factor = (grow + (render_scale - grow) * progress).max(0.);

    (current_center, factor)
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    /// The centre and size a tile of `geo_size` covers when drawn at `factor` about `center`.
    fn drawn(
        geo_size: Size<f64, Logical>,
        (center, factor): (Point<f64, Logical>, f64),
    ) -> (Point<f64, Logical>, Size<f64, Logical>) {
        (center, geo_size.upscale(factor))
    }

    fn assert_close(
        (center, size): (Point<f64, Logical>, Size<f64, Logical>),
        expected: (Point<f64, Logical>, Size<f64, Logical>),
        render_scale: f64,
    ) {
        let msg =
            format!("render_scale: {render_scale}, got {center:?} {size:?}, want {expected:?}");
        assert_abs_diff_eq!(center.x, expected.0.x, epsilon = 1e-9);
        assert_abs_diff_eq!(center.y, expected.0.y, epsilon = 1e-9);
        assert!((size.w - expected.1.w).abs() < 1e-9, "{msg}");
        assert!((size.h - expected.1.h).abs() < 1e-9, "{msg}");
    }

    #[test]
    fn travel_runs_from_the_bubble_to_the_shrunk_slot() {
        let bubble = Rectangle::new(Point::new(1000., 900.), Size::from((40., 40.)));
        let bubble_center = Point::new(1020., 920.);
        let geo_size = Size::from((800., 400.));
        let location = Point::new(100., 50.);
        let slot_center = Point::new(500., 250.);

        for render_scale in [1., 0.9, 0.5] {
            let at = |progress| {
                drawn(
                    geo_size,
                    travel_transform(bubble, geo_size, location, progress, render_scale),
                )
            };

            // It starts over the bubble, fitted inside it.
            let bubble_fit = Size::from((40., 20.));
            assert_close(at(0.), (bubble_center, bubble_fit), render_scale);

            // And ends where the tile is drawn once the animation is over: its slot, shrunk by
            // the scale rule about the slot's centre.
            let shrunk = geo_size.upscale(render_scale);
            assert_close(at(1.), (slot_center, shrunk), render_scale);
        }
    }
}
