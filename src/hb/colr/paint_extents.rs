use core::f32;

use skrifa::{
    color::{Brush, ColorPainter, CompositeMode, Transform},
    outline::{pen::PathStyle, DrawSettings, OutlinePen},
    prelude::Size,
    raw::types::{BoundingBox, F2Dot14},
    GlyphId, OutlineGlyphCollection,
};
use smallvec::{smallvec, SmallVec};

trait BoundingBoxExt {
    #[must_use]
    fn union(&self, other: &Self) -> Self;
    #[must_use]
    fn intersection(&self, other: &Self) -> Self;
    #[must_use]
    fn extend_to_point(&self, x: f32, y: f32) -> Self;
    fn is_valid(&self) -> bool;
}

impl BoundingBoxExt for BoundingBox<f32> {
    fn union(&self, other: &Self) -> Self {
        Self {
            x_min: self.x_min.min(other.x_min),
            x_max: self.x_max.max(other.x_max),
            y_min: self.y_min.min(other.y_min),
            y_max: self.y_max.max(other.y_max),
        }
    }

    fn intersection(&self, other: &Self) -> Self {
        Self {
            x_min: self.x_min.max(other.x_min),
            x_max: self.x_max.min(other.x_max),
            y_min: self.y_min.max(other.y_min),
            y_max: self.y_max.min(other.y_max),
        }
    }

    fn extend_to_point(&self, x: f32, y: f32) -> Self {
        Self {
            x_min: self.x_min.min(x),
            x_max: self.x_max.max(x),
            y_min: self.y_min.min(y),
            y_max: self.y_max.max(y),
        }
    }

    fn is_valid(&self) -> bool {
        self.x_min.is_finite()
            && self.y_min.is_finite()
            && self.x_max.is_finite()
            && self.y_max.is_finite()
            && self.x_min <= self.x_max
            && self.y_min <= self.y_max
    }
}

#[derive(Clone, Copy, Debug)]
enum Bounds {
    Empty,
    Unbounded,
    Bounded(BoundingBox<f32>),
}

impl Bounds {
    #[must_use]
    fn union(&self, other: &Self) -> Self {
        match other {
            Bounds::Empty => *self,
            Bounds::Unbounded => Self::Unbounded,
            Bounds::Bounded(other_bounding_box) => match self {
                Bounds::Empty => *other,
                Bounds::Unbounded => *self,
                Bounds::Bounded(my_bounding_box) => {
                    Bounds::Bounded(my_bounding_box.union(other_bounding_box))
                }
            },
        }
    }

    #[must_use]
    fn intersection(&self, other: &Self) -> Self {
        match other {
            Bounds::Empty => Self::Empty,
            Bounds::Unbounded => *self,
            Bounds::Bounded(other_bounding_box) => match self {
                Bounds::Empty => *self,
                Bounds::Unbounded => *other,
                Bounds::Bounded(my_bounding_box) => {
                    Bounds::Bounded(my_bounding_box.intersection(other_bounding_box))
                }
            },
        }
    }
}

trait TransformExt {
    #[must_use]
    fn transform_point(&self, point: [f32; 2]) -> [f32; 2];
    #[must_use]
    fn transform_extents(&self, extents: BoundingBox<f32>) -> BoundingBox<f32>;
}

impl TransformExt for Transform {
    fn transform_point(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let new_x = self.xx * x + self.xy * y + self.dx;
        let new_y = self.yx * x + self.yy * y + self.dy;
        [new_x, new_y]
    }

    fn transform_extents(&self, extents: BoundingBox<f32>) -> BoundingBox<f32> {
        let corners = [
            self.transform_point([extents.x_min, extents.y_min]),
            self.transform_point([extents.x_min, extents.y_max]),
            self.transform_point([extents.x_max, extents.y_min]),
            self.transform_point([extents.x_max, extents.y_max]),
        ];

        let mut new_extents = BoundingBox {
            x_min: corners[0][0],
            y_min: corners[0][1],
            x_max: corners[0][0],
            y_max: corners[0][1],
        };

        for i in 1..4 {
            new_extents = new_extents.extend_to_point(corners[i][0], corners[i][1]);
        }

        new_extents
    }
}

