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
    Panadapter,
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
    Antenna,
    /// Catch-all for a pane that this build no longer knows about. Layout
    /// files that mention a removed panel still deserialize instead of failing
    /// wholesale; the loader strips these before the tree is rendered.
    #[serde(other)]
    Unsupported,
}

impl Pane {
    /// Catalogue, in menu order.
    pub const ALL: [Pane; 19] = [
        Pane::VfoA,
        Pane::VfoB,
        Pane::Operate,
        Pane::Panadapter,
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
        Pane::Antenna,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Pane::VfoA => "VFO A",
            Pane::VfoB => "VFO B",
            Pane::Operate => "Operate",
            Pane::Panadapter => "Panadapter",
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
            Pane::Antenna => "Antenna",
            Pane::Unsupported => "Unsupported",
        }
    }

    /// Panels that only exist in the native build.
    pub fn native_only(self) -> bool {
        matches!(self, Pane::CatServer | Pane::Vox | Pane::Antenna)
    }

    /// Whether this panel can be shown on the current host.
    pub fn available(self, dual_receiver: bool) -> bool {
        if self == Pane::Unsupported {
            return false;
        }
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
            Pane::Antenna,
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
    let vfo_row = linear(&mut t, LinearDir::Horizontal, &[(vfo_a, 1.0), (vfo_b, 1.0)]);
    let panadapter = tabs(&mut t, &[Pane::Panadapter]);
    let center = linear(
        &mut t,
        LinearDir::Vertical,
        &[(operate, 0.7), (vfo_row, 1.6), (panadapter, 3.6)],
    );

    let root = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(left, 0.42), (center, 1.0)],
    );
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
    let panadapter = tabs(&mut t, &[Pane::Panadapter]);
    let middle = linear(
        &mut t,
        LinearDir::Horizontal,
        &[(vfo_a, 1.0), (panadapter, 1.6)],
    );

    let vfo_b = tabs(&mut t, &[Pane::VfoB, Pane::Clarifier, Pane::Dsp]);
    let root = linear(
        &mut t,
        LinearDir::Vertical,
        &[(top, 1.0), (middle, 1.8), (vfo_b, 0.8)],
    );
    Tree::new(TREE_ID, root, t)
}

// ---- Persistence --------------------------------------------------------
//
// User layouts are stored one file per layout under `$CONFIG/layouts/<id>.json`
// so they are never entangled with the built-in arrangements. The remaining
// live state (the working draft, the active selection and the last state of the
// two built-ins) lives in `layouts.json`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub tree: Tree<Pane>,
}

/// The full in-memory layout state: the working draft, the active selection and
/// every preset (built-in Default/Dashboard plus user layouts).
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

/// The `layouts.json` workspace: the live draft plus the two built-in
/// arrangements. User layouts are deliberately absent — their own files are the
/// source of truth, so changing the built-ins can never clobber them.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Workspace {
    #[serde(default = "layout_version")]
    version: u32,
    #[serde(default = "default_active_id")]
    active_id: String,
    #[serde(default = "default_tree")]
    draft: Tree<Pane>,
    #[serde(default = "default_tree")]
    default_layout: Tree<Pane>,
    #[serde(default = "dashboard_tree")]
    dashboard_layout: Tree<Pane>,
    #[serde(default)]
    popped: Vec<Pane>,
    /// User presets were once embedded in this file. Read once for migration
    /// and never written back.
    #[serde(default, rename = "presets", skip_serializing)]
    legacy_presets: Vec<Preset>,
}

fn layout_version() -> u32 {
    LAYOUT_VERSION
}

fn default_active_id() -> String {
    DEFAULT_ID.to_string()
}

impl Default for LayoutsFile {
    fn default() -> Self {
        let mut file = Self {
            version: LAYOUT_VERSION,
            active_id: DEFAULT_ID.to_string(),
            draft: default_tree(),
            presets: Vec::new(),
            popped: Vec::new(),
        };
        file.ensure_builtins();
        file
    }
}

