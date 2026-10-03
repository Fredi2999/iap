//! Worktree-Lebenszyklus für Agent Flow.
//!
//! Ablauf: Projekt prüfen (Vorprüfung) → Startbasis bestimmen (letzter Commit
//! oder Snapshot der aktuellen Änderungen, ohne den Arbeitsbaum anzufassen) →
//! je Kandidat ein sichtbarer Worktree mit eigenem Branch → Änderungen nur dort
//! anwenden und committen → Diff je Kandidat → Übernahme des Gewinners nur nach
//! Konfliktprüfung und mit Sicherung der überschriebenen Dateien.
//!
//! Alles Schreibende läuft über [`GitCli`] (Policy) beziehungsweise über
//! `pa_policy::resolve_absolute_in_scope` für Dateien. Kein Push, kein Stash,
//! keine Änderung am ursprünglichen Index oder Arbeitsbaum.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

use pa_policy::{resolve_absolute_in_scope, PathScope};
use pa_types::agent_flow::{
    AdoptConflict, AdoptPreview, AdoptResult, BaseChoice, BaseInfo, DirtyFile, FileState,
    ProjectLocation, ProjectPreflight,
};

use super::{
    cli::{simplify_path, AuditHook, GitCall, GitCli},
    repo::{self, RepoInfo},
    GitError,
};
use crate::patch::{validate_relative_path, ChangeKind, ResolvedChange};

/// Mindestens so viel freier Platz ist nötig, sonst startet Agent Flow nicht.
pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024;
/// Darunter erscheint eine Warnung.
pub const WARN_FREE_BYTES: u64 = 1024 * 1024 * 1024;

