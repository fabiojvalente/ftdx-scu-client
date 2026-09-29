//! Flex layout: dockable panels backed by [`egui_tiles`].
//!
//! This module owns the [`Pane`] catalogue, the persisted [`LayoutsFile`] and
//! the [`egui_tiles::Behavior`] that renders each pane. The live tree itself
//! lives on [`ScuApp`](crate::app::ScuApp).

use eframe::egui::{self, Color32, Stroke, WidgetText};
use egui_tiles::{
    Behavior, Container, EditAction, LinearDir, Tile, TileId, Tiles, Tree, UiResponse,
};
use serde::{Deserialize, Serialize};

use crate::app::ScuApp;
use crate::theme;

/// Identifier stored in the layout file for the built-in default arrangement.
pub const DEFAULT_ID: &str = "__default__";

/// Identifier for the built-in alternative dashboard arrangement.
pub const DASHBOARD_ID: &str = "__dashboard__";

/// Bumped when the on-disk layout shape changes so stale files fall back.
const LAYOUT_VERSION: u32 = 1;

/// Globally-unique id for the dock tree.
pub const TREE_ID: &str = "scu-flex-tree";

/// Every panel the workspace can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    VfoA,
    VfoB,
    Operate,
    Spectrum,
    Waterfall,
    Radio,
    Tuning,
    Clarifier,
    Dsp,
    Receiver,
    Meters,
    Scope,
    Audio,
    Transmit,
    CatConsole,
    CatServer,
    Vox,
}

impl Pane {
    /// Catalogue, in menu order.
    pub const ALL: [Pane; 17] = [
        Pane::VfoA,
        Pane::VfoB,
        Pane::Operate,
        Pane::Spectrum,
        Pane::Waterfall,
        Pane::Radio,
        Pane::Tuning,
        Pane::Clarifier,
        Pane::Dsp,
        Pane::Receiver,
        Pane::Meters,
        Pane::Scope,
        Pane::Audio,
        Pane::Transmit,
        Pane::CatConsole,
        Pane::CatServer,
        Pane::Vox,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Pane::VfoA => "VFO A",
            Pane::VfoB => "VFO B",
            Pane::Operate => "Operate",
            Pane::Spectrum => "Spectrum",
            Pane::Waterfall => "Waterfall",
            Pane::Radio => "Radio",
            Pane::Tuning => "Tuning",
            Pane::Clarifier => "Clarifier",
            Pane::Dsp => "DSP",
            Pane::Receiver => "Receiver",
            Pane::Meters => "Meters",
            Pane::Scope => "Scope",
            Pane::Audio => "Audio",
            Pane::Transmit => "Transmit",
            Pane::CatConsole => "CAT Console",
            Pane::CatServer => "Radio Server (CAT)",
            Pane::Vox => "VOX",
        }
    }

    /// Panels that only exist in the native build.
    pub fn native_only(self) -> bool {
        matches!(self, Pane::CatServer | Pane::Vox)
    }

    /// Whether this panel can be shown on the current host.
    pub fn available(self, dual_receiver: bool) -> bool {
        if self.native_only() && cfg!(target_arch = "wasm32") {
            return false;
        }
        if self == Pane::VfoB && !dual_receiver {
            return false;
        }
        true
    }
}

// ---- Tree construction --------------------------------------------------

/// Insert a [`Container::Tabs`] holding the given panes.
fn tabs(tiles: &mut Tiles<Pane>, panes: &[Pane]) -> TileId {
    let ids = panes.iter().map(|p| tiles.insert_pane(*p)).collect();
    tiles.insert_tab_tile(ids)
}

/// Insert a linear container with explicit relative shares.
fn linear(tiles: &mut Tiles<Pane>, dir: LinearDir, children: &[(TileId, f32)]) -> TileId {
    let ids: Vec<TileId> = children.iter().map(|(id, _)| *id).collect();
    let id = tiles.insert_container(Container::new_linear(dir, ids));
    if let Some(Tile::Container(Container::Linear(lin))) = tiles.get_mut(id) {
        for (child, share) in children {
            lin.shares.set_share(*child, *share);
        }
    }
    id
}

