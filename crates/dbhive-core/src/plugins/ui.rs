//! The declarative plugin UI: what a plugin is allowed to put on screen.
//!
//! # Why this exists
//!
//! In the web shell a plugin extended the interface by having the host emit
//! `plugin-register-ui`, and React rendered whatever descriptor came back.
//! A native shell has no React, and "render arbitrary markup" is not
//! something egui can or should do. The replacement is a **schema-driven UI**:
//! a plugin returns *data* describing panels, labels, form fields and
//! buttons, and the host draws them with its own widgets, its own palette and
//! its own event handling.
//!
//! # The security properties this buys
//!
//! * There is no escape hatch. `Item` is a closed enum. A plugin cannot
//!   describe HTML, a script, an iframe, an image path, a URL to open, a
//!   colour, or a font. It picks from a fixed vocabulary or it gets nothing.
//! * **The type is not recursive.** A panel holds items; an item holds no
//!   panels. That is deliberate: a hostile plugin cannot emit a descriptor
//!   nested deeply enough to blow the stack in the deserialiser or the
//!   renderer, because depth is bounded by the type itself.
//! * Every quantity is bounded (see the `MAX_*` constants) and every string
//!   is truncated and stripped of control characters before it reaches the
//!   renderer.
//! * Events flow back only for controls the plugin *itself declared* in the
//!   descriptor the host is currently showing. [`dispatch_for`] refuses an
//!   event whose id is not in the descriptor, so a stale or forged event
//!   cannot reach a handler.
//! * The handler name a button may nominate must be a plain JS identifier.
//!   The runtime builds a call expression by name, so `"x(); steal(); //"`
//!   would otherwise have been code injection into the host's own eval.
//!
//! # Shape
//!
//! ```json
//! {
//!   "title": "CSV Exporter",
//!   "toolbar": [{ "id": "export", "label": "Export CSV", "icon": "export" }],
//!   "menu":    [{ "id": "about", "label": "About this plugin" }],
//!   "panels": [{
//!     "title": "Options",
//!     "items": [
//!       { "type": "text",     "style": "muted", "text": "Choose a delimiter." },
//!       { "type": "select",   "id": "delim", "label": "Delimiter",
//!         "value": ",", "options": [{ "value": ",", "label": "Comma" }] },
//!       { "type": "checkbox", "id": "headers", "label": "Include headers", "value": true },
//!       { "type": "button",   "id": "run", "label": "Export", "style": "primary" }
//!     ]
//!   }]
//! }
//! ```

#![allow(dead_code)] // shared by both shells; neither uses every accessor.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Descriptor size cap, applied to the raw bytes *before* parsing so a
/// hostile plugin cannot make the host allocate its way to death.
pub const MAX_DESCRIPTOR_BYTES: usize = 256 * 1024;
pub const MAX_PANELS: usize = 32;
pub const MAX_ITEMS_PER_PANEL: usize = 256;
pub const MAX_TOTAL_ITEMS: usize = 1024;
pub const MAX_TOOLBAR_ITEMS: usize = 16;
pub const MAX_MENU_ITEMS: usize = 32;
pub const MAX_SELECT_OPTIONS: usize = 256;
pub const MAX_TABLE_ROWS: usize = 1_000;
pub const MAX_TABLE_COLUMNS: usize = 64;
pub const MAX_ID_CHARS: usize = 128;
pub const MAX_LABEL_CHARS: usize = 200;
pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_CELL_CHARS: usize = 512;
pub const MAX_FIELD_VALUE_CHARS: usize = 8_192;
/// Longest handler name a button may nominate.
pub const MAX_HANDLER_CHARS: usize = 64;

/// The exported function the host calls to obtain a descriptor.
pub const RENDER_HANDLER: &str = "renderUi";
/// The exported function that receives events, unless a control names its own.
pub const DEFAULT_HANDLER: &str = "onUiEvent";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiError {
    /// Raw descriptor exceeded [`MAX_DESCRIPTOR_BYTES`].
    TooLarge { bytes: usize, limit: usize },
    /// Not valid JSON, or not this schema.
    Malformed(String),
    TooMany {
        what: &'static str,
        count: usize,
        limit: usize,
    },
    /// An id that is empty, over-long, or not a plain identifier.
    BadId(String),
    /// Two interactive controls claimed the same id.
    DuplicateId(String),
    /// A handler name that is not a plain JavaScript identifier.
    BadHandler(String),
    /// An event referring to a control the current descriptor does not have.
    UnknownTarget(String),
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::TooLarge { bytes, limit } => write!(
                f,
                "plugin UI descriptor is {} bytes, limit is {}",
                bytes, limit
            ),
            UiError::Malformed(m) => write!(f, "plugin UI descriptor is not valid: {}", m),
            UiError::TooMany { what, count, limit } => {
                write!(
                    f,
                    "plugin UI declares {} {}, limit is {}",
                    count, what, limit
                )
            }
            UiError::BadId(id) => write!(f, "'{}' is not a usable control id", id),
            UiError::DuplicateId(id) => write!(f, "control id '{}' is used twice", id),
            UiError::BadHandler(h) => write!(f, "'{}' is not a valid handler name", h),
            UiError::UnknownTarget(id) => write!(
                f,
                "event for '{}', which this plugin's interface does not contain",
                id
            ),
        }
    }
}

impl std::error::Error for UiError {}

// ---------------------------------------------------------------------------
// The descriptor
// ---------------------------------------------------------------------------

/// A whole plugin interface, as returned by the plugin's `renderUi()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginUi {
    pub title: Option<String>,
    /// Buttons the host may surface in its toolbar.
    pub toolbar: Vec<ToolbarItem>,
    /// Entries the host may surface in a plugin menu.
    pub menu: Vec<MenuItem>,
    pub panels: Vec<Panel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolbarItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub tooltip: Option<String>,
    /// One of a fixed set of icons. Not a path and not a URL: a plugin cannot
    /// make the host load an image it chose.
    #[serde(default)]
    pub icon: Icon,
    /// Exported function to call. Defaults to [`DEFAULT_HANDLER`].
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    /// Display only. The host does not register a shortcut on a plugin's say-so.
    #[serde(default)]
    pub shortcut_hint: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub collapsible: bool,
    #[serde(default = "yes")]
    pub open: bool,
    pub items: Vec<Item>,
}

