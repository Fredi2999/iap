//! Lesender Zugriff auf ein Repository mit `gix`.
//!
//! Hier wird nichts verändert: Ort, letzter Commit, Zweig sowie die Frage, ob das
//! Repository Dinge enthält, die beim Anlegen eines Worktrees fremde Programme
//! starten könnten (externe Filter) oder als Hooks eingerichtet sind.

use std::path::{Path, PathBuf};

use super::GitError;

/// Zusammenfassung eines Repositorys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    /// Arbeitsverzeichnis (Wurzel des Projekts).
    pub root: PathBuf,
    /// Git-Verzeichnis.
    pub git_dir: PathBuf,
    /// Commit von `HEAD`, falls es schon einen gibt.
    pub head: Option<String>,
    /// Kurzname des Zweigs, `None` bei abgelöstem `HEAD`.
    pub branch: Option<String>,
    /// Anzahl konfigurierter externer Filter (`filter.<name>.*`).
    pub filter_sections: usize,
    /// Aktive (nicht `.sample`) Hooks im Repository.
    pub active_hooks: Vec<String>,
    /// Bestehende Worktrees außer dem Hauptarbeitsverzeichnis.
    pub linked_worktrees: usize,
}

/// Öffnet das Repository, das `path` enthält (auch aus einem Unterordner).
///
/// # Errors
/// [`GitError::Repo`], wenn dort kein nutzbares Arbeitsverzeichnis liegt.
pub fn inspect(path: &Path) -> Result<RepoInfo, GitError> {
    let repo =
        gix::discover(path).map_err(|e| GitError::Repo(format!("Kein Git-Repository: {e}")))?;
    let root = repo
        .workdir()
        .ok_or_else(|| {
            GitError::Repo("Das Repository hat kein Arbeitsverzeichnis (bare).".to_owned())
        })?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let head = repo.head_id().ok().map(|id| id.to_string());
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string());
    // Nur Filter aus der Konfiguration des Repositorys zählen: IAP ruft Git ohne
    // System- und Nutzerkonfiguration auf, dort eingetragene Filter (etwa Git LFS
    // der Installation) werden also nie ausgeführt.
    let filter_sections = repo
        .config_snapshot()
        .plumbing()
        .sections_by_name("filter")
        .map_or(0, |sections| {
            sections
                .filter(|section| {
                    matches!(
                        section.meta().source,
                        gix::config::Source::Local | gix::config::Source::Worktree
                    )
                })
                .count()
        });
    let active_hooks = hooks_in(&repo.common_dir().join("hooks"));
    let linked_worktrees = repo.worktrees().map(|list| list.len()).unwrap_or(0);
    Ok(RepoInfo {
        root,
        git_dir,
        head,
        branch,
        filter_sections,
        active_hooks,
        linked_worktrees,
    })
}

fn hooks_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.ends_with(".sample"))
        .collect();
    names.sort();
    names
}

/// Ob der Text einer `.gitattributes`-Datei einen externen Filter vorschreibt.
pub fn attributes_use_filter(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .any(|line| {
            line.split_whitespace()
                .skip(1)
                .any(|attr| attr.starts_with("filter="))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_attributes_are_detected() {
        assert!(attributes_use_filter(
            "*.psd filter=lfs diff=lfs merge=lfs -text\n"
        ));
        assert!(!attributes_use_filter("# filter=lfs\n*.txt text eol=lf\n"));
    }

    #[test]
    fn non_repository_is_a_clear_error() {
        let temp = tempfile::TempDir::new().unwrap();
        // Ein Ordner ohne .git; darüber liegt (im Temp-Ordner) kein Repository.
        let result = inspect(temp.path());
        assert!(matches!(result, Err(GitError::Repo(_))));
    }
}