/// The built-in arrangement, laid out to feel like the classic fixed UI:
/// a control column on the left, VFOs over spectrum over waterfall on the right.
pub fn default_tree() -> Tree<Pane> {
    let mut t = Tiles::default();

    let radio = tabs(&mut t, &[Pane::Radio, Pane::Tuning, Pane::Clarifier]);
    let dsp = tabs(
        &mut t,
        &[Pane::Dsp, Pane::Receiver, Pane::Meters, Pane::Scope],
    );
    let io = tabs(
        &mut t,
        &[
            Pane::Audio,
            Pane::Transmit,
            Pane::CatConsole,
            Pane::CatServer,
            Pane::Vox,
        ],
    );
    let left = linear(
        &mut t,
        LinearDir::Vertical,
        &[(radio, 1.0), (dsp, 1.0), (io, 1.0)],
    );

    let operate = tabs(&mut t, &[Pane::Operate]);
    let vfo_a = tabs(&mut t, &[Pane::VfoA]);
    let vfo_b = tabs(&mut t, &[Pane::VfoB]);
    let vfo_row = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(vfo_a, 1.0), (vfo_b, 1.0)],
    );
    let spectrum = tabs(&mut t, &[Pane::Spectrum]);
    let waterfall = tabs(&mut t, &[Pane::Waterfall]);
    let center = linear(
        &mut t,
        LinearDir::Vertical,
        &[
            (operate, 0.7),
            (vfo_row, 1.6),
            (spectrum, 1.4),
            (waterfall, 2.2),
        ],
    );

    let root = linear(&mut t, LinearDir::Horizontal, &[(left, 0.42), (center, 1.0)]);
    Tree::new(TREE_ID, root, t)
}

/// An alternative dashboard-style arrangement.
pub fn dashboard_tree() -> Tree<Pane> {
    let mut t = Tiles::default();

    let actions = tabs(&mut t, &[Pane::Operate, Pane::Tuning]);
    let meters = tabs(&mut t, &[Pane::Meters, Pane::Receiver]);
    let top = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(actions, 1.0), (meters, 1.2)],
    );

    let vfo_a = tabs(&mut t, &[Pane::VfoA]);
    let spectrum = tabs(&mut t, &[Pane::Spectrum]);
    let middle = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(vfo_a, 1.0), (spectrum, 1.6)],
    );

    let waterfall = tabs(&mut t, &[Pane::Waterfall]);
    let vfo_b = tabs(&mut t, &[Pane::VfoB, Pane::Clarifier, Pane::Dsp]);
    let bottom = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(waterfall, 1.7), (vfo_b, 1.0)],
    );

    let root = linear(
        &mut t,
        LinearDir::Vertical,
        &[(top, 1.0), (middle, 1.4), (bottom, 1.4)],
    );
    Tree::new(TREE_ID, root, t)
}

// ---- Persistence --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub tree: Tree<Pane>,
}

/// The on-disk layout file: the working draft, the active selection and any
/// user-saved presets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutsFile {
    #[serde(default = "layout_version")]
    pub version: u32,
    #[serde(default = "default_active_id")]
    pub active_id: String,
    pub draft: Tree<Pane>,
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Panes floating in their own OS windows (native only), restored on launch.
    #[serde(default)]
    pub popped: Vec<Pane>,
}

fn layout_version() -> u32 {
    LAYOUT_VERSION
}

fn default_active_id() -> String {
    DEFAULT_ID.to_string()
}

impl Default for LayoutsFile {
    fn default() -> Self {
        Self {
            version: LAYOUT_VERSION,
            active_id: DEFAULT_ID.to_string(),
            draft: default_tree(),
            presets: Vec::new(),
            popped: Vec::new(),
        }
    }
}

