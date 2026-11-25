//! Drawing of a suspended window: its last frame, blurred.
//!
//! A suspended client is frozen and never commits, so the blurred texture is kept and reused
//! until the window's offscreen damage or texture, its size, the output scale or the blur options
//! change.

use std::cell::RefCell;

use anyhow::Context as _;
use smithay::backend::renderer::element::{Element as _, Kind, RenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{ContextId, Offscreen as _, Renderer as _, Texture as _};
use smithay::utils::{Buffer, Logical, Point, Rectangle, Scale, Size, Transform};

use crate::niri_render_elements;
use crate::render_helpers::blur::{Blur, BlurOptions};
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenData, OffscreenRenderElement};
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};

niri_render_elements! {
    SuspendedRenderElement => {
        Blurred = PrimaryGpuTextureRenderElement,
        Unblurred = OffscreenRenderElement,
    }
}

/// The last frame of a suspended window, rendered offscreen and blurred.
///
/// Owned by the tile. GPU resources are created on the first render and released by
/// [`SuspendedSnapshot::clear`].
#[derive(Debug, Default)]
pub struct SuspendedSnapshot {
    inner: RefCell<Option<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Where the window's elements get rendered.
    offscreen: OffscreenBuffer,
    /// Blurring program and its intermediate textures, if available.
    blur: Option<Blur>,
    /// The last blurred texture and what it was blurred from.
    cached: Option<Cached>,
}

#[derive(Debug)]
struct Cached {
    key: BlurKey,
    renderer_context_id: ContextId<GlesTexture>,
    buffer: TextureBuffer<GlesTexture>,
}

/// Everything a blurred texture depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BlurKey {
    /// Generation of the offscreen texture; the commit below restarts when it changes.
    generation: u64,
    /// Commit of the offscreen element; it moves on with every damage to the offscreen.
    commit: CommitCounter,
    /// Size of the offscreen texture.
    size: Size<i32, Buffer>,
    scale: Scale<f64>,
    options: BlurOptions,
}

/// Whether a texture blurred for `cached` is stale for `current`.
fn needs_reblur(cached: Option<&BlurKey>, current: &BlurKey) -> bool {
    cached != Some(current)
}

impl SuspendedSnapshot {
    /// Drops the offscreen and blurred textures.
    pub fn clear(&self) {
        self.inner.replace(None);
    }

    /// Renders `elements` offscreen and returns one element of them, blurred.
    ///
    /// `elements` are positioned relative to the window; the returned element is placed at
    /// `location` plus their offset. Falls back to the unblurred offscreen when the blur program
    /// is missing or fails.
    pub fn render(
        &self,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
        options: BlurOptions,
        elements: &[impl RenderElement<GlesRenderer>],
        location: Point<f64, Logical>,
        alpha: f32,
    ) -> anyhow::Result<(SuspendedRenderElement, OffscreenData)> {
        let _span = tracy_client::span!("SuspendedSnapshot::render");

        let mut inner = self.inner.borrow_mut();
        let inner = inner.get_or_insert_with(Inner::default);

        let (elem, _sync, mut data) = inner
            .offscreen
            .render(renderer, scale, elements)
            .context("error rendering window to offscreen")?;
        let location = location + elem.offset();

        let blurred = inner
            .blurred(renderer, &elem, scale, options)
            .map_err(|err| warn!("error blurring suspended window: {err:?}"))
            .ok()
            .flatten();

        let Some(buffer) = blurred else {
            let elem = elem.with_offset(location).with_alpha(alpha);
            data.id = elem.id().clone();
            return Ok((elem.into(), data));
        };

        let size = elem.logical_size();
        let elem = TextureRenderElement::from_texture_buffer(
            buffer,
            location,
            alpha,
            Some(Rectangle::from_size(size)),
            Some(size),
            Kind::Unspecified,
        );
        let elem = PrimaryGpuTextureRenderElement(elem);
        data.id = elem.id().clone();
        Ok((elem.into(), data))
    }
}

impl Inner {
    /// Returns the blurred offscreen, blurring it again only if it went stale.
    ///
    /// `None` when there is no blur program.
    fn blurred(
        &mut self,
        renderer: &mut GlesRenderer,
        elem: &OffscreenRenderElement,
        scale: Scale<f64>,
        options: BlurOptions,
    ) -> anyhow::Result<Option<TextureBuffer<GlesTexture>>> {
        let source = elem.texture();
        let key = BlurKey {
            generation: elem.generation(),
            commit: elem.current_commit(),
            size: source.size(),
            scale,
            options,
        };
        let context_id = renderer.context_id();

        if let Some(cached) = &self.cached {
            if cached.renderer_context_id == context_id && !needs_reblur(Some(&cached.key), &key) {
                return Ok(Some(cached.buffer.clone()));
            }
        }

        // Drop the stale texture first: it is the blur's output texture, which is reused only
        // while it is uniquely referenced.
        self.cached = None;

        if self
            .blur
            .as_ref()
            .is_some_and(|b| b.context_id() != context_id)
        {
            self.blur = None;
        }
        if self.blur.is_none() {
            let Some(blur) = Blur::new(renderer) else {
                return Ok(None);
            };
            self.blur = Some(blur);
        }
        let blur = self.blur.as_mut().unwrap();

        blur.prepare_textures(
            |fourcc, size| renderer.create_buffer(fourcc, size),
            source,
            options,
        )
        .context("error preparing blur textures")?;
        let texture = blur
            .render(renderer, source, options)
            .context("error rendering blur")?;

        // A new buffer has a new id, which damages the whole element once.
        let buffer =
            TextureBuffer::from_texture(renderer, texture, scale, Transform::Normal, Vec::new());
        self.cached = Some(Cached {
            key,
            renderer_context_id: context_id,
            buffer: buffer.clone(),
        });

        Ok(Some(buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> BlurKey {
        BlurKey {
            generation: 1,
            commit: CommitCounter::default(),
            size: Size::new(800, 600),
            scale: Scale::from(1.),
            options: BlurOptions {
                passes: 3,
                offset: 3.,
            },
        }
    }

    #[test]
    fn first_render_blurs() {
        assert!(needs_reblur(None, &key()));
    }

    #[test]
    fn unchanged_frozen_window_reuses_the_blur() {
        assert!(!needs_reblur(Some(&key()), &key()));
    }

    #[test]
    fn offscreen_damage_reblurs() {
        let mut current = key();
        current.commit.increment();
        assert!(needs_reblur(Some(&key()), &current));
    }

    #[test]
    fn recreated_offscreen_texture_reblurs() {
        // A recreated texture restarts its commit counter, so equal commit, size, scale and
        // options can still mean different content.
        let mut current = key();
        current.generation += 1;
        assert!(needs_reblur(Some(&key()), &current));
    }

    #[test]
    fn size_scale_or_options_change_reblurs() {
        let cached = key();

        let mut current = key();
        current.size = Size::new(801, 600);
        assert!(needs_reblur(Some(&cached), &current));

        let mut current = key();
        current.scale = Scale::from(2.);
        assert!(needs_reblur(Some(&cached), &current));

        let mut current = key();
        current.options.passes = 4;
        assert!(needs_reblur(Some(&cached), &current));

        let mut current = key();
        current.options.offset = 2.5;
        assert!(needs_reblur(Some(&cached), &current));
    }
}
