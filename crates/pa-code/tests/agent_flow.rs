//! Ende-zu-Ende-Prüfung des Worktree-Betriebs mit dem echten MinGit.
//!
//! Die Tests laufen nur mit `IAP_GIT=<Pfad zu …\git\cmd\git.exe>` und
//! `cargo test -- --ignored`, weil das Git-Paket nicht im Repository liegt. Test-
//! Repositories werden mit demselben Git direkt angelegt (nur in den Tests); der
//! Produktpfad läuft ausschließlich über `GitCli` und die Policy.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use pa_code::{
    git::flow::FlowProject,
    patch::{ChangeKind, ChangeProposal},
};
use pa_types::agent_flow::{BaseChoice, ProjectLocation};

fn git_exe() -> PathBuf {
    PathBuf::from(std::env::var("IAP_GIT").expect("IAP_GIT auf …\\git\\cmd\\git.exe setzen"))
}

fn raw_git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new(git_exe())
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=t@example.org",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git startet");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Legt ein kleines Repository an: zwei Dateien, ein Commit.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::TempDir::with_prefix("iap-flow").unwrap();
    let repo = temp.path().join("projekt");
    fs::create_dir_all(repo.join("src")).unwrap();
    raw_git(&repo, &["init", "-q"]);
    fs::write(repo.join("src").join("lib.txt"), "eins\nzwei\ndrei\n").unwrap();
    fs::write(repo.join("README.md"), "# Projekt\n").unwrap();
    raw_git(&repo, &["add", "-A"]);
    raw_git(&repo, &["commit", "-q", "-m", "Start"]);
    (temp, repo)
}