impl Default for Panel {
    /// Hand-written rather than derived so that an omitted `open` means
    /// "shown", both here and through serde's container default.
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            collapsible: false,
            open: true,
            items: Vec::new(),
        }
    }
}

/// One control. Note there is no variant that contains a `Panel`: the schema
/// is intentionally two levels deep and no more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Item {
    /// A run of text. `style` selects a palette role; the plugin cannot pick
    /// a colour.
    Text {
        text: String,
        #[serde(default)]
        style: TextStyle,
    },
    Separator,
    Button {
        id: String,
        label: String,
        #[serde(default)]
        tooltip: Option<String>,
        #[serde(default)]
        style: ButtonStyle,
        /// Exported function to call. Defaults to [`DEFAULT_HANDLER`].
        #[serde(default)]
        action: Option<String>,
        #[serde(default = "yes")]
        enabled: bool,
    },
    Input {
        id: String,
        label: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        multiline: bool,
        #[serde(default = "yes")]
        enabled: bool,
    },
    Number {
        id: String,
        label: String,
        #[serde(default)]
        value: f64,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default = "yes")]
        enabled: bool,
    },
    Checkbox {
        id: String,
        label: String,
        #[serde(default)]
        value: bool,
        #[serde(default = "yes")]
        enabled: bool,
    },
    Select {
        id: String,
        label: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        options: Vec<SelectOption>,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Read-only tabular output. Values are strings: a plugin does not get to
    /// hand the host a typed cell it will then have to trust.
    Table {
        #[serde(default)]
        title: String,
        #[serde(default)]
        columns: Vec<String>,
        #[serde(default)]
        rows: Vec<Vec<String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub value: String,
    #[serde(default)]
    pub label: String,
}

/// Palette roles a plugin may pick from. Not colours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextStyle {
    #[default]
    Body,
    Heading,
    Strong,
    Muted,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonStyle {
    #[default]
    Normal,
    Primary,
    Danger,
}

/// Fixed icon vocabulary. Anything a plugin sends that is not in this list
/// deserialises to `Plug` rather than failing the descriptor: an unknown icon
/// is a cosmetic problem, not a reason to show the user nothing.
///
/// `Deserialize` is hand-written for exactly that fallback. Serde's
/// `#[serde(other)]` only applies to internally tagged enums, and deriving it
/// plainly would make `"icon": "sparkles"` reject the whole descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Icon {
    #[default]
    Plug,
    Database,
    Code,
    Export,
    Import,
    Palette,
    Settings,
    Chart,
    Search,
    Box,
}

impl<'de> Deserialize<'de> for Icon {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Icon::from_wire(&raw))
    }
}

impl Icon {
    /// Unknown names fall back to `Plug`.
    pub fn from_wire(name: &str) -> Icon {
        match name {
            "database" => Icon::Database,
            "code" => Icon::Code,
            "export" => Icon::Export,
            "import" => Icon::Import,
            "palette" => Icon::Palette,
            "settings" => Icon::Settings,
            "chart" => Icon::Chart,
            "search" => Icon::Search,
            "box" => Icon::Box,
            _ => Icon::Plug,
        }
    }
}

impl Icon {
    /// A short text glyph. egui ships no icon set, and loading one on a
    /// plugin's behalf would be exactly the capability this design removes.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Plug => "\u{2b1a}",
            Icon::Database => "\u{25a4}",
            Icon::Code => "{ }",
            Icon::Export => "\u{2191}",
            Icon::Import => "\u{2193}",
            Icon::Palette => "\u{25d0}",
            Icon::Settings => "\u{2699}",
            Icon::Chart => "\u{25eb}",
            Icon::Search => "\u{2315}",
            Icon::Box => "\u{25a1}",
        }
    }
}

fn yes() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Values and events
// ---------------------------------------------------------------------------

/// The value of one form field. Untagged, so a plugin writes plain JSON:
/// `true`, `12`, `"text"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl FieldValue {
    pub fn as_text(&self) -> String {
        match self {
            FieldValue::Bool(b) => b.to_string(),
            FieldValue::Number(n) => format_number(*n),
            FieldValue::Text(s) => s.clone(),
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        match self {
            FieldValue::Bool(b) => serde_json::Value::Bool(*b),
            FieldValue::Number(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            FieldValue::Text(s) => serde_json::Value::String(s.clone()),
        }
    }
}

/// Trailing ".0" on whole numbers reads badly in a form.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Current value of every form field in a descriptor, keyed by control id.
pub type Fields = BTreeMap<String, FieldValue>;

/// Something the user did to a plugin's interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    /// The view was opened: ask the plugin to describe itself.
    Opened,
    ButtonPressed {
        id: String,
        /// Every field's current value, so a handler does not have to ask.
        #[serde(default)]
        fields: Fields,
    },
    ValueChanged {
        id: String,
        value: FieldValue,
    },
    MenuSelected {
        id: String,
    },
}

impl UiEvent {
    pub fn target(&self) -> Option<&str> {
        match self {
            UiEvent::Opened => None,
            UiEvent::ButtonPressed { id, .. }
            | UiEvent::ValueChanged { id, .. }
            | UiEvent::MenuSelected { id } => Some(id),
        }
    }
}

/// A resolved call into the plugin: which exported function, with what
/// argument. The runtime executes exactly this and nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub function: String,
    pub argument: serde_json::Value,
}

