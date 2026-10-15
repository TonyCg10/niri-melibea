use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::backend::renderer::Renderer;
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Transform};

/// Element that shrinks another element about a point without opening seams.
///
/// smithay's `RescaleRenderElement` rounds the scaled location and the scaled size separately, so
/// two elements that share an edge can end up a pixel apart or a pixel overlapped. That passes
/// unnoticed in a zoom that is over in a moment, but a tile shrunk by the scale rule sits still
/// for as long as it is unfocused, its border strips, window and corners abutting each other. Here
/// every edge is scaled and rounded on its own, so a shared edge lands on the same pixel from both
/// sides.
///
/// Meant for scales up to 1: the damage padding covers what minifying spreads, not magnifying.
#[derive(Debug)]
pub struct SeamlessRescaleRenderElement<E> {
    element: E,
    origin: Point<i32, Physical>,
    scale: Scale<f64>,
}

impl<E: Element> SeamlessRescaleRenderElement<E> {
    /// Wraps `element` so that it is drawn scaled by `scale` about `origin`.
    ///
    /// `origin` is in the same physical space as the element's geometry and is the point that
    /// stays put.
    pub fn from_element(
        element: E,
        origin: Point<i32, Physical>,
        scale: impl Into<Scale<f64>>,
    ) -> Self {
        Self {
            element,
            origin,
            scale: scale.into(),
        }
    }

    /// Scales the absolute `rect` into its start and end corners, rounding the start edges with
    /// `start` and the end edges with `end`.
    ///
    /// Each edge depends only on its own input, which is what keeps shared edges shared.
    fn scale_edges(
        &self,
        rect: Rectangle<i32, Physical>,
        start: fn(f64) -> f64,
        end: fn(f64) -> f64,
    ) -> (Point<i32, Physical>, Point<i32, Physical>) {
        let (o, s) = (self.origin, self.scale);
        let edge = |p: Point<i32, Physical>, round: fn(f64) -> f64| {
            Point::new(
                round((p.x - o.x) as f64 * s.x + o.x as f64) as i32,
                round((p.y - o.y) as f64 * s.y + o.y as f64) as i32,
            )
        };
        (
            edge(rect.loc, start),
            edge(rect.loc + rect.size.to_point(), end),
        )
    }

    /// Scales the element-relative `rects` the same way as the element, back into
    /// element-relative coordinates, grown by `grow` pixels on every side (negative shrinks) and
    /// kept within the element. Rectangles left empty are dropped.
    fn scale_local<C: FromIterator<Rectangle<i32, Physical>>>(
        &self,
        scale: Scale<f64>,
        rects: impl IntoIterator<Item = Rectangle<i32, Physical>>,
        start: fn(f64) -> f64,
        end: fn(f64) -> f64,
        grow: i32,
    ) -> C {
        let element_loc = self.element.geometry(scale).loc;
        let geo = self.geometry(scale);
        let grow = Point::new(grow, grow);
        rects
            .into_iter()
            .filter_map(|mut rect| {
                rect.loc += element_loc;
                let (from, to) = self.scale_edges(rect, start, end);
                let (from, to) = (from - geo.loc - grow, to - geo.loc + grow);
                let from = Point::new(from.x.max(0), from.y.max(0));
                let to = Point::new(to.x.min(geo.size.w), to.y.min(geo.size.h));
                (from.x < to.x && from.y < to.y).then(|| Rectangle::from_extremities(from, to))
            })
            .collect()
    }
}

impl<E: Element> Element for SeamlessRescaleRenderElement<E> {
    fn id(&self) -> &Id {
        self.element.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.element.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.element.src()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        let (from, to) = self.scale_edges(self.element.geometry(scale), f64::round, f64::round);
        Rectangle::from_extremities(from, to)
    }

    fn transform(&self) -> Transform {
        self.element.transform()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        // Minifying filters a changed texel into the pixels on either side of where it lands, so
        // those are damaged too, or a still tile keeps a stale fringe around whatever changed in
        // it.
        let damage = self.element.damage_since(scale, commit);
        self.scale_local(scale, damage, f64::floor, f64::ceil, 1)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        // Only pixels the content covers whole, minus the ones along its edge, which filtering
        // blends with whatever lies beyond it. Claiming more would let the damage tracker skip
        // drawing what shows through them.
        let opaque = self.element.opaque_regions(scale);
        self.scale_local(scale, opaque, f64::ceil, f64::floor, -1)
    }

    fn alpha(&self) -> f32 {
        self.element.alpha()
    }

    fn kind(&self) -> Kind {
        self.element.kind()
    }

