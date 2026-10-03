use std::{ffi::OsString, net::TcpListener, path::PathBuf};

use pa_types::model::KvQuantization;
use rand_core::{OsRng, RngCore};

use crate::InferenceError;

/// Enthält nur die bereits berechneten und sicherheitsrelevanten Serverparameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub executable: PathBuf,
    pub model: PathBuf,
    pub model_alias: String,
    pub port: u16,
    pub api_key: String,
    pub context_tokens: u32,
    pub threads: usize,
    pub gpu_layers: usize,
    pub kv_quantization: KvQuantization,
    /// Bildprojektor für Bildverständnis. `None` lädt nur das Textmodell und
    /// spart damit den Arbeitsspeicher des Projektors.
    pub mmproj: Option<PathBuf>,
}

impl ServerConfig {
    /// Reserviert kurz einen Loopback-Port und erzeugt den Zugriffsschlüssel aus OS-Zufall.
    pub fn random_endpoint() -> Result<(u16, String), InferenceError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let mut token = [0_u8; 32];
        OsRng.fill_bytes(&mut token);
        Ok((port, hex::encode(token)))
    }

    /// Erzwingt Offline-, Loopback- und Ein-Slot-Betrieb unabhängig von llama.cpp-Defaults.
    pub fn arguments(&self) -> Vec<OsString> {
        let kv = match self.kv_quantization {
            KvQuantization::F16 => "f16",
            KvQuantization::Q8_0 => "q8_0",
        };
        let mut arguments = vec![
            ("--offline", None),
            ("--host", Some("127.0.0.1".to_owned())),
            ("--port", Some(self.port.to_string())),
            ("--api-key", Some(self.api_key.clone())),
            (
                "--model",
                Some(self.model.as_os_str().to_string_lossy().into_owned()),
            ),
            ("--alias", Some(self.model_alias.clone())),
            ("--ctx-size", Some(self.context_tokens.to_string())),
            ("--threads", Some(self.threads.to_string())),
            ("--gpu-layers", Some(self.gpu_layers.to_string())),
            ("--cache-type-k", Some(kv.to_owned())),
            ("--cache-type-v", Some(kv.to_owned())),
            ("--parallel", Some("1".to_owned())),
            ("--cache-ram", Some("64".to_owned())),
            ("--no-ui", None),
            ("--no-agent", None),
            ("--no-ui-mcp-proxy", None),
            // Die Denkstufe wird pro Anfrage begrenzt; ein globales "off"
            // würde diese Auswahl unabhängig vom Request unterdrücken.
            ("--reasoning", Some("auto".to_owned())),
        ];
        if let Some(mmproj) = &self.mmproj {
            // Der Projektor bleibt auf der CPU, wie das Textmodell (kein GPU-Backend im Paket).
            arguments.push((
                "--mmproj",
                Some(mmproj.as_os_str().to_string_lossy().into_owned()),
            ));
            arguments.push(("--no-mmproj-offload", None));
        }
        arguments
            .into_iter()
            .flat_map(|(flag, value)| {
                std::iter::once(OsString::from(flag)).chain(value.map(OsString::from))
            })
            .collect()
    }
}