/// Map an event to the call it becomes.
///
/// `ui` is the descriptor currently on screen. The event must name a control
/// that descriptor declares; anything else is refused. This is what keeps the
/// callable surface equal to what the plugin asked for, rather than to
/// whatever the host happens to send.
pub fn dispatch_for(ui: &PluginUi, event: &UiEvent) -> Result<Dispatch, UiError> {
    let function = match event {
        UiEvent::Opened => RENDER_HANDLER.to_string(),

        UiEvent::ButtonPressed { id, .. } => {
            let action = ui
                .find_button(id)
                .ok_or_else(|| UiError::UnknownTarget(id.clone()))?;
            handler_or_default(action)?
        }

        UiEvent::MenuSelected { id } => {
            let action = ui
                .menu
                .iter()
                .find(|m| &m.id == id)
                .map(|m| m.action.as_deref())
                .ok_or_else(|| UiError::UnknownTarget(id.clone()))?;
            handler_or_default(action)?
        }

        UiEvent::ValueChanged { id, .. } => {
            // Fields never nominate their own handler: an edit is chatty and
            // should land in one place.
            if !ui.has_field(id) {
                return Err(UiError::UnknownTarget(id.clone()));
            }
            DEFAULT_HANDLER.to_string()
        }
    };

    let argument = match event {
        UiEvent::Opened => serde_json::Value::Null,
        other => serde_json::to_value(other)
            .map_err(|e| UiError::Malformed(format!("event is not serialisable: {}", e)))?,
    };

    Ok(Dispatch { function, argument })
}

fn handler_or_default(action: Option<&str>) -> Result<String, UiError> {
    match action {
        None => Ok(DEFAULT_HANDLER.to_string()),
        Some(name) => {
            if is_safe_handler_name(name) {
                Ok(name.to_string())
            } else {
                Err(UiError::BadHandler(name.to_string()))
            }
        }
    }
}

/// Is this a plain JavaScript identifier the runtime can safely name?
///
/// The runtime builds `__plugin_exports__.<name>(...)` as source text. Without
/// this check a name like `x(); fetch(evil); //` would be injected straight
/// into the host's own `eval`.
pub fn is_safe_handler_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_HANDLER_CHARS {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Is this a usable control id? Ids key form state and egui widget ids, so
/// keep them boring.
pub fn is_valid_control_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b':')
}

// ---------------------------------------------------------------------------
// Parsing, validation, sanitising
// ---------------------------------------------------------------------------

impl PluginUi {
    /// Parse a descriptor a plugin returned.
    ///
    /// Order matters: the byte cap is checked before serde is asked to look
    /// at anything, then structure, then strings are clamped. Callers get a
    /// descriptor that is already safe to draw.
    pub fn parse(raw: &str) -> Result<PluginUi, UiError> {
        if raw.len() > MAX_DESCRIPTOR_BYTES {
            return Err(UiError::TooLarge {
                bytes: raw.len(),
                limit: MAX_DESCRIPTOR_BYTES,
            });
        }
        // A descriptor is an object. serde will happily build a struct out of
        // a JSON *array* by position, so `[]` would otherwise deserialise to
        // an empty interface: accepting a shape the schema does not describe.
        if raw.trim_start().as_bytes().first() != Some(&b'{') {
            return Err(UiError::Malformed(
                "descriptor must be a JSON object".to_string(),
            ));
        }
        // serde_json enforces its own nesting limit and returns an error
        // rather than recursing without bound, which is what makes a
        // "[[[[[..." bomb harmless here.
        let mut ui: PluginUi =
            serde_json::from_str(raw).map_err(|e| UiError::Malformed(e.to_string()))?;
        ui.validate()?;
        ui.sanitize();
        Ok(ui)
    }

    /// Same, for a descriptor already in memory as JSON.
    pub fn from_value(value: &serde_json::Value) -> Result<PluginUi, UiError> {
        let raw = serde_json::to_string(value).map_err(|e| UiError::Malformed(e.to_string()))?;
        PluginUi::parse(&raw)
    }