impl LayoutsFile {
    /// Load from disk/localStorage, falling back to the default arrangement.
    pub fn load() -> Self {
        let (mut file, legacy) = match workspace_load() {
            Some(text) => Self::from_workspace(&text),
            None => (Self::default(), Vec::new()),
        };
        // Individual user layout files win over anything still embedded in the
        // workspace, then adopt legacy presets that have no file yet.
        file.load_user_layouts();
        file.adopt_user_presets(legacy);
        file.ensure_builtins();
        // Strip panels this build no longer supports, then write the cleaned
        // user layouts back so a stale panel is dropped without the layout
        // itself ever being discarded.
        file.sanitize();
        // Floating panes are stored as hidden tiles so docking can restore
        // their original slot; make an older file (where they were removed)
        // match before persisting.
        file.sync_popped_visibility();
        file.write_user_layouts();
        // Rewriting the workspace drops any legacy embedded presets now that
        // they have their own files.
        file.save_workspace();
        file
    }

    fn from_workspace(text: &str) -> (Self, Vec<Preset>) {
        let Ok(ws) = serde_json::from_str::<Workspace>(text) else {
            return (Self::default(), Vec::new());
        };
        if ws.version != LAYOUT_VERSION {
            return (Self::default(), Vec::new());
        }
        let mut file = Self {
            version: LAYOUT_VERSION,
            active_id: ws.active_id,
            draft: ws.draft,
            popped: ws.popped,
            presets: vec![
                Preset {
                    id: DEFAULT_ID.to_string(),
                    name: "Default".to_string(),
                    tree: ws.default_layout,
                },
                Preset {
                    id: DASHBOARD_ID.to_string(),
                    name: "Dashboard".to_string(),
                    tree: ws.dashboard_layout,
                },
            ],
        };
        let mut legacy = Vec::new();
        for preset in ws.legacy_presets {
            match preset.id.as_str() {
                DEFAULT_ID => {
                    if let Some(slot) = file.presets.iter_mut().find(|p| p.id == DEFAULT_ID) {
                        slot.tree = preset.tree;
                    }
                }
                DASHBOARD_ID => {
                    if let Some(slot) = file.presets.iter_mut().find(|p| p.id == DASHBOARD_ID) {
                        slot.tree = preset.tree;
                    }
                }
                _ => legacy.push(preset),
            }
        }
        (file, legacy)
    }

    fn adopt_user_presets(&mut self, presets: Vec<Preset>) {
        for preset in presets {
            if !self.presets.iter().any(|p| p.id == preset.id) {
                self.presets.push(preset);
            }
        }
    }

    fn load_user_layouts(&mut self) {
        for preset in read_user_layouts() {
            if preset.id == DEFAULT_ID || preset.id == DASHBOARD_ID {
                continue;
            }
            if !self.presets.iter().any(|p| p.id == preset.id) {
                self.presets.push(preset);
            }
        }
    }

    /// Make sure the two built-in arrangements exist as ordinary entries in
    /// `presets`. Treating them as presets means a switch to Default or
    /// Dashboard loads the arrangement the user last left there instead of
    /// regenerating it from code, and edits to them are persisted like any
    /// other preset. The menus hide these two ids because they are listed
    /// explicitly.
    pub fn ensure_builtins(&mut self) {
        if !self.presets.iter().any(|p| p.id == DEFAULT_ID) {
            self.presets.push(Preset {
                id: DEFAULT_ID.to_string(),
                name: "Default".to_string(),
                tree: default_tree(),
            });
        }
        if !self.presets.iter().any(|p| p.id == DASHBOARD_ID) {
            self.presets.push(Preset {
                id: DASHBOARD_ID.to_string(),
                name: "Dashboard".to_string(),
                tree: dashboard_tree(),
            });
        }
    }

    /// The user-saved presets, in menu order (built-ins excluded).
    pub fn user_presets(&self) -> impl Iterator<Item = &Preset> {
        self.presets
            .iter()
            .filter(|p| p.id != DEFAULT_ID && p.id != DASHBOARD_ID)
    }

    /// Drop panes this build can no longer show from every tree. A pane the
    /// code no longer knows about deserializes as [`Pane::Unsupported`];
    /// host-only panes (native panels on the web) are unavailable too.
    fn sanitize(&mut self) {
        sanitize_tree(&mut self.draft);
        for preset in &mut self.presets {
            sanitize_tree(&mut preset.tree);
        }
    }

    /// Reconcile the working tree with the floating-pane list: every floating
    /// pane is present in the tree but hidden. Keeping a popped pane as a hidden
    /// tile is what lets docking put it back in exactly the container it left.
    /// A pane missing from an older layout is re-added first so it still has a
    /// slot to return to. Panes hidden for any other reason (closed from the
    /// Panels menu, the tab close button, or their own toggle) are left hidden,
    /// so their slot survives until they are shown again.
    pub fn sync_popped_visibility(&mut self) {
        self.popped.retain(|pane| pane.available(true));
        for pane in self.popped.clone() {
            if !set_pane_visible(&mut self.draft, pane, false) {
                add_pane(&mut self.draft, pane);
                set_pane_visible(&mut self.draft, pane, false);
            }
        }
    }

