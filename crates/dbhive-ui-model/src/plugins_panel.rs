//! The plugin manager panel as headless state, for the Slint shell (and any
//! later shell).
//!
//! [`crate::plugins`] holds the few small helpers the egui screen already
//! shared (the tab set, carrying field edits across a re-render, install
//! dates). The plugin engine, the marketplace catalogue and the plugin store
//! stay in `dbhive_core::plugins`. This module is what a retained-mode shell
//! needs on top of them and the egui shell kept inline in its draw code:
//!
//! - [`PluginsPanel`], the state: what is installed, what is busy, the
//!   marketplace filters, the interface each plugin last described, the
//!   settings form;
//! - flat, toolkit-free row types ([`InstalledRow`], [`MarketRow`],
//!   [`DetailView`], [`ItemRow`], [`ToolbarEntry`]) a screen binds to;
//! - [`absorb`], which turns what a plugin asked the host to do (the outbox)
//!   into toasts and a replacement interface.
//!
//! # What a plugin can do here
//!
//! Only describe an interface as data. [`ItemRow`] is a flattened copy of the
//! closed vocabulary in `dbhive_core::plugins::ui`: it carries text, a style
//! role, a value. No colour, no markup, no image, no URL to open. A plugin
//! cannot reach Slint, and nothing it sends can name a callback the host did
//! not declare: events are resolved against the descriptor on screen by
//! `PluginManager::dispatch_ui_event` before any plugin code runs.
//!
//! No toolkit types.

use std::collections::{BTreeMap, HashMap, HashSet};

use dbhive_core::plugins::manifest::{Plugin, PluginCategory};
use dbhive_core::plugins::marketplace::{self, Filters, MarketplacePlugin, SortOption};
use dbhive_core::plugins::ui::{
    self as descriptor, ButtonStyle, Fields, FieldValue, Icon, Item, PluginUi, TextStyle, UiEvent,
};
use dbhive_core::plugins::{HostCall, NotificationKind};

use crate::plugins::{merge_fields, short_date, Tab};

/// A table in a plugin's interface is drawn without virtualisation, so only
/// the first rows are shown and the rest are counted.
pub const MAX_TABLE_ROWS_SHOWN: usize = 200;

/// How many plugin buttons a screen header shows. The workspace header already
/// carries six of the application's own.
pub const HEADER_TOOLBAR_LIMIT: usize = 3;

// ─────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────

/// Whether the plugin directory has been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load {
    Loading,
    Ready,
    Failed(String),
}

/// What is in flight for one plugin. One operation per plugin at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Installing,
    Toggling,
    Uninstalling,
    Saving,
    /// Loading the plugin and reading its interface in the background.
    Starting,
    /// The user asked to open the plugin's interface.
    Opening,
    /// A plugin handler is running.
    Running,
}

/// Which item list an edit or a press belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The interface the plugin described.
    View,
    /// The settings form built from the manifest's `configSchema`.
    Form,
}

/// The interface one plugin last described, with the user's edits.
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub descriptor: PluginUi,
    pub fields: Fields,
    /// Why the last call into the plugin failed.
    pub error: Option<String>,
    /// Panel indexes the user folded.
    pub collapsed: HashSet<usize>,
    /// Number fields whose text did not parse, with the reason.
    pub bad_numbers: HashMap<String, String>,
}

impl Interface {
    pub fn new(descriptor: PluginUi) -> Self {
        Self {
            fields: descriptor.initial_fields(),
            collapsed: initially_folded(&descriptor, 0),
            descriptor,
            error: None,
            bad_numbers: HashMap::new(),
        }
    }

    /// Take a new descriptor from the plugin, keeping what the user typed
    /// into fields it still declares (`merge_fields`).
    fn replace(&mut self, next: PluginUi) {
        self.fields = merge_fields(&next, &self.fields);
        self.bad_numbers.retain(|id, _| next.has_field(id));
        // The user's folds stay; panels that are new start as the plugin asked.
        let known = self.descriptor.panels.len();
        self.collapsed.retain(|i| *i < next.panels.len());
        self.collapsed.extend(initially_folded(&next, known));
        self.descriptor = next;
        self.error = None;
    }
}

/// Collapsible panels from index `from` on that the plugin wants folded.
fn initially_folded(ui: &PluginUi, from: usize) -> HashSet<usize> {
    ui.panels
        .iter()
        .enumerate()
        .skip(from)
        .filter(|(_, p)| p.collapsible && !p.open)
        .map(|(i, _)| i)
        .collect()
}

/// The settings form for one plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsForm {
    pub plugin_id: String,
    pub plugin_name: String,
    pub interface: Interface,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginsPanel {
    pub tab: Tab,
    pub load: Load,
    pub installed: Vec<Plugin>,
    /// Plugins that are on disk but could not be loaded, one line each.
    pub problems: Vec<String>,
    pub selected: Option<String>,
    pub filters: Filters,
    pub catalog: Vec<MarketplacePlugin>,
    pub busy: HashMap<String, Busy>,
    /// Why the last install of a marketplace entry failed, per plugin id.
    pub install_errors: HashMap<String, String>,
    /// The interface each loaded plugin last described. Kept after the panel
    /// closes: a plugin's toolbar buttons come from here.
    pub interfaces: BTreeMap<String, Interface>,
    /// The plugin whose interface fills the detail pane.
    pub open_view: Option<String>,
    pub form: Option<SettingsForm>,
}

impl Default for PluginsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginsPanel {
    pub fn new() -> Self {
        Self::with_catalog(marketplace::catalog())
    }

    pub fn with_catalog(catalog: Vec<MarketplacePlugin>) -> Self {
        Self {
            tab: Tab::Installed,
            load: Load::Loading,
            installed: Vec::new(),
            problems: Vec::new(),
            selected: None,
            filters: Filters::default(),
            catalog,
            busy: HashMap::new(),
            install_errors: HashMap::new(),
            interfaces: BTreeMap::new(),
            open_view: None,
            form: None,
        }
    }

    pub fn find(&self, id: &str) -> Option<&Plugin> {
        self.installed.iter().find(|p| p.manifest.id == id)
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.find(id).is_some()
    }

    pub fn market_entry(&self, id: &str) -> Option<&MarketplacePlugin> {
        self.catalog.iter().find(|p| p.manifest.id == id)
    }

    pub fn is_busy(&self, id: &str) -> bool {
        self.busy.contains_key(id)
    }

    // -- Loading ------------------------------------------------------------

    pub fn begin_load(&mut self) {
        self.load = Load::Loading;
    }

    /// Take the result of reading the plugin directory.
    ///
    /// The selection survives when its plugin does; otherwise the first
    /// plugin is selected. Interfaces of plugins that are gone or disabled
    /// are dropped, so a plugin cannot leave a toolbar button behind.
    pub fn apply_scan(&mut self, result: Result<(Vec<Plugin>, Vec<String>), String>) {
        match result {
            Ok((plugins, problems)) => {
                self.installed = plugins;
                self.problems = problems;
                self.load = Load::Ready;
                self.prune();
            }
            Err(e) => {
                self.problems.clear();
                self.load = Load::Failed(e);
            }
        }
    }