impl LayoutsFile {
    /// Load from disk/localStorage, falling back to the default arrangement.
    pub fn load() -> Self {
        match storage_load() {
            Some(text) => match serde_json::from_str::<LayoutsFile>(&text) {
                Ok(file) if file.version == LAYOUT_VERSION => file,
                _ => Self::default(),
            },
            None => Self::default(),
        }
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            storage_save(&text);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn layouts_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".config/scu-client/layouts.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn storage_load() -> Option<String> {
    std::fs::read_to_string(layouts_path()?).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn storage_save(text: &str) {
    let Some(path) = layouts_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

#[cfg(target_arch = "wasm32")]
const LAYOUTS_KEY: &str = "scu-client-layouts";

#[cfg(target_arch = "wasm32")]
fn storage_load() -> Option<String> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    storage.get_item(LAYOUTS_KEY).ok()?
}

#[cfg(target_arch = "wasm32")]
fn storage_save(text: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    let _ = storage.set_item(LAYOUTS_KEY, text);
}

// ---- Row/col helpers for the Panels menu --------------------------------

/// Find the first [`Container::Tabs`] in the tree (a good place to add panes).
fn first_tabset(tree: &Tree<Pane>) -> Option<TileId> {
    fn walk(tiles: &Tiles<Pane>, id: TileId) -> Option<TileId> {
        match tiles.get(id)? {
            Tile::Container(Container::Tabs(_)) => Some(id),
            Tile::Container(container) => {
                container.children().find_map(|child| walk(tiles, *child))
            }
            Tile::Pane(_) => None,
        }
    }
    tree.root().and_then(|root| walk(&tree.tiles, root))
}

/// The tile id of the given pane, if present.
pub fn pane_tile(tree: &Tree<Pane>, pane: Pane) -> Option<TileId> {
    tree.tiles
        .iter()
        .find_map(|(id, tile)| match tile {
            Tile::Pane(p) if *p == pane => Some(*id),
            _ => None,
        })
}

/// Add `pane` to the tree, preferring the active tabset, else the first one.
pub fn add_pane(tree: &mut Tree<Pane>, pane: Pane) {
    if pane_tile(tree, pane).is_some() {
        return;
    }
    let target = tree
        .active_tiles()
        .into_iter()
        .find(|id| matches!(tree.tiles.get(*id), Some(Tile::Container(Container::Tabs(_)))))
        .or_else(|| first_tabset(tree));
    let Some(target) = target else {
        // No tabset yet — start a fresh tree.
        *tree = Tree::new_tabs(TREE_ID, vec![pane]);
        return;
    };
    let pane_id = tree.tiles.insert_pane(pane);
    if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(target) {
        tabs.children.push(pane_id);
        tabs.active = Some(pane_id);
    }
}

/// Remove `pane` from the tree if present.
pub fn remove_pane(tree: &mut Tree<Pane>, pane: Pane) {
    if let Some(id) = pane_tile(tree, pane) {
        tree.remove_recursively(id);
    }
}

// ---- Behavior -----------------------------------------------------------

/// Bridges the tile tree to [`ScuApp`]'s pane rendering.
pub struct FlexBehavior<'a> {
    pub app: &'a mut ScuApp,
    pub theme: theme::Theme,
}

impl Behavior<Pane> for FlexBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut Pane) -> UiResponse {
        let theme = self.theme;
        self.app.render_pane(*pane, ui, theme);
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> WidgetText {
        self.app.pane_tab_title(*pane).into()
    }

    fn is_tab_closable(&self, _tiles: &Tiles<Pane>, _tile_id: TileId) -> bool {
        true
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        26.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        4.0
    }