    /// Structural limits. Rejects rather than truncates, because a descriptor
    /// this far out of bounds is a bug or an attack, not a long list.
    pub fn validate(&self) -> Result<(), UiError> {
        if self.panels.len() > MAX_PANELS {
            return Err(UiError::TooMany {
                what: "panels",
                count: self.panels.len(),
                limit: MAX_PANELS,
            });
        }
        if self.toolbar.len() > MAX_TOOLBAR_ITEMS {
            return Err(UiError::TooMany {
                what: "toolbar items",
                count: self.toolbar.len(),
                limit: MAX_TOOLBAR_ITEMS,
            });
        }
        if self.menu.len() > MAX_MENU_ITEMS {
            return Err(UiError::TooMany {
                what: "menu items",
                count: self.menu.len(),
                limit: MAX_MENU_ITEMS,
            });
        }

        let mut total = 0usize;
        let mut seen: BTreeSet<&str> = BTreeSet::new();

        for t in &self.toolbar {
            check_id(&t.id)?;
            if !seen.insert(&t.id) {
                return Err(UiError::DuplicateId(t.id.clone()));
            }
            handler_or_default(t.action.as_deref())?;
        }
        for m in &self.menu {
            check_id(&m.id)?;
            if !seen.insert(&m.id) {
                return Err(UiError::DuplicateId(m.id.clone()));
            }
            handler_or_default(m.action.as_deref())?;
        }

        for panel in &self.panels {
            if !panel.id.is_empty() {
                check_id(&panel.id)?;
            }
            if panel.items.len() > MAX_ITEMS_PER_PANEL {
                return Err(UiError::TooMany {
                    what: "items in a panel",
                    count: panel.items.len(),
                    limit: MAX_ITEMS_PER_PANEL,
                });
            }
            total += panel.items.len();
            if total > MAX_TOTAL_ITEMS {
                return Err(UiError::TooMany {
                    what: "items",
                    count: total,
                    limit: MAX_TOTAL_ITEMS,
                });
            }

            for item in &panel.items {
                if let Some(id) = item.id() {
                    check_id(id)?;
                    if !seen.insert(id) {
                        return Err(UiError::DuplicateId(id.to_string()));
                    }
                }
                match item {
                    Item::Button { action, .. } => {
                        handler_or_default(action.as_deref())?;
                    }
                    Item::Select { options, .. } => {
                        if options.len() > MAX_SELECT_OPTIONS {
                            return Err(UiError::TooMany {
                                what: "select options",
                                count: options.len(),
                                limit: MAX_SELECT_OPTIONS,
                            });
                        }
                    }
                    Item::Table { columns, rows, .. } => {
                        if columns.len() > MAX_TABLE_COLUMNS {
                            return Err(UiError::TooMany {
                                what: "table columns",
                                count: columns.len(),
                                limit: MAX_TABLE_COLUMNS,
                            });
                        }
                        if rows.len() > MAX_TABLE_ROWS {
                            return Err(UiError::TooMany {
                                what: "table rows",
                                count: rows.len(),
                                limit: MAX_TABLE_ROWS,
                            });
                        }
                        for row in rows {
                            if row.len() > MAX_TABLE_COLUMNS {
                                return Err(UiError::TooMany {
                                    what: "table cells in a row",
                                    count: row.len(),
                                    limit: MAX_TABLE_COLUMNS,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Clamp every string and strip control characters.
    ///
    /// Renderers are not the right place to worry about a label containing a
    /// megabyte of newlines, so it is dealt with once, here.
    pub fn sanitize(&mut self) {
        if let Some(t) = &mut self.title {
            clamp_line(t, MAX_LABEL_CHARS);
        }
        for t in &mut self.toolbar {
            clamp_line(&mut t.label, MAX_LABEL_CHARS);
            if let Some(tip) = &mut t.tooltip {
                clamp_line(tip, MAX_LABEL_CHARS);
            }
        }
        for m in &mut self.menu {
            clamp_line(&mut m.label, MAX_LABEL_CHARS);
            if let Some(s) = &mut m.shortcut_hint {
                clamp_line(s, 32);
            }
        }
        for panel in &mut self.panels {
            clamp_line(&mut panel.title, MAX_LABEL_CHARS);
            for item in &mut panel.items {
                item.sanitize();
            }
        }
    }

    /// The action of the button with this id, if the descriptor has one.
    /// `Some(None)` means "declared, with no handler of its own".
    pub fn find_button(&self, id: &str) -> Option<Option<&str>> {
        for t in &self.toolbar {
            if t.id == id {
                return Some(t.action.as_deref());
            }
        }
        for panel in &self.panels {
            for item in &panel.items {
                if let Item::Button {
                    id: bid, action, ..
                } = item
                {
                    if bid == id {
                        return Some(action.as_deref());
                    }
                }
            }
        }
        None
    }

    pub fn has_field(&self, id: &str) -> bool {
        self.panels
            .iter()
            .flat_map(|p| &p.items)
            .any(|i| i.is_field() && i.id() == Some(id))
    }

    /// Starting value of every field, for the host to hold across frames.
    pub fn initial_fields(&self) -> Fields {
        let mut out = Fields::new();
        for item in self.panels.iter().flat_map(|p| &p.items) {
            if let (Some(id), Some(v)) = (item.id(), item.initial_value()) {
                out.insert(id.to_string(), v);
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.panels.iter().all(|p| p.items.is_empty())
            && self.toolbar.is_empty()
            && self.menu.is_empty()
    }
}

impl Item {
    pub fn id(&self) -> Option<&str> {
        match self {
            Item::Button { id, .. }
            | Item::Input { id, .. }
            | Item::Number { id, .. }
            | Item::Checkbox { id, .. }
            | Item::Select { id, .. } => Some(id),
            Item::Text { .. } | Item::Separator | Item::Table { .. } => None,
        }
    }

    /// Does this control hold a value the host tracks?
    pub fn is_field(&self) -> bool {
        matches!(
            self,
            Item::Input { .. } | Item::Number { .. } | Item::Checkbox { .. } | Item::Select { .. }
        )
    }

    pub fn initial_value(&self) -> Option<FieldValue> {
        match self {
            Item::Input { value, .. } => Some(FieldValue::Text(value.clone())),
            Item::Number { value, .. } => Some(FieldValue::Number(*value)),
            Item::Checkbox { value, .. } => Some(FieldValue::Bool(*value)),
            Item::Select { value, .. } => Some(FieldValue::Text(value.clone())),
            _ => None,
        }
    }

    fn sanitize(&mut self) {
        match self {
            Item::Text { text, .. } => clamp_text(text, MAX_TEXT_CHARS),
            Item::Separator => {}
            Item::Button { label, tooltip, .. } => {
                clamp_line(label, MAX_LABEL_CHARS);
                if let Some(t) = tooltip {
                    clamp_line(t, MAX_LABEL_CHARS);
                }
            }
            Item::Input {
                label,
                value,
                placeholder,
                multiline,
                ..
            } => {
                clamp_line(label, MAX_LABEL_CHARS);
                clamp_line(placeholder, MAX_LABEL_CHARS);
                if *multiline {
                    clamp_text(value, MAX_FIELD_VALUE_CHARS);
                } else {
                    clamp_line(value, MAX_FIELD_VALUE_CHARS);
                }
            }
            Item::Number {
                label,
                value,
                min,
                max,
                ..
            } => {
                clamp_line(label, MAX_LABEL_CHARS);
                // NaN and infinities would poison a DragValue's range.
                if !value.is_finite() {
                    *value = 0.0;
                }
                if min.map(|m| !m.is_finite()).unwrap_or(false) {
                    *min = None;
                }
                if max.map(|m| !m.is_finite()).unwrap_or(false) {
                    *max = None;
                }
                if let (Some(lo), Some(hi)) = (*min, *max) {
                    if lo > hi {
                        *min = None;
                        *max = None;
                    }
                }
            }
            Item::Checkbox { label, .. } => clamp_line(label, MAX_LABEL_CHARS),
            Item::Select {
                label,
                value,
                options,
                ..
            } => {
                clamp_line(label, MAX_LABEL_CHARS);
                clamp_line(value, MAX_FIELD_VALUE_CHARS);
                for o in options.iter_mut() {
                    clamp_line(&mut o.value, MAX_LABEL_CHARS);
                    if o.label.is_empty() {
                        o.label = o.value.clone();
                    }
                    clamp_line(&mut o.label, MAX_LABEL_CHARS);
                }
            }
            Item::Table {
                title,
                columns,
                rows,
            } => {
                clamp_line(title, MAX_LABEL_CHARS);
                for c in columns.iter_mut() {
                    clamp_line(c, MAX_LABEL_CHARS);
                }
                for row in rows.iter_mut() {
                    for cell in row.iter_mut() {
                        clamp_line(cell, MAX_CELL_CHARS);
                    }
                }
            }
        }
    }
}

fn check_id(id: &str) -> Result<(), UiError> {
    if is_valid_control_id(id) {
        Ok(())
    } else {
        Err(UiError::BadId(id.to_string()))
    }
}

/// Single-line text: control characters (including newlines) become spaces,
/// then truncate on a character boundary.
fn clamp_line(s: &mut String, max_chars: usize) {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_chars)
        .collect();
    *s = cleaned;
}

/// Multi-line text: newlines and tabs survive, other control characters do not.
fn clamp_text(s: &mut String, max_chars: usize) {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c == '\n' || c == '\t' {
                c
            } else if c.is_control() {
                ' '
            } else {
                c
            }
        })
        .take(max_chars)
        .collect();
    *s = cleaned;
}

// ---------------------------------------------------------------------------
// Settings forms from a manifest's configSchema
// ---------------------------------------------------------------------------

/// Turn a plugin's JSON-schema-like `configSchema` into a panel of the same
/// vocabulary the plugin itself would emit.
///
/// The React shell had a second, separate renderer for this. Reusing the one
/// descriptor type means the settings form and a plugin's own UI cannot drift,
/// and it works with no JavaScript executed at all: this is pure data.
///
/// Property order is whatever `serde_json` yields (alphabetical, since the
/// `preserve_order` feature is off). `enum` becomes a select, `boolean` a
/// checkbox, `number`/`integer` a number field, anything else a text input.
pub fn config_schema_panel(
    schema: &serde_json::Value,
    current: Option<&serde_json::Value>,
) -> Panel {
    let mut panel = Panel {
        id: "config".to_string(),
        title: "Settings".to_string(),
        collapsible: false,
        open: true,
        items: Vec::new(),
    };

    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return panel;
    };

    for (key, spec) in props.iter().take(MAX_ITEMS_PER_PANEL) {
        if !is_valid_control_id(key) {
            continue;
        }
        let label = spec
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string();
        let description = spec.get("description").and_then(|v| v.as_str());
        let default = spec.get("default");
        let value = current
            .and_then(|c| c.get(key))
            .or(default)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let ty = spec
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let enum_values = spec.get("enum").and_then(|v| v.as_array());

        let item = if let Some(values) = enum_values {
            Item::Select {
                id: key.clone(),
                label,
                value: json_to_text(&value),
                options: values
                    .iter()
                    .take(MAX_SELECT_OPTIONS)
                    .map(|v| {
                        let raw = json_to_text(v);
                        SelectOption {
                            label: display_option(&raw),
                            value: raw,
                        }
                    })
                    .collect(),
                enabled: true,
            }
        } else {
            match ty {
                "boolean" => Item::Checkbox {
                    id: key.clone(),
                    label,
                    value: value.as_bool().unwrap_or(false),
                    enabled: true,
                },
                "number" | "integer" => Item::Number {
                    id: key.clone(),
                    label,
                    value: value.as_f64().unwrap_or(0.0),
                    min: spec.get("minimum").and_then(|v| v.as_f64()),
                    max: spec.get("maximum").and_then(|v| v.as_f64()),
                    enabled: true,
                },
                _ => Item::Input {
                    id: key.clone(),
                    label,
                    value: json_to_text(&value),
                    placeholder: default.map(json_to_text).unwrap_or_default(),
                    multiline: false,
                    enabled: true,
                },
            }
        };

        panel.items.push(item);
        if let Some(d) = description {
            panel.items.push(Item::Text {
                text: d.to_string(),
                style: TextStyle::Muted,
            });
        }
    }

    for item in &mut panel.items {
        item.sanitize();
    }
    panel
}

/// A tab or other invisible delimiter must not render as an empty option.
fn display_option(raw: &str) -> String {
    match raw {
        "\t" => "Tab".to_string(),
        " " => "Space".to_string(),
        "" => "(empty)".to_string(),
        other => other.to_string(),
    }
}

fn json_to_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Collapse edited form fields back into a config object to persist.
pub fn fields_to_config(fields: &Fields) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in fields {
        map.insert(k.clone(), v.as_json());
    }
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(id: &str, action: Option<&str>) -> Item {
        Item::Button {
            id: id.to_string(),
            label: id.to_string(),
            tooltip: None,
            style: ButtonStyle::Normal,
            action: action.map(|s| s.to_string()),
            enabled: true,
        }
    }

    fn ui_with(items: Vec<Item>) -> PluginUi {
        PluginUi {
            title: None,
            toolbar: vec![],
            menu: vec![],
            panels: vec![Panel {
                id: "p".into(),
                title: "P".into(),
                collapsible: false,
                open: true,
                items,
            }],
        }
    }

    // -- deserialisation ---------------------------------------------------

    #[test]
    fn parses_a_realistic_descriptor() {
        let raw = r#"{
          "title": "CSV Exporter",
          "toolbar": [{ "id": "export", "label": "Export", "icon": "export" }],
          "menu": [{ "id": "about", "label": "About" }],
          "panels": [{
            "title": "Options",
            "items": [
              { "type": "text", "style": "muted", "text": "Pick a delimiter." },
              { "type": "select", "id": "delim", "label": "Delimiter", "value": ",",
                "options": [{ "value": "," , "label": "Comma" }, { "value": ";" }] },
              { "type": "checkbox", "id": "headers", "label": "Headers", "value": true },
              { "type": "number", "id": "limit", "label": "Rows", "value": 100, "min": 1, "max": 1000 },
              { "type": "input", "id": "name", "label": "File name", "value": "out.csv" },
              { "type": "separator" },
              { "type": "table", "columns": ["a"], "rows": [["1"], ["2"]] },
              { "type": "button", "id": "run", "label": "Export", "style": "primary" }
            ]
          }]
        }"#;
        let ui = PluginUi::parse(raw).expect("descriptor should parse");

        assert_eq!(ui.title.as_deref(), Some("CSV Exporter"));
        assert_eq!(ui.toolbar.len(), 1);
        assert_eq!(ui.toolbar[0].icon, Icon::Export);
        assert!(ui.toolbar[0].enabled, "enabled defaults to true");
        assert_eq!(ui.menu.len(), 1);
        assert_eq!(ui.panels[0].items.len(), 8);
        assert!(ui.panels[0].open, "open defaults to true");

        let fields = ui.initial_fields();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields["delim"], FieldValue::Text(",".into()));
        assert_eq!(fields["headers"], FieldValue::Bool(true));
        assert_eq!(fields["limit"], FieldValue::Number(100.0));
        assert_eq!(fields["name"], FieldValue::Text("out.csv".into()));

        // An option with no label falls back to its value.
        if let Item::Select { options, .. } = &ui.panels[0].items[1] {
            assert_eq!(options[1].label, ";");
        } else {
            panic!("expected a select");
        }
    }

    #[test]
    fn an_empty_object_is_a_valid_empty_ui() {
        let ui = PluginUi::parse("{}").unwrap();
        assert!(ui.is_empty());
    }

    #[test]
    fn unknown_item_type_is_rejected() {
        let raw = r#"{"panels":[{"title":"p","items":[{"type":"iframe","src":"https://evil"}]}]}"#;
        assert!(matches!(PluginUi::parse(raw), Err(UiError::Malformed(_))));
    }

    #[test]
    fn unknown_icon_degrades_instead_of_failing() {
        let raw = r#"{"toolbar":[{"id":"a","label":"A","icon":"nuclear-launch"}]}"#;
        let ui = PluginUi::parse(raw).unwrap();
        assert_eq!(ui.toolbar[0].icon, Icon::Plug);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for raw in [
            "",
            "null",
            "[]",
            "\"a string\"",
            "{",
            "{\"panels\": 3}",
            "{\"panels\": [{\"items\": [{}]}]}",
            "{\"panels\": [{\"items\": [{\"type\": \"button\"}]}]}",
        ] {
            assert!(PluginUi::parse(raw).is_err(), "{:?} should not parse", raw);
        }
    }

    // -- hostile input -----------------------------------------------------

    #[test]
    fn oversized_descriptor_is_refused_before_parsing() {
        let raw = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_DESCRIPTOR_BYTES + 1));
        match PluginUi::parse(&raw) {
            Err(UiError::TooLarge { limit, .. }) => assert_eq!(limit, MAX_DESCRIPTOR_BYTES),
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }

    #[test]
    fn deeply_nested_json_errors_rather_than_overflowing_the_stack() {
        // A descriptor cannot legally nest at all: Item holds no Panel. This
        // is the bomb that would have mattered if it could.
        let depth = 20_000;
        let raw = format!("{{\"panels\":{}{}}}", "[".repeat(depth), "]".repeat(depth));
        assert!(
            matches!(
                PluginUi::parse(&raw),
                Err(UiError::Malformed(_)) | Err(UiError::TooLarge { .. })
            ),
            "a nesting bomb must be an error"
        );
    }

    #[test]
    fn absurd_quantities_are_refused() {
        let many = |n: usize| {
            let items: Vec<String> = (0..n)
                .map(|i| format!(r#"{{"type":"button","id":"b{}","label":"b"}}"#, i))
                .collect();
            format!(
                r#"{{"panels":[{{"title":"p","items":[{}]}}]}}"#,
                items.join(",")
            )
        };
        match PluginUi::parse(&many(MAX_ITEMS_PER_PANEL + 1)) {
            Err(UiError::TooMany { limit, .. }) => assert_eq!(limit, MAX_ITEMS_PER_PANEL),
            other => panic!("expected TooMany, got {:?}", other),
        }
        assert!(PluginUi::parse(&many(4)).is_ok());

        let panels: Vec<String> = (0..MAX_PANELS + 1)
            .map(|i| format!(r#"{{"title":"p{}","items":[]}}"#, i))
            .collect();
        assert!(matches!(
            PluginUi::parse(&format!(r#"{{"panels":[{}]}}"#, panels.join(","))),
            Err(UiError::TooMany { .. })
        ));

        let rows: Vec<String> = (0..MAX_TABLE_ROWS + 1).map(|_| r#"["x"]"#.into()).collect();
        assert!(matches!(
            PluginUi::parse(&format!(
                r#"{{"panels":[{{"title":"p","items":[{{"type":"table","columns":["a"],"rows":[{}]}}]}}]}}"#,
                rows.join(",")
            )),
            Err(UiError::TooMany { .. })
        ));
    }

    #[test]
    fn total_item_budget_is_enforced_across_panels() {
        let panel = |i: usize| {
            let items: Vec<String> = (0..MAX_ITEMS_PER_PANEL)
                .map(|j| format!(r#"{{"type":"button","id":"b{}_{}","label":"b"}}"#, i, j))
                .collect();
            format!(r#"{{"title":"p{}","items":[{}]}}"#, i, items.join(","))
        };
        let panels: Vec<String> = (0..6).map(panel).collect();
        match PluginUi::parse(&format!(r#"{{"panels":[{}]}}"#, panels.join(","))) {
            Err(UiError::TooMany { what, limit, .. }) => {
                assert_eq!(what, "items");
                assert_eq!(limit, MAX_TOTAL_ITEMS);
            }
            other => panic!("expected TooMany, got {:?}", other),
        }
    }

    #[test]
    fn hostile_ids_are_rejected() {
        for bad in ["", "a b", "a/b", "../x", "<script>"] {
            let raw = format!(
                r#"{{"panels":[{{"title":"p","items":[{{"type":"button","id":"{}","label":"x"}}]}}]}}"#,
                bad
            );
            assert!(
                matches!(
                    PluginUi::parse(&raw),
                    Err(UiError::BadId(_)) | Err(UiError::Malformed(_))
                ),
                "{:?} should be rejected",
                bad
            );
        }
        assert!(!is_valid_control_id(&"a".repeat(MAX_ID_CHARS + 1)));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let raw = r#"{"panels":[{"title":"p","items":[
            {"type":"input","id":"x","label":"a"},
            {"type":"input","id":"x","label":"b"}]}]}"#;
        assert!(matches!(PluginUi::parse(raw), Err(UiError::DuplicateId(_))));
    }

    #[test]
    fn control_characters_are_stripped_and_text_is_clamped() {
        let raw = format!(
            r#"{{"panels":[{{"title":"a\u0007b","items":[
                {{"type":"text","text":"{}"}},
                {{"type":"button","id":"b","label":"one\ntwo"}}]}}]}}"#,
            "y".repeat(MAX_TEXT_CHARS + 500)
        );
        let ui = PluginUi::parse(&raw).unwrap();
        assert_eq!(ui.panels[0].title, "a b");
        match &ui.panels[0].items[0] {
            Item::Text { text, .. } => assert_eq!(text.chars().count(), MAX_TEXT_CHARS),
            other => panic!("expected text, got {:?}", other),
        }
        match &ui.panels[0].items[1] {
            // A label is one line: the newline becomes a space.
            Item::Button { label, .. } => assert_eq!(label, "one two"),
            other => panic!("expected button, got {:?}", other),
        }
    }

    #[test]
    fn non_finite_numbers_are_neutralised() {
        // JSON has no NaN literal, so build the item directly: this guards
        // the renderer against a value arriving by any other route.
        let mut ui = ui_with(vec![Item::Number {
            id: "n".into(),
            label: "n".into(),
            value: f64::NAN,
            min: Some(f64::NEG_INFINITY),
            max: Some(f64::INFINITY),
            enabled: true,
        }]);
        ui.sanitize();
        match &ui.panels[0].items[0] {
            Item::Number {
                value, min, max, ..
            } => {
                assert_eq!(*value, 0.0);
                assert_eq!(*min, None);
                assert_eq!(*max, None);
            }
            other => panic!("expected number, got {:?}", other),
        }

        let mut inverted = ui_with(vec![Item::Number {
            id: "n".into(),
            label: "n".into(),
            value: 5.0,
            min: Some(10.0),
            max: Some(1.0),
            enabled: true,
        }]);
        inverted.sanitize();
        match &inverted.panels[0].items[0] {
            Item::Number { min, max, .. } => {
                assert_eq!((*min, *max), (None, None), "an inverted range is dropped");
            }
            other => panic!("expected number, got {:?}", other),
        }
    }

    // -- event dispatch ----------------------------------------------------

    #[test]
    fn button_without_an_action_uses_the_default_handler() {
        let ui = ui_with(vec![button("run", None)]);
        let d = dispatch_for(
            &ui,
            &UiEvent::ButtonPressed {
                id: "run".into(),
                fields: Fields::new(),
            },
        )
        .unwrap();
        assert_eq!(d.function, DEFAULT_HANDLER);
        assert_eq!(d.argument["type"], "buttonPressed");
        assert_eq!(d.argument["id"], "run");
    }

    #[test]
    fn button_may_nominate_its_own_handler() {
        let ui = ui_with(vec![button("run", Some("exportToCsv"))]);
        let d = dispatch_for(
            &ui,
            &UiEvent::ButtonPressed {
                id: "run".into(),
                fields: Fields::new(),
            },
        )
        .unwrap();
        assert_eq!(d.function, "exportToCsv");
    }

    #[test]
    fn a_handler_name_that_is_not_an_identifier_is_refused() {
        // This is the injection case: the runtime builds the call by name.
        for evil in [
            "x(); fetch('http://evil'); //",
            "constructor.constructor('...')()",
            "a.b",
            "a b",
            "1abc",
            "",
            "toString()",
        ] {
            assert!(!is_safe_handler_name(evil), "{:?} must be refused", evil);
            let ui = ui_with(vec![button("run", Some(evil))]);
            assert!(
                matches!(
                    dispatch_for(
                        &ui,
                        &UiEvent::ButtonPressed {
                            id: "run".into(),
                            fields: Fields::new()
                        }
                    ),
                    Err(UiError::BadHandler(_))
                ),
                "{:?} must not reach the runtime",
                evil
            );
        }
        for ok in ["onUiEvent", "_private", "$dollar", "a1", "exportToCsv"] {
            assert!(is_safe_handler_name(ok), "{:?} should be accepted", ok);
        }
        assert!(!is_safe_handler_name(&"a".repeat(MAX_HANDLER_CHARS + 1)));
    }

    #[test]
    fn a_descriptor_carrying_an_injecting_handler_fails_validation_up_front() {
        let raw = r#"{"panels":[{"title":"p","items":[
            {"type":"button","id":"b","label":"b","action":"x();evil();//"}]}]}"#;
        assert!(matches!(PluginUi::parse(raw), Err(UiError::BadHandler(_))));
    }

