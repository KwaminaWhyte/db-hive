//! Maps the plugin manager's plain view types
//! (`dbhive_ui_model::plugins_panel`) to the Slint structs the panel draws,
//! and writes a whole [`PluginsPanel`] into the `PluginsState` global.
//!
//! Kept apart from the controller so the snapshot tool builds the very same
//! models without a controller behind them.

use dbhive_ui_model::plugins::Tab;
use dbhive_ui_model::plugins_panel::{
    DetailView, InstalledRow, ItemRow, Load, MarketRow, PluginsPanel, Scope, ToolbarEntry,
};
use slint::{Model, ModelRc, SharedString, VecModel};

use crate::{
    PluginFact, PluginInstalledRow, PluginItem, PluginMarketItem, PluginPermissionItem,
    PluginToolbarItem, PluginsState,
};

/// At most this many unreadable plugins get a banner each; the rest are counted.
const MAX_PROBLEM_BANNERS: usize = 3;

fn strings(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>(),
    ))
}

pub fn installed_item(r: &InstalledRow) -> PluginInstalledRow {
    PluginInstalledRow {
        id: r.id.as_str().into(),
        name: r.name.as_str().into(),
        subtitle: r.subtitle.as_str().into(),
        category: r.category.as_str().into(),
        enabled: r.enabled,
        busy: r.busy.unwrap_or("").into(),
        selected: r.selected,
    }
}

pub fn market_item(r: &MarketRow) -> PluginMarketItem {
    PluginMarketItem {
        id: r.id.as_str().into(),
        name: r.name.as_str().into(),
        byline: r.byline.as_str().into(),
        description: r.description.as_str().into(),
        meta: r.meta.as_str().into(),
        category: r.category.as_str().into(),
        keywords: strings(&r.keywords),
        requests: r.requests.as_str().into(),
        sensitive: r.sensitive,
        verified: r.verified,
        state: r.state_kind,
        note: r.state_note.as_str().into(),
    }
}

pub fn item(r: &ItemRow) -> PluginItem {
    PluginItem {
        kind: r.kind,
        id: r.id.as_str().into(),
        label: r.label.as_str().into(),
        text: r.text.as_str().into(),
        hint: r.hint.as_str().into(),
        error: r.error.as_str().into(),
        style: r.style,
        placeholder: r.placeholder.as_str().into(),
        checked: r.checked,
        enabled: r.enabled,
        multiline: r.multiline,
        options: strings(&r.options),
        option_index: r.option_index,
        cells: strings(&r.cells),
        collapsible: r.collapsible,
        open: r.open,
        panel: r.panel,
    }
}

pub fn toolbar_item(t: &ToolbarEntry) -> PluginToolbarItem {
    PluginToolbarItem {
        plugin_id: t.plugin_id.as_str().into(),
        plugin_name: t.plugin_name.as_str().into(),
        id: t.id.as_str().into(),
        label: t.label.as_str().into(),
        tooltip: t.tooltip.as_str().into(),
        icon: t.icon,
        enabled: t.enabled,
    }
}

fn facts(pairs: &[(String, String)]) -> ModelRc<PluginFact> {
    ModelRc::new(VecModel::from(
        pairs
            .iter()
            .map(|(l, v)| PluginFact { label: l.as_str().into(), value: v.as_str().into() })
            .collect::<Vec<_>>(),
    ))
}

fn model<T: Clone + 'static, U>(rows: &[U], f: impl Fn(&U) -> T) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows.iter().map(f).collect::<Vec<_>>()))
}

fn clear_detail(s: &PluginsState) {
    s.set_has_detail(false);
    s.set_d_interface_open(false);
    s.set_d_busy("".into());
}

fn set_detail(s: &PluginsState, d: &DetailView) {
    s.set_has_detail(true);
    s.set_d_name(d.name.as_str().into());
    s.set_d_version(d.version.as_str().into());
    s.set_d_description(d.description.as_str().into());
    s.set_d_enabled(d.enabled);
    s.set_d_busy(d.busy.unwrap_or("").into());
    s.set_d_can_open(d.can_open);
    s.set_d_has_settings(d.has_settings);
    s.set_d_interface_open(d.interface_open);
    s.set_d_info(facts(&d.info));
    s.set_d_stats(facts(&d.stats));
    s.set_d_permissions(model(&d.permissions, |p| PluginPermissionItem {
        label: p.label.as_str().into(),
        detail: p.detail.as_str().into(),
        sensitive: p.sensitive,
    }));
    s.set_d_keywords(strings(&d.keywords));
    s.set_d_links(facts(&d.links));
}

/// Put `rows` into a property's model. When the new rows line up with the old
/// ones (same kinds and ids) only the cells that differ are written, so a
/// text box being typed into keeps its focus and caret; otherwise the model is
/// replaced and every row is rebuilt.
pub fn sync_items(current: ModelRc<PluginItem>, rows: &[ItemRow]) -> ModelRc<PluginItem> {
    let fresh: Vec<PluginItem> = rows.iter().map(item).collect();
    if let Some(vec) = current.as_any().downcast_ref::<VecModel<PluginItem>>() {
        let same_shape = vec.row_count() == fresh.len()
            && fresh.iter().enumerate().all(|(i, f)| {
                vec.row_data(i).is_some_and(|old| old.kind == f.kind && old.id == f.id)
            });
        if same_shape {
            for (i, f) in fresh.into_iter().enumerate() {
                if vec.row_data(i).as_ref() != Some(&f) {
                    vec.set_row_data(i, f);
                }
            }
            return current;
        }
    }
    ModelRc::new(VecModel::from(fresh))
}