    /// Re-establish the invariants after the installed set changed.
    pub fn prune(&mut self) {
        let live: HashSet<String> = self
            .installed
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.manifest.id.clone())
            .collect();
        self.interfaces.retain(|id, _| live.contains(id));
        if self.open_view.as_ref().is_some_and(|id| !live.contains(id)) {
            self.open_view = None;
        }
        if self
            .form
            .as_ref()
            .is_some_and(|f| !self.installed.iter().any(|p| p.manifest.id == f.plugin_id))
        {
            self.form = None;
        }
        let known = self.selected.as_ref().is_some_and(|id| self.is_installed(id));
        if !known {
            self.selected = self.installed.first().map(|p| p.manifest.id.clone());
        }
        let installed: HashSet<&str> = self.installed.iter().map(|p| p.manifest.id.as_str()).collect();
        self.install_errors.retain(|id, _| !installed.contains(id.as_str()));
    }

    /// Replace one plugin after an operation returned its new state.
    pub fn upsert(&mut self, plugin: Plugin) {
        match self.installed.iter().position(|p| p.manifest.id == plugin.manifest.id) {
            Some(i) => self.installed[i] = plugin,
            None => {
                self.installed.push(plugin);
                self.installed
                    .sort_by(|a, b| a.manifest.name.to_lowercase().cmp(&b.manifest.name.to_lowercase()));
            }
        }
        self.prune();
    }

    pub fn remove(&mut self, id: &str) {
        self.installed.retain(|p| p.manifest.id != id);
        self.busy.remove(id);
        self.prune();
    }

    // -- Selection and tabs ---------------------------------------------------

    pub fn select(&mut self, id: &str) {
        if self.selected.as_deref() != Some(id) && self.is_installed(id) {
            self.selected = Some(id.to_string());
            self.open_view = None;
            self.form = None;
        }
    }

    // -- Install state --------------------------------------------------------

    pub fn install_state(&self, id: &str) -> InstallState {
        if self.is_installed(id) {
            InstallState::Installed
        } else if self.busy.get(id) == Some(&Busy::Installing) {
            InstallState::Installing
        } else if let Some(e) = self.install_errors.get(id) {
            InstallState::Failed(e.clone())
        } else {
            InstallState::Available
        }
    }

    // -- Interfaces -----------------------------------------------------------

    /// A plugin described its interface (opening it, or after an event).
    pub fn set_interface(&mut self, plugin_id: &str, next: PluginUi) {
        match self.interfaces.get_mut(plugin_id) {
            Some(existing) => existing.replace(next),
            None => {
                self.interfaces.insert(plugin_id.to_string(), Interface::new(next));
            }
        }
    }

    /// Record that a call into the plugin failed, on its interface if it has
    /// one, so the pane can show the reason beside the controls.
    pub fn set_interface_error(&mut self, plugin_id: &str, error: String) {
        if let Some(i) = self.interfaces.get_mut(plugin_id) {
            i.error = Some(error);
        }
    }

    /// Opening a plugin's interface failed. A plugin that never described one
    /// gets an empty interface to carry the reason, so the pane has somewhere
    /// to show it; it has no toolbar buttons.
    pub fn fail_interface(&mut self, plugin_id: &str, error: String) {
        self.interfaces
            .entry(plugin_id.to_string())
            .or_insert_with(|| Interface::new(PluginUi::default()))
            .error = Some(error);
    }

    fn interface(&self, scope: Scope) -> Option<&Interface> {
        match scope {
            Scope::View => self.open_view.as_ref().and_then(|id| self.interfaces.get(id)),
            Scope::Form => self.form.as_ref().map(|f| &f.interface),
        }
    }

    fn interface_mut(&mut self, scope: Scope) -> Option<&mut Interface> {
        match scope {
            Scope::View => {
                let id = self.open_view.clone()?;
                self.interfaces.get_mut(&id)
            }
            Scope::Form => self.form.as_mut().map(|f| &mut f.interface),
        }
    }

    /// The title above the open interface.
    pub fn view_title(&self) -> String {
        self.interface(Scope::View)
            .and_then(|i| i.descriptor.title.clone())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "Plugin interface".to_string())
    }

    pub fn view_error(&self) -> String {
        self.interface(Scope::View)
            .and_then(|i| i.error.clone())
            .unwrap_or_default()
    }

    /// The buttons the open interface declares in its own toolbar.
    pub fn view_toolbar(&self) -> Vec<ToolbarEntry> {
        let (Some(id), Some(i)) = (self.open_view.as_ref(), self.interface(Scope::View)) else {
            return Vec::new();
        };
        let name = self.find(id).map(|p| p.manifest.name.clone()).unwrap_or_default();
        toolbar_of(id, &name, &i.descriptor)
    }

    pub fn items(&self, scope: Scope) -> Vec<ItemRow> {
        self.interface(scope).map(item_rows).unwrap_or_default()
    }

    pub fn toggle_panel(&mut self, scope: Scope, index: usize) {
        if let Some(i) = self.interface_mut(scope) {
            if !i.collapsed.remove(&index) {
                i.collapsed.insert(index);
            }
        }
    }

    // -- Field edits ------------------------------------------------------------

    /// Store text typed into an input or a select-by-text field.
    pub fn set_text(&mut self, scope: Scope, id: &str, text: &str) {
        if let Some(i) = self.interface_mut(scope) {
            if i.descriptor.has_field(id) {
                i.fields.insert(id.to_string(), FieldValue::Text(text.to_string()));
            }
        }
    }

    /// Flip a checkbox. Returns the new value.
    pub fn toggle_check(&mut self, scope: Scope, id: &str) -> Option<FieldValue> {
        let i = self.interface_mut(scope)?;
        if !i.descriptor.has_field(id) {
            return None;
        }
        let now = !matches!(i.fields.get(id), Some(FieldValue::Bool(true)));
        let v = FieldValue::Bool(now);
        i.fields.insert(id.to_string(), v.clone());
        Some(v)
    }

    /// Choose option `index` of a select. Returns the new value, or `None`
    /// when it did not change.
    pub fn pick_option(&mut self, scope: Scope, id: &str, index: usize) -> Option<FieldValue> {
        let i = self.interface_mut(scope)?;
        let value = select_options(&i.descriptor, id)?.get(index)?.value.clone();
        let current = i.fields.get(id).map(|v| v.as_text());
        if current.as_deref() == Some(value.as_str()) {
            return None;
        }
        let v = FieldValue::Text(value);
        i.fields.insert(id.to_string(), v.clone());
        Some(v)
    }

    /// Parse text typed into a number field. A value that does not parse keeps
    /// the old number and is flagged; one outside the declared range is
    /// clamped into it. Returns the stored value when it changed.
    pub fn set_number(&mut self, scope: Scope, id: &str, text: &str) -> Option<FieldValue> {
        let i = self.interface_mut(scope)?;
        let (min, max) = number_range(&i.descriptor, id)?;
        match parse_number(text, min, max) {
            Ok(n) => {
                i.bad_numbers.remove(id);
                let v = FieldValue::Number(n);
                if i.fields.get(id) == Some(&v) {
                    None
                } else {
                    i.fields.insert(id.to_string(), v.clone());
                    Some(v)
                }
            }
            Err(reason) => {
                i.bad_numbers.insert(id.to_string(), reason);
                None
            }
        }
    }

    // -- Events -----------------------------------------------------------------

    /// A press of the button `id` in the open interface, carrying every
    /// field's current value so a handler does not have to ask.
    pub fn button_event(&self, scope: Scope, id: &str) -> Option<(PluginUi, UiEvent)> {
        let i = self.interface(scope)?;
        Some((
            i.descriptor.clone(),
            UiEvent::ButtonPressed {
                id: id.to_string(),
                fields: i.fields.clone(),
            },
        ))
    }

    /// A press of a toolbar button from any enabled plugin that has an
    /// interface, whether or not the panel is open.
    pub fn toolbar_event(&self, plugin_id: &str, item_id: &str) -> Option<(PluginUi, UiEvent)> {
        let i = self.interfaces.get(plugin_id)?;
        if !self.find(plugin_id).is_some_and(|p| p.enabled) {
            return None;
        }
        Some((
            i.descriptor.clone(),
            UiEvent::ButtonPressed {
                id: item_id.to_string(),
                fields: i.fields.clone(),
            },
        ))
    }

    /// The field `id` changed to its stored value: the event to tell the
    /// plugin, or `None` when the descriptor does not declare that field.
    pub fn value_event(&self, id: &str) -> Option<(PluginUi, UiEvent)> {
        let i = self.interface(Scope::View)?;
        let value = i.fields.get(id)?.clone();
        i.descriptor.has_field(id).then(|| {
            (
                i.descriptor.clone(),
                UiEvent::ValueChanged {
                    id: id.to_string(),
                    value,
                },
            )
        })
    }

    // -- Settings form ----------------------------------------------------------

    /// Build the settings form from the manifest's `configSchema`, with the
    /// saved values. Pure data: no plugin code runs.
    pub fn open_settings(&mut self, plugin_id: &str) -> bool {
        let Some(plugin) = self.find(plugin_id) else { return false };
        let Some(schema) = plugin.manifest.config_schema.clone() else { return false };
        let panel = descriptor::config_schema_panel(&schema, plugin.config.as_ref());
        let ui = PluginUi {
            title: None,
            toolbar: Vec::new(),
            menu: Vec::new(),
            panels: vec![panel],
        };
        self.form = Some(SettingsForm {
            plugin_id: plugin.manifest.id.clone(),
            plugin_name: plugin.manifest.name.clone(),
            interface: Interface::new(ui),
        });
        true
    }

    pub fn close_settings(&mut self) {
        self.form = None;
    }

    /// The plugin and the config object to save, or why it cannot be saved.
    pub fn settings_to_save(&self) -> Result<(String, serde_json::Value), String> {
        let form = self.form.as_ref().ok_or("No settings form is open")?;
        if let Some((id, reason)) = form.interface.bad_numbers.iter().next() {
            return Err(format!("{}: {}", id, reason));
        }
        Ok((form.plugin_id.clone(), descriptor::fields_to_config(&form.interface.fields)))
    }

    // -- Projections --------------------------------------------------------------

    pub fn installed_rows(&self) -> Vec<InstalledRow> {
        self.installed
            .iter()
            .map(|p| InstalledRow {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                subtitle: format!("v{} · {}", p.manifest.version, p.manifest.author.name),
                category: p.manifest.category.label().to_string(),
                enabled: p.enabled,
                busy: self.busy.get(&p.manifest.id).copied().map(busy_label),
                errors: p.stats.error_count,
                selected: self.selected.as_deref() == Some(p.manifest.id.as_str()),
            })
            .collect()
    }

    pub fn installed_summary(&self) -> String {
        let n = self.installed.len();
        let on = self.installed.iter().filter(|p| p.enabled).count();
        match n {
            0 => "No plugins installed".to_string(),
            _ => format!("{} installed, {} enabled", n, on),
        }
    }

    /// The marketplace listing after the current search, category, verified
    /// filter and sort.
    pub fn market_rows(&self) -> Vec<MarketRow> {
        marketplace::filter_and_sort(&self.catalog, &self.filters)
            .into_iter()
            .map(|e| {
                let id = e.manifest.id.clone();
                let state = self.install_state(&id);
                let perms = e.manifest.declared_permissions();
                MarketRow {
                    name: e.manifest.name.clone(),
                    byline: format!("v{} · {}", e.manifest.version, e.manifest.author.name),
                    description: e.manifest.description.clone(),
                    meta: format!(
                        "{} downloads · {:.1} rating · {}",
                        marketplace::format_downloads(e.stats.downloads),
                        e.stats.rating,
                        marketplace::format_size(e.size)
                    ),
                    category: e.manifest.category.label().to_string(),
                    keywords: e.manifest.keywords.iter().take(4).cloned().collect(),
                    requests: if perms.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "Requests: {}",
                            dbhive_core::plugins::permission_summary(&e.manifest.permissions)
                        )
                    },
                    sensitive: perms.iter().any(|p| p.is_sensitive()),
                    verified: e.verified,
                    state_kind: state.kind(),
                    state_note: match &state {
                        InstallState::Failed(m) => m.clone(),
                        _ => String::new(),
                    },
                    id,
                }
            })
            .collect()
    }

    pub fn market_summary(&self, shown: usize) -> String {
        let total = self.catalog.len();
        if shown == total {
            format!("{} plugin{}", total, if total == 1 { "" } else { "s" })
        } else {
            format!("{} of {} plugins", shown, total)
        }
    }

    pub fn filters_active(&self) -> bool {
        !self.filters.search.trim().is_empty()
            || self.filters.category.is_some()
            || self.filters.verified_only
    }

    pub fn clear_filters(&mut self) {
        let sort = self.filters.sort;
        self.filters = Filters::default();
        self.filters.sort = sort;
    }

    /// "All categories" followed by every category, in `PluginCategory::ALL`
    /// order. Index 0 is "all".
    pub fn category_options() -> Vec<String> {
        std::iter::once("All categories".to_string())
            .chain(PluginCategory::ALL.iter().map(|c| c.label().to_string()))
            .collect()
    }

    pub fn category_index(&self) -> i32 {
        self.filters
            .category
            .and_then(|c| PluginCategory::ALL.iter().position(|x| *x == c))
            .map_or(0, |i| i as i32 + 1)
    }

    pub fn set_category_index(&mut self, index: usize) {
        self.filters.category = index.checked_sub(1).and_then(|i| PluginCategory::ALL.get(i).copied());
    }

    pub fn sort_options() -> Vec<String> {
        SortOption::ALL.iter().map(|s| s.label().to_string()).collect()
    }

    pub fn sort_index(&self) -> i32 {
        SortOption::ALL.iter().position(|s| *s == self.filters.sort).unwrap_or(0) as i32
    }

    pub fn set_sort_index(&mut self, index: usize) {
        if let Some(s) = SortOption::ALL.get(index) {
            self.filters.sort = *s;
        }
    }

    /// The detail pane for the selected plugin.
    pub fn detail(&self) -> Option<DetailView> {
        let p = self.find(self.selected.as_deref()?)?;
        let m = &p.manifest;
        let id = m.id.clone();
        let mut links = Vec::new();
        if let Some(h) = m.homepage.as_ref().filter(|s| !s.is_empty()) {
            links.push(("Homepage".to_string(), h.clone()));
        }
        if let Some(r) = m.repository.as_ref().filter(|s| !s.is_empty()) {
            links.push(("Repository".to_string(), r.clone()));
        }
        let mut info = vec![
            ("Author".to_string(), m.author.name.clone()),
            ("License".to_string(), or_dash(&m.license)),
            ("Category".to_string(), m.category.label().to_string()),
            ("Type".to_string(), m.plugin_type.label().to_string()),
            ("Identifier".to_string(), m.id.clone()),
        ];
        if let Some(email) = m.author.email.as_ref().filter(|s| !s.is_empty()) {
            info.insert(1, ("Contact".to_string(), email.clone()));
        }
        let mut stats = vec![
            ("Installed".to_string(), or_dash(&short_date(&p.stats.install_date))),
            ("Executions".to_string(), p.stats.execution_count.to_string()),
            ("Errors".to_string(), p.stats.error_count.to_string()),
        ];
        if let Some(last) = p.stats.last_used.as_ref() {
            stats.push(("Last used".to_string(), short_date(last)));
        }
        Some(DetailView {
            name: m.name.clone(),
            version: format!("v{}", m.version),
            description: m.description.clone(),
            enabled: p.enabled,
            busy: self.busy.get(&id).copied().map(busy_label),
            // Opening the interface runs the plugin, which only an enabled
            // plugin may do.
            can_open: p.enabled && !self.is_busy(&id),
            has_settings: m.config_schema.is_some(),
            info,
            stats,
            has_errors: p.stats.error_count > 0,
            permissions: m
                .declared_permissions()
                .into_iter()
                .map(|perm| PermissionView {
                    label: perm.label().to_string(),
                    detail: perm.detail().to_string(),
                    sensitive: perm.is_sensitive(),
                })
                .collect(),
            keywords: m.keywords.clone(),
            links,
            interface_open: self.open_view.as_deref() == Some(id.as_str()),
            id,
        })
    }

    /// The buttons that fit in a screen's header: the first
    /// [`HEADER_TOOLBAR_LIMIT`] of [`Self::toolbar`]. The rest stay reachable
    /// in the plugin's own interface, so a chatty plugin cannot push the
    /// application's own buttons out of the header.
    pub fn header_toolbar(&self) -> Vec<ToolbarEntry> {
        self.toolbar().into_iter().take(HEADER_TOOLBAR_LIMIT).collect()
    }

    /// Toolbar buttons of every enabled plugin that has described an
    /// interface, in plugin-id order.
    pub fn toolbar(&self) -> Vec<ToolbarEntry> {
        self.interfaces
            .iter()
            .filter_map(|(id, i)| {
                let p = self.find(id).filter(|p| p.enabled)?;
                Some(toolbar_of(id, &p.manifest.name, &i.descriptor))
            })
            .flatten()
            .collect()
    }
}

