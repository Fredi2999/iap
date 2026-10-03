use std::{ffi::OsString, path::PathBuf};

/// Enthält nur nicht geheime Startoptionen; Passphrasen sind absichtlich kein Feld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub root: PathBuf,
    pub vault: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub context: Option<u32>,
    pub cli: bool,
    /// Aktiviert die Werkzeugschleife aus pa-core direkt beim Start (Phase 2).
    /// Ohne diese Flag bleibt der Chat rein streaming, wie in Schritt 3.
    pub tools: bool,
    /// Wurzel des Werkzeug-Workspace; wenn nicht gesetzt, benutzt der CLI
    /// `AI\data\workspace` unter `--root`.
    pub workspace: Option<PathBuf>,
}

impl CliArgs {
    /// Parst einen kleinen festen Vertrag, damit Tippfehler nicht still als Defaults laufen.
    pub fn parse<I, S>(arguments: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut arguments = arguments.into_iter().map(Into::into);
        let _program = arguments.next();
        let mut parsed = Self {
            root: PathBuf::from("."),
            vault: None,
            model: None,
            context: None,
            cli: false,
            tools: false,
            workspace: None,
        };
        while let Some(argument) = arguments.next() {
            let flag = argument.to_string_lossy();
            match flag.as_ref() {
                "--root" => parsed.root = required_path(&mut arguments, "--root")?,
                "--vault" => parsed.vault = Some(required_path(&mut arguments, "--vault")?),
                "--model" => parsed.model = Some(required_path(&mut arguments, "--model")?),
                "--context" => {
                    let raw = required_value(&mut arguments, "--context")?;
                    let context = raw
                        .to_string_lossy()
                        .parse::<u32>()
                        .map_err(|_| "--context erwartet eine positive Ganzzahl".to_owned())?;
                    if context == 0 {
                        return Err("--context muss größer als null sein".to_owned());
                    }
                    parsed.context = Some(context);
                }
                "--cli" => parsed.cli = true,
                "--tools" => parsed.tools = true,
                "--workspace" => {
                    parsed.workspace = Some(required_path(&mut arguments, "--workspace")?);
                }
                _ => return Err(format!("unbekanntes Argument `{flag}`")),
            }
        }
        Ok(parsed)
    }
}

fn required_path<I>(arguments: &mut I, flag: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = OsString>,
{
    let value = required_value(arguments, flag)?;
    if value.is_empty() {
        return Err(format!("{flag} erwartet einen nicht leeren Pfad"));
    }
    Ok(PathBuf::from(value))
}

fn required_value<I>(arguments: &mut I, flag: &str) -> Result<OsString, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .filter(|value| !value.to_string_lossy().starts_with("--"))
        .ok_or_else(|| format!("{flag} erwartet einen Wert"))
}