    #[test]
    fn events_for_controls_the_plugin_did_not_declare_are_refused() {
        let ui = ui_with(vec![button("run", None)]);
        assert!(matches!(
            dispatch_for(
                &ui,
                &UiEvent::ButtonPressed {
                    id: "ghost".into(),
                    fields: Fields::new()
                }
            ),
            Err(UiError::UnknownTarget(_))
        ));
        assert!(matches!(
            dispatch_for(&ui, &UiEvent::MenuSelected { id: "ghost".into() }),
            Err(UiError::UnknownTarget(_))
        ));
        assert!(matches!(
            dispatch_for(
                &ui,
                &UiEvent::ValueChanged {
                    id: "run".into(), // a button, not a field
                    value: FieldValue::Bool(true)
                }
            ),
            Err(UiError::UnknownTarget(_))
        ));
    }

    #[test]
    fn opened_asks_the_plugin_to_render() {
        let d = dispatch_for(&PluginUi::default(), &UiEvent::Opened).unwrap();
        assert_eq!(d.function, RENDER_HANDLER);
        assert_eq!(d.argument, serde_json::Value::Null);
    }

    #[test]
    fn value_changes_carry_their_typed_value() {
        let ui = ui_with(vec![
            Item::Checkbox {
                id: "flag".into(),
                label: "f".into(),
                value: false,
                enabled: true,
            },
            Item::Number {
                id: "n".into(),
                label: "n".into(),
                value: 0.0,
                min: None,
                max: None,
                enabled: true,
            },
        ]);
        let d = dispatch_for(
            &ui,
            &UiEvent::ValueChanged {
                id: "flag".into(),
                value: FieldValue::Bool(true),
            },
        )
        .unwrap();
        assert_eq!(d.function, DEFAULT_HANDLER);
        assert_eq!(d.argument["value"], serde_json::json!(true));

        let d = dispatch_for(
            &ui,
            &UiEvent::ValueChanged {
                id: "n".into(),
                value: FieldValue::Number(4.5),
            },
        )
        .unwrap();
        assert_eq!(d.argument["value"], serde_json::json!(4.5));
    }

