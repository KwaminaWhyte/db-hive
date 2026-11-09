//! What a native file or folder picker should ask for.
//!
//! The shell owns the dialog itself (it is a platform call); this module
//! decides its title, filters, starting folder and suggested name, from the
//! path already typed in the field the Browse button sits beside. Typing a
//! path and then pressing Browse opens the picker where that path points.

use std::path::{Path, PathBuf};

/// Which kind of picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    /// An existing file.
    Open,
    /// A file to write.
    Save,
    /// A directory.
    Folder,
}

/// One extension filter: a label and extensions without dots.
pub type Filter = (String, Vec<String>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRequest {
    pub kind: PickKind,
    pub title: String,
    pub filters: Vec<Filter>,
    /// Folder to open in, when one is known and exists.
    pub directory: Option<PathBuf>,
    /// Suggested file name for a save.
    pub file_name: Option<String>,
}

fn filter(label: &str, exts: &[&str]) -> Filter {
    (
        label.to_string(),
        exts.iter().map(|e| (*e).to_string()).collect(),
    )
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/x` to an absolute path; anything else unchanged.
pub fn expand_home(raw: &str) -> PathBuf {
    let t = raw.trim();
    if let Some(rest) = t.strip_prefix("~/") {
        if let Some(h) = home() {
            return h.join(rest);
        }
    }
    if t == "~" {
        if let Some(h) = home() {
            return h;
        }
    }
    PathBuf::from(t)
}

/// Split a typed path into (folder to start in, file name), keeping only a
/// folder that exists, so the picker never opens on a dead path.
pub fn split_current(current: &str) -> (Option<PathBuf>, Option<String>) {
    let t = current.trim();
    if t.is_empty() {
        return (None, None);
    }
    let path = expand_home(t);
    if path.is_dir() {
        return (Some(path), None);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty());
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty() && d.is_dir())
        .map(Path::to_path_buf);
    (dir, name)
}

fn open(title: &str, filters: Vec<Filter>, current: &str) -> PickRequest {
    let (directory, _) = split_current(current);
    PickRequest {
        kind: PickKind::Open,
        title: title.to_string(),
        filters,
        directory,
        file_name: None,
    }
}

/// A SQLite database file for the connection editor. No extension filter:
/// SQLite files often have none (or an odd one), and a filter would hide
/// them. Platform pickers have no reliable "all files" filter to add instead.
pub fn sqlite_file(current: &str) -> PickRequest {
    open("Choose a SQLite database", Vec::new(), current)
}

/// An SSH private key. Opens in `~/.ssh` when nothing is typed yet.
pub fn ssh_key(current: &str) -> PickRequest {
    let mut req = open("Choose an SSH private key", Vec::new(), current);
    if req.directory.is_none() {
        req.directory = home().map(|h| h.join(".ssh")).filter(|d| d.is_dir());
    }
    req
}

/// A data file to import.
pub fn data_file(current: &str) -> PickRequest {
    open(
        "Choose a file to import",
        vec![
            filter("Data files", &["csv", "tsv", "txt", "xlsx", "xls"]),
            filter("CSV", &["csv"]),
            filter("Excel", &["xlsx", "xls"]),
        ],
        current,
    )
}

/// A `.sql` file to run.
pub fn sql_file(current: &str) -> PickRequest {
    open("Choose a SQL file", vec![filter("SQL", &["sql"])], current)
}

/// Where to write a file, starting from the path in the field (often a
/// suggested, date-stamped name). The filter follows that name's extension.
pub fn save_as(title: &str, current: &str) -> PickRequest {
    let (directory, file_name) = split_current(current);
    let filters = file_name
        .as_deref()
        .and_then(|n| Path::new(n).extension())
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| !e.is_empty())
        .map(|e| vec![filter(&e.to_uppercase(), &[&e])])
        .unwrap_or_default();
    PickRequest {
        kind: PickKind::Save,
        title: title.to_string(),
        filters,
        directory,
        file_name,
    }
}

/// A folder, opening at the typed one when it exists.
pub fn folder(title: &str, current: &str) -> PickRequest {
    let t = current.trim();
    let path = expand_home(t);
    let directory = if !t.is_empty() && path.is_dir() {
        Some(path)
    } else {
        split_current(t).0
    };
    PickRequest {
        kind: PickKind::Folder,
        title: title.to_string(),
        filters: Vec::new(),
        directory,
        file_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("dbhive-file-pick-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_field_opens_wherever_the_platform_likes() {
        assert_eq!(split_current(""), (None, None));
        assert_eq!(split_current("   "), (None, None));
        let r = sqlite_file("");
        assert_eq!(r.kind, PickKind::Open);
        assert!(r.directory.is_none());
        assert!(r.filters.is_empty(), "SQLite files often have no extension");
        assert_eq!(sql_file("").filters[0].1, ["sql"]);
    }

    #[test]
    fn a_typed_path_in_an_existing_folder_starts_there() {
        let d = tmp();
        let typed = d.join("data.csv");
        let r = data_file(&typed.to_string_lossy());
        assert_eq!(r.directory.as_deref(), Some(d.as_path()));
        assert!(r.file_name.is_none(), "open pickers suggest no name");
    }

    #[test]
    fn a_dead_folder_is_dropped_but_the_name_kept() {
        let (dir, name) = split_current("/no/such/dir/at/all/x.sql");
        assert!(dir.is_none());
        assert_eq!(name.as_deref(), Some("x.sql"));
    }

    #[test]
    fn save_suggests_the_typed_name_and_filters_by_its_extension() {
        let d = tmp();
        let typed = d.join("orders_2026-10-21.csv");
        let r = save_as("Export", &typed.to_string_lossy());
        assert_eq!(r.kind, PickKind::Save);
        assert_eq!(r.directory.as_deref(), Some(d.as_path()));
        assert_eq!(r.file_name.as_deref(), Some("orders_2026-10-21.csv"));
        assert_eq!(
            r.filters,
            vec![("CSV".to_string(), vec!["csv".to_string()])]
        );

        let r = save_as("Export", "dump");
        assert!(r.filters.is_empty());
        assert_eq!(r.file_name.as_deref(), Some("dump"));
    }

    #[test]
    fn folder_opens_at_an_existing_typed_folder() {
        let d = tmp();
        let r = folder("Backups", &d.to_string_lossy());
        assert_eq!(r.kind, PickKind::Folder);
        assert_eq!(r.directory.as_deref(), Some(d.as_path()));
        assert!(folder("Backups", "").directory.is_none());
    }

    #[test]
    fn tilde_expands_to_home() {
        if let Some(h) = home() {
            assert_eq!(expand_home("~/x/y"), h.join("x/y"));
            assert_eq!(expand_home("~"), h);
        }
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
    }
}
