//! Theming and UI scaling.
//!
//! Every colour is data: a [`Theme`] is one palette, selected by [`ThemeKind`].
//! Widget code reads the active palette through the accessor functions
//! (`theme::accent()`, `theme::text()`, ...) which look up the process-local
//! [`current`] theme. [`ScuApp`](crate::app::ScuApp) owns the source of truth
//! and pushes it here with [`set_current`] whenever it changes.
//!
//! [`UiScale`] is deliberately separate from the theme: it changes *size*
//! (fonts, spacing, hit targets) and never colours or layout positions.

use std::cell::Cell;

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};
use serde::{Deserialize, Serialize};

// ---- Theme selection ----------------------------------------------------

/// A named palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ThemeKind {
    /// The original dark operator console (default).
    #[default]
    Dark,
    /// Bright, high-contrast light theme.
    Light,
    /// Yaesu instrument chassis: charcoal face, amber LCD, red TX.
    Yaesu,
    /// High-contrast neon: electric cyan and green on near-black.
    Neon,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 4] = [
        ThemeKind::Dark,
        ThemeKind::Light,
        ThemeKind::Yaesu,
        ThemeKind::Neon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark",
            ThemeKind::Light => "Light",
            ThemeKind::Yaesu => "Yaesu",
            ThemeKind::Neon => "Neon",
        }
    }
}

// ---- UI scale -----------------------------------------------------------

/// Named UI size steps. Applied as an egui zoom factor so fonts, spacing and
/// hit targets all scale together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum UiScale {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

impl UiScale {
    pub const ALL: [UiScale; 5] = [
        UiScale::XSmall,
        UiScale::Small,
        UiScale::Medium,
        UiScale::Large,
        UiScale::XLarge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UiScale::XSmall => "Extra small",
            UiScale::Small => "Small",
            UiScale::Medium => "Medium",
            UiScale::Large => "Large",
            UiScale::XLarge => "Extra large",
        }
    }

    /// Zoom factor handed to `egui::Context::set_zoom_factor`.
    pub fn factor(self) -> f32 {
        match self {
            UiScale::XSmall => 0.8,
            UiScale::Small => 0.9,
            UiScale::Medium => 1.0,
            UiScale::Large => 1.2,
            UiScale::XLarge => 1.5,
        }
    }
}

// ---- Palette ------------------------------------------------------------

