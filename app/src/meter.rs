//! A horizontal linear meter, in the style of a Yaesu/Flex web-control meter:
//! a recessed graticule track with minor and major graduations, a colour-zoned
//! fill, an optional peak-hold marker and a numeric read-out.
//!
//! Meter definitions (`Scale`) are keyed by [`MeterKind`] via [`scale_for`];
//! the widget itself is drawing-only and holds no state, so peak-hold is owned
//! by the caller and passed in on each frame.

use eframe::egui::{
    self, pos2, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use scu_cat::MeterKind;

use crate::theme;

/// A single labelled graduation on a meter scale (`frac` is 0.0..=1.0).
#[derive(Clone, Copy)]
pub struct Tick {
    pub frac: f32,
    pub label: &'static str,
}

/// Severity band used to tint the danger end of the track and to colour the
/// fill as it advances.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Good,
    Warn,
    Bad,
}

/// The scale definition for one meter type.
pub struct Scale {
    /// Major, labelled graduations.
    pub ticks: &'static [Tick],
    /// Ascending upper-bound fractions mapped to a severity zone.
    pub zones: &'static [(f32, Zone)],
    /// Number of equal minor divisions across the full width.
    pub minor_divisions: u32,
}

// ---- Scale tables -------------------------------------------------------

static S_TICKS: &[Tick] = &[
    Tick {
        frac: 0.051,
        label: "S1",
    },
    Tick {
        frac: 0.149,
        label: "S3",
    },
    Tick {
        frac: 0.251,
        label: "S5",
    },
    Tick {
        frac: 0.357,
        label: "S7",
    },
    Tick {
        frac: 0.510,
        label: "S9",
    },
    Tick {
        frac: 0.671,
        label: "+20",
    },
    Tick {
        frac: 0.812,
        label: "+40",
    },
    Tick {
        frac: 1.000,
        label: "+60",
    },
];
static S_ZONES: &[(f32, Zone)] = &[(0.510, Zone::Good), (0.812, Zone::Warn), (1.0, Zone::Bad)];

static COMP_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "0",
    },
    Tick {
        frac: 0.25,
        label: "5",
    },
    Tick {
        frac: 0.5,
        label: "10",
    },
    Tick {
        frac: 0.75,
        label: "15",
    },
    Tick {
        frac: 1.0,
        label: "20",
    },
];
static ALC_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "0",
    },
    Tick {
        frac: 0.25,
        label: "25",
    },
    Tick {
        frac: 0.5,
        label: "50",
    },
    Tick {
        frac: 0.75,
        label: "75",
    },
    Tick {
        frac: 1.0,
        label: "100",
    },
];
static POWER_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "0",
    },
    Tick {
        frac: 0.25,
        label: "25",
    },
    Tick {
        frac: 0.5,
        label: "50",
    },
    Tick {
        frac: 0.75,
        label: "75",
    },
    Tick {
        frac: 1.0,
        label: "100",
    },
];
static SWR_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "1",
    },
    Tick {
        frac: 0.125,
        label: "1.5",
    },
    Tick {
        frac: 0.25,
        label: "2",
    },
    Tick {
        frac: 0.5,
        label: "3",
    },
    Tick {
        frac: 1.0,
        label: "5",
    },
];
static ID_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "0",
    },
    Tick {
        frac: 0.196,
        label: "5",
    },
    Tick {
        frac: 0.392,
        label: "10",
    },
    Tick {
        frac: 0.588,
        label: "15",
    },
    Tick {
        frac: 0.784,
        label: "20",
    },
    Tick {
        frac: 0.980,
        label: "25",
    },
];
static VDD_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "0",
    },
    Tick {
        frac: 0.278,
        label: "5",
    },
    Tick {
        frac: 0.556,
        label: "10",
    },
    Tick {
        frac: 0.833,
        label: "15",
    },
];
static TEMP_TICKS: &[Tick] = &[
    Tick {
        frac: 0.0,
        label: "",
    },
    Tick {
        frac: 0.25,
        label: "",
    },
    Tick {
        frac: 0.5,
        label: "",
    },
    Tick {
        frac: 0.75,
        label: "",
    },
    Tick {
        frac: 1.0,
        label: "",
    },
];

static ALL_GOOD: &[(f32, Zone)] = &[(1.0, Zone::Good)];
static ALC_ZONES: &[(f32, Zone)] = &[(0.66, Zone::Good), (1.0, Zone::Warn)];
static POWER_ZONES: &[(f32, Zone)] = &[(0.80, Zone::Good), (1.0, Zone::Warn)];
static SWR_ZONES: &[(f32, Zone)] = &[(0.125, Zone::Good), (0.25, Zone::Warn), (1.0, Zone::Bad)];
static TEMP_ZONES: &[(f32, Zone)] = &[(0.60, Zone::Good), (0.85, Zone::Warn), (1.0, Zone::Bad)];