fn or_dash(s: &str) -> String {
    if s.trim().is_empty() {
        "-".to_string()
    } else {
        s.to_string()
    }
}

pub fn busy_label(b: Busy) -> &'static str {
    match b {
        Busy::Installing => "Installing…",
        Busy::Toggling => "Updating…",
        Busy::Uninstalling => "Removing…",
        Busy::Saving => "Saving…",
        Busy::Starting => "Starting…",
        Busy::Opening => "Opening…",
        Busy::Running => "Running…",
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallState {
    Available,
    Installing,
    Installed,
    Failed(String),
}

impl InstallState {
    /// 0 available, 1 installing, 2 installed, 3 failed.
    pub fn kind(&self) -> i32 {
        match self {
            InstallState::Available => 0,
            InstallState::Installing => 1,
            InstallState::Installed => 2,
            InstallState::Failed(_) => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledRow {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub category: String,
    pub enabled: bool,
    /// What is in flight, as a label, when something is.
    pub busy: Option<&'static str>,
    pub errors: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketRow {
    pub id: String,
    pub name: String,
    pub byline: String,
    pub description: String,
    pub meta: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub requests: String,
    pub sensitive: bool,
    pub verified: bool,
    /// See [`InstallState::kind`].
    pub state_kind: i32,
    pub state_note: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionView {
    pub label: String,
    pub detail: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetailView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub busy: Option<&'static str>,
    pub can_open: bool,
    pub has_settings: bool,
    pub info: Vec<(String, String)>,
    pub stats: Vec<(String, String)>,
    pub has_errors: bool,
    pub permissions: Vec<PermissionView>,
    pub keywords: Vec<String>,
    pub links: Vec<(String, String)>,
    pub interface_open: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarEntry {
    pub plugin_id: String,
    pub plugin_name: String,
    pub id: String,
    pub label: String,
    pub tooltip: String,
    /// See [`icon_index`].
    pub icon: i32,
    pub enabled: bool,
}

fn toolbar_of(plugin_id: &str, plugin_name: &str, ui: &PluginUi) -> Vec<ToolbarEntry> {
    ui.toolbar
        .iter()
        .map(|t| ToolbarEntry {
            plugin_id: plugin_id.to_string(),
            plugin_name: plugin_name.to_string(),
            id: t.id.clone(),
            label: t.label.clone(),
            // The tooltip always names the plugin: a button in the
            // application's own chrome should say whose it is.
            tooltip: match &t.tooltip {
                Some(tip) if !tip.is_empty() => format!("{} ({})", tip, plugin_name),
                _ => plugin_name.to_string(),
            },
            icon: icon_index(t.icon),
            enabled: t.enabled,
        })
        .collect()
}

/// The fixed icon vocabulary as numbers the screen maps to kit icons:
/// 0 plug, 1 database, 2 code, 3 export, 4 import, 5 palette, 6 settings,
/// 7 chart, 8 search, 9 box.
pub fn icon_index(icon: Icon) -> i32 {
    match icon {
        Icon::Plug => 0,
        Icon::Database => 1,
        Icon::Code => 2,
        Icon::Export => 3,
        Icon::Import => 4,
        Icon::Palette => 5,
        Icon::Settings => 6,
        Icon::Chart => 7,
        Icon::Search => 8,
        Icon::Box => 9,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// A plugin's interface as flat rows
// ─────────────────────────────────────────────────────────────────────────

pub mod kind {
    pub const PANEL: i32 = 0;
    pub const TEXT: i32 = 1;
    pub const SEPARATOR: i32 = 2;
    pub const BUTTON: i32 = 3;
    pub const INPUT: i32 = 4;
    pub const NUMBER: i32 = 5;
    pub const CHECKBOX: i32 = 6;
    pub const SELECT: i32 = 7;
    pub const TABLE_TITLE: i32 = 8;
    pub const TABLE_HEAD: i32 = 9;
    pub const TABLE_ROW: i32 = 10;
    pub const TABLE_NOTE: i32 = 11;
}

/// One row of a plugin's interface. `style` is a palette role, never a
/// colour: for text 0 body, 1 heading, 2 strong, 3 muted, 4 success,
/// 5 warning, 6 error; for a button 0 normal, 1 primary, 2 danger.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemRow {
    pub kind: i32,
    pub id: String,
    pub label: String,
    pub text: String,
    pub hint: String,
    pub error: String,
    pub style: i32,
    pub placeholder: String,
    pub checked: bool,
    pub enabled: bool,
    pub multiline: bool,
    pub options: Vec<String>,
    pub option_index: i32,
    pub cells: Vec<String>,
    /// Panel headers only.
    pub collapsible: bool,
    pub open: bool,
    pub panel: i32,
}

fn base(kind: i32) -> ItemRow {
    ItemRow {
        kind,
        enabled: true,
        option_index: -1,
        panel: -1,
        ..ItemRow::default()
    }
}

fn text_style(s: TextStyle) -> i32 {
    match s {
        TextStyle::Body => 0,
        TextStyle::Heading => 1,
        TextStyle::Strong => 2,
        TextStyle::Muted => 3,
        TextStyle::Success => 4,
        TextStyle::Warning => 5,
        TextStyle::Error => 6,
    }
}

fn button_style(s: ButtonStyle) -> i32 {
    match s {
        ButtonStyle::Normal => 0,
        ButtonStyle::Primary => 1,
        ButtonStyle::Danger => 2,
    }
}

fn select_options<'a>(ui: &'a PluginUi, id: &str) -> Option<&'a [descriptor::SelectOption]> {
    ui.panels.iter().flat_map(|p| &p.items).find_map(|i| match i {
        Item::Select { id: sid, options, .. } if sid == id => Some(options.as_slice()),
        _ => None,
    })
}

fn number_range(ui: &PluginUi, id: &str) -> Option<(Option<f64>, Option<f64>)> {
    ui.panels.iter().flat_map(|p| &p.items).find_map(|i| match i {
        Item::Number { id: nid, min, max, .. } if nid == id => Some((*min, *max)),
        _ => None,
    })
}

/// Parse a number field's text, clamping it into the declared range.
pub fn parse_number(text: &str, min: Option<f64>, max: Option<f64>) -> Result<f64, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Enter a number".to_string());
    }
    let n: f64 = t.parse().map_err(|_| format!("\"{}\" is not a number", t))?;
    if !n.is_finite() {
        return Err(format!("\"{}\" is not a number", t));
    }
    let n = min.map_or(n, |lo| n.max(lo));
    Ok(max.map_or(n, |hi| n.min(hi)))
}

fn range_hint(min: Option<f64>, max: Option<f64>) -> String {
    let show = |n: f64| FieldValue::Number(n).as_text();
    match (min, max) {
        (Some(lo), Some(hi)) => format!("From {} to {}", show(lo), show(hi)),
        (Some(lo), None) => format!("At least {}", show(lo)),
        (None, Some(hi)) => format!("At most {}", show(hi)),
        (None, None) => String::new(),
    }
}

/// Flatten a descriptor into rows, with the user's current values, honouring
/// folded panels.
pub fn item_rows(interface: &Interface) -> Vec<ItemRow> {
    let mut rows = Vec::new();
    let fields = &interface.fields;
    for (pi, panel) in interface.descriptor.panels.iter().enumerate() {
        let folded = panel.collapsible && interface.collapsed.contains(&pi);
        let mut header = base(kind::PANEL);
        header.label = panel.title.clone();
        header.collapsible = panel.collapsible;
        header.open = !folded;
        header.panel = pi as i32;
        if !panel.title.is_empty() || panel.collapsible {
            rows.push(header);
        }
        if folded {
            continue;
        }
        for item in &panel.items {
            match item {
                Item::Text { text, style } => {
                    let mut r = base(kind::TEXT);
                    r.text = text.clone();
                    r.style = text_style(*style);
                    rows.push(r);
                }
                Item::Separator => rows.push(base(kind::SEPARATOR)),
                Item::Button { id, label, tooltip, style, enabled, .. } => {
                    let mut r = base(kind::BUTTON);
                    r.id = id.clone();
                    r.label = label.clone();
                    r.hint = tooltip.clone().unwrap_or_default();
                    r.style = button_style(*style);
                    r.enabled = *enabled;
                    rows.push(r);
                }
                Item::Input { id, label, placeholder, multiline, enabled, .. } => {
                    let mut r = base(kind::INPUT);
                    r.id = id.clone();
                    r.label = label.clone();
                    r.text = fields.get(id).map(|v| v.as_text()).unwrap_or_default();
                    r.placeholder = placeholder.clone();
                    r.multiline = *multiline;
                    r.enabled = *enabled;
                    rows.push(r);
                }
                Item::Number { id, label, min, max, enabled, .. } => {
                    let mut r = base(kind::NUMBER);
                    r.id = id.clone();
                    r.label = label.clone();
                    r.text = fields.get(id).map(|v| v.as_text()).unwrap_or_default();
                    r.hint = range_hint(*min, *max);
                    r.error = interface.bad_numbers.get(id).cloned().unwrap_or_default();
                    r.enabled = *enabled;
                    rows.push(r);
                }
                Item::Checkbox { id, label, enabled, .. } => {
                    let mut r = base(kind::CHECKBOX);
                    r.id = id.clone();
                    r.label = label.clone();
                    r.checked = matches!(fields.get(id), Some(FieldValue::Bool(true)));
                    r.enabled = *enabled;
                    rows.push(r);
                }
                Item::Select { id, label, options, enabled, .. } => {
                    let current = fields.get(id).map(|v| v.as_text()).unwrap_or_default();
                    let mut r = base(kind::SELECT);
                    r.id = id.clone();
                    r.label = label.clone();
                    r.option_index = options
                        .iter()
                        .position(|o| o.value == current)
                        .map_or(-1, |i| i as i32);
                    r.text = options
                        .iter()
                        .find(|o| o.value == current)
                        .map(|o| if o.label.is_empty() { o.value.clone() } else { o.label.clone() })
                        .unwrap_or(current);
                    r.options = options
                        .iter()
                        .map(|o| if o.label.is_empty() { o.value.clone() } else { o.label.clone() })
                        .collect();
                    r.enabled = *enabled;
                    rows.push(r);
                }
                Item::Table { title, columns, rows: data } => {
                    if !title.is_empty() {
                        let mut r = base(kind::TABLE_TITLE);
                        r.text = title.clone();
                        rows.push(r);
                    }
                    let mut head = base(kind::TABLE_HEAD);
                    head.cells = columns.clone();
                    rows.push(head);
                    for line in data.iter().take(MAX_TABLE_ROWS_SHOWN) {
                        let mut r = base(kind::TABLE_ROW);
                        // Pad or cut to the header so columns line up.
                        r.cells = (0..columns.len())
                            .map(|c| line.get(c).cloned().unwrap_or_default())
                            .collect();
                        rows.push(r);
                    }
                    if data.len() > MAX_TABLE_ROWS_SHOWN {
                        let mut r = base(kind::TABLE_NOTE);
                        r.text = format!(
                            "Showing the first {} of {} rows",
                            MAX_TABLE_ROWS_SHOWN,
                            data.len()
                        );
                        rows.push(r);
                    } else if data.is_empty() {
                        let mut r = base(kind::TABLE_NOTE);
                        r.text = "No rows".to_string();
                        rows.push(r);
                    }
                }
            }
        }
    }
    rows
}

// ─────────────────────────────────────────────────────────────────────────
// What a plugin asked the host to do
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTone {
    Neutral,
    Success,
    Warning,
    Danger,
}

/// A toast a plugin raised. Already permission-checked by the runtime; the
/// text names the plugin when the plugin gave no title of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToast {
    pub text: String,
    pub tone: NoteTone,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Absorbed {
    pub descriptor: Option<PluginUi>,
    pub toasts: Vec<PluginToast>,
}

/// Turn a plugin's outbox into things the shell can show.
///
/// `SetUi` wins over the call's own return value: a plugin that asks for a
/// specific interface should get it. `OpenTab` and `Message` have no route in
/// this shell and are dropped, as the egui shell dropped them.
pub fn absorb(plugin_name: &str, returned: Option<PluginUi>, outbox: Vec<HostCall>) -> Absorbed {
    let mut out = Absorbed {
        descriptor: returned,
        toasts: Vec::new(),
    };
    for call in outbox {
        match call {
            HostCall::Notify(n) => {
                let heading = if n.title.trim().is_empty() { plugin_name } else { n.title.trim() };
                let text = match (n.message.trim().is_empty(), heading.is_empty()) {
                    (true, _) => heading.to_string(),
                    (false, true) => n.message.clone(),
                    (false, false) => format!("{}: {}", heading, n.message.trim()),
                };
                out.toasts.push(PluginToast {
                    text,
                    tone: match n.kind {
                        NotificationKind::Info => NoteTone::Neutral,
                        NotificationKind::Success => NoteTone::Success,
                        NotificationKind::Warning => NoteTone::Warning,
                        NotificationKind::Error => NoteTone::Danger,
                    },
                });
            }
            HostCall::SetUi(ui) => out.descriptor = Some(*ui),
            HostCall::OpenTab { .. } | HostCall::Message { .. } => {}
        }
    }
    out
}

/// What a failed install says. The engine's wording for "there is no
/// download" is about plugin capabilities, which is the wrong message here.
pub fn install_error_message(plugin_name: &str, engine_error: &str) -> String {
    if engine_error.contains("downloading") {
        format!(
            "{} is not bundled with this build, and downloading plugins is not supported yet.",
            plugin_name
        )
    } else {
        engine_error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::plugins::manifest::{PluginManifest, PluginStats};
    use dbhive_core::plugins::Notification;

    const MANIFEST: &str = r#"{
      "id": "com.test.alpha", "name": "Alpha", "version": "1.2.0",
      "description": "Does alpha things",
      "author": { "name": "Ann", "email": "ann@example.com" },
      "category": "export", "main": "index.js", "pluginType": "javascript",
      "permissions": ["showNotification", "writeFiles", "modifyUI"],
      "minVersion": "0.1.0", "license": "MIT",
      "homepage": "https://example.com/alpha",
      "keywords": ["csv", "export"],
      "configSchema": { "type": "object", "properties": {
        "delimiter": { "type": "string", "enum": [",", ";"], "default": "," },
        "headers": { "type": "boolean", "default": true },
        "limit": { "type": "integer", "default": 10, "minimum": 1, "maximum": 100 }
      } }
    }"#;

    fn plugin(id: &str, name: &str, enabled: bool) -> Plugin {
        let mut manifest = PluginManifest::parse(MANIFEST).unwrap();
        manifest.id = id.to_string();
        manifest.name = name.to_string();
        Plugin {
            manifest,
            path: std::path::PathBuf::from("/x"),
            enabled,
            loaded: false,
            config: None,
            stats: PluginStats::new("2026-01-05T10:00:00Z".into()),
        }
    }

    fn ready(plugins: Vec<Plugin>) -> PluginsPanel {
        let mut p = PluginsPanel::new();
        p.apply_scan(Ok((plugins, vec![])));
        p
    }

    fn ui(json: &str) -> PluginUi {
        PluginUi::parse(json).unwrap()
    }

    const FORM_UI: &str = r#"{
      "title": "Alpha",
      "toolbar": [{ "id": "go", "label": "Go", "icon": "export", "tooltip": "Run it" }],
      "panels": [
        { "title": "Options", "items": [
          { "type": "text", "text": "Pick", "style": "muted" },
          { "type": "input", "id": "name", "label": "Name", "value": "out.csv" },
          { "type": "number", "id": "n", "label": "Count", "value": 3, "min": 1, "max": 5 },
          { "type": "checkbox", "id": "flag", "label": "Flag", "value": true },
          { "type": "select", "id": "fmt", "label": "Format", "value": "b",
            "options": [{ "value": "a", "label": "A" }, { "value": "b", "label": "B" }] },
          { "type": "button", "id": "run", "label": "Run", "style": "primary" }
        ] },
        { "title": "Results", "collapsible": true, "items": [
          { "type": "table", "title": "T", "columns": ["x", "y"], "rows": [["1", "2"], ["3"]] }
        ] }
      ]
    }"#;

    #[test]
    fn a_scan_selects_the_first_plugin_and_keeps_the_selection_while_it_exists() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", true)]);
        assert_eq!(p.load, Load::Ready);
        assert_eq!(p.selected.as_deref(), Some("com.a"));

        p.select("com.b");
        p.apply_scan(Ok((vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", true)], vec![])));
        assert_eq!(p.selected.as_deref(), Some("com.b"), "a rescan keeps the selection");

        p.apply_scan(Ok((vec![plugin("com.a", "Alpha", true)], vec![])));
        assert_eq!(p.selected.as_deref(), Some("com.a"), "a gone plugin cannot stay selected");

        p.apply_scan(Ok((vec![], vec![])));
        assert_eq!(p.selected, None);
        assert_eq!(p.installed_summary(), "No plugins installed");
    }

    #[test]
    fn a_failed_scan_is_an_error_state_not_an_empty_list() {
        let mut p = PluginsPanel::new();
        p.apply_scan(Err("permission denied".into()));
        assert_eq!(p.load, Load::Failed("permission denied".into()));
        p.begin_load();
        assert_eq!(p.load, Load::Loading);
    }

    #[test]
    fn problems_from_a_scan_are_kept_for_display() {
        let mut p = PluginsPanel::new();
        p.apply_scan(Ok((vec![], vec!["broken: bad manifest".into()])));
        assert_eq!(p.problems, vec!["broken: bad manifest".to_string()]);
    }

    #[test]
    fn installed_rows_show_enabled_state_and_what_is_busy() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", false)]);
        p.busy.insert("com.b".into(), Busy::Toggling);
        let rows = p.installed_rows();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].enabled && rows[0].selected && rows[0].busy.is_none());
        assert!(!rows[1].enabled);
        assert_eq!(rows[1].busy, Some("Updating…"));
        assert_eq!(rows[0].subtitle, "v1.2.0 · Ann");
        assert_eq!(p.installed_summary(), "2 installed, 1 enabled");
    }

    #[test]
    fn marketplace_cards_carry_their_install_state() {
        let mut p = PluginsPanel::new();
        p.apply_scan(Ok((vec![], vec![])));
        let id = p.catalog[0].manifest.id.clone();

        let state = |p: &PluginsPanel| p.market_rows().into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(state(&p).state_kind, 0);

        p.busy.insert(id.clone(), Busy::Installing);
        assert_eq!(state(&p).state_kind, 1);

        p.busy.remove(&id);
        p.install_errors.insert(id.clone(), "not bundled".into());
        let row = state(&p);
        assert_eq!((row.state_kind, row.state_note.as_str()), (3, "not bundled"));

        p.upsert(plugin(&id, "Installed one", true));
        assert_eq!(state(&p).state_kind, 2);
        assert!(p.install_errors.is_empty(), "an installed plugin has no stale failure");
    }

    #[test]
    fn marketplace_search_category_verified_and_sort_filter_the_listing() {
        let mut p = PluginsPanel::new();
        let all = p.market_rows().len();
        assert!(all >= 3);

        p.filters.search = "csv".into();
        let hits = p.market_rows();
        assert!(!hits.is_empty() && hits.len() < all);
        assert!(p.filters_active());

        p.clear_filters();
        assert!(!p.filters_active());
        assert_eq!(p.market_rows().len(), all);

        p.set_category_index(1 + PluginCategory::ALL.iter().position(|c| *c == PluginCategory::Theme).unwrap());
        assert!(p.market_rows().iter().all(|r| r.category == "Themes"));
        assert_eq!(PluginsPanel::category_options()[p.category_index() as usize], "Themes");
        p.set_category_index(0);
        assert_eq!(p.filters.category, None);

        p.filters.verified_only = true;
        assert!(p.market_rows().iter().all(|r| r.verified));
        p.filters.verified_only = false;

        p.set_sort_index(4);
        let names: Vec<String> = p.market_rows().iter().map(|r| r.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "sorted by name");
        assert_eq!(PluginsPanel::sort_options()[p.sort_index() as usize], "Name");
    }

    #[test]
    fn clearing_filters_keeps_the_chosen_sort() {
        let mut p = PluginsPanel::new();
        p.set_sort_index(2);
        p.filters.search = "x".into();
        p.clear_filters();
        assert_eq!(p.sort_index(), 2);
    }

    #[test]
    fn the_detail_pane_lists_everything_the_checklist_asks_for() {
        let mut pl = plugin("com.a", "Alpha", true);
        pl.stats.execution_count = 7;
        pl.stats.error_count = 2;
        pl.stats.last_used = Some("2026-02-01T00:00:00Z".into());
        let p = ready(vec![pl]);
        let d = p.detail().unwrap();

        assert_eq!((d.name.as_str(), d.version.as_str()), ("Alpha", "v1.2.0"));
        let info: HashMap<_, _> = d.info.iter().cloned().collect();
        assert_eq!(info["Author"], "Ann");
        assert_eq!(info["License"], "MIT");
        assert_eq!(info["Category"], "Export Formats");
        assert_eq!(info["Type"], "JavaScript");
        let stats: HashMap<_, _> = d.stats.iter().cloned().collect();
        assert_eq!(stats["Installed"], "2026-01-05");
        assert_eq!(stats["Executions"], "7");
        assert_eq!(stats["Errors"], "2");
        assert_eq!(stats["Last used"], "2026-02-01");
        assert!(d.has_errors);
        assert_eq!(d.keywords, vec!["csv".to_string(), "export".to_string()]);
        assert_eq!(d.links, vec![("Homepage".to_string(), "https://example.com/alpha".to_string())]);
        assert!(d.has_settings && d.can_open);
        assert_eq!(d.permissions.len(), 3);
        assert!(d.permissions.iter().any(|x| x.sensitive), "writeFiles is flagged");
    }

    #[test]
    fn a_disabled_plugin_cannot_open_its_interface() {
        let p = ready(vec![plugin("com.a", "Alpha", false)]);
        assert!(!p.detail().unwrap().can_open);
    }

    #[test]
    fn descriptor_rows_carry_kinds_values_and_style_roles() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        let rows = p.items(Scope::View);

        let kinds: Vec<i32> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                kind::PANEL,
                kind::TEXT,
                kind::INPUT,
                kind::NUMBER,
                kind::CHECKBOX,
                kind::SELECT,
                kind::BUTTON,
                kind::PANEL,
                kind::TABLE_TITLE,
                kind::TABLE_HEAD,
                kind::TABLE_ROW,
                kind::TABLE_ROW,
            ]
        );
        assert_eq!(rows[1].style, 3, "muted text");
        assert_eq!(rows[2].text, "out.csv");
        assert_eq!(rows[3].hint, "From 1 to 5");
        assert!(rows[4].checked);
        assert_eq!((rows[5].text.as_str(), rows[5].option_index), ("B", 1));
        assert_eq!(rows[5].options, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(rows[6].style, 1, "primary button");
        assert!(rows[7].collapsible && rows[7].open);
        assert_eq!(rows[11].cells, vec!["3".to_string(), String::new()], "short rows are padded");
    }

    #[test]
    fn a_folded_panel_hides_its_items_and_unfolds() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        let open = p.items(Scope::View).len();
        p.toggle_panel(Scope::View, 1);
        let folded = p.items(Scope::View);
        assert!(folded.len() < open);
        assert!(!folded.last().unwrap().open, "the header shows it is folded");
        p.toggle_panel(Scope::View, 1);
        assert_eq!(p.items(Scope::View).len(), open);
    }

    #[test]
    fn long_tables_are_cut_and_counted() {
        let rows: Vec<Vec<String>> = (0..MAX_TABLE_ROWS_SHOWN + 30).map(|i| vec![i.to_string()]).collect();
        let ui = PluginUi {
            panels: vec![descriptor::Panel {
                title: "P".into(),
                items: vec![Item::Table { title: String::new(), columns: vec!["n".into()], rows }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = item_rows(&Interface::new(ui));
        let shown = out.iter().filter(|r| r.kind == kind::TABLE_ROW).count();
        assert_eq!(shown, MAX_TABLE_ROWS_SHOWN);
        assert_eq!(out.last().unwrap().text, format!("Showing the first {} of {} rows", MAX_TABLE_ROWS_SHOWN, MAX_TABLE_ROWS_SHOWN + 30));
    }

    #[test]
    fn edits_land_in_the_fields_and_come_back_in_the_rows() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());

        p.set_text(Scope::View, "name", "typed.csv");
        assert_eq!(p.toggle_check(Scope::View, "flag"), Some(FieldValue::Bool(false)));
        assert_eq!(p.pick_option(Scope::View, "fmt", 0), Some(FieldValue::Text("a".into())));
        assert_eq!(p.pick_option(Scope::View, "fmt", 0), None, "same option is no change");
        assert_eq!(p.pick_option(Scope::View, "fmt", 9), None, "out of range is ignored");
        assert_eq!(p.set_number(Scope::View, "n", "4"), Some(FieldValue::Number(4.0)));

        let rows = p.items(Scope::View);
        assert_eq!(rows[2].text, "typed.csv");
        assert!(!rows[4].checked);
        assert_eq!(rows[5].text, "A");
        assert_eq!(rows[3].text, "4");
    }

    #[test]
    fn edits_to_a_control_the_plugin_never_declared_are_ignored() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        p.set_text(Scope::View, "ghost", "x");
        assert_eq!(p.toggle_check(Scope::View, "ghost"), None);
        assert_eq!(p.set_number(Scope::View, "name", "3"), None, "not a number field");
        assert!(p.value_event("ghost").is_none());
        assert!(!p.interfaces["com.a"].fields.contains_key("ghost"));
    }

    #[test]
    fn numbers_are_clamped_and_bad_text_is_flagged_without_losing_the_value() {
        assert_eq!(parse_number("99", Some(1.0), Some(5.0)), Ok(5.0));
        assert_eq!(parse_number("-4", Some(1.0), Some(5.0)), Ok(1.0));
        assert_eq!(parse_number(" 2.5 ", None, None), Ok(2.5));
        assert!(parse_number("", None, None).is_err());
        assert!(parse_number("abc", None, None).is_err());
        assert!(parse_number("NaN", None, None).is_err());
        assert!(parse_number("inf", None, None).is_err());

        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        assert_eq!(p.set_number(Scope::View, "n", "oops"), None);
        let rows = p.items(Scope::View);
        assert_eq!(rows[3].error, "\"oops\" is not a number");
        assert_eq!(rows[3].text, "3", "the stored number is unchanged");
        p.set_number(Scope::View, "n", "2");
        assert_eq!(p.items(Scope::View)[3].error, "");
    }

    #[test]
    fn a_new_descriptor_keeps_what_the_user_typed_into_surviving_fields() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        p.set_text(Scope::View, "name", "typed");
        p.set_interface("com.a", ui(r#"{"panels":[{"title":"x","items":[
            {"type":"input","id":"name","label":"Name","value":"default"},
            {"type":"input","id":"fresh","label":"F","value":"new"}]}]}"#));
        let f = &p.interfaces["com.a"].fields;
        assert_eq!(f["name"], FieldValue::Text("typed".into()));
        assert_eq!(f["fresh"], FieldValue::Text("new".into()));
        assert!(!f.contains_key("flag"));
    }

    #[test]
    fn button_and_value_events_carry_the_fields_and_resolve_against_the_descriptor() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        p.set_text(Scope::View, "name", "x.csv");

        let (current, ev) = p.button_event(Scope::View, "run").unwrap();
        match &ev {
            UiEvent::ButtonPressed { id, fields } => {
                assert_eq!(id, "run");
                assert_eq!(fields["name"], FieldValue::Text("x.csv".into()));
            }
            other => panic!("{:?}", other),
        }
        assert!(descriptor::dispatch_for(&current, &ev).is_ok());

        let (current, ev) = p.value_event("name").unwrap();
        assert!(matches!(ev, UiEvent::ValueChanged { .. }));
        assert!(descriptor::dispatch_for(&current, &ev).is_ok());
    }

    #[test]
    fn toolbar_buttons_come_only_from_enabled_plugins_and_name_their_plugin() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.set_interface("com.b", ui(FORM_UI));
        let bar = p.toolbar();
        assert_eq!(bar.len(), 2);
        assert_eq!(bar[0].plugin_name, "Alpha");
        assert_eq!(bar[0].tooltip, "Run it (Alpha)");
        assert_eq!(bar[0].icon, 3, "export icon");

        // Disabling drops the plugin's buttons on the next scan.
        p.apply_scan(Ok((vec![plugin("com.a", "Alpha", false), plugin("com.b", "Beta", true)], vec![])));
        let bar = p.toolbar();
        assert_eq!(bar.len(), 1);
        assert_eq!(bar[0].plugin_id, "com.b");
        assert!(p.toolbar_event("com.a", "go").is_none(), "a disabled plugin cannot be driven");
        assert!(p.toolbar_event("com.b", "go").is_some());
        assert!(p.toolbar_event("com.b", "ghost").is_some(), "unknown ids are refused later, by dispatch_for");
    }

    #[test]
    fn the_header_shows_only_the_first_few_plugin_buttons() {
        let many = PluginUi {
            toolbar: (0..6)
                .map(|i| descriptor::ToolbarItem {
                    id: format!("b{}", i),
                    label: format!("B{}", i),
                    tooltip: None,
                    icon: Icon::Plug,
                    action: None,
                    enabled: true,
                })
                .collect(),
            ..Default::default()
        };
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", many);
        assert_eq!(p.toolbar().len(), 6);
        let header = p.header_toolbar();
        assert_eq!(header.len(), HEADER_TOOLBAR_LIMIT);
        assert_eq!(header[0].id, "b0");
    }

    #[test]
    fn the_toolbar_event_for_an_undeclared_button_is_refused_by_dispatch() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        let (current, ev) = p.toolbar_event("com.a", "not-a-button").unwrap();
        assert!(descriptor::dispatch_for(&current, &ev).is_err());
    }

    #[test]
    fn the_settings_form_is_built_from_the_schema_and_saved_as_config() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        assert!(p.open_settings("com.a"));
        let rows = p.items(Scope::Form);
        assert!(rows.iter().any(|r| r.kind == kind::SELECT && r.id == "delimiter"));
        assert!(rows.iter().any(|r| r.kind == kind::CHECKBOX && r.id == "headers" && r.checked));
        assert!(rows.iter().any(|r| r.kind == kind::NUMBER && r.id == "limit" && r.text == "10"));

        p.pick_option(Scope::Form, "delimiter", 1);
        p.toggle_check(Scope::Form, "headers");
        p.set_number(Scope::Form, "limit", "500");

        let (id, config) = p.settings_to_save().unwrap();
        assert_eq!(id, "com.a");
        assert_eq!(config["delimiter"], ";");
        assert_eq!(config["headers"], false);
        assert_eq!(config["limit"], 100.0, "clamped into the schema's range");
    }

    #[test]
    fn saved_settings_come_back_into_the_form() {
        let mut pl = plugin("com.a", "Alpha", true);
        pl.config = Some(serde_json::json!({ "delimiter": ";", "headers": false }));
        let mut p = ready(vec![pl]);
        p.open_settings("com.a");
        let rows = p.items(Scope::Form);
        let d = rows.iter().find(|r| r.id == "delimiter").unwrap();
        assert_eq!(d.text, ";");
        assert!(!rows.iter().find(|r| r.id == "headers").unwrap().checked);
    }

    #[test]
    fn a_bad_number_blocks_saving_settings() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.open_settings("com.a");
        p.set_number(Scope::Form, "limit", "lots");
        assert!(p.settings_to_save().is_err());
    }

    #[test]
    fn a_plugin_without_a_schema_has_no_settings() {
        let mut pl = plugin("com.a", "Alpha", true);
        pl.manifest.config_schema = None;
        let mut p = ready(vec![pl]);
        assert!(!p.open_settings("com.a"));
        assert!(!p.detail().unwrap().has_settings);
    }

    #[test]
    fn selecting_another_plugin_closes_its_neighbours_view_and_form() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.open_view = Some("com.a".into());
        p.open_settings("com.a");
        p.select("com.b");
        assert!(p.open_view.is_none() && p.form.is_none());
        assert!(p.interfaces.contains_key("com.a"), "its toolbar stays registered");
    }

    #[test]
    fn removing_a_plugin_drops_its_interface_busy_flag_and_selection() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true), plugin("com.b", "Beta", true)]);
        p.set_interface("com.a", ui(FORM_UI));
        p.busy.insert("com.a".into(), Busy::Uninstalling);
        p.remove("com.a");
        assert!(p.interfaces.is_empty() && p.busy.is_empty());
        assert_eq!(p.selected.as_deref(), Some("com.b"));
    }

    #[test]
    fn the_outbox_becomes_toasts_and_set_ui_wins() {
        let notify = |t: &str, m: &str, k| {
            HostCall::Notify(Notification { title: t.into(), message: m.into(), kind: k })
        };
        let pushed = PluginUi { title: Some("pushed".into()), ..Default::default() };
        let out = absorb(
            "Alpha",
            Some(PluginUi { title: Some("returned".into()), ..Default::default() }),
            vec![
                notify("CSV", "done", NotificationKind::Success),
                notify("", "plain", NotificationKind::Error),
                notify("Title only", "", NotificationKind::Warning),
                HostCall::SetUi(Box::new(pushed.clone())),
                HostCall::OpenTab { title: "t".into() },
                HostCall::Message { to: "x".into(), body: "y".into() },
            ],
        );
        assert_eq!(out.descriptor, Some(pushed));
        assert_eq!(
            out.toasts,
            vec![
                PluginToast { text: "CSV: done".into(), tone: NoteTone::Success },
                PluginToast { text: "Alpha: plain".into(), tone: NoteTone::Danger },
                PluginToast { text: "Title only".into(), tone: NoteTone::Warning },
            ]
        );
    }

    #[test]
    fn install_failures_read_as_a_sentence_about_the_plugin() {
        let m = install_error_message("Dark Theme Plus", "Not available to plugins: downloading 'x'");
        assert!(m.starts_with("Dark Theme Plus is not bundled"));
        assert_eq!(install_error_message("X", "Incompatible version: requires 9"), "Incompatible version: requires 9");
    }

    #[test]
    fn a_failed_open_gives_a_plugin_with_no_interface_somewhere_to_show_why() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.open_view = Some("com.a".into());
        assert_eq!(p.view_error(), "");
        p.fail_interface("com.a", "no interface".into());
        assert_eq!(p.view_error(), "no interface");
        assert!(p.items(Scope::View).is_empty());
        assert!(p.toolbar().is_empty(), "an empty interface registers no buttons");
        assert_eq!(p.view_title(), "Plugin interface");
    }

    #[test]
    fn a_panel_the_plugin_asked_to_start_folded_starts_folded() {
        let mut p = ready(vec![plugin("com.a", "Alpha", true)]);
        p.set_interface(
            "com.a",
            ui(r#"{"panels":[{"title":"Closed","collapsible":true,"open":false,"items":[
                {"type":"text","text":"hidden"}]}]}"#),
        );
        p.open_view = Some("com.a".into());
        let rows = p.items(Scope::View);
        assert_eq!(rows.len(), 1, "only the header shows");
        assert!(!rows[0].open);
        p.toggle_panel(Scope::View, 0);
        assert_eq!(p.items(Scope::View).len(), 2);
    }

    #[test]
    fn icons_map_to_distinct_indexes() {
        let all = [
            Icon::Plug, Icon::Database, Icon::Code, Icon::Export, Icon::Import,
            Icon::Palette, Icon::Settings, Icon::Chart, Icon::Search, Icon::Box,
        ];
        let mut seen: Vec<i32> = all.iter().map(|i| icon_index(*i)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), all.len());
    }
}
