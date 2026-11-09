//! The Redis key browser's controller: scan, filter, read a key, copy it.
//!
//! The driver does the scanning (`scan_keys`) and reading (`key_detail`);
//! `dbhive_ui_model::redis_browser` decides the wording; this file moves
//! values between them and the `RedisState` global.

use std::rc::Rc;

use dbhive_core::drivers::{KeyDetail, KeyEntry};
use dbhive_core::models::DbDriver;
use dbhive_ui_model::redis_browser;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{with_ctx, Ctx};
use crate::{runtime, KeyValueRow, RedisKeyItem, RedisState, Tone};

const PAGE: usize = 200;

#[derive(Default)]
pub(super) struct RedisSession {
    open: bool,
    database: i32,
    pattern: String,
    type_index: i32,
    entries: Vec<KeyEntry>,
    cursor: u64,
    loading: bool,
    error: String,
    selected: String,
    detail: Option<KeyDetail>,
    detail_loading: bool,
    detail_error: String,
    /// Bumped per scan, so a slow older page cannot overwrite a newer one.
    generation: u64,
}

impl Ctx {
    fn redis_supported(&self) -> bool {
        matches!(
            self.session.borrow().workspace.driver,
            Some(DbDriver::Redis)
        )
    }

    pub(super) fn redis_open(self: &Rc<Self>) {
        if !self.redis_supported() {
            self.toast("The key browser is for Redis connections", Tone::Danger);
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            s.redis = RedisSession {
                open: true,
                database: s
                    .workspace
                    .database
                    .as_deref()
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(0),
                pattern: "*".to_string(),
                type_index: 0,
                ..RedisSession::default()
            };
        }
        self.redis_refresh();
        self.redis_scan(false);
    }

