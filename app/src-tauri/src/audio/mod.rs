//! Lokale Sprachbausteine: Aufnahme, Erkennung, Sprachausgabe.
//!
//! Alles läuft ohne Netz. Die Bausteine haben keine Rechte von sich aus; die
//! Freigabe (Ticket, Tresor entsperrt, T0-Tor) prüft `voice.rs` über
//! `pa-policy`, bevor eines davon startet.

pub mod capture;
pub mod command;
pub mod pcm;
pub mod playback;
pub mod stt;
pub mod text;
pub mod tts;
