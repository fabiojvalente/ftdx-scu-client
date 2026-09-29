//! Dark operator-console colour scheme.
//!
//! Dark charcoal chassis with blue accent buttons, a bright cyan frequency readout
//! and red/green transmit/receive cues.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};

// ---- Surfaces -----------------------------------------------------------

/// Window background, sitting behind every panel.
pub const APP_BG: Color32 = Color32::from_rgb(24, 26, 30);
/// Background of the top-level panel chrome.
pub const PANEL_BG: Color32 = Color32::from_rgb(31, 34, 40);
/// Raised card background (VFO read-outs, grouped controls).
pub const CARD_BG: Color32 = Color32::from_rgb(41, 45, 53);
/// Recessed background for text edits, scope, meters.
pub const INSET_BG: Color32 = Color32::from_rgb(14, 16, 19);
/// Resting button fill.
pub const BUTTON_BG: Color32 = Color32::from_rgb(54, 60, 70);
/// Hovered button fill.
pub const BUTTON_HOVER_BG: Color32 = Color32::from_rgb(68, 76, 88);
/// Strong outline around cards and controls.
pub const OUTLINE: Color32 = Color32::from_rgb(62, 68, 78);
/// Subtle divider / inactive outline.
pub const OUTLINE_SOFT: Color32 = Color32::from_rgb(47, 52, 60);

// ---- Text ---------------------------------------------------------------

pub const TEXT: Color32 = Color32::from_rgb(223, 228, 235);
pub const TEXT_DIM: Color32 = Color32::from_rgb(151, 159, 171);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(104, 111, 123);

// ---- Accents ------------------------------------------------------------

pub const ACCENT: Color32 = Color32::from_rgb(74, 142, 218);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(38, 74, 116);
pub const RX_GREEN: Color32 = Color32::from_rgb(72, 178, 114);
pub const TX_RED: Color32 = Color32::from_rgb(207, 74, 68);
pub const WARN_AMBER: Color32 = Color32::from_rgb(226, 172, 72);
pub const FREQ_CYAN: Color32 = Color32::from_rgb(124, 201, 255);
pub const SPECTRUM_GREEN: Color32 = Color32::from_rgb(88, 224, 168);

/// Near-black used for text drawn on top of a bright accent fill.
pub const ON_ACCENT: Color32 = Color32::from_rgb(13, 18, 24);

/// Apply the dark console styling to `ctx`. Safe to call once at startup.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.slider_width = 132.0;
        style.spacing.combo_width = 128.0;
        style.spacing.interact_size.y = 22.0;
    });
}

fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;

    v.panel_fill = PANEL_BG;
    v.window_fill = CARD_BG;
    v.window_stroke = Stroke::new(1.0, OUTLINE);
    v.extreme_bg_color = INSET_BG;
    v.text_edit_bg_color = Some(INSET_BG);
    v.faint_bg_color = APP_BG;
    v.code_bg_color = INSET_BG;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);

    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = FREQ_CYAN;
    v.warn_fg_color = WARN_AMBER;
    v.error_fg_color = TX_RED;

    v.widgets.noninteractive.bg_fill = PANEL_BG;
    v.widgets.noninteractive.weak_bg_fill = PANEL_BG;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, OUTLINE_SOFT);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.corner_radius = CornerRadius::same(5);

    v.widgets.inactive.bg_fill = BUTTON_BG;
    v.widgets.inactive.weak_bg_fill = BUTTON_BG;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, OUTLINE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = CornerRadius::same(5);
    v.widgets.inactive.expansion = 0.0;

    v.widgets.hovered.bg_fill = BUTTON_HOVER_BG;
    v.widgets.hovered.weak_bg_fill = BUTTON_HOVER_BG;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.hovered.corner_radius = CornerRadius::same(5);
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.corner_radius = CornerRadius::same(5);
    v.widgets.active.expansion = 0.0;

    v.widgets.open.bg_fill = BUTTON_HOVER_BG;
    v.widgets.open.weak_bg_fill = BUTTON_HOVER_BG;
    v.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.open.corner_radius = CornerRadius::same(5);

    v
}

/// Frame for the connection bar.
pub fn top_bar_frame() -> Frame {
    Frame::new()
        .fill(APP_BG)
        .inner_margin(Margin::symmetric(10, 4))
}

/// Frame for the dual-VFO header.
pub fn header_frame() -> Frame {
    Frame::new()
        .fill(PANEL_BG)
        .inner_margin(Margin::symmetric(10, 2))
}

/// Frame for the left-hand control rail.
pub fn rail_frame() -> Frame {
    Frame::new()
        .fill(PANEL_BG)
        .inner_margin(Margin::symmetric(10, 6))
}

/// A small uppercase section title in the accent colour.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .small()
            .strong()
            .color(ACCENT),
    );
    ui.add_space(1.0);
}
