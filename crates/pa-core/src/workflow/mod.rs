//! Kleiner Workflow-Builder: Prüfung und Ausführung fester, typisierter Knoten.
//!
//! Der Kern kennt weder Netz noch Modell. Beides kommt über [`runner::WorkflowPorts`]
//! herein; die App implementiert diese Anschlüsse mit Policy, Air Gap und
//! Freigaben. So lässt sich die Ablauflogik (Datenherkunft, Budgets, Abbruch,
//! Wiederholungen) vollständig ohne Internet testen.
//!
//! Sicherheitsleitlinien, die hier im Code erzwungen werden – nicht in der Oberfläche:
//!
//! - Jeder Wert trägt seine Herkunft ([`pa_policy::DataOrigin`]). Nur ein
//!   [`pa_policy::PublicText`] erreicht Exa; er lässt sich aus privater
//!   Herkunft nicht bilden.
//! - Modellaufrufe im Workflow haben einen frischen, getrennten Kontext (keine
//!   Chat-Verläufe, kein Gedächtnis, keine Dokumente) und keine Werkzeuge.
//! - Webinhalte stehen als markierte, unvertrauenswürdige Daten im Prompt; sie
//!   ändern weder die Knotenfolge noch Rechte. Neue Suchanfragen entstehen nur
//!   aus den vom Nutzer geschriebenen Texten, nie aus Webinhalten.

pub mod runner;
pub mod validate;

pub use runner::{
    run, EventSink, ExaContentsOutcome, ExaSearchOutcome, ModelOutcome, PortError, RunOutcome,
    RunSettings, SearchHit, WorkflowPorts,
};
pub use validate::{input_ports, output_ports, validate, PortSpec};