static S_SCALE: Scale = Scale {
    ticks: S_TICKS,
    zones: S_ZONES,
    minor_divisions: 20,
};
static COMP_SCALE: Scale = Scale {
    ticks: COMP_TICKS,
    zones: ALL_GOOD,
    minor_divisions: 20,
};
static ALC_SCALE: Scale = Scale {
    ticks: ALC_TICKS,
    zones: ALC_ZONES,
    minor_divisions: 20,
};
static POWER_SCALE: Scale = Scale {
    ticks: POWER_TICKS,
    zones: POWER_ZONES,
    minor_divisions: 20,
};
static SWR_SCALE: Scale = Scale {
    ticks: SWR_TICKS,
    zones: SWR_ZONES,
    minor_divisions: 20,
};
static ID_SCALE: Scale = Scale {
    ticks: ID_TICKS,
    zones: ALL_GOOD,
    minor_divisions: 20,
};
static VDD_SCALE: Scale = Scale {
    ticks: VDD_TICKS,
    zones: ALL_GOOD,
    minor_divisions: 20,
};
static TEMP_SCALE: Scale = Scale {
    ticks: TEMP_TICKS,
    zones: TEMP_ZONES,
    minor_divisions: 10,
};
static GENERIC_SCALE: Scale = Scale {
    ticks: TEMP_TICKS,
    zones: ALL_GOOD,
    minor_divisions: 10,
};

/// The scale table for a meter kind.
pub fn scale_for(kind: MeterKind) -> &'static Scale {
    match kind {
        MeterKind::S => &S_SCALE,
        MeterKind::Comp => &COMP_SCALE,
        MeterKind::Alc => &ALC_SCALE,
        MeterKind::Power => &POWER_SCALE,
        MeterKind::Swr => &SWR_SCALE,
        MeterKind::Id => &ID_SCALE,
        MeterKind::Vdd => &VDD_SCALE,
        MeterKind::Temp => &TEMP_SCALE,
        MeterKind::Unknown(_) => &GENERIC_SCALE,
    }
}

fn zone_color(zone: Zone) -> Color32 {
    match zone {
        Zone::Good => theme::rx_green(),
        Zone::Warn => theme::warn_amber(),
        Zone::Bad => theme::tx_red(),
    }
}

// ---- Widget -------------------------------------------------------------

/// A linear meter row. When `label` is `Some`, the read-out is placed in a
/// left-hand gutter beneath the label; otherwise the value is centred on the
/// bar (used for the compact S-meter in the VFO panes).
pub struct LinearMeter<'a> {
    pub label: Option<&'a str>,
    pub value: &'a str,
    pub fraction: f32,
    pub scale: &'a Scale,
    pub peak: Option<f32>,
    /// Render in a muted "inactive" style (S-meter on a non-receiving VFO).
    pub dim: bool,
    pub height: f32,
}