    pub fn save(&self) {
        self.save_workspace();
        self.write_user_layouts();
    }

    fn save_workspace(&self) {
        let default_layout = self
            .presets
            .iter()
            .find(|p| p.id == DEFAULT_ID)
            .map(|p| p.tree.clone())
            .unwrap_or_else(default_tree);
        let dashboard_layout = self
            .presets
            .iter()
            .find(|p| p.id == DASHBOARD_ID)
            .map(|p| p.tree.clone())
            .unwrap_or_else(dashboard_tree);
        let workspace = Workspace {
            version: LAYOUT_VERSION,
            active_id: self.active_id.clone(),
            draft: self.draft.clone(),
            default_layout,
            dashboard_layout,
            popped: self.popped.clone(),
            legacy_presets: Vec::new(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&workspace) {
            workspace_save(&text);
        }
    }

    fn write_user_layouts(&self) {
        for preset in self.user_presets() {
            if let Ok(text) = serde_json::to_string_pretty(preset) {
                user_layout_write(&preset.id, &text);
            }
        }
    }
}

/// Remove every pane this host cannot show from the tree.
fn sanitize_tree(tree: &mut Tree<Pane>) {
    let stale: Vec<Pane> = tree
        .tiles
        .iter()
        .filter_map(|(_, tile)| match tile {
            Tile::Pane(pane) if !pane.available(true) => Some(*pane),
            _ => None,
        })
        .collect();
    for pane in stale {
        remove_pane(tree, pane);
    }
}

/// Turn a preset id into a safe file name. Reading uses the id stored inside
/// the file, so a lossy mapping is fine.
#[cfg(not(target_arch = "wasm32"))]
fn file_stem(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Delete a user layout's file (native) or localStorage entry (web).
pub fn delete_user_layout(id: &str) {
    user_layout_delete(id)
}

// ---- Native storage -----------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
fn config_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".config/scu-client"))
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_path() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join("layouts.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn user_layouts_dir() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join("layouts"))
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_load() -> Option<String> {
    std::fs::read_to_string(workspace_path()?).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_save(text: &str) {
    let Some(path) = workspace_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

#[cfg(not(target_arch = "wasm32"))]
fn read_user_layouts() -> Vec<Preset> {
    let Some(dir) = user_layouts_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            serde_json::from_str::<Preset>(&text).ok()
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn user_layout_write(id: &str, text: &str) {
    let Some(dir) = user_layouts_dir() else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join(format!("{}.json", file_stem(id))), text);
}

#[cfg(not(target_arch = "wasm32"))]
fn user_layout_delete(id: &str) {
    let Some(dir) = user_layouts_dir() else {
        return;
    };
    let _ = std::fs::remove_file(dir.join(format!("{}.json", file_stem(id))));
}

// ---- Web storage --------------------------------------------------------

#[cfg(target_arch = "wasm32")]
const WORKSPACE_KEY: &str = "scu-client-layouts";

#[cfg(target_arch = "wasm32")]
const USER_LAYOUT_PREFIX: &str = "scu-client-layout-";

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
fn workspace_load() -> Option<String> {
    local_storage()?.get_item(WORKSPACE_KEY).ok()?
}

#[cfg(target_arch = "wasm32")]
fn workspace_save(text: &str) {
    let Some(storage) = local_storage() else {
        return;
    };
    let _ = storage.set_item(WORKSPACE_KEY, text);
}

#[cfg(target_arch = "wasm32")]
fn read_user_layouts() -> Vec<Preset> {
    let Some(storage) = local_storage() else {
        return Vec::new();
    };
    let len = storage.length().unwrap_or(0);
    let mut presets = Vec::new();
    for i in 0..len {
        let Ok(Some(key)) = storage.key(i) else {
            continue;
        };
        if !key.starts_with(USER_LAYOUT_PREFIX) {
            continue;
        }
        if let Ok(Some(text)) = storage.get_item(&key) {
            if let Ok(preset) = serde_json::from_str::<Preset>(&text) {
                presets.push(preset);
            }
        }
    }
    presets
}

#[cfg(target_arch = "wasm32")]
fn user_layout_write(id: &str, text: &str) {
    let Some(storage) = local_storage() else {
        return;
    };
    let _ = storage.set_item(&format!("{USER_LAYOUT_PREFIX}{id}"), text);
}

#[cfg(target_arch = "wasm32")]
fn user_layout_delete(id: &str) {
    let Some(storage) = local_storage() else {
        return;
    };
    let _ = storage.remove_item(&format!("{USER_LAYOUT_PREFIX}{id}"));
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
    tree.tiles.iter().find_map(|(id, tile)| match tile {
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
        .find(|id| {
            matches!(
                tree.tiles.get(*id),
                Some(Tile::Container(Container::Tabs(_)))
            )
        })
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

/// Show or hide `pane` without removing it from the tree. A hidden tile keeps
/// its place in the hierarchy, so a pane that has been popped out into its own
/// window returns to exactly the container (tab set or split) it came from when
/// it is docked again. Returns `false` if the pane is not in the tree.
pub fn set_pane_visible(tree: &mut Tree<Pane>, pane: Pane, visible: bool) -> bool {
    let Some(id) = pane_tile(tree, pane) else {
        return false;
    };
    tree.tiles.set_visible(id, visible);
    true
}

/// Whether `pane` is currently shown (present and visible) in the tree.
pub fn pane_open(tree: &Tree<Pane>, pane: Pane) -> bool {
    pane_tile(tree, pane).is_some_and(|id| tree.tiles.is_visible(id))
}

/// Open or close `pane`, keeping its tile in the tree either way so that a
/// closed pane reopens in the same place. Opening a pane that has no remembered
/// slot (it was never in this arrangement, or the tile was removed) falls back
/// to adding it to the active tab set, as before.
pub fn set_pane_open(tree: &mut Tree<Pane>, pane: Pane, open: bool) {
    if open {
        if !set_pane_visible(tree, pane, true) {
            add_pane(tree, pane);
        }
    } else {
        set_pane_visible(tree, pane, false);
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
        // A zero horizontal padding plus a square `min_size` gives the icon a
        // square button instead of a wide rectangle.
        let clicked = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                ui.add(
                    egui::Button::new(egui::RichText::new("\u{2b08}").size(9.0))
                        .small()
                        .min_size(egui::Vec2::splat(18.0)),
                )
                .on_hover_text("Open this panel in its own window")
                .clicked()
            })
            .inner;
        if clicked {
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
        _state: &egui_tiles::TabState,
    ) -> Stroke {
        Stroke::NONE
    }

    /// Mirror of the default `egui_tiles` tab UI, with one change: the active
    /// tab's title is painted twice with a sub-pixel offset to fake a heavier
    /// weight. egui has no bold font face (`RichText::strong` only tweaks the
    /// colour), so this is how we add a little weight to the selected tab.
    fn tab_ui(
        &mut self,
        tiles: &mut Tiles<Pane>,
        ui: &mut egui::Ui,
        id: egui::Id,
        tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Response {
        let text = self.tab_title_for_tile(tiles, tile_id);
        let close_btn_size = egui::Vec2::splat(self.close_button_outer_size());
        let close_btn_left_padding = 4.0;
        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

        let x_margin = self.tab_title_spacing(ui.visuals());

        let button_width = galley.size().x
            + 2.0 * x_margin
            + f32::from(state.closable) * (close_btn_left_padding + close_btn_size.x);
        let (_, tab_rect) = ui.allocate_space(egui::vec2(button_width, ui.available_height()));

        let draggable = self.is_tile_draggable(tiles, tile_id);
        let sense = if draggable {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        let tab_response = ui.interact(tab_rect, id, sense);
        let tab_response = if draggable {
            tab_response.on_hover_cursor(self.tab_hover_cursor_icon())
        } else {
            tab_response
        };

        tab_response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                ui.is_enabled(),
                state.active,
                galley.text(),
            )
        });

        if ui.is_rect_visible(tab_rect) && !state.is_being_dragged {
            let bg_color = self.tab_bg_color(ui.visuals(), tiles, tile_id, state);
            let stroke = self.tab_outline_stroke(ui.visuals(), tiles, tile_id, state);
            ui.painter().rect(
                tab_rect.shrink(0.5),
                0.0,
                bg_color,
                stroke,
                egui::StrokeKind::Inside,
            );

            if state.active {
                // Make the tab name area connect with the tab ui area:
                ui.painter().hline(
                    tab_rect.x_range(),
                    tab_rect.bottom(),
                    Stroke::new(stroke.width + 1.0, bg_color),
                );
            }

            let text_color = self.tab_text_color(ui.visuals(), tiles, tile_id, state);
            let text_position = egui::Align2::LEFT_CENTER
                .align_size_within_rect(galley.size(), tab_rect.shrink(x_margin))
                .min;

            ui.painter()
                .galley(text_position, galley.clone(), text_color);
            if state.active {
                ui.painter()
                    .galley(text_position + egui::vec2(0.5, 0.0), galley, text_color);
            }

            if state.closable {
                let close_btn_rect = egui::Align2::RIGHT_CENTER
                    .align_size_within_rect(close_btn_size, tab_rect.shrink(x_margin));

                let close_btn_id = ui.auto_id_with("tab_close_btn");
                let close_btn_response = ui
                    .interact(close_btn_rect, close_btn_id, egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Default);

                close_btn_response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Close")
                });

                let visuals = ui.style().interact(&close_btn_response);

                let rect = close_btn_rect
                    .shrink(self.close_button_inner_margin())
                    .expand(visuals.expansion);
                let stroke = visuals.fg_stroke;

                ui.painter()
                    .line_segment([rect.left_top(), rect.right_bottom()], stroke);
                ui.painter()
                    .line_segment([rect.right_top(), rect.left_bottom()], stroke);

                if (close_btn_response.clicked()
                    || tab_response.clicked_by(egui::PointerButton::Middle))
                    && self.on_tab_close(tiles, tile_id)
                {
                    tiles.remove(tile_id);
                }
            }
        }

        self.on_tab_button(tiles, tile_id, tab_response)
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

    /// Closing a pane's tab hides the pane in place (rather than deleting its
    /// tile) so that re-enabling it from the Panels menu restores its slot.
    /// Containers keep the default removal.
    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        let pane = match tiles.get(tile_id) {
            Some(Tile::Pane(pane)) => *pane,
            _ => return true,
        };
        self.app.request_close_pane(pane);
        false
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
        assert_eq!(restored.user_presets().count(), 1);
        assert_eq!(restored.active_id, DEFAULT_ID);
        assert_eq!(restored.popped, vec![Pane::Meters]);
        assert!(restored.presets.iter().any(|p| p.id == DEFAULT_ID));
        assert!(restored.presets.iter().any(|p| p.id == DASHBOARD_ID));
    }

    #[test]
    fn builtins_are_seeded_for_old_files() {
        // A file written before built-ins were stored as presets has no entries
        // with the built-in ids; seeding must add them without touching the
        // user's working draft.
        let mut file = LayoutsFile {
            version: LAYOUT_VERSION,
            active_id: DEFAULT_ID.to_string(),
            draft: dashboard_tree(),
            presets: Vec::new(),
            popped: Vec::new(),
        };
        file.ensure_builtins();
        assert_eq!(file.draft, dashboard_tree());
        assert!(file.presets.iter().any(|p| p.id == DEFAULT_ID));
        assert!(file.presets.iter().any(|p| p.id == DASHBOARD_ID));
    }

    #[test]
    fn unknown_pane_deserializes_as_unsupported() {
        // A layout written by a build that had a panel this one does not know
        // about must still deserialize rather than collapsing to the default.
        let pane: Pane = serde_json::from_str("\"SomeRemovedPanel\"").expect("deserialize");
        assert_eq!(pane, Pane::Unsupported);
        assert!(!pane.available(true));
    }

    #[test]
    fn sanitize_drops_unsupported_panes() {
        let mut tree = default_tree();
        add_pane(&mut tree, Pane::Unsupported);
        assert!(pane_tile(&tree, Pane::Unsupported).is_some());
        sanitize_tree(&mut tree);
        assert!(pane_tile(&tree, Pane::Unsupported).is_none());
        // The supported panes are untouched.
        assert!(pane_tile(&tree, Pane::Panadapter).is_some());
    }

    #[test]
    fn legacy_user_presets_migrate_out_of_the_workspace() {
        let default = serde_json::to_value(default_tree()).expect("value");
        let dashboard = serde_json::to_value(dashboard_tree()).expect("value");
        let legacy = serde_json::json!({
            "version": LAYOUT_VERSION,
            "active_id": DEFAULT_ID,
            "draft": default.clone(),
            "presets": [
                { "id": DEFAULT_ID, "name": "Default", "tree": default },
                { "id": "layout-1", "name": "Mine", "tree": dashboard },
            ],
            "popped": [],
        });
        let text = serde_json::to_string(&legacy).expect("serialize");
        let (file, migrated) = LayoutsFile::from_workspace(&text);
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].id, "layout-1");
        // Built-ins are always rebuilt in memory from the workspace fields.
        assert!(file.presets.iter().any(|p| p.id == DEFAULT_ID));
        assert!(file.presets.iter().any(|p| p.id == DASHBOARD_ID));
    }

    #[test]
    fn hiding_a_popped_pane_keeps_its_slot() {
        let mut tree = default_tree();
        // Tuning shares a tab set with Radio and Clarifier in the default tree.
        let slot = pane_tile(&tree, Pane::Tuning).expect("Tuning present");
        assert!(set_pane_visible(&mut tree, Pane::Tuning, false));
        assert!(!tree.tiles.is_visible(slot));
        // Simplifying must not prune the hidden pane or collapse its tab set.
        tree.simplify(&egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        });
        assert_eq!(pane_tile(&tree, Pane::Tuning), Some(slot));
        // Docking is exactly making it visible again, in the same slot.
        assert!(set_pane_visible(&mut tree, Pane::Tuning, true));
        assert!(tree.tiles.is_visible(slot));
    }

    #[test]
    fn sync_rehides_popped_panes_and_leaves_closed_ones_hidden() {
        let mut file = LayoutsFile::default();
        // Tuning was removed outright by an older layout, yet is still floating.
        remove_pane(&mut file.draft, Pane::Tuning);
        file.popped.push(Pane::Tuning);
        // Meters was closed from the menu (hidden, not floating); it must stay
        // hidden so reopening it later can restore its slot.
        set_pane_visible(&mut file.draft, Pane::Meters, false);
        file.sync_popped_visibility();
        let tuning = pane_tile(&file.draft, Pane::Tuning).expect("re-added");
        assert!(!file.draft.tiles.is_visible(tuning));
        let meters = pane_tile(&file.draft, Pane::Meters).expect("present");
        assert!(!file.draft.tiles.is_visible(meters));
    }

    #[test]
    fn closing_a_pane_keeps_its_slot_for_reopening() {
        let mut tree = default_tree();
        // Tuning shares a tab set with Radio and Clarifier in the default tree.
        let slot = pane_tile(&tree, Pane::Tuning).expect("Tuning present");
        assert!(pane_open(&tree, Pane::Tuning));
        set_pane_open(&mut tree, Pane::Tuning, false);
        assert!(!pane_open(&tree, Pane::Tuning));
        assert_eq!(pane_tile(&tree, Pane::Tuning), Some(slot));
        // A closed pane is not pruned by normalization.
        tree.simplify(&egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        });
        set_pane_open(&mut tree, Pane::Tuning, true);
        assert!(pane_open(&tree, Pane::Tuning));
        assert_eq!(pane_tile(&tree, Pane::Tuning), Some(slot));
    }

    #[test]
    fn opening_a_pane_without_a_slot_adds_it() {
        let mut tree = default_tree();
        remove_pane(&mut tree, Pane::Vox);
        assert!(!pane_open(&tree, Pane::Vox));
        set_pane_open(&mut tree, Pane::Vox, true);
        assert!(pane_open(&tree, Pane::Vox));
    }

    #[test]
    fn file_stem_keeps_ids_inside_the_directory() {
        assert_eq!(file_stem("layout-1"), "layout-1");
        assert_eq!(file_stem("../escape"), "___escape");
        assert_eq!(file_stem("a/b c"), "a_b_c");
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
        add_pane(&mut tree, Pane::Panadapter);
        assert!(!tree.is_empty());
        assert!(pane_tile(&tree, Pane::Panadapter).is_some());
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

        // Every pane in the default arrangement should still be reachable
        // after a layout pass. The standalone Spectrum/Waterfall panes exist
        // for custom layouts but are intentionally absent from the default.
        for pane in Pane::ALL {
            if matches!(pane, Pane::Spectrum | Pane::Waterfall) {
                assert!(pane_tile(&tree, pane).is_none(), "unexpected {pane:?}");
                continue;
            }
            assert!(pane_tile(&tree, pane).is_some(), "missing {pane:?}");
        }
    }
}
