use std::path::PathBuf;

use pa_launcher::cli::CliArgs;

#[test]
fn parses_the_documented_cli_contract_without_accepting_a_passphrase() {
    let parsed = CliArgs::parse([
        "pa-launcher",
        "--root",
        "D:\\PortableAI",
        "--vault",
        "D:\\private\\vault.db",
        "--model",
        "D:\\models\\gemma.gguf",
        "--context",
        "4096",
        "--cli",
    ])
    .expect("documented arguments");

    assert_eq!(parsed.root, PathBuf::from("D:\\PortableAI"));
    assert_eq!(parsed.vault, Some(PathBuf::from("D:\\private\\vault.db")));
    assert_eq!(parsed.model, Some(PathBuf::from("D:\\models\\gemma.gguf")));
    assert_eq!(parsed.context, Some(4096));
    assert!(parsed.cli);

    let error = CliArgs::parse(["pa-launcher", "--passphrase", "secret"])
        .expect_err("secrets must never be command-line arguments");
    assert!(error.contains("unbekanntes Argument"));
    assert!(!error.contains("secret"));
}

#[test]
fn rejects_missing_values_invalid_context_and_unknown_arguments() {
    assert!(CliArgs::parse(["pa-launcher", "--root"]).is_err());
    assert!(CliArgs::parse(["pa-launcher", "--context", "zero"]).is_err());
    assert!(CliArgs::parse(["pa-launcher", "--network"]).is_err());
}

#[test]
fn tools_and_workspace_are_accepted() {
    let parsed = CliArgs::parse([
        "pa-launcher",
        "--cli",
        "--root",
        ".",
        "--tools",
        "--workspace",
        "D:\\PortableAI\\workspace",
    ])
    .expect("tools flag");
    assert!(parsed.tools);
    assert_eq!(
        parsed.workspace,
        Some(PathBuf::from("D:\\PortableAI\\workspace"))
    );
    assert!(!CliArgs::parse(["pa-launcher", "--workspace"])
        .unwrap_err()
        .is_empty());
}