impl LinearMeter<'_> {
    fn current_zone(&self, frac: f32) -> Zone {
        self.scale
            .zones
            .iter()
            .find(|(upper, _)| frac <= *upper)
            .map(|(_, zone)| *zone)
            .unwrap_or(Zone::Bad)
    }

    fn paint(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter().clone();
        let radius = theme::current().radius;
        let corner = CornerRadius::same(radius);

        let gutter = if self.label.is_some() {
            (rect.width() * 0.34)
                .clamp(60.0, 96.0)
                .min((rect.width() - 30.0).max(0.0))
        } else {
            0.0
        };
        let bar = Rect::from_min_max(pos2(rect.left() + gutter, rect.top()), rect.max);

        // Left gutter, single row: the meter name sits at the far left and the
        // read-out hugs the track.
        if let Some(label) = self.label {
            let (name_color, value_color) = if self.dim {
                (theme::text_faint(), theme::text_faint())
            } else {
                (theme::text_dim(), theme::text())
            };
            painter.text(
                pos2(rect.left(), rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::new(10.0, FontFamily::Monospace),
                name_color,
            );
            painter.text(
                pos2(bar.left() - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                self.value,
                FontId::new(11.0, FontFamily::Monospace),
                value_color,
            );
        }

        // Recessed track.
        painter.rect_filled(bar, corner, theme::inset_bg());

        // Tint the warning/danger bands so the limits read at a glance.
        let mut lower = 0.0;
        for (upper, zone) in self.scale.zones {
            if *zone != Zone::Good {
                let band = Rect::from_min_max(
                    pos2(bar.left() + lower * bar.width(), bar.top()),
                    pos2(bar.left() + upper * bar.width(), bar.bottom()),
                );
                painter.rect_filled(band, 0.0, zone_color(*zone).gamma_multiply(0.13));
            }
            lower = *upper;
        }

        // Graticule and fill are clipped to the track.
        let tp = painter.with_clip_rect(bar);
        let div = self.scale.minor_divisions.max(1);
        for i in 1..div {
            let x = bar.left() + bar.width() * (i as f32 / div as f32);
            let h = bar.height() * if i % 2 == 0 { 0.38 } else { 0.24 };
            tp.line_segment(
                [pos2(x, bar.bottom()), pos2(x, bar.bottom() - h)],
                Stroke::new(1.0, theme::current().outline_soft),
            );
        }
        for tick in self.scale.ticks {
            let x = bar.left() + tick.frac * bar.width();
            tp.line_segment(
                [pos2(x, bar.top()), pos2(x, bar.bottom())],
                Stroke::new(1.0, theme::current().outline),
            );
        }

        let frac = self.fraction.clamp(0.0, 1.0);
        let fill_w = bar.width() * frac;
        if fill_w > 0.5 {
            let fill = Rect::from_min_max(bar.min, pos2(bar.left() + fill_w, bar.bottom()));
            let cr = (radius as f32).min(fill_w * 0.5).max(0.0) as u8;
            let color = if self.dim {
                theme::button_bg()
            } else {
                zone_color(self.current_zone(frac))
            };
            tp.rect_filled(fill, CornerRadius::same(cr), color);
            if !self.dim {
                // Glassy highlight across the top half of the bar.
                let top = Rect::from_min_max(
                    fill.min,
                    pos2(fill.right(), fill.top() + fill.height() * 0.45),
                );
                tp.rect_filled(
                    top,
                    CornerRadius {
                        nw: cr,
                        ne: cr,
                        sw: 0,
                        se: 0,
                    },
                    Color32::from_white_alpha(26),
                );
                tp.line_segment(
                    [
                        pos2(fill.right(), bar.top()),
                        pos2(fill.right(), bar.bottom()),
                    ],
                    Stroke::new(1.5, color.gamma_multiply(1.5)),
                );
            }
        }

        // Peak-hold marker.
        if let Some(peak) = self.peak {
            let peak = peak.clamp(0.0, 1.0);
            if peak > frac + 0.003 {
                let x = bar.left() + peak * bar.width();
                let color = if self.dim {
                    theme::text_faint()
                } else {
                    theme::freq_cyan()
                };
                tp.line_segment(
                    [pos2(x, bar.top() + 1.0), pos2(x, bar.bottom() - 1.0)],
                    Stroke::new(1.5, color),
                );
            }
        }

        // Crisp border on top of the fill.
        painter.rect_stroke(
            bar,
            corner,
            Stroke::new(1.0, theme::current().outline_soft),
            StrokeKind::Inside,
        );

        // Value centred on the bar when there is no gutter.
        if self.label.is_none() {
            painter.text(
                bar.center(),
                Align2::CENTER_CENTER,
                self.value,
                FontId::new(11.0, FontFamily::Monospace),
                if self.dim {
                    theme::text_faint()
                } else {
                    theme::text()
                },
            );
        }

        // Numeric graduations, drawn along the bottom of the track when there
        // is room for them (they would only clutter the half-height row).
        if bar.height() >= 20.0 {
            let inset = 12.0;
            for tick in self.scale.ticks {
                if tick.label.is_empty() || bar.width() <= 2.0 * inset {
                    continue;
                }
                let x = (bar.left() + tick.frac * bar.width())
                    .clamp(bar.left() + inset, bar.right() - inset);
                painter.text(
                    pos2(x, bar.bottom() - 1.0),
                    Align2::CENTER_BOTTOM,
                    tick.label,
                    FontId::new(8.0, FontFamily::Monospace),
                    theme::text_faint(),
                );
            }
        }
    }
}

impl egui::Widget for LinearMeter<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let width = ui.available_width().max(1.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, self.height), Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(ctx: &egui::Context, width: f32, kind: MeterKind, label: bool) {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_width(width);
            for height in [15.0, 30.0] {
                for frac in [0.0, 0.5, 1.0] {
                    ui.add(LinearMeter {
                        label: label.then_some("PO"),
                        value: "S9+40dB",
                        fraction: frac,
                        scale: scale_for(kind),
                        peak: Some(0.7),
                        dim: false,
                        height,
                    });
                }
            }
        });
        // Acknowledge texture deltas so the debug assertion in
        // `TexturesDelta::drop` does not fire.
        output.textures_delta.clear();
    }

    #[test]
    fn paints_every_scale_at_several_widths() {
        let ctx = egui::Context::default();
        let kinds = [
            MeterKind::S,
            MeterKind::Comp,
            MeterKind::Alc,
            MeterKind::Power,
            MeterKind::Swr,
            MeterKind::Id,
            MeterKind::Vdd,
            MeterKind::Temp,
            MeterKind::Unknown(42),
        ];
        for kind in kinds {
            for width in [40.0, 120.0, 400.0] {
                paint(&ctx, width, kind, true);
            }
        }
    }

    #[test]
    fn paints_compact_meter_without_gutter() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_width(180.0);
            ui.add(LinearMeter {
                label: None,
                value: "S9+10dB",
                fraction: 0.8,
                scale: scale_for(MeterKind::S),
                peak: None,
                dim: true,
                height: 22.0,
            });
        });
        output.textures_delta.clear();
    }
}