/// One complete colour palette plus a couple of shape parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub kind: ThemeKind,
    /// Whether egui should use its dark-mode widget defaults as a base.
    pub is_dark: bool,

    // Surfaces
    pub page_bg: Color32,
    pub panel_bg: Color32,
    pub card_bg: Color32,
    pub inset_bg: Color32,
    pub button_bg: Color32,
    pub button_hover_bg: Color32,
    pub outline: Color32,
    pub outline_soft: Color32,

    // Text
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,

    // Accents
    pub accent: Color32,
    pub accent_deep: Color32,
    pub rx_green: Color32,
    pub tx_red: Color32,
    pub warn_amber: Color32,
    pub freq_cyan: Color32,
    pub spectrum_green: Color32,
    /// Text drawn on top of a bright accent fill.
    pub on_accent: Color32,

    /// Corner radius for cards and controls.
    pub radius: u8,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Theme {
    pub fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
            ThemeKind::Yaesu => Self::yaesu(),
            ThemeKind::Neon => Self::neon(),
        }
    }

    fn dark() -> Self {
        Self {
            kind: ThemeKind::Dark,
            is_dark: true,
            page_bg: rgb(24, 26, 30),
            panel_bg: rgb(31, 34, 40),
            card_bg: rgb(41, 45, 53),
            inset_bg: rgb(14, 16, 19),
            button_bg: rgb(54, 60, 70),
            button_hover_bg: rgb(68, 76, 88),
            outline: rgb(62, 68, 78),
            outline_soft: rgb(47, 52, 60),
            text: rgb(223, 228, 235),
            text_dim: rgb(151, 159, 171),
            text_faint: rgb(104, 111, 123),
            accent: rgb(74, 142, 218),
            accent_deep: rgb(38, 74, 116),
            rx_green: rgb(72, 178, 114),
            tx_red: rgb(207, 74, 68),
            warn_amber: rgb(226, 172, 72),
            freq_cyan: rgb(124, 201, 255),
            spectrum_green: rgb(88, 224, 168),
            on_accent: rgb(13, 18, 24),
            radius: 5,
        }
    }

    fn light() -> Self {
        Self {
            kind: ThemeKind::Light,
            is_dark: false,
            page_bg: rgb(238, 240, 243),
            panel_bg: rgb(255, 255, 255),
            card_bg: rgb(248, 249, 250),
            inset_bg: rgb(255, 255, 255),
            button_bg: rgb(233, 236, 239),
            button_hover_bg: rgb(222, 226, 230),
            outline: rgb(206, 212, 218),
            outline_soft: rgb(222, 226, 230),
            text: rgb(33, 37, 41),
            text_dim: rgb(108, 117, 125),
            text_faint: rgb(134, 142, 150),
            accent: rgb(13, 110, 253),
            accent_deep: rgb(10, 88, 202),
            rx_green: rgb(25, 135, 84),
            tx_red: rgb(220, 53, 69),
            warn_amber: rgb(255, 193, 7),
            freq_cyan: rgb(13, 110, 253),
            spectrum_green: rgb(25, 135, 84),
            on_accent: rgb(255, 255, 255),
            radius: 5,
        }
    }

    fn yaesu() -> Self {
        Self {
            kind: ThemeKind::Yaesu,
            is_dark: true,
            page_bg: rgb(18, 18, 18),
            panel_bg: rgb(42, 42, 42),
            card_bg: rgb(42, 42, 42),
            inset_bg: rgb(26, 26, 26),
            button_bg: rgb(61, 61, 61),
            button_hover_bg: rgb(74, 74, 74),
            outline: rgb(13, 13, 13),
            outline_soft: rgb(51, 51, 51),
            text: rgb(230, 230, 230),
            text_dim: rgb(154, 163, 173),
            text_faint: rgb(110, 110, 110),
            accent: rgb(255, 122, 24),
            accent_deep: rgb(179, 84, 14),
            rx_green: rgb(88, 207, 154),
            tx_red: rgb(220, 53, 69),
            warn_amber: rgb(255, 193, 7),
            freq_cyan: rgb(242, 163, 60),
            spectrum_green: rgb(88, 207, 154),
            on_accent: rgb(18, 18, 18),
            radius: 2,
        }
    }

    fn neon() -> Self {
        Self {
            kind: ThemeKind::Neon,
            is_dark: true,
            page_bg: rgb(5, 6, 10),
            panel_bg: rgb(11, 14, 23),
            card_bg: rgb(18, 23, 42),
            inset_bg: rgb(5, 7, 14),
            button_bg: rgb(27, 35, 64),
            button_hover_bg: rgb(39, 48, 90),
            outline: rgb(43, 58, 107),
            outline_soft: rgb(26, 36, 64),
            text: rgb(234, 246, 255),
            text_dim: rgb(127, 168, 201),
            text_faint: rgb(79, 109, 138),
            accent: rgb(0, 229, 255),
            accent_deep: rgb(0, 119, 170),
            rx_green: rgb(57, 255, 20),
            tx_red: rgb(255, 43, 94),
            warn_amber: rgb(255, 212, 0),
            freq_cyan: rgb(0, 240, 255),
            spectrum_green: rgb(57, 255, 20),
            on_accent: rgb(0, 16, 24),
            radius: 4,
        }
    }

    /// A contrast-boosted variant for low-vision use.
    fn high_contrast(self) -> Self {
        let strong = if self.is_dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        let mid = if self.is_dark {
            rgb(200, 208, 218)
        } else {
            rgb(60, 66, 74)
        };
        Self {
            text: strong,
            text_dim: mid,
            text_faint: mid,
            outline: strong,
            outline_soft: mid,
            ..self
        }
    }

    /// Build and install the egui [`egui::Visuals`] for this palette.
    fn visuals(&self) -> egui::Visuals {
        let mut v = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.dark_mode = self.is_dark;

        v.panel_fill = self.panel_bg;
        v.window_fill = self.card_bg;
        v.window_stroke = Stroke::new(1.0, self.outline);
        v.extreme_bg_color = self.inset_bg;
        v.text_edit_bg_color = Some(self.inset_bg);
        v.faint_bg_color = self.page_bg;
        v.code_bg_color = self.inset_bg;
        v.window_corner_radius = CornerRadius::same(self.radius);
        v.menu_corner_radius = CornerRadius::same(self.radius);

        v.selection.bg_fill = self.accent;
        v.selection.stroke = Stroke::new(1.0, self.on_accent);
        v.hyperlink_color = self.freq_cyan;
        v.warn_fg_color = self.warn_amber;
        v.error_fg_color = self.tx_red;

        v.widgets.noninteractive.bg_fill = self.panel_bg;
        v.widgets.noninteractive.weak_bg_fill = self.panel_bg;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.outline_soft);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.noninteractive.corner_radius = CornerRadius::same(self.radius);

        v.widgets.inactive.bg_fill = self.button_bg;
        v.widgets.inactive.weak_bg_fill = self.button_bg;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, self.outline);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.inactive.corner_radius = CornerRadius::same(self.radius);
        v.widgets.inactive.expansion = 0.0;

        v.widgets.hovered.bg_fill = self.button_hover_bg;
        v.widgets.hovered.weak_bg_fill = self.button_hover_bg;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, self.on_accent);
        v.widgets.hovered.corner_radius = CornerRadius::same(self.radius);
        v.widgets.hovered.expansion = 0.0;

        v.widgets.active.bg_fill = self.accent;
        v.widgets.active.weak_bg_fill = self.accent;
        v.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.on_accent);
        v.widgets.active.corner_radius = CornerRadius::same(self.radius);
        v.widgets.active.expansion = 0.0;

        v.widgets.open.bg_fill = self.button_hover_bg;
        v.widgets.open.weak_bg_fill = self.button_hover_bg;
        v.widgets.open.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.open.fg_stroke = Stroke::new(1.0, self.on_accent);
        v.widgets.open.corner_radius = CornerRadius::same(self.radius);

        v
    }
}