fn replace_items(rows: &[ItemRow]) -> ModelRc<PluginItem> {
    ModelRc::new(VecModel::from(rows.iter().map(item).collect::<Vec<_>>()))
}

/// How the item lists are written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Items {
    /// Rebuild every row (a plugin sent a new interface).
    Rebuild,
    /// Update in place where the shape allows (the user edited a field).
    Patch,
}

/// Write everything the panel shows.
pub fn apply(s: &PluginsState, p: &PluginsPanel, items: Items) {
    s.set_tab(match p.tab {
        Tab::Installed => 0,
        Tab::Marketplace => 1,
    });
    match &p.load {
        Load::Loading => {
            s.set_load(1);
            s.set_error("".into());
        }
        Load::Ready => {
            s.set_load(0);
            s.set_error("".into());
        }
        Load::Failed(e) => {
            s.set_load(2);
            s.set_error(e.as_str().into());
        }
    }

    let mut problems: Vec<String> = p
        .problems
        .iter()
        .take(MAX_PROBLEM_BANNERS)
        .map(|m| format!("Could not load a plugin: {}", m))
        .collect();
    if p.problems.len() > MAX_PROBLEM_BANNERS {
        problems.push(format!(
            "and {} more plugins could not be loaded",
            p.problems.len() - MAX_PROBLEM_BANNERS
        ));
    }
    s.set_problems(strings(&problems));

    s.set_installed(model(&p.installed_rows(), installed_item));
    s.set_installed_summary(p.installed_summary().into());
    match p.detail() {
        Some(d) => set_detail(s, &d),
        None => clear_detail(s),
    }

    // The interface and the settings form.
    s.set_view_title(p.view_title().into());
    s.set_view_error(p.view_error().into());
    s.set_view_toolbar(model(&p.view_toolbar(), toolbar_item));
    let (view, form) = (p.items(Scope::View), p.items(Scope::Form));
    let (view_model, form_model) = match items {
        Items::Rebuild => (replace_items(&view), replace_items(&form)),
        Items::Patch => (sync_items(s.get_view_items(), &view), sync_items(s.get_form_items(), &form)),
    };
    s.set_view_items(view_model);
    s.set_form_items(form_model);
    s.set_form_open(p.form.is_some());
    s.set_form_title(
        p.form
            .as_ref()
            .map(|f| format!("{} settings", f.plugin_name))
            .unwrap_or_default()
            .into(),
    );

    // Marketplace.
    s.set_categories(strings(&PluginsPanel::category_options()));
    s.set_category_index(p.category_index());
    s.set_sort_options(strings(&PluginsPanel::sort_options()));
    s.set_sort_index(p.sort_index());
    s.set_verified_only(p.filters.verified_only);
    s.set_filters_active(p.filters_active());
    let market = p.market_rows();
    s.set_market_summary(p.market_summary(market.len()).into());
    s.set_market(model(&market, market_item));

    // The application's own toolbar.
    s.set_toolbar(model(&p.header_toolbar(), toolbar_item));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_ui_model::plugins_panel::kind;

    fn row(kind: i32, id: &str, text: &str) -> ItemRow {
        ItemRow {
            kind,
            id: id.into(),
            text: text.into(),
            enabled: true,
            option_index: -1,
            panel: -1,
            ..ItemRow::default()
        }
    }

    #[test]
    fn rows_keep_their_kind_id_and_value_across_the_mapping() {
        let mut r = row(kind::SELECT, "fmt", "B");
        r.options = vec!["A".into(), "B".into()];
        r.option_index = 1;
        let m = item(&r);
        assert_eq!((m.kind, m.id.as_str(), m.text.as_str(), m.option_index), (kind::SELECT, "fmt", "B", 1));
        assert_eq!(m.options.row_count(), 2);
        assert_eq!(m.options.row_data(1).unwrap().as_str(), "B");
    }

    #[test]
    fn a_patch_with_the_same_shape_updates_rows_in_place() {
        let first = vec![row(kind::INPUT, "name", "a"), row(kind::BUTTON, "run", "")];
        let model = replace_items(&first);
        let before = model.as_any().downcast_ref::<VecModel<PluginItem>>().unwrap() as *const _;

        let next = vec![row(kind::INPUT, "name", "abc"), row(kind::BUTTON, "run", "")];
        let out = sync_items(model, &next);
        let after = out.as_any().downcast_ref::<VecModel<PluginItem>>().unwrap() as *const _;
        assert_eq!(before, after, "the same model, so the text box keeps focus and caret");
        assert_eq!(out.row_data(0).unwrap().text.as_str(), "abc");
    }

    #[test]
    fn a_patch_with_a_different_shape_rebuilds_the_model() {
        let first = vec![row(kind::INPUT, "name", "a")];
        let model = replace_items(&first);
        let before = model.as_any().downcast_ref::<VecModel<PluginItem>>().unwrap() as *const _;

        let next = vec![row(kind::INPUT, "other", "a"), row(kind::BUTTON, "run", "")];
        let out = sync_items(model, &next);
        let after = out.as_any().downcast_ref::<VecModel<PluginItem>>().unwrap() as *const _;
        assert_ne!(before, after);
        assert_eq!(out.row_count(), 2);
    }
}