pub struct ExtentsPainter<'a> {
    outline_glyphs: &'a OutlineGlyphCollection<'a>,
    coords: &'a [F2Dot14],
    clips: SmallVec<[Bounds; 8]>,
    groups: SmallVec<[Bounds; 8]>,
    transforms: SmallVec<[Transform; 8]>,
    composite_modes: SmallVec<[CompositeMode; 8]>,
}

impl<'a> ExtentsPainter<'a> {
    pub fn new(outline_glyphs: &'a OutlineGlyphCollection<'a>, coords: &'a [F2Dot14]) -> Self {
        Self {
            outline_glyphs,
            coords,
            clips: smallvec![Bounds::Unbounded],
            groups: smallvec![Bounds::Empty],
            transforms: smallvec![Default::default()],
            composite_modes: smallvec![Default::default()],
        }
    }

    pub fn extents(mut self) -> Option<BoundingBox<f32>> {
        match self.groups.pop()? {
            Bounds::Bounded(bounding_box) => Some(bounding_box),
            _ => None,
        }
    }
}

impl ColorPainter for ExtentsPainter<'_> {
    fn push_transform(&mut self, transform: Transform) {
        let t = self
            .transforms
            .last()
            .copied()
            .unwrap_or(Transform::default());
        let new = t * transform;
        self.transforms.push(new);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        if let Some(glyph_bounds) = self.outline_glyphs.get(glyph_id).and_then(|glyph| {
            let mut extents_pen = ExtentsPen::new();
            let draw_result = glyph.draw(
                DrawSettings::unhinted(Size::unscaled(), self.coords)
                    .with_path_style(PathStyle::HarfBuzz),
                &mut extents_pen,
            );
            (draw_result.is_ok() && extents_pen.bounds.is_valid()).then_some(extents_pen.bounds)
        }) {
            self.push_clip_box(glyph_bounds);
        } else {
            // Push a dummy bounding box so as not to mess up the stack when this clip is popped
            self.clips.push(Bounds::Empty);
        }
    }

    fn push_clip_box(&mut self, mut clip_box: BoundingBox<f32>) {
        if let Some(r) = self.transforms.last_mut() {
            clip_box = r.transform_extents(clip_box);
        }

        let mut b = Bounds::Bounded(clip_box);
        if let Some(clip) = self.clips.last() {
            b = b.intersection(clip);
        }
        self.clips.push(b);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn fill(&mut self, _brush: Brush<'_>) {
        if let (Some(clip), Some(group)) = (self.clips.last(), self.groups.last_mut()) {
            *group = group.union(clip);
        }
    }

    fn push_layer(&mut self, composite_mode: CompositeMode) {
        self.composite_modes.push(composite_mode);
        self.groups.push(Bounds::Empty);
    }

    fn pop_layer(&mut self) {
        if let Some(mode) = self.composite_modes.pop() {
            if let Some(src_bounds) = self.groups.pop() {
                if let Some(backdrop_bounds) = self.groups.last_mut() {
                    match mode {
                        CompositeMode::Clear => *backdrop_bounds = Bounds::Empty,
                        CompositeMode::Src | CompositeMode::SrcOut => *backdrop_bounds = src_bounds,
                        CompositeMode::Dest | CompositeMode::DestOut => {}
                        CompositeMode::SrcIn | CompositeMode::DestIn => {
                            *backdrop_bounds = backdrop_bounds.intersection(&src_bounds)
                        }
                        _ => *backdrop_bounds = backdrop_bounds.union(&src_bounds),
                    }
                }
            }
        }
    }
}

struct ExtentsPen {
    bounds: BoundingBox<f32>,
}

impl ExtentsPen {
    fn new() -> Self {
        Self {
            bounds: BoundingBox {
                x_min: f32::INFINITY,
                y_min: f32::INFINITY,
                x_max: f32::NEG_INFINITY,
                y_max: f32::NEG_INFINITY,
            },
        }
    }
}

impl OutlinePen for ExtentsPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.bounds = self.bounds.extend_to_point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.bounds = self.bounds.extend_to_point(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.bounds = self.bounds.extend_to_point(cx0, cy0);
        self.bounds = self.bounds.extend_to_point(x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.bounds = self.bounds.extend_to_point(cx0, cy0);
        self.bounds = self.bounds.extend_to_point(cx1, cy1);
        self.bounds = self.bounds.extend_to_point(x, y);
    }

    fn close(&mut self) {}
}
