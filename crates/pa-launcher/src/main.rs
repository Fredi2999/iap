use std::{
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pa_core::{
    budget::BudgetPolicy,
    conversation::Conversations,
    engine::{ChatEngine, EngineReady},
    orchestrator::{ChatOrchestrator, TurnCallbacks, TurnError, TurnRequest},
    prompt::PromptPrefix,
    tool_loop::{ToolEvent, ToolLoopError},
};
use pa_inference::{config::ServerConfig, loopback::LoopbackEndpoint, supervisor::Supervisor};
use pa_launcher::{
    bootstrap::{prepare, BootstrapContext, BootstrapOptions},
    cache::{CacheResult, CopyObserver},
    cli::CliArgs,
    inference_backend::LlamaServerEngine,
    runtime::{SharedConversations, VaultRuntime},
    tool_runtime::ToolRuntime,
    LauncherError,
};
use pa_policy::Mode;
use pa_types::{
    chat::{MessageRole, MessageStatus, StreamOutcomeDto},
    model::{KvQuantization, ModelDescriptor, ResourcePlan},
};
use pa_vault::{
    hot_copy::{HotVault, RecoveryMode},
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    VaultError,
};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const READY_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    if let Err(error) = run() {
        eprintln!("Fehler: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = CliArgs::parse(std::env::args_os()).map_err(invalid_input)?;
    if !arguments.cli {
        return Err(invalid_input("in Schritt 3 muss --cli angegeben werden").into());
    }

    println!("[1/6] Paketintegrität wird geprüft ...");
    println!("[2/6] Hardware wird gemessen ...");
    println!("[3/6] Modell-Cache wird geprüft ...");
    let mut progress = ConsoleCopyProgress::default();
    let context = prepare(
        &arguments.root,
        BootstrapOptions {
            tier_override: None,
            context_override: arguments.context,
            kv_quantization: KvQuantization::F16,
        },
        &mut progress,
    )?;
    println!(
        "      {} Dateien / {:.2} GiB verifiziert (Manifest {}).",
        context.manifest_summary.verified_files,
        gib(context.manifest_summary.verified_bytes),
        context.manifest_summary.version
    );
    print_hardware_and_plan(&context.hardware, &context.plan);
    match &context.cache_result {
        CacheResult::Copied(path) => println!("      Auf SSD kopiert: {}", path.display()),
        CacheResult::Reused(path) => println!(
            "      Verifizierte SSD-Kopie wiederverwendet: {}",
            path.display()
        ),
    }

    if let Some(override_path) = arguments.model.as_deref() {
        verify_model_override(&context, override_path)?;
    }

    let BootstrapContext {
        package,
        descriptor,
        plan,
        host_identifier,
        cached_model_path,
        server_executable,
        ..
    } = context;

    println!("[4/6] Verschlüsselter Vault wird geöffnet ...");
    let portable_vault = arguments
        .vault
        .unwrap_or_else(|| package.as_path().join("AI").join("data").join("vault.db"));
    let meta_path = portable_vault.with_extension("meta");
    let meta = load_or_create_meta(&portable_vault, &meta_path)?;
    let passphrase = prompt_passphrase(!portable_vault.exists())?;
    let key = derive_key(&passphrase, &meta)?;
    let hot_directory = hot_vault_directory(&portable_vault, &host_identifier)?;
    let first_attempt = HotVault::start(&portable_vault, &hot_directory, key);
    let vault_result = match first_attempt {
        Err(VaultError::RecoveryChoiceRequired {
            portable_generation,
            host_generation,
        }) => {
            println!(
                "      Nach einem harten Ende weicht die Hostkopie vom Stick ab (Host {host_generation}, Stick {portable_generation})."
            );
            print!("      Hostdaten wiederherstellen? [j/N]: ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let recovery_mode = if matches!(answer.trim().to_lowercase().as_str(), "j" | "ja") {
                RecoveryMode::UseNewestHost
            } else {
                RecoveryMode::UsePortable
            };
            let retry_key = derive_key(&passphrase, &meta)?;
            HotVault::start_with_recovery(&portable_vault, &hot_directory, retry_key, recovery_mode)
        }
        result => result,
    };
    let vault = vault_result?;
    println!(
        "      SQLCipher-Integrität bestätigt; Hot-Copy: {}",
        vault.hot_path().display()
    );

    println!("[5/6] Lokales Modell wird geladen ...");
    let (config, endpoint) =
        server_configuration(server_executable, cached_model_path, &descriptor, &plan)?;
    let mut supervisor = Supervisor::new(config.clone());
    let load_started = Instant::now();
    supervisor.start(READY_TIMEOUT)?;
    println!(
        "      Bereit nach {:.3} s.",
        load_started.elapsed().as_secs_f64()
    );
    let adapter =
        pa_inference::adapter::AdapterKind::from_family(&descriptor.family, &descriptor.id);
    let mut engine =
        LlamaServerEngine::from_running(supervisor, endpoint, adapter, config, READY_TIMEOUT);

    let vault_runtime = VaultRuntime::start(vault, |error| {
        eprintln!("Vault-Rücksync ausstehend: {error}. Die intakte Hostkopie bleibt erhalten.");
    });

    let mut tool_runtime = if arguments.tools {
        let workspace_dir = arguments
            .workspace
            .clone()
            .unwrap_or_else(|| package.as_path().join("AI").join("data").join("workspace"));
        println!(
            "[6/6] CLI-Chat mit Werkzeugen bereit. Workspace: {}, Audit-Log im Vault.",
            workspace_dir.display()
        );
        let vault_shared = vault_runtime.shared()?;
        let sink: Box<dyn pa_policy::AuditSink + Send> =
            Box::new(pa_launcher::vault_audit::VaultAuditSink::new(vault_shared));
        Some(
            ToolRuntime::with_sink(&workspace_dir, sink).map_err(|error| -> Box<dyn Error> {
                Box::new(invalid_input(format!(
                    "Werkzeug-Runtime konnte nicht gestartet werden: {error}"
                )))
            })?,
        )
    } else {
        println!("[6/6] CLI-Chat bereit. /hilfe zeigt Befehle; Ctrl+C bricht eine Antwort ab.");
        None
    };

    let chat_result = interactive_chat(
        &vault_runtime,
        &mut engine,
        &descriptor,
        tool_runtime.as_mut(),
    );

    if let Some(runtime) = tool_runtime.as_mut() {
        runtime.end_session();
    }
    let stop_result = engine.stop();
    let vault_result = vault_runtime.shutdown();
    stop_result?;
    vault_result?;
    chat_result
}

fn load_or_create_meta(vault: &Path, meta_path: &Path) -> Result<VaultMeta, Box<dyn Error>> {
    if meta_path.exists() {
        return Ok(VaultMeta::load(meta_path)?);
    }
    if vault.exists() {
        return Err(invalid_input("zu einem vorhandenen Vault fehlt die vault.meta-Datei").into());
    }
    let meta = VaultMeta::random(Argon2Parameters::default());
    meta.save_atomic(meta_path)?;
    Ok(meta)
}

fn prompt_passphrase(confirm: bool) -> Result<Zeroizing<String>, Box<dyn Error>> {
    let passphrase = Zeroizing::new(rpassword::prompt_password("Vault-Passphrase: ")?);
    if passphrase.is_empty() {
        return Err(invalid_input("die Vault-Passphrase darf nicht leer sein").into());
    }
    if confirm {
        let repeated = Zeroizing::new(rpassword::prompt_password(
            "Vault-Passphrase wiederholen: ",
        )?);
        let equal = repeated == passphrase;
        if !equal {
            return Err(invalid_input("die Passphrasen stimmen nicht überein").into());
        }
    }
    Ok(passphrase)
}

fn hot_vault_directory(vault: &Path, host_identifier: &str) -> io::Result<PathBuf> {
    let absolute = if vault.is_absolute() {
        vault.to_path_buf()
    } else {
        std::env::current_dir()?.join(vault)
    };
    let vault_id = hex::encode(Sha256::digest(absolute.to_string_lossy().as_bytes()));
    let host_prefix = &host_identifier[..16.min(host_identifier.len())];
    let vault_prefix = &vault_id[..16.min(vault_id.len())];
    let new_dir = std::env::temp_dir()
        .join("IAP")
        .join("vault-hot")
        .join(format!("{host_prefix}_{vault_prefix}"));

    let legacy_dir = std::env::temp_dir()
        .join("IAP")
        .join("vault-hot")
        .join(host_identifier)
        .join(&vault_id);
    if legacy_dir.exists() && !new_dir.exists() {
        let _ = std::fs::create_dir_all(&new_dir);
        let legacy_hot = legacy_dir.join("hot.db");
        if legacy_hot.exists() {
            let _ = std::fs::copy(&legacy_hot, new_dir.join("hot.db"));
        }
        let _ = std::fs::remove_dir_all(&legacy_dir);
    }

    Ok(new_dir)
}

fn server_configuration(
    executable: PathBuf,
    model: PathBuf,
    descriptor: &ModelDescriptor,
    plan: &ResourcePlan,
) -> Result<(ServerConfig, LoopbackEndpoint), Box<dyn Error>> {
    let (port, token) = ServerConfig::random_endpoint()?;
    let config = ServerConfig {
        executable,
        model,
        model_alias: descriptor.id.clone(),
        port,
        api_key: token.clone(),
        context_tokens: plan.context_tokens,
        threads: plan.threads,
        // Der mit Phase 0 gepinnte Build enthält nur CPU-Backends.
        gpu_layers: 0,
        kv_quantization: plan.kv_quantization,
        mmproj: None,
    };
    let endpoint = LoopbackEndpoint::new(port, token)?;
    Ok((config, endpoint))
}

fn interactive_chat(
    vault: &VaultRuntime,
    engine: &mut LlamaServerEngine,
    descriptor: &ModelDescriptor,
    mut tool_runtime: Option<&mut ToolRuntime>,
) -> Result<(), Box<dyn Error>> {
    let interrupts = Arc::new(AtomicUsize::new(0));
    let handler_counter = Arc::clone(&interrupts);
    ctrlc::set_handler(move || {
        handler_counter.fetch_add(1, Ordering::SeqCst);
    })?;

    let conversation_id = random_id("conversation");
    let mut created = false;
    let policy = BudgetPolicy::new(engine.config().context_tokens);
    let orchestrator = ChatOrchestrator::new(PromptPrefix::empty(), policy);
    // Werkzeug-Toggle spiegelt den Startparameter; kann interaktiv umgeschaltet werden.
    let mut tools_active = tool_runtime.is_some();

    loop {
        if interrupts.load(Ordering::SeqCst) >= 2 {
            println!("\nZweites Ctrl+C: Anwendung wird sicher beendet.");
            break;
        }
        print!("\nDu> ");
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if let Some(requested_model) = input.strip_prefix("/model ") {
            if requested_model.trim() != descriptor.id {
                println!(
                    "Nicht installiert. Verfügbar: {} ({})",
                    descriptor.id, descriptor.display_name
                );
                continue;
            }
            engine.reload()?;
            println!(
                "Modell {} wurde ohne Launcher-Neustart geladen.",
                descriptor.id
            );
            continue;
        }
        if let Some(argument) = input.strip_prefix("/werkzeuge ") {
            match argument.trim() {
                "on" => {
                    if tool_runtime.is_none() {
                        println!(
                            "Werkzeuge sind nicht initialisiert. Starte den Launcher mit --tools."
                        );
                    } else {
                        tools_active = true;
                        println!("Werkzeuge aktiv.");
                    }
                }
                "off" => {
                    tools_active = false;
                    println!("Werkzeuge deaktiviert; nur Streaming-Chat.");
                }
                other => println!("/werkzeuge erwartet on oder off (bekam: {other})."),
            }
            continue;
        }
        if let Some(argument) = input.strip_prefix("/modus ") {
            match argument.trim() {
                "m0" | "M0" => set_mode(tool_runtime.as_deref_mut(), Mode::M0Observe),
                "m1" | "M1" => set_mode(tool_runtime.as_deref_mut(), Mode::M1Workspace),
                "m2" | "M2" => set_mode(tool_runtime.as_deref_mut(), Mode::M2Extended),
                "m3" | "M3" => set_mode(tool_runtime.as_deref_mut(), Mode::M3Autonomous),
                other => println!("/modus erwartet m0..m3 (bekam: {other})."),
            }
            continue;
        }
        match input {
            "/quit" | "/exit" => break,
            "/hilfe" => {
                println!(
                    "/model {} wählt das installierte Modell; /reload lädt es neu; \
                     /werkzeuge on|off schaltet die Werkzeugschleife; \
                     /modus m0..m3 wechselt den Berechtigungsmodus; \
                     /quit beendet mit Vault-Sync.",
                    descriptor.id
                );
                continue;
            }
            "/reload" => {
                engine.reload()?;
                println!("Modell wurde ohne Neustart des Launchers neu geladen.");
                continue;
            }
            _ => {}
        }

        interrupts.store(0, Ordering::SeqCst);
        let now = now_unix_ms();
        if !created {
            let title = input.chars().take(64).collect::<String>();
            let mut conversations = SharedConversations::new(vault.shared()?);
            conversations.create(&conversation_id, &title, now)?;
            created = true;
        }

        let user_id = random_id("message");
        let assistant_id = random_id("message");

        // Werkzeug-Pfad: läuft nicht-streaming, damit die Toolschleife pro
        // Iteration eine vollständige Modellantwort sieht. Nutzernachricht und
        // finale Antwort werden hier direkt in der Konversation persistiert;
        // Zwischenrunden (Envelope+Werkzeugresultate) bleiben transient.
        if tools_active {
            if let Some(runtime) = tool_runtime.as_deref_mut() {
                run_tool_turn(
                    runtime,
                    vault,
                    engine,
                    &conversation_id,
                    &user_id,
                    &assistant_id,
                    input,
                    now,
                    &interrupts,
                )?;
                continue;
            }
        }

        let request = TurnRequest {
            conversation_id: &conversation_id,
            user_message_id: &user_id,
            assistant_message_id: &assistant_id,
            user_input: input,
            now_unix_ms: now,
        };

        let dispatched = Instant::now();
        let mut first_delta: Option<Instant> = None;
        print!("IAP> ");
        io::stdout().flush()?;

        let interrupts_for_should = Arc::clone(&interrupts);
        let mut should_continue = move || interrupts_for_should.load(Ordering::SeqCst) == 0;
        let mut on_delta = |delta: &str| {
            if first_delta.is_none() {
                first_delta = Some(Instant::now());
            }
            print!("{delta}");
            let _ = io::stdout().flush();
        };
        let mut on_engine_ready = |status: EngineReady| {
            if status == EngineReady::Restarted {
                println!("[llama-server wurde vor dem Auftrag automatisch neu gestartet]");
            }
        };
        let mut callbacks = TurnCallbacks {
            should_continue: &mut should_continue,
            on_delta: &mut on_delta,
            on_engine_ready: &mut on_engine_ready,
        };

        let mut conversations = SharedConversations::new(vault.shared()?);
        let result = orchestrator.run_turn(&mut conversations, engine, request, &mut callbacks);

        match result {
            Ok(outcome) => {
                println!();
                if outcome.outcome.aborted {
                    println!("[Antwort abgebrochen; Teiltext wurde gespeichert]");
                }
                if outcome.dropped_older_turns > 0 {
                    println!(
                        "[{} älteste Runde(n) wurden verworfen, um im Kontextfenster zu bleiben]",
                        outcome.dropped_older_turns
                    );
                }
                print_timings(dispatched, first_delta, &outcome.outcome);
            }
            Err(TurnError::Stream(failure)) => {
                let server_status = engine.ensure_ready();
                println!(
                    "\nInferenzfehler: {}; Serverstatus: {:?}",
                    failure.source.message, server_status
                );
            }
            Err(TurnError::Core(error)) => {
                println!("\nCore-Fehler: {error}");
            }
        }
    }
    Ok(())
}

fn set_mode(runtime: Option<&mut ToolRuntime>, mode: Mode) {
    match runtime {
        Some(runtime) => {
            let previous = runtime.mode();
            runtime.set_mode(mode);
            if previous.requires_reauth_on_upgrade(mode) {
                println!(
                    "Modus {previous:?} → {mode:?}. Nach Konzept 10.1 ist eine Reauth im UI vorgesehen; \
                     im CLI-Prototypen wird der Wechsel direkt übernommen."
                );
            } else {
                println!("Modus jetzt {mode:?}.");
            }
        }
        None => println!("Werkzeuge sind nicht aktiv; Modus wird nur mit --tools verwendet."),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_tool_turn(
    runtime: &mut ToolRuntime,
    vault: &VaultRuntime,
    engine: &mut LlamaServerEngine,
    conversation_id: &str,
    user_id: &str,
    assistant_id: &str,
    input: &str,
    now: i64,
    interrupts: &Arc<AtomicUsize>,
) -> Result<(), Box<dyn Error>> {
    // Nutzernachricht sofort persistieren; Platzhalter für die Antwort
    // bleibt im Streaming-Status, damit ein harter Abbruch nicht mit einer
    // leeren Assistentenzeile endet.
    {
        let mut conversations = SharedConversations::new(vault.shared()?);
        conversations.append(
            user_id,
            conversation_id,
            MessageRole::User,
            input,
            MessageStatus::Complete,
            now,
        )?;
        conversations.append(
            assistant_id,
            conversation_id,
            MessageRole::Assistant,
            "",
            MessageStatus::Streaming,
            now,
        )?;
    }

    let dispatched = Instant::now();
    let interrupts_for_engine = Arc::clone(interrupts);
    let engine_result = runtime.run_turn(
        input,
        now,
        |messages| {
            let mut collected = String::new();
            let mut should_continue = || interrupts_for_engine.load(Ordering::SeqCst) == 0;
            let mut on_delta = |delta: &str| -> bool {
                collected.push_str(delta);
                true
            };
            match engine.stream_chat(messages, &mut should_continue, &mut on_delta) {
                Ok(outcome) => {
                    if outcome.aborted {
                        Err(ToolLoopError::Engine(
                            "Stream durch Ctrl+C abgebrochen".to_owned(),
                        ))
                    } else {
                        Ok(collected)
                    }
                }
                Err(error) => Err(ToolLoopError::Engine(error.message)),
            }
        },
        |event| match event {
            ToolEvent::ModelText { text } => println!("\n[Modelltext]\n{text}"),
            ToolEvent::ToolCall { tool, arguments } => {
                println!("[Werkzeug ⇒ {tool}] {arguments}")
            }
            ToolEvent::ToolResult {
                tool,
                content,
                is_untrusted,
            } => {
                let marker = if is_untrusted { "UNTRUSTED" } else { "intern" };
                println!("[Ergebnis {tool} ({marker})]\n{content}");
            }
            ToolEvent::ToolError { tool, message } => {
                println!("[Werkzeugfehler {tool}: {message}]");
            }
        },
    );

    let mut conversations = SharedConversations::new(vault.shared()?);
    match engine_result {
        Ok(final_text) => {
            println!("\nIAP> {final_text}");
            println!(
                "[Werkzeug-Turn abgeschlossen in {:.3} s]",
                dispatched.elapsed().as_secs_f64()
            );
            conversations.finish(assistant_id, &final_text, MessageStatus::Complete)?;
        }
        Err(error) => {
            let message = error.to_string();
            println!("\n[Werkzeugschleife: {message}]");
            conversations.finish(
                assistant_id,
                &format!("[abgebrochen] {message}"),
                MessageStatus::Aborted,
            )?;
        }
    }
    Ok(())
}

fn print_timings(dispatched: Instant, first_delta: Option<Instant>, outcome: &StreamOutcomeDto) {
    let first_delta_seconds =
        first_delta.map(|instant| instant.duration_since(dispatched).as_secs_f64());
    let post_prompt_seconds = match (first_delta_seconds, outcome.timings.prompt_ms) {
        (Some(first), Some(prompt_ms)) => Some(first - prompt_ms / 1000.0),
        _ => None,
    };
    println!(
        "[Zeit: erstes Delta={} s, Prompt={} ms, nach Prompt={} s, Prompt={}/s, Generierung={}/s]",
        measured(first_delta_seconds),
        measured(outcome.timings.prompt_ms),
        measured(post_prompt_seconds),
        measured(outcome.timings.prompt_per_second),
        measured(outcome.timings.predicted_per_second),
    );
}

fn print_hardware_and_plan(hardware: &pa_types::hardware::HardwareProfile, plan: &ResourcePlan) {
    println!("      CPU: {}", hardware.cpu_model);
    println!(
        "      RAM: {:.2} GiB frei / {:.2} GiB gesamt; Tier {:?}{}",
        gib(hardware.available_ram_bytes),
        gib(hardware.total_ram_bytes),
        plan.tier,
        if plan.tier_hardware_validated {
            " (gemessen)"
        } else {
            " (nicht hardwarevalidiert)"
        }
    );
    println!(
        "      Startparameter: Kontext {}, Threads {}, GPU-Layer berechnet {}, tatsächlich 0 (CPU-Build), KV {:?}",
        plan.context_tokens, plan.threads, plan.gpu_layers, plan.kv_quantization
    );
    for warning in &plan.warnings {
        println!("      Warnung: {warning}");
    }
}

fn measured(value: Option<f64>) -> String {
    value.map_or_else(
        || "nicht gemessen".to_owned(),
        |number| format!("{number:.3}"),
    )
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!("{prefix}-{}", hex::encode(bytes))
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1024_f64.powi(3)
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn verify_model_override(context: &BootstrapContext, override_path: &Path) -> io::Result<()> {
    let candidate = if override_path.is_absolute() {
        override_path.to_path_buf()
    } else {
        context.package.as_path().join(override_path)
    };
    let canonical = candidate.canonicalize()?;
    if !canonical.starts_with(context.package.as_path()) {
        return Err(invalid_input(
            "--model muss auf eine manifestierte Datei im Projektordner zeigen",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct ConsoleCopyProgress {
    last_percent: u64,
}

impl CopyObserver for ConsoleCopyProgress {
    fn copied(&mut self, bytes: u64, total: u64) -> Result<(), LauncherError> {
        let percent = bytes.saturating_mul(100) / total.max(1);
        if percent >= self.last_percent.saturating_add(10) || percent == 100 {
            println!("      Modellkopie: {percent}%");
            self.last_percent = percent;
        }
        Ok(())
    }
}
