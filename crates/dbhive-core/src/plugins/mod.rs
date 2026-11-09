//! Plugins: manifests, lifecycle, the JavaScript runtime, and the
//! declarative interface a plugin may describe.
//!
//! The plugin engine, shared by the app and its tests. Nothing here knows
//! about a window, a framework handle or an event emitter; state changes are published on a broadcast channel and
//! anything a plugin asks the host to do is returned to the caller.
//!
//! # What a plugin is
//!
//! A directory with a `manifest.json` and a JavaScript entry point, executed
//! by [Boa](https://boajs.dev), an interpreter written in Rust. There is no
//! `eval` of plugin code into the host, no native code loading, and no
//! WebAssembly (the manifest can say `webAssembly`, and loading it fails).
//!
//! # What a plugin can do
//!
//! Only what its manifest declares, and only through the functions the
//! runtime installs:
//!
//! * read and write files **inside its own data directory** and nowhere else;
//! * make http/https requests to public addresses (private, loopback and
//!   link-local ranges are refused, before and after DNS resolution);
//! * read and write the clipboard;
//! * raise a notification;
//! * describe an interface as data, which the host draws with its own
//!   widgets (see [`ui`]).
//!
//! # What a plugin cannot do
//!
//! * Reach a database. There is no connection, driver, credential or query
//!   API in the runtime. `executeQuery` throws.
//! * Reach the filesystem outside its data directory, by any path.
//! * Run a subprocess, load a library, or open a socket other than through
//!   the guarded HTTP function.
//! * Draw anything the host did not implement. The interface vocabulary is a
//!   closed enum; there is no HTML, no script, no image and no colour in it.
//! * Cause the host to call a function it did not itself declare in the
//!   interface currently on screen.

pub mod api;
pub mod error;
pub mod manager;
pub mod manifest;
pub mod marketplace;
pub mod runtime;
pub mod sandbox;
pub mod store;
pub mod ui;

pub use api::{Notification, NotificationKind, PluginApi, PluginContext};
pub use error::{PluginError, PluginResult};
pub use manager::{PluginManager, RunOutcome};
pub use manifest::{
    permission_summary, Plugin, PluginAuthor, PluginCategory, PluginManifest, PluginPermission,
    PluginStats, PluginType,
};
pub use marketplace::{Filters, MarketplacePlugin, MarketplaceStats, SortOption};
pub use runtime::{HostCall, PluginRuntimeSync};
pub use store::{PluginStore, ScanResult};
pub use ui::{Dispatch, FieldValue, Item, Panel, PluginUi, UiError, UiEvent};

use serde::{Deserialize, Serialize};

/// A lifecycle change, published on [`PluginManager::subscribe`].
///
/// This is the payload the app emitted as `plugin-event`; it is kept
/// identical so the two shells report the same things.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PluginEvent {
    Installed {
        plugin_id: String,
        version: String,
    },
    Uninstalled {
        plugin_id: String,
    },
    Enabled {
        plugin_id: String,
    },
    Disabled {
        plugin_id: String,
    },
    Updated {
        plugin_id: String,
        old_version: String,
        new_version: String,
    },
    Error {
        plugin_id: String,
        error: String,
    },
}