// ---- Current theme ------------------------------------------------------

thread_local! {
    static CURRENT: Cell<Theme> = Cell::new(Theme::for_kind(ThemeKind::Dark));
}

/// Install `theme` as the active palette for this thread.
pub fn set_current(theme: Theme) {
    CURRENT.with(|c| c.set(theme));
}

/// The active palette.
pub fn current() -> Theme {
    CURRENT.with(|c| c.get())
}

// ---- Colour accessors ---------------------------------------------------

pub fn accent() -> Color32 {
    current().accent
}
pub fn accent_deep() -> Color32 {
    current().accent_deep
}
pub fn rx_green() -> Color32 {
    current().rx_green
}
pub fn tx_red() -> Color32 {
    current().tx_red
}
pub fn warn_amber() -> Color32 {
    current().warn_amber
}
pub fn freq_cyan() -> Color32 {
    current().freq_cyan
}
pub fn spectrum_green() -> Color32 {
    current().spectrum_green
}
pub fn on_accent() -> Color32 {
    current().on_accent
}
pub fn text() -> Color32 {
    current().text
}
pub fn text_dim() -> Color32 {
    current().text_dim
}
pub fn text_faint() -> Color32 {
    current().text_faint
}
pub fn outline() -> Color32 {
    current().outline
}
pub fn card_bg() -> Color32 {
    current().card_bg
}
pub fn inset_bg() -> Color32 {
    current().inset_bg
}
pub fn button_bg() -> Color32 {
    current().button_bg
}
pub fn page_bg() -> Color32 {
    current().page_bg
}

// ---- Application --------------------------------------------------------

/// Apply a palette, UI scale and accessibility options to `ctx`.
///
/// Safe to call every time any of them changes (and once at startup).
pub fn apply(
    ctx: &egui::Context,
    theme: Theme,
    scale: UiScale,
    high_contrast: bool,
    large_targets: bool,
) {
    let theme = if high_contrast {
        theme.high_contrast()
    } else {
        theme
    };
    ctx.set_theme(if theme.is_dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    ctx.set_visuals(theme.visuals());
    ctx.set_zoom_factor(scale.factor());
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.slider_width = 132.0;
        style.spacing.combo_width = 128.0;
        // Large targets bump the hit area (pre-zoom) for easier pointing.
        style.spacing.interact_size.y = if large_targets { 30.0 } else { 22.0 };
    });
    set_current(theme);
}

/// Frame for the connection bar.
pub fn top_bar_frame() -> Frame {
    Frame::new()
        .fill(page_bg())
        .inner_margin(Margin::symmetric(10, 4))
}

/// A small uppercase section title in the accent colour.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .small()
            .strong()
            .color(accent()),
    );
    ui.add_space(1.0);
}