    fn is_framebuffer_effect(&self) -> bool {
        self.element.is_framebuffer_effect()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for SeamlessRescaleRenderElement<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), R::Error> {
        self.element
            .draw(frame, src, dst, damage, opaque_regions, cache)
    }

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        self.element.underlying_storage(renderer)
    }

    fn capture_framebuffer(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) -> Result<(), R::Error> {
        self.element.capture_framebuffer(frame, src, dst, cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare element with a fixed geometry, damage and opaque regions.
    struct TestElement {
        id: Id,
        geometry: Rectangle<i32, Physical>,
        damage: Vec<Rectangle<i32, Physical>>,
        opaque: Vec<Rectangle<i32, Physical>>,
    }

    impl TestElement {
        fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
            Self {
                id: Id::new(),
                geometry: Rectangle::new(Point::new(x, y), (w, h).into()),
                damage: Vec::new(),
                opaque: Vec::new(),
            }
        }
    }

    impl Element for TestElement {
        fn id(&self) -> &Id {
            &self.id
        }

        fn current_commit(&self) -> CommitCounter {
            CommitCounter::default()
        }

        fn src(&self) -> Rectangle<f64, Buffer> {
            Rectangle::from_size((1., 1.).into())
        }

        fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
            self.geometry
        }

        fn damage_since(
            &self,
            _: Scale<f64>,
            _: Option<CommitCounter>,
        ) -> DamageSet<i32, Physical> {
            self.damage.iter().copied().collect()
        }

        fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
            self.opaque.iter().copied().collect()
        }
    }

    const SCALES: [f64; 6] = [0.95, 0.9, 0.75, 0.5, 0.33, 0.01];
    const ORIGINS: [i32; 4] = [0, 7, 513, -40];

    fn rescaled(
        elem: TestElement,
        origin: i32,
        s: f64,
    ) -> SeamlessRescaleRenderElement<TestElement> {
        SeamlessRescaleRenderElement::from_element(elem, Point::new(origin, origin), s)
    }

    fn geometry(x: i32, y: i32, w: i32, h: i32, origin: i32, s: f64) -> Rectangle<i32, Physical> {
        rescaled(TestElement::new(x, y, w, h), origin, s).geometry(Scale::from(1.))
    }

    #[test]
    fn abutting_elements_stay_abutting() {
        // Like a border strip next to the window it surrounds: the edge they share must land on
        // the same pixel for both, or a still, shrunk tile shows a gap or a double line there.
        for s in SCALES {
            for origin in ORIGINS {
                for x in -20..60 {
                    for w in 1..24 {
                        let left = geometry(x, x, w, w, origin, s);
                        let right = geometry(x + w, x + w, 17, 17, origin, s);
                        assert_eq!(
                            left.loc.x + left.size.w,
                            right.loc.x,
                            "x: {x}, w: {w}, origin: {origin}, s: {s}"
                        );
                        assert_eq!(
                            left.loc.y + left.size.h,
                            right.loc.y,
                            "y: {x}, h: {w}, origin: {origin}, s: {s}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn edges_land_on_the_nearest_pixel() {
        for s in SCALES {
            for origin in ORIGINS {
                for x in -20..60 {
                    let w = 16;
                    let geo = geometry(x, 0, w, 1, origin, s);
                    let exact = |v: i32| (v - origin) as f64 * s + origin as f64;
                    assert!((geo.loc.x as f64 - exact(x)).abs() <= 0.5);
                    assert!(((geo.loc.x + geo.size.w) as f64 - exact(x + w)).abs() <= 0.5);
                }
            }
        }
    }

    #[test]
    fn damage_covers_everything_filtering_can_reach() {
        // Minifying blends a changed texel into the output pixels on either side of where it
        // lands, so the damage has to reach one pixel past the scaled texel, within the element.
        for s in SCALES {
            for origin in ORIGINS {
                for x in 0..30 {
                    let mut elem = TestElement::new(10, 10, 100, 100);
                    elem.damage = vec![Rectangle::new(Point::new(x, x), (1, 1).into())];
                    let elem = rescaled(elem, origin, s);

                    let geo = elem.geometry(Scale::from(1.));
                    let damage: Vec<_> = elem
                        .damage_since(Scale::from(1.), None)
                        .into_iter()
                        .collect();
                    assert_eq!(damage.len(), 1);
                    let damage = damage[0];

                    let exact = |v: i32| (v - origin) as f64 * s + origin as f64;
                    let lo = (exact(10 + x).floor() as i32 - 1).max(geo.loc.x);
                    let hi = (exact(10 + x + 1).ceil() as i32 + 1).min(geo.loc.x + geo.size.w);
                    let msg = format!("x: {x}, origin: {origin}, s: {s}, damage: {damage:?}");
                    assert!(geo.loc.x + damage.loc.x <= lo, "{msg}");
                    assert!(geo.loc.x + damage.loc.x + damage.size.w >= hi, "{msg}");
                    assert!(geo.loc.y + damage.loc.y <= lo, "{msg}");
                    assert!(geo.loc.y + damage.loc.y + damage.size.h >= hi, "{msg}");

                    // And no further than the element itself.
                    let local = Rectangle::from_size(geo.size);
                    assert_eq!(local.intersection(damage), Some(damage), "{msg}");
                }
            }
        }
    }

    #[test]
    fn opaque_regions_claim_only_fully_covered_pixels() {
        // An opaque region that reaches a pixel the content only partly covers lets the damage
        // tracker skip drawing what shows through that pixel's other part.
        for s in SCALES {
            for origin in ORIGINS {
                for x in 0..30 {
                    let mut elem = TestElement::new(10, 10, 100, 100);
                    elem.opaque = vec![Rectangle::new(Point::new(x, x), (40, 40).into())];
                    let elem = rescaled(elem, origin, s);

                    let geo = elem.geometry(Scale::from(1.));
                    let exact = |v: i32| (v - origin) as f64 * s + origin as f64;
                    let msg = format!("x: {x}, origin: {origin}, s: {s}");
                    for opaque in elem.opaque_regions(Scale::from(1.)) {
                        let lo = (geo.loc.x + opaque.loc.x) as f64;
                        let hi = (geo.loc.x + opaque.loc.x + opaque.size.w) as f64;
                        assert!(lo >= exact(10 + x), "{msg}");
                        assert!(hi <= exact(10 + x + 40), "{msg}");
                    }
                }
            }
        }
    }
}