/// Dateinamen, die standardmäßig nicht in einen Snapshot gelangen.
pub fn is_sensitive(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("credentials")
        || name.starts_with("secrets")
        || [
            ".pem",
            ".key",
            ".pfx",
            ".p12",
            ".kdbx",
            ".ppk",
            ".keystore",
            ".jks",
        ]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Ein Projekt im Agent-Flow-Betrieb.
pub struct FlowProject {
    info: RepoInfo,
    cli: GitCli,
    worktrees_root: PathBuf,
    project_scope: PathScope,
    worktrees_scope: PathScope,
    /// Nur bei [`FlowProject::open_for_check`]: Arbeitsordner, der beim Freigeben verschwindet.
    scratch: Option<PathBuf>,
}

impl Drop for FlowProject {
    fn drop(&mut self) {
        if let Some(scratch) = self.scratch.take() {
            let _ = fs::remove_dir_all(scratch);
        }
    }
}

impl FlowProject {
    /// Öffnet das Projekt nur zum Prüfen.
    ///
    /// Warum: Die Vorprüfung darf beim Nutzer nichts anlegen; der sichtbare
    /// Worktree-Ordner entsteht erst beim Start. Der nötige private Git-Ordner
    /// liegt kurz im Temp-Ordner und wird beim Freigeben wieder gelöscht.
    pub fn open_for_check(
        git_program: &Path,
        project_path: &Path,
        audit: Option<AuditHook>,
    ) -> Result<Self, GitError> {
        let info = repo::inspect(project_path)?;
        let name = info
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| GitError::Repo("Repository-Ordner ohne Namen".to_owned()))?;
        let parent = info
            .root
            .parent()
            .ok_or_else(|| GitError::Repo("Repository liegt in der Laufwerkswurzel".to_owned()))?;
        let worktrees_root = parent.join(format!("{name}.iap-worktrees"));
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let scratch =
            std::env::temp_dir().join(format!("iap-check-{}-{unique}", std::process::id()));
        fs::create_dir_all(&scratch)?;
        let build = || -> Result<Self, GitError> {
            let project_scope =
                PathScope::new(&info.root).map_err(|e| GitError::Policy(e.to_string()))?;
            let scratch_scope =
                PathScope::new(&scratch).map_err(|e| GitError::Policy(e.to_string()))?;
            let private = simplify_path(scratch_scope.root());
            let cli = GitCli::new(
                git_program,
                vec![project_scope.clone(), scratch_scope.clone()],
                &private,
                audit,
            )?;
            Ok(Self {
                info: info.clone(),
                cli,
                worktrees_root: worktrees_root.clone(),
                project_scope,
                worktrees_scope: scratch_scope,
                scratch: Some(scratch.clone()),
            })
        };
        build().inspect_err(|_| {
            let _ = fs::remove_dir_all(&scratch);
        })
    }

    /// Öffnet das Projekt und legt (sichtbar) den Worktree-Ordner neben dem
    /// Repository an: `<Repository>.iap-worktrees`.
    pub fn open(
        git_program: &Path,
        project_path: &Path,
        audit: Option<AuditHook>,
    ) -> Result<Self, GitError> {
        let info = repo::inspect(project_path)?;
        let name = info
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| GitError::Repo("Repository-Ordner ohne Namen".to_owned()))?;
        let parent = info
            .root
            .parent()
            .ok_or_else(|| GitError::Repo("Repository liegt in der Laufwerkswurzel".to_owned()))?;
        let worktrees_root = parent.join(format!("{name}.iap-worktrees"));
        fs::create_dir_all(&worktrees_root)?;
        let project_scope =
            PathScope::new(&info.root).map_err(|e| GitError::Policy(e.to_string()))?;
        let worktrees_scope =
            PathScope::new(&worktrees_root).map_err(|e| GitError::Policy(e.to_string()))?;
        // Git bekommt gewöhnliche Laufwerkspfade; die kanonischen `\\?\`-Pfade der Policy
        // sind für Git-Konfigurationsdateien und Arbeitsverzeichnisse unzuverlässig.
        let worktrees_root = simplify_path(worktrees_scope.root());
        let private = worktrees_root.join(".iap-tmp");
        fs::create_dir_all(&private)?;
        let cli = GitCli::new(
            git_program,
            vec![project_scope.clone(), worktrees_scope.clone()],
            &private,
            audit,
        )?;
        Ok(Self {
            info,
            cli,
            worktrees_root,
            project_scope,
            worktrees_scope,
            scratch: None,
        })
    }

    /// Repository-Angaben.
    pub fn info(&self) -> &RepoInfo {
        &self.info
    }

    /// Sichtbarer Ort der Worktrees.
    pub fn worktrees_root(&self) -> &Path {
        &self.worktrees_root
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError> {
        Ok(self.cli.run(GitCall::new(cwd, args))?.text())
    }

    /// Alle versionierten Dateien (relative Pfade, `/` als Trenner).
    pub fn tracked_files(&self) -> Result<Vec<String>, GitError> {
        let out = self
            .cli
            .run(GitCall::new(self.project_scope.root(), &["ls-files", "-z"]))?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Geänderte und neue Dateien des Arbeitsbaums (ohne ignorierte).
    pub fn dirty_files(&self) -> Result<Vec<DirtyFile>, GitError> {
        let out = self.cli.run(GitCall::new(
            self.project_scope.root(),
            &["status", "--porcelain=v1", "-z", "-uall"],
        ))?;
        Ok(parse_status(&out.stdout))
    }

    /// Vorprüfung vor jedem Start. `filesystem` und `free_bytes` liefert der Aufrufer
    /// (Betriebssystemabfrage), `location` ist der gewählte Ort.
    pub fn preflight(
        &self,
        location: ProjectLocation,
        filesystem: &str,
        free_bytes: u64,
    ) -> Result<ProjectPreflight, GitError> {
        let mut blockers = Vec::new();
        let mut warnings = Vec::new();
        if self.info.head.is_none() {
            blockers.push(
                "Das Repository hat noch keinen Commit. Bitte zuerst einen ersten Commit anlegen."
                    .to_owned(),
            );
        }
        if self.info.filter_sections > 0 {
            blockers.push("Das Repository konfiguriert externe Git-Filter (z. B. Git LFS). Beim Anlegen der Worktrees würden sie Programme starten; Agent Flow verweigert das.".to_owned());
        }
        for candidate in [".gitattributes"] {
            if let Ok(text) = fs::read_to_string(self.info.root.join(candidate)) {
                if repo::attributes_use_filter(&text) {
                    blockers.push("`.gitattributes` schreibt externe Filter vor (filter=…). Agent Flow verweigert das.".to_owned());
                }
            }
        }
        if free_bytes < MIN_FREE_BYTES {
            blockers.push(format!(
                "Zu wenig freier Speicher am Zielort ({} MB frei).",
                free_bytes / (1024 * 1024)
            ));
        } else if free_bytes < WARN_FREE_BYTES {
            warnings.push(format!(
                "Wenig freier Speicher am Zielort ({} MB frei).",
                free_bytes / (1024 * 1024)
            ));
        }
        let fs_lower = filesystem.to_ascii_lowercase();
        if fs_lower.contains("fat") {
            warnings.push(format!("Das Dateisystem {filesystem} unterstützt keine Symlinks und keine Dateien über 4 GB; Projekte mit Symlinks sind hier ungeeignet."));
        }
        if !self.info.active_hooks.is_empty() {
            warnings.push(format!(
                "Git-Hooks vorhanden ({}); IAP führt sie nie aus.",
                self.info.active_hooks.join(", ")
            ));
        }
        if self.info.linked_worktrees > 0 {
            warnings.push(format!(
                "Das Repository hat bereits {} weitere Worktrees.",
                self.info.linked_worktrees
            ));
        }
        let dirty_files = self.dirty_files().unwrap_or_default();
        if !dirty_files.is_empty() {
            warnings.push(format!(
                "{} Datei(en) mit uncommitteten Änderungen: die Startbasis ist wählbar.",
                dirty_files.len()
            ));
        }
        Ok(ProjectPreflight {
            repo_root: self.info.root.to_string_lossy().into_owned(),
            location,
            branch: self.info.branch.clone(),
            head: self.info.head.clone(),
            worktrees_root: self.worktrees_root.to_string_lossy().into_owned(),
            filesystem: filesystem.to_owned(),
            free_bytes,
            dirty_files,
            blockers,
            warnings,
        })
    }

    /// Bestimmt die Startbasis. Der Arbeitsbaum und der echte Index bleiben unberührt:
    /// der Snapshot entsteht in einem eigenen, temporären Index.
    pub fn create_base(&self, task_id: &str, choice: &BaseChoice) -> Result<BaseInfo, GitError> {
        let head = self
            .info
            .head
            .clone()
            .ok_or_else(|| GitError::Repo("Kein Commit als Startpunkt vorhanden".to_owned()))?;
        let base_commit = match choice {
            BaseChoice::HeadCommit => head.clone(),
            BaseChoice::Snapshot { include_untracked } => {
                self.snapshot(task_id, &head, include_untracked)?
            }
        };
        Ok(BaseInfo {
            choice: choice.clone(),
            base_commit,
            target_head: head,
        })
    }

    fn snapshot(
        &self,
        task_id: &str,
        head: &str,
        include_untracked: &[String],
    ) -> Result<String, GitError> {
        let root = self.project_scope.root().to_path_buf();
        let index = self
            .worktrees_root
            .join(".iap-tmp")
            .join(format!("index-{task_id}"));
        let index_text = index.to_string_lossy().into_owned();
        let with_index = |args: &[&str]| GitCall {
            cwd: &root,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            declared: vec![index.clone()],
            env: vec![("GIT_INDEX_FILE".to_owned(), index_text.clone())],
            timeout: Duration::from_secs(120),
            cancel: None,
        };
        let run = || -> Result<String, GitError> {
            self.cli.run(with_index(&["read-tree", head]))?;
            // Nur bereits versionierte Dateien (auch gelöschte); Neues nur, was ausdrücklich gewählt wurde.
            self.cli.run(with_index(&["add", "-u"]))?;
            for path in include_untracked {
                let path = validate_relative_path(path).map_err(GitError::Patch)?;
                self.cli.run(with_index(&["add", "--", &path]))?;
            }
            let tree = self.cli.run(with_index(&["write-tree"]))?.text();
            let commit = self
                .cli
                .run(GitCall::new(
                    &root,
                    &[
                        "commit-tree",
                        &tree,
                        "-p",
                        head,
                        "-m",
                        "IAP-Snapshot der aktuellen Änderungen",
                    ],
                ))?
                .text();
            self.cli.run(GitCall::new(
                &root,
                &[
                    "update-ref",
                    &format!("refs/iap/snapshots/{task_id}"),
                    &commit,
                ],
            ))?;
            Ok(commit)
        };
        let result = run();
        let _ = fs::remove_file(&index);
        result
    }

    /// Legt den Worktree eines Kandidaten an und liefert `(Pfad, Branch)`.
    pub fn add_worktree(
        &self,
        task_id: &str,
        index: u32,
        base_commit: &str,
    ) -> Result<(PathBuf, String), GitError> {
        let path = self.worktrees_root.join(task_id).join(format!("k{index}"));
        let branch = format!("iap/{task_id}/k{index}");
        fs::create_dir_all(path.parent().unwrap_or(&self.worktrees_root))?;
        let mut call = GitCall::new(
            self.project_scope.root(),
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &path.to_string_lossy(),
                base_commit,
            ],
        );
        call.declared = vec![path.clone()];
        call.timeout = Duration::from_secs(300);
        self.cli.run(call)?;
        Ok((path, branch))
    }

    /// Wendet geprüfte Änderungen im Worktree an und committet sie dort.
    /// Liefert den neuen Commit. Jeder Pfad wird über `pa-policy` aufgelöst.
    pub fn apply_and_commit(
        &self,
        worktree: &Path,
        changes: &[ResolvedChange],
        message: &str,
        cancel: Option<&AtomicBool>,
    ) -> Result<String, GitError> {
        let scope = PathScope::new(worktree).map_err(|e| GitError::Policy(e.to_string()))?;
        for change in changes {
            let relative = validate_relative_path(&change.path)?;
            let target = resolve_absolute_in_scope(&scope, &scope.root().join(&relative))
                .map_err(|e| GitError::Policy(e.to_string()))?;
            match &change.kind {
                ChangeKind::Write(content) => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let existing = fs::read(&target).ok();
                    fs::write(&target, in_original_style(existing.as_deref(), content))?;
                }
                ChangeKind::Delete => {
                    if target.is_file() {
                        fs::remove_file(&target)?;
                    }
                }
            }
        }
        let mut add = GitCall::new(worktree, &["add", "-A"]);
        add.cancel = cancel;
        self.cli.run(add)?;
        let status = self.git(worktree, &["status", "--porcelain=v1"])?;
        if status.trim().is_empty() {
            return Err(GitError::Repo(
                "Die Änderung hat nichts verändert.".to_owned(),
            ));
        }
        let mut commit = GitCall::new(worktree, &["commit", "-m", message]);
        commit.cancel = cancel;
        self.cli.run(commit)?;
        self.git(worktree, &["rev-parse", "HEAD"])
    }

    /// Vollständiger Diff Basis → Kandidat.
    pub fn diff(&self, base: &str, branch: &str) -> Result<String, GitError> {
        let out = self.cli.run(GitCall::new(
            self.project_scope.root(),
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                base,
                branch,
            ],
        ))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Geänderte Dateien Basis → Kandidat als `(Status, Pfad)`; Status `M`, `A` oder `D`.
    pub fn changed_files(&self, base: &str, branch: &str) -> Result<Vec<(char, String)>, GitError> {
        let out = self.cli.run(GitCall::new(
            self.project_scope.root(),
            &["diff", "--name-status", "-z", "--no-renames", base, branch],
        ))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut parts = text.split('\0').filter(|p| !p.is_empty());
        let mut files = Vec::new();
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            files.push((status.chars().next().unwrap_or('M'), path.to_owned()));
        }
        Ok(files)
    }

    fn blob(&self, commit: &str, path: &str) -> Result<Vec<u8>, GitError> {
        Ok(self
            .cli
            .run(GitCall::new(
                self.project_scope.root(),
                &["cat-file", "blob", &format!("{commit}:{path}")],
            ))?
            .stdout)
    }

    fn head_now(&self) -> Result<String, GitError> {
        self.git(self.project_scope.root(), &["rev-parse", "HEAD"])
    }

    /// Prüft die Übernahme, ohne etwas zu schreiben.
    pub fn adopt_preview(
        &self,
        candidate_id: &str,
        base: &BaseInfo,
        branch: &str,
    ) -> Result<AdoptPreview, GitError> {
        let mut conflicts = Vec::new();
        if self.head_now()? != base.target_head {
            conflicts.push(AdoptConflict {
                path: "(Projekt)".to_owned(),
                reason: "Der letzte Commit des Projekts hat sich seit dem Start geändert."
                    .to_owned(),
            });
        }
        let files = self.changed_files(&base.base_commit, branch)?;
        for (status, path) in &files {
            let relative = validate_relative_path(path)?;
            let target = resolve_absolute_in_scope(
                &self.project_scope,
                &self.project_scope.root().join(&relative),
            )
            .map_err(|e| GitError::Policy(e.to_string()))?;
            let current = fs::read(&target).ok();
            let reason = match status {
                'A' => current
                    .as_ref()
                    .map(|_| "Die Datei gibt es im Projekt inzwischen schon.".to_owned()),
                _ => {
                    let original = self.blob(&base.base_commit, &relative)?;
                    match current {
                        None => Some("Die Datei wurde seit dem Start gelöscht.".to_owned()),
                        Some(now) if normalize_eol(&now) != normalize_eol(&original) => {
                            Some("Die Datei wurde seit dem Start verändert.".to_owned())
                        }
                        _ => None,
                    }
                }
            };
            if let Some(reason) = reason {
                conflicts.push(AdoptConflict {
                    path: relative,
                    reason,
                });
            }
        }
        Ok(AdoptPreview {
            candidate_id: candidate_id.to_owned(),
            diff: self.diff(&base.base_commit, branch)?,
            files: files.into_iter().map(|(_, p)| p).collect(),
            conflicts,
        })
    }

    /// Schreibt den Kandidaten ins Zielprojekt. Bei jedem Konflikt wird **nichts**
    /// geschrieben. Überschriebene Dateien werden vorher gesichert; scheitert ein
    /// Schritt, stellt die Rücknahme den Ausgangszustand wieder her.
    pub fn adopt(
        &self,
        task_id: &str,
        candidate_id: &str,
        base: &BaseInfo,
        branch: &str,
    ) -> Result<AdoptResult, GitError> {
        let preview = self.adopt_preview(candidate_id, base, branch)?;
        if !preview.conflicts.is_empty() {
            return Err(GitError::Repo(format!(
                "Übernahme gestoppt: {} Konflikt(e). Es wurde nichts geschrieben.",
                preview.conflicts.len()
            )));
        }
        let backup_root = self.worktrees_root.join(".iap-backup").join(task_id);
        let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        let files = self.changed_files(&base.base_commit, branch)?;
        let apply = |done: &mut Vec<(PathBuf, Option<PathBuf>)>| -> Result<Vec<String>, GitError> {
            let mut written = Vec::new();
            for (status, path) in &files {
                let relative = validate_relative_path(path)?;
                let target = resolve_absolute_in_scope(
                    &self.project_scope,
                    &self.project_scope.root().join(&relative),
                )
                .map_err(|e| GitError::Policy(e.to_string()))?;
                let existing = fs::read(&target).ok();
                let backup = match &existing {
                    Some(bytes) => {
                        let backup = backup_root.join(&relative);
                        if let Some(parent) = backup.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&backup, bytes)?;
                        Some(backup)
                    }
                    None => None,
                };
                done.push((target.clone(), backup));
                if *status == 'D' {
                    if target.is_file() {
                        fs::remove_file(&target)?;
                    }
                } else {
                    let content = self.blob(branch, &relative)?;
                    let content = match &existing {
                        Some(old) if old.windows(2).any(|w| w == b"\r\n") => to_crlf(&content),
                        _ => content,
                    };
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&target, content)?;
                }
                written.push(relative);
            }
            Ok(written)
        };
        match apply(&mut done) {
            Ok(written) => Ok(AdoptResult {
                written_files: written,
                note: "Die Dateien liegen im Projekt. Es wurde nichts committet und nichts hochgeladen; Sicherungen der überschriebenen Dateien liegen im Worktree-Ordner.".to_owned(),
            }),
            Err(error) => {
                for (target, backup) in done.iter().rev() {
                    match backup {
                        Some(backup) => {
                            let _ = fs::copy(backup, target);
                        }
                        None => {
                            let _ = fs::remove_file(target);
                        }
                    }
                }
                Err(error)
            }
        }
    }

    /// Entfernt Worktree, Branch und Snapshot-Verweis eines Kandidaten (bewusste Bereinigung).
    pub fn remove_candidate(
        &self,
        task_id: &str,
        worktree: &Path,
        branch: &str,
    ) -> Result<(), GitError> {
        let mut remove = GitCall::new(
            self.project_scope.root(),
            &["worktree", "remove", "--force", &worktree.to_string_lossy()],
        );
        remove.declared = vec![worktree.to_path_buf()];
        self.cli.run(remove)?;
        self.git(self.project_scope.root(), &["branch", "-D", branch])?;
        let _ = self.git(
            self.project_scope.root(),
            &["update-ref", "-d", &format!("refs/iap/snapshots/{task_id}")],
        );
        Ok(())
    }

    /// Räumt verwaiste Worktree-Verwaltungsdaten auf (z. B. nach Laufwerkswechsel).
    pub fn repair(&self) -> Result<(), GitError> {
        self.git(self.project_scope.root(), &["worktree", "prune"])?;
        Ok(())
    }

    /// Der Scope des Worktree-Ordners (für Aufrufer, die Dateien dort lesen).
    pub fn worktrees_scope(&self) -> &PathScope {
        &self.worktrees_scope
    }
}

