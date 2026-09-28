//! Rolling waterfall texture.

use eframe::egui;
use scu_scope::Colormap;

/// A fixed-size scrolling waterfall backed by an RGBA texture.
///
/// Rows are stored top-first: row 0 is the newest sweep. Each [`push`] shifts
/// the buffer down by one row, which keeps the texture upload trivial.
pub struct Waterfall {
    pub width: usize,
    pub height: usize,
    rows: Vec<u8>,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    pub colormap: Colormap,
    /// Magnitudes at or below this are clamped to the colormap floor.
    pub black_level: f32,
    /// Contrast gain applied above the black level.
    pub gain: f32,
}

impl Waterfall {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(16);
        let height = height.max(16);
        Self {
            width,
            height,
            rows: vec![0; width * height * 4],
            texture: None,
            dirty: true,
            colormap: Colormap::Turbo,
            black_level: 0.30,
            gain: 1.4,
        }
    }

    pub fn clear(&mut self) {
        self.rows.fill(0);
        self.dirty = true;
    }

    fn color_for(&self, magnitude: f32) -> [u8; 4] {
        let denom = (1.0 - self.black_level).max(1e-3);
        let adjusted = ((magnitude - self.black_level) / denom * self.gain).clamp(0.0, 1.0);
        self.colormap.rgba(adjusted)
    }

    /// Append one sweep. Bins are nearest-neighbour resampled to the texture width.
    pub fn push(&mut self, bins: &[f32]) {
        if bins.is_empty() {
            return;
        }
        let row_bytes = self.width * 4;

        // Shift existing rows down by one.
        self.rows
            .copy_within(0..(self.height - 1) * row_bytes, row_bytes);

        // Write the newest row at the top.
        for x in 0..self.width {
            let src = x * bins.len() / self.width;
            let color = self.color_for(bins[src]);
            let idx = x * 4;
            self.rows[idx..idx + 4].copy_from_slice(&color);
        }
        self.dirty = true;
    }

    /// Upload the CPU buffer to the GPU texture if it changed.
    pub fn update_texture(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &self.rows);
        match &mut self.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("waterfall", image, egui::TextureOptions::NEAREST))
            }
        }
        self.dirty = false;
    }

    /// Paint the texture into `rect`.
    pub fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(texture) = &self.texture else {
            return;
        };
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.add(egui::Shape::image(
            texture.id(),
            rect,
            uv,
            egui::Color32::WHITE,
        ));
    }
}