fn proposal(text: &str, repo: &Path) -> Vec<pa_code::patch::ResolvedChange> {
    ChangeProposal::parse(text)
        .unwrap()
        .resolve(|p| fs::read_to_string(repo.join(p)).ok())
        .unwrap()
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn candidates_get_own_worktrees_and_the_original_stays_untouched() {
    let (_temp, repo) = fixture();
    let before = raw_git(&repo, &["status", "--porcelain=v1"]);
    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let preflight = project
        .preflight(ProjectLocation::Host, "NTFS", 50 * 1024 * 1024 * 1024)
        .unwrap();
    assert!(preflight.blockers.is_empty(), "{:?}", preflight.blockers);
    assert!(preflight.worktrees_root.ends_with("projekt.iap-worktrees"));

    let base = project
        .create_base("t-eins", &BaseChoice::HeadCommit)
        .unwrap();
    let (path1, branch1) = project
        .add_worktree("t-eins", 1, &base.base_commit)
        .unwrap();
    let (path2, branch2) = project
        .add_worktree("t-eins", 2, &base.base_commit)
        .unwrap();
    assert_ne!(path1, path2);
    assert_eq!(branch1, "iap/t-eins/k1");

    let changes = proposal("### Datei: src/lib.txt\n```\neins\nZWEI\ndrei\n```", &path1);
    project
        .apply_and_commit(&path1, &changes, "Kandidat 1", None)
        .unwrap();
    let other = proposal(
        "### Datei: README.md\n```\n# Anders\n```\n### Datei: neu.txt\n```\nhallo\n```",
        &path2,
    );
    project
        .apply_and_commit(&path2, &other, "Kandidat 2", None)
        .unwrap();

    let diff1 = project.diff(&base.base_commit, &branch1).unwrap();
    assert!(
        diff1.contains("-zwei") && diff1.contains("+ZWEI"),
        "{diff1}"
    );
    assert_eq!(
        project
            .changed_files(&base.base_commit, &branch2)
            .unwrap()
            .len(),
        2
    );

    // Das Ausgangsprojekt ist unverändert: gleicher Status, gleiche Datei.
    assert_eq!(raw_git(&repo, &["status", "--porcelain=v1"]), before);
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.txt")).unwrap(),
        "eins\nzwei\ndrei\n"
    );
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn snapshot_base_carries_selected_changes_without_touching_index_or_worktree() {
    let (_temp, repo) = fixture();
    fs::write(repo.join("src").join("lib.txt"), "eins\nzwei\nvier\n").unwrap();
    fs::write(repo.join("neu.txt"), "neue Datei\n").unwrap();
    fs::write(repo.join(".env"), "GEHEIM=1\n").unwrap();
    let status_before = raw_git(&repo, &["status", "--porcelain=v1"]);

    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let dirty = project.dirty_files().unwrap();
    assert!(dirty.iter().any(|f| f.path == ".env" && f.sensitive));

    let base = project
        .create_base(
            "t-snap",
            &BaseChoice::Snapshot {
                include_untracked: vec!["neu.txt".to_owned()],
            },
        )
        .unwrap();
    assert_ne!(base.base_commit, base.target_head);
    let snapshot_files = raw_git(&repo, &["ls-tree", "-r", "--name-only", &base.base_commit]);
    assert!(snapshot_files.contains("neu.txt"));
    assert!(
        !snapshot_files.contains(".env"),
        "sensible Datei darf nicht unbemerkt hinein"
    );
    assert_eq!(
        raw_git(
            &repo,
            &["show", &format!("{}:src/lib.txt", base.base_commit)]
        ),
        "eins\nzwei\nvier"
    );

    // Der Arbeitsbaum und der echte Index sind exakt wie vorher.
    assert_eq!(raw_git(&repo, &["status", "--porcelain=v1"]), status_before);
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.txt")).unwrap(),
        "eins\nzwei\nvier\n"
    );
    assert!(repo.join(".env").exists());
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn adopt_stops_on_conflict_writes_nothing_and_backs_up_on_success() {
    let (_temp, repo) = fixture();
    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let base = project
        .create_base("t-adopt", &BaseChoice::HeadCommit)
        .unwrap();
    let (path, branch) = project
        .add_worktree("t-adopt", 1, &base.base_commit)
        .unwrap();
    let changes = proposal(
        "### Datei: src/lib.txt\n```\neins\nZWEI\ndrei\n```\n### Datei: neu.txt\n```\nhallo\n```",
        &path,
    );
    project
        .apply_and_commit(&path, &changes, "Kandidat", None)
        .unwrap();

    // Zwischenzeitliche Änderung am Zielprojekt -> Konflikt, nichts wird geschrieben.
    fs::write(repo.join("src").join("lib.txt"), "eins\nanders\ndrei\n").unwrap();
    let preview = project.adopt_preview("c1", &base, &branch).unwrap();
    assert!(
        preview.conflicts.iter().any(|c| c.path == "src/lib.txt"),
        "{:?}",
        preview.conflicts
    );
    assert!(project.adopt("t-adopt", "c1", &base, &branch).is_err());
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.txt")).unwrap(),
        "eins\nanders\ndrei\n"
    );
    assert!(!repo.join("neu.txt").exists());

    // Ursprungszustand wiederherstellen -> Übernahme gelingt, Sicherung liegt vor.
    fs::write(repo.join("src").join("lib.txt"), "eins\nzwei\ndrei\n").unwrap();
    let result = project.adopt("t-adopt", "c1", &base, &branch).unwrap();
    assert_eq!(result.written_files.len(), 2);
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.txt")).unwrap(),
        "eins\nZWEI\ndrei\n"
    );
    assert_eq!(fs::read_to_string(repo.join("neu.txt")).unwrap(), "hallo\n");
    let backup = project
        .worktrees_root()
        .join(".iap-backup")
        .join("t-adopt")
        .join("src")
        .join("lib.txt");
    assert_eq!(fs::read_to_string(backup).unwrap(), "eins\nzwei\ndrei\n");
    // Nichts wurde committet.
    assert_eq!(raw_git(&repo, &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn head_change_and_filters_block_start_or_adoption() {
    let (_temp, repo) = fixture();
    fs::write(repo.join(".gitattributes"), "*.bin filter=lfs -text\n").unwrap();
    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let preflight = project
        .preflight(ProjectLocation::Stick, "exFAT", 100 * 1024 * 1024)
        .unwrap();
    assert!(
        preflight.blockers.iter().any(|b| b.contains("Filter")),
        "{:?}",
        preflight.blockers
    );
    assert!(
        preflight.blockers.iter().any(|b| b.contains("Speicher")),
        "{:?}",
        preflight.blockers
    );
    assert!(preflight.warnings.iter().any(|w| w.contains("exFAT")));
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn candidate_cannot_write_outside_or_into_git_dir() {
    let (_temp, repo) = fixture();
    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let base = project
        .create_base("t-pfad", &BaseChoice::HeadCommit)
        .unwrap();
    let (path, _branch) = project
        .add_worktree("t-pfad", 1, &base.base_commit)
        .unwrap();
    let evil = vec![pa_code::patch::ResolvedChange {
        path: "../ausbruch.txt".to_owned(),
        kind: ChangeKind::Write("x".to_owned()),
    }];
    assert!(project
        .apply_and_commit(&path, &evil, "böse", None)
        .is_err());
    assert!(!path.parent().unwrap().join("ausbruch.txt").exists());
    let git_dir = vec![pa_code::patch::ResolvedChange {
        path: ".git/hooks/pre-commit".to_owned(),
        kind: ChangeKind::Write("#!/bin/sh\n".to_owned()),
    }];
    assert!(project
        .apply_and_commit(&path, &git_dir, "böse", None)
        .is_err());
}

#[test]
#[ignore = "braucht MinGit (IAP_GIT)"]
fn cleanup_removes_worktree_and_branch() {
    let (_temp, repo) = fixture();
    let project = FlowProject::open(&git_exe(), &repo, None).unwrap();
    let base = project
        .create_base("t-weg", &BaseChoice::HeadCommit)
        .unwrap();
    let (path, branch) = project.add_worktree("t-weg", 1, &base.base_commit).unwrap();
    assert!(path.exists());
    project.remove_candidate("t-weg", &path, &branch).unwrap();
    assert!(!path.exists());
    assert!(!raw_git(&repo, &["branch", "--list", "iap/*"]).contains("iap/"));
}

#[test]
#[ignore = "braucht IAP_GIT"]
fn the_pre_check_leaves_nothing_behind_in_the_users_folders() {
    let (temp, repo) = fixture();
    let before: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    {
        let project = FlowProject::open_for_check(&git_exe(), &repo, None).expect("Prüfung");
        let report = project
            .preflight(ProjectLocation::Host, "NTFS", 10 * 1024 * 1024 * 1024)
            .expect("Vorprüfung");
        assert!(report.blockers.is_empty(), "{:?}", report.blockers);
    }
    let after: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        before, after,
        "keine Worktree-Ordner bei der bloßen Prüfung"
    );
    assert!(!repo.join(".git").join("worktrees").exists());
}

#[test]
#[ignore = "braucht IAP_GIT"]
fn candidate_files_keep_bom_and_crlf_of_the_original() {
    let (temp, repo) = fixture();
    fs::write(
        repo.join("crlf.txt"),
        b"\xEF\xBB\xBFzeile eins\r\nzeile zwei\r\n",
    )
    .unwrap();
    raw_git(&repo, &["add", "-A"]);
    raw_git(&repo, &["commit", "-q", "-m", "CRLF"]);
    let project = FlowProject::open(&git_exe(), &repo, None).expect("öffnen");
    let base = project
        .create_base("t-style", &BaseChoice::HeadCommit)
        .expect("Basis");
    let (worktree, branch) = project
        .add_worktree("t-style", 1, &base.base_commit)
        .expect("Worktree");
    let changes = proposal(
        "### Datei: crlf.txt\n```\nzeile eins\nzeile ZWEI\n```\n",
        &worktree,
    );
    project
        .apply_and_commit(&worktree, &changes, "Stil", None)
        .expect("anwenden");
    let diff = project.diff(&base.base_commit, &branch).expect("diff");
    // Nur die eine Zeile ändert sich; BOM und Zeilenenden des Originals bleiben erhalten.
    assert_eq!(
        diff.matches("\n-zeile").count() + diff.matches("\n+zeile").count(),
        2,
        "{diff}"
    );
    let bytes = fs::read(worktree.join("crlf.txt")).unwrap();
    assert!(bytes.starts_with(b"\xEF\xBB\xBF") && bytes.windows(2).any(|w| w == b"\r\n"));
    drop(temp);
}