    fn redis_close(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.redis.open = false;
            s.redis.entries.clear();
            s.redis.detail = None;
            s.redis.selected.clear();
        }
        self.redis_refresh();
    }

    /// Scan with the current pattern. `append` continues from the last cursor.
    fn redis_scan(self: &Rc<Self>, append: bool) {
        let prep = {
            let mut s = self.session.borrow_mut();
            let Some(kind) = s.workspace.driver.clone() else {
                return;
            };
            if kind != DbDriver::Redis {
                return;
            }
            s.redis.generation += 1;
            let generation = s.redis.generation;
            let pattern = s.redis.pattern.trim().to_string();
            let cursor = if append { s.redis.cursor } else { 0 };
            s.redis.loading = true;
            s.redis.error.clear();
            if !append {
                s.redis.entries.clear();
                s.redis.cursor = 0;
            }
            (
                generation,
                pattern,
                cursor,
                s.workspace.connection_id.clone(),
            )
        };
        let (generation, pattern, cursor, connection_id) = prep;
        let Some(driver) = self.open_driver(&connection_id) else {
            self.redis_scanned(
                generation,
                append,
                Err("The connection is closed".to_string()),
            );
            return;
        };
        self.redis_refresh();
        runtime::spawn(
            self.ui_id,
            async move {
                driver
                    .scan_keys(&pattern, cursor, PAGE)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| with_ctx(|ctx| ctx.redis_scanned(generation, append, result)),
        );
    }

    fn redis_scanned(
        &self,
        generation: u64,
        append: bool,
        result: Result<dbhive_core::drivers::KeyPage, String>,
    ) {
        {
            let mut s = self.session.borrow_mut();
            if s.redis.generation != generation {
                return;
            }
            s.redis.loading = false;
            match result {
                Ok(page) => {
                    if !append {
                        s.redis.entries.clear();
                    }
                    s.redis.entries.extend(page.entries);
                    s.redis.cursor = page.cursor;
                }
                Err(e) => s.redis.error = e,
            }
        }
        self.redis_refresh();
    }

    fn redis_key_picked(self: &Rc<Self>, key: String) {
        {
            let mut s = self.session.borrow_mut();
            s.redis.selected = key.clone();
            s.redis.detail = None;
            s.redis.detail_error.clear();
            s.redis.detail_loading = true;
        }
        self.redis_refresh();
        let connection_id = self.session.borrow().workspace.connection_id.clone();
        let Some(driver) = self.open_driver(&connection_id) else {
            self.redis_detail_loaded(key, Err("The connection is closed".to_string()));
            return;
        };
        let worker_key = key.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                driver
                    .key_detail(&worker_key)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| with_ctx(|ctx| ctx.redis_detail_loaded(key, result)),
        );
    }

    fn redis_detail_loaded(&self, key: String, result: Result<KeyDetail, String>) {
        {
            let mut s = self.session.borrow_mut();
            if s.redis.selected != key {
                return;
            }
            s.redis.detail_loading = false;
            match result {
                Ok(detail) => s.redis.detail = Some(detail),
                Err(e) => {
                    s.redis.detail = None;
                    s.redis.detail_error = e;
                }
            }
        }
        self.redis_refresh();
    }

    /// Switch logical database, then scan again. `SELECT` runs on the same
    /// connection, so every later command sees the new database.
    fn redis_database_picked(self: &Rc<Self>, index: i32) {
        if !(0..=15).contains(&index) {
            return;
        }
        if self.session.borrow().redis.database == index {
            return;
        }
        let connection_id = self.session.borrow().workspace.connection_id.clone();
        let Some(driver) = self.open_driver(&connection_id) else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            s.redis.database = index;
            s.redis.entries.clear();
            s.redis.cursor = 0;
            s.redis.selected.clear();
            s.redis.detail = None;
            s.redis.loading = true;
        }
        self.redis_refresh();
        let statement = format!("SELECT {index}");
        runtime::spawn(
            self.ui_id,
            async move {
                driver
                    .execute_query(&statement)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| match result {
                    Ok(_) => ctx.redis_scan(false),
                    Err(e) => {
                        {
                            let mut s = ctx.session.borrow_mut();
                            s.redis.loading = false;
                            s.redis.error = format!("Could not switch database: {e}");
                        }
                        ctx.redis_refresh();
                    }
                })
            },
        );
    }

    fn redis_type_picked(&self, index: i32) {
        self.session.borrow_mut().redis.type_index = index.max(0);
        self.redis_refresh();
    }

    fn redis_pattern_changed(&self, text: String) {
        self.session.borrow_mut().redis.pattern = text;
    }

    fn redis_copy(self: &Rc<Self>) {
        let detail = self.session.borrow().redis.detail.clone();
        let Some(detail) = detail else {
            return;
        };
        let text = redis_browser::detail_tsv(&detail);
        if let Some(w) = self.window.upgrade() {
            w.invoke_copy_to_clipboard(text.as_str().into());
        }
        self.toast("Copied the value", Tone::Success);
    }

    fn redis_refresh(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let g = w.global::<RedisState>();
        let s = self.session.borrow();
        let r = &s.redis;

        let databases: Vec<slint::SharedString> =
            (0..16).map(|i| format!("db{i}").into()).collect();
        let type_options = redis_browser::type_options();
        let type_index = r.type_index.clamp(0, type_options.len() as i32 - 1);
        let kind = if type_index == 0 {
            ""
        } else {
            type_options[type_index as usize].as_str()
        };
        let visible = redis_browser::filter(&r.entries, "", kind);
        let keys: Vec<RedisKeyItem> = visible
            .iter()
            .map(|e| RedisKeyItem {
                key: e.key.as_str().into(),
                kind: e.kind.as_str().into(),
                ttl: redis_browser::ttl_label(e.ttl).into(),
            })
            .collect();

        g.set_visible(r.open);
        g.set_databases(ModelRc::new(VecModel::from(databases)));
        g.set_database_index(r.database);
        g.set_database_label(format!("db{}", r.database).into());
        g.set_pattern(r.pattern.as_str().into());
        g.set_type_options(ModelRc::new(VecModel::from(
            type_options
                .iter()
                .map(|o| slint::SharedString::from(o.as_str()))
                .collect::<Vec<_>>(),
        )));
        g.set_type_index(type_index);
        g.set_type_label(type_options[type_index as usize].as_str().into());
        g.set_keys(ModelRc::new(VecModel::from(keys)));
        g.set_list_note(redis_browser::list_note(visible.len(), r.cursor).into());
        g.set_has_more(r.cursor != 0);
        g.set_loading(r.loading);
        g.set_error(r.error.as_str().into());
        g.set_selected(r.selected.as_str().into());
        g.set_detail_loading(r.detail_loading);
        g.set_detail_error(r.detail_error.as_str().into());

        let detail = r.detail.as_ref();
        g.set_detail_summary(
            detail
                .map(redis_browser::detail_summary)
                .unwrap_or_default()
                .into(),
        );
        g.set_detail_note(detail.map(|d| d.note.clone()).unwrap_or_default().into());
        g.set_detail_two_columns(detail.map(|d| !d.columns.1.is_empty()).unwrap_or(false));
        let rows: Vec<KeyValueRow> = detail
            .map(|d| {
                d.rows
                    .iter()
                    .map(|(a, b)| KeyValueRow {
                        a: a.as_str().into(),
                        b: b.as_str().into(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        g.set_detail_rows(ModelRc::new(VecModel::from(rows)));
    }
}

pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let g = window.global::<RedisState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            g.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_close, |c| c.redis_close());
    on!(on_scan, |c| c.redis_scan(false));
    on!(on_load_more, |c| c.redis_scan(true));
    on!(on_database_picked, |c, i| c.redis_database_picked(i));
    on!(on_type_picked, |c, i| c.redis_type_picked(i));
    on!(on_pattern_changed, |c, t| c
        .redis_pattern_changed(t.to_string()));
    on!(on_key_picked, |c, k| c.redis_key_picked(k.to_string()));
    on!(on_detail_copy, |c| c.redis_copy());
}