/// Schreibt neuen Dateiinhalt im Stil der bestehenden Datei.
///
/// Warum: Modelle liefern Dateien mit `\n`, ohne BOM und oft ohne letzten
/// Zeilenumbruch. Ohne Angleichung entstünde in jedem Diff eine Änderung an
/// jeder Zeile (CRLF → LF) oder am ersten Zeichen (BOM), die niemand wollte.
/// Neue Dateien bleiben unverändert (LF, kein BOM).
pub fn in_original_style(existing: Option<&[u8]>, content: &str) -> Vec<u8> {
    let Some(old) = existing else {
        return content.as_bytes().to_vec();
    };
    let has_bom = old.starts_with(&[0xEF, 0xBB, 0xBF]);
    let crlf_count = old.windows(2).filter(|w| w == b"\r\n").count();
    let lf_count = old.iter().filter(|&&b| b == b'\n').count();
    let crlf = crlf_count > 0 && crlf_count * 2 > lf_count;
    let ends_with_newline = old.last() == Some(&b'\n');

    let mut text = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if ends_with_newline && !text.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    let mut bytes = Vec::with_capacity(text.len() + 8);
    if has_bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    if crlf {
        bytes.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    } else {
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

fn normalize_eol(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn to_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 20);
    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            out.push(b'\r');
        }
        out.push(byte);
    }
    out
}