    #[test]
    fn a_button_press_carries_the_whole_form() {
        let ui = ui_with(vec![
            Item::Input {
                id: "name".into(),
                label: "n".into(),
                value: "out.csv".into(),
                placeholder: String::new(),
                multiline: false,
                enabled: true,
            },
            button("run", None),
        ]);
        let d = dispatch_for(
            &ui,
            &UiEvent::ButtonPressed {
                id: "run".into(),
                fields: ui.initial_fields(),
            },
        )
        .unwrap();
        assert_eq!(d.argument["fields"]["name"], "out.csv");
    }

    #[test]
    fn toolbar_buttons_dispatch_like_panel_buttons() {
        let ui = PluginUi {
            toolbar: vec![ToolbarItem {
                id: "t".into(),
                label: "T".into(),
                tooltip: None,
                icon: Icon::Export,
                action: Some("go".into()),
                enabled: true,
            }],
            ..Default::default()
        };
        let d = dispatch_for(
            &ui,
            &UiEvent::ButtonPressed {
                id: "t".into(),
                fields: Fields::new(),
            },
        )
        .unwrap();
        assert_eq!(d.function, "go");
    }

    #[test]
    fn field_values_round_trip_through_json() {
        let raw = r#"{"type":"valueChanged","id":"a","value":"text"}"#;
        let e: UiEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(
            e,
            UiEvent::ValueChanged {
                id: "a".into(),
                value: FieldValue::Text("text".into())
            }
        );

        let e: UiEvent =
            serde_json::from_str(r#"{"type":"valueChanged","id":"a","value":true}"#).unwrap();
        assert!(matches!(
            e,
            UiEvent::ValueChanged {
                value: FieldValue::Bool(true),
                ..
            }
        ));

        let e: UiEvent =
            serde_json::from_str(r#"{"type":"valueChanged","id":"a","value":7}"#).unwrap();
        assert!(matches!(
            e,
            UiEvent::ValueChanged {
                value: FieldValue::Number(_),
                ..
            }
        ));
    }

    #[test]
    fn numbers_display_without_a_pointless_decimal() {
        assert_eq!(FieldValue::Number(100.0).as_text(), "100");
        assert_eq!(FieldValue::Number(1.5).as_text(), "1.5");
        assert_eq!(FieldValue::Bool(true).as_text(), "true");
    }

    // -- config schema forms ----------------------------------------------

    #[test]
    fn config_schema_becomes_a_panel_of_the_same_vocabulary() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "delimiter": {
                    "type": "string",
                    "enum": [",", ";", "\t"],
                    "default": ",",
                    "title": "Field Delimiter",
                    "description": "Character to separate fields"
                },
                "includeHeaders": { "type": "boolean", "default": true, "title": "Include Headers" },
                "maxRows": { "type": "number", "default": 100, "minimum": 1, "maximum": 5000 },
                "nullValue": { "type": "string", "default": "" }
            }
        });
        let current = serde_json::json!({ "includeHeaders": false });
        let panel = config_schema_panel(&schema, Some(&current));

        let fields: Vec<&str> = panel.items.iter().filter_map(|i| i.id()).collect();
        assert_eq!(
            fields,
            vec!["delimiter", "includeHeaders", "maxRows", "nullValue"]
        );

        match &panel.items[0] {
            Item::Select {
                label,
                value,
                options,
                ..
            } => {
                assert_eq!(label, "Field Delimiter");
                assert_eq!(value, ",");
                assert_eq!(options.len(), 3);
                assert_eq!(options[2].label, "Tab", "a tab must not render as blank");
            }
            other => panic!("expected a select, got {:?}", other),
        }
        // The description is rendered as muted text under its control.
        assert!(matches!(
            &panel.items[1],
            Item::Text {
                style: TextStyle::Muted,
                ..
            }
        ));
        match &panel.items[2] {
            Item::Checkbox { value, .. } => {
                assert!(!value, "the saved config wins over the schema default")
            }
            other => panic!("expected a checkbox, got {:?}", other),
        }
        match panel.items.iter().find(|i| i.id() == Some("maxRows")) {
            Some(Item::Number {
                value, min, max, ..
            }) => {
                assert_eq!(*value, 100.0);
                assert_eq!(*min, Some(1.0));
                assert_eq!(*max, Some(5000.0));
            }
            other => panic!("expected a number, got {:?}", other),
        }
    }

    #[test]
    fn a_missing_or_odd_config_schema_yields_an_empty_form() {
        assert!(config_schema_panel(&serde_json::json!({}), None)
            .items
            .is_empty());
        assert!(config_schema_panel(&serde_json::json!(null), None)
            .items
            .is_empty());
        assert!(
            config_schema_panel(&serde_json::json!({"properties": "not an object"}), None)
                .items
                .is_empty()
        );
    }

    #[test]
    fn config_schema_property_names_that_are_not_ids_are_dropped() {
        let schema = serde_json::json!({
            "properties": {
                "ok": { "type": "string" },
                "not ok": { "type": "string" },
                "../escape": { "type": "string" }
            }
        });
        let panel = config_schema_panel(&schema, None);
        let ids: Vec<&str> = panel.items.iter().filter_map(|i| i.id()).collect();
        assert_eq!(ids, vec!["ok"]);
    }

    #[test]
    fn form_fields_collapse_back_into_a_config_object() {
        let mut fields = Fields::new();
        fields.insert("a".into(), FieldValue::Text("x".into()));
        fields.insert("b".into(), FieldValue::Bool(true));
        fields.insert("c".into(), FieldValue::Number(3.0));
        assert_eq!(
            fields_to_config(&fields),
            serde_json::json!({ "a": "x", "b": true, "c": 3.0 })
        );
    }
}
