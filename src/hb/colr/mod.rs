use paint_extents::ExtentsPainter;
use skrifa::{
    color::ColorGlyphCollection,
    prelude::Size,
    raw::types::{BoundingBox, F2Dot14},
    GlyphId, MetadataProvider, OutlineGlyphCollection,
};

mod paint_extents;

#[derive(Clone)]
pub struct ColrTables<'a> {
    color_glyphs: ColorGlyphCollection<'a>,
    outline_glyphs: OutlineGlyphCollection<'a>,
}

impl<'a> ColrTables<'a> {
    pub fn new(font: &impl MetadataProvider<'a>) -> Self {
        let color_glyphs = font.color_glyphs();
        let outline_glyphs = font.outline_glyphs();

        Self {
            color_glyphs,
            outline_glyphs,
        }
    }

    pub fn bounding_box(&self, glyph_id: GlyphId, coords: &[F2Dot14]) -> Option<BoundingBox<f32>> {
        let glyph = self.color_glyphs.get(glyph_id)?;
        if let Some(bounds) = glyph.bounding_box(coords, Size::unscaled()) {
            return Some(bounds);
        }

        let mut painter = ExtentsPainter::new(&self.outline_glyphs, coords);
        // Match HarfBuzz: return (0, 0, 0, 0) if the glyph can't be painted.
        if glyph.paint(coords, &mut painter).is_err() {
            return Some(BoundingBox::default());
        }
        painter.extents()
    }
}