/// Liest die NUL-getrennte Ausgabe von `git status --porcelain=v1 -z`.
pub fn parse_status(raw: &[u8]) -> Vec<DirtyFile> {
    let text = String::from_utf8_lossy(raw).into_owned();
    let mut entries = text.split('\0').filter(|e| !e.is_empty());
    let mut out = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let code = &entry[..2];
        let path = entry[3..].to_owned();
        if code.starts_with('R') || code.starts_with('C') {
            // Bei Umbenennung folgt der alte Pfad als eigener Eintrag.
            let _ = entries.next();
        }
        let state = match code {
            "??" => FileState::Untracked,
            "!!" => FileState::Ignored,
            c if c.contains('D') => FileState::Deleted,
            c if c.contains('A') => FileState::Added,
            _ => FileState::Modified,
        };
        let sensitive = is_sensitive(&path);
        out.push(DirtyFile {
            path,
            state,
            sensitive,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewritten_files_keep_bom_line_endings_and_final_newline() {
        let old = b"\xEF\xBB\xBFdef a():\r\n    pass\r\n";
        let new = in_original_style(Some(old), "def a():\n    return 1");
        assert_eq!(new, b"\xEF\xBB\xBFdef a():\r\n    return 1\r\n");
        // Ein vom Modell mitgeliefertes BOM wird nicht verdoppelt.
        let twice = in_original_style(Some(old), "\u{feff}def a():\n    pass\n");
        assert_eq!(twice, old);
    }

    #[test]
    fn lf_files_stay_lf_and_new_files_are_untouched() {
        let old = b"a\nb\n";
        assert_eq!(in_original_style(Some(old), "a\r\nc\r\n"), b"a\nc\n");
        assert_eq!(in_original_style(None, "neu\n"), b"neu\n");
        // Ohne letzten Zeilenumbruch im Original bleibt das so.
        assert_eq!(in_original_style(Some(b"x"), "y\n"), b"y\n");
        assert_eq!(in_original_style(Some(b"x"), "y"), b"y");
    }

    #[test]
    fn sensitive_names_are_recognized() {
        for name in [
            ".env",
            "config/.env.local",
            "keys/server.pem",
            "a/id_rsa",
            "secrets.yaml",
            "x.PFX",
        ] {
            assert!(is_sensitive(name), "{name}");
        }
        for name in ["src/main.rs", "environment.md", "README.md"] {
            assert!(!is_sensitive(name), "{name}");
        }
    }

    #[test]
    fn porcelain_output_is_parsed_including_renames() {
        let raw = b" M src/lib.rs\0?? neu.txt\0?? .env\0 D alt.txt\0R  b.txt\0a.txt\0A  x.rs\0";
        let files = parse_status(raw);
        assert_eq!(files.len(), 6);
        assert_eq!(files[0].state, FileState::Modified);
        assert_eq!(files[1].state, FileState::Untracked);
        assert!(files[2].sensitive);
        assert_eq!(files[3].state, FileState::Deleted);
        assert_eq!(files[4].path, "b.txt");
        assert_eq!(files[5].state, FileState::Added);
    }

    #[test]
    fn line_ending_helpers_round_trip() {
        assert_eq!(normalize_eol(b"a\r\nb\n"), b"a\nb\n");
        assert_eq!(to_crlf(b"a\nb\n"), b"a\r\nb\r\n");
        assert_eq!(to_crlf(b"a\r\nb\n"), b"a\r\nb\r\n");
    }
}