    /// Keep a tab bar (with close button) around every pane, even when it is
    /// the only tab in its set — otherwise the default pruning removes the tab
    /// and there is nothing left to drag or close.
    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    /// Add a small "Pop" button to each tab bar that floats the active pane
    /// into its own window (native: a real OS window; web: an embedded one).
    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        _scroll_offset: &mut f32,
    ) {
        let Some(active) = tabs.active else {
            return;
        };
        let Some(Tile::Pane(pane)) = tiles.get(active) else {
            return;
        };
        let pane = *pane;
        if self.app.is_popped(pane) {
            return;
        }
        if ui
            .small_button("\u{2b08}")
            .on_hover_text("Open this panel in its own window")
            .clicked()
        {
            self.app.request_popout(pane);
        }
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        self.theme.inset_bg
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        if state.active {
            self.theme.panel_bg
        } else {
            self.theme.card_bg
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Stroke {
        if state.active {
            Stroke::new(1.0, self.theme.accent)
        } else {
            Stroke::new(1.0, self.theme.outline)
        }
    }

    fn tab_text_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        if state.active {
            self.theme.text
        } else {
            self.theme.text_dim
        }
    }

    fn on_edit(&mut self, _action: EditAction) {
        self.app.mark_layout_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tree_round_trips_through_json() {
        let tree = default_tree();
        let text = serde_json::to_string(&tree).expect("serialize");
        let restored: Tree<Pane> = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(tree, restored);
    }

    #[test]
    fn layouts_file_round_trips() {
        let mut file = LayoutsFile::default();
        file.presets.push(Preset {
            id: "p1".into(),
            name: "Mine".into(),
            tree: dashboard_tree(),
        });
        file.popped.push(Pane::Meters);
        let text = serde_json::to_string_pretty(&file).expect("serialize");
        let restored: LayoutsFile = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(restored.presets.len(), 1);
        assert_eq!(restored.presets[0].name, "Mine");
        assert_eq!(restored.popped, vec![Pane::Meters]);
        assert_eq!(restored.active_id, DEFAULT_ID);
    }

    #[test]
    fn add_remove_pane_toggles_presence() {
        let mut tree = default_tree();
        // VOX is present in the default arrangement; remove and re-add it.
        assert!(pane_tile(&tree, Pane::Vox).is_some());
        remove_pane(&mut tree, Pane::Vox);
        assert!(pane_tile(&tree, Pane::Vox).is_none());
        add_pane(&mut tree, Pane::Vox);
        assert!(pane_tile(&tree, Pane::Vox).is_some());
        // Adding twice is a no-op.
        let before = tree.tiles.len();
        add_pane(&mut tree, Pane::Vox);
        assert_eq!(tree.tiles.len(), before);
    }

    #[test]
    fn add_pane_to_empty_tree_starts_a_tabs_container() {
        let mut tree = Tree::empty(TREE_ID);
        assert!(tree.is_empty());
        add_pane(&mut tree, Pane::Spectrum);
        assert!(!tree.is_empty());
        assert!(pane_tile(&tree, Pane::Spectrum).is_some());
    }

    #[test]
    fn native_only_panes_hide_on_wasm() {
        // Native-only availability is expressed via `native_only`; on native
        // builds every pane is available while wasm hides the native trio.
        for pane in Pane::ALL {
            if pane.native_only() {
                assert_eq!(pane.available(true), !cfg!(target_arch = "wasm32"));
            } else {
                assert!(pane.available(true));
            }
        }
    }

    /// Lays the tree out and paints it in a headless egui context, which
    /// exercises the layout/simplification code paths without a window.
    #[test]
    fn tree_lays_out_headless() {
        struct Noop;
        impl Behavior<Pane> for Noop {
            fn pane_ui(
                &mut self,
                _ui: &mut egui::Ui,
                _tile_id: TileId,
                _pane: &mut Pane,
            ) -> UiResponse {
                UiResponse::None
            }

            fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
                pane.title().into()
            }

            fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
                egui_tiles::SimplificationOptions {
                    all_panes_must_have_tabs: true,
                    ..Default::default()
                }
            }
        }

        let mut tree = default_tree();
        let mut behavior = Noop;
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            tree.ui(&mut behavior, ui);
        });
        // Headless: acknowledge the texture deltas so the debug assertion in
        // `TexturesDelta::drop` does not fire.
        output.textures_delta.clear();

        // Every pane should still be reachable after a layout pass.
        for pane in Pane::ALL {
            assert!(pane_tile(&tree, pane).is_some(), "missing {pane:?}");
        }
    }
}
