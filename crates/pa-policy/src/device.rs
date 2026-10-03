//! Freigabe für Mikrofon und Bildschirm (Konzept 10.5, Punkte 3 und 4).
//!
//! Aufnahme ist nie ein Dauerzustand: Jede Aufnahme braucht ein
//! [`CaptureTicket`], das eine sichtbare Nutzeraktion ausstellt und das nach
//! genau einer Aufnahme verbraucht ist. Für den Bildschirm heißt das
//! „genau eine Aufnahme je Auftrag“.

use serde::{Deserialize, Serialize};

use crate::capability::{Capability, CapabilityAction, Decision};

/// Aufnahmegerät.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Microphone,
    Screen,
}

impl DeviceKind {
    fn action(self) -> CapabilityAction {
        match self {
            DeviceKind::Microphone => CapabilityAction::MicCapture,
            DeviceKind::Screen => CapabilityAction::ScreenCapture,
        }
    }
}

/// Einmal-Ticket für genau eine Aufnahme.
#[derive(Debug, Clone)]
pub struct CaptureTicket {
    kind: DeviceKind,
    used: bool,
}

impl CaptureTicket {
    /// Stellt ein Ticket aus. Aufrufer ist der Befehl, der aus einer
    /// sichtbaren Nutzeraktion entsteht (Knopf, Tastenkürzel, Menüpunkt).
    pub fn issue(kind: DeviceKind) -> Self {
        Self { kind, used: false }
    }

    /// Gerät, für das das Ticket gilt.
    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// Ob das Ticket schon verbraucht ist.
    pub fn is_used(&self) -> bool {
        self.used
    }
}

/// Zustand, gegen den die Aufnahme geprüft wird.
#[derive(Debug, Clone, Copy)]
pub struct CaptureContext {
    /// Vault ist entsperrt. Ein gesperrter Vault erlaubt keine neue Sprach-
    /// oder Bildschirmverarbeitung.
    pub vault_unlocked: bool,
    /// Das hardwareabhängige Freigabetor ist offen (T0-Sprachmessung
    /// bestanden bzw. lokaler Bildpfad nachgewiesen).
    pub gate_open: bool,
}

/// Entscheidet über eine Aufnahme und verbraucht bei Erfolg das Ticket.
pub fn authorize_capture(
    context: &CaptureContext,
    ticket: &mut CaptureTicket,
    requested: DeviceKind,
) -> Decision {
    if ticket.kind != requested {
        return Decision::Deny("Das Ticket gilt für ein anderes Gerät".to_owned());
    }
    if ticket.used {
        return Decision::Deny(
            "Diese Aufnahme wurde bereits durchgeführt; eine weitere braucht einen neuen Auftrag"
                .to_owned(),
        );
    }
    if !context.vault_unlocked {
        return Decision::Deny("Der Tresor ist gesperrt".to_owned());
    }
    if !context.gate_open {
        return Decision::Deny(
            "Auf diesem Rechner noch nicht freigegeben (Messung oder Bildpfad fehlt)".to_owned(),
        );
    }
    ticket.used = true;
    Decision::Allow(Capability {
        action: requested.action(),
        canonical_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: CaptureContext = CaptureContext {
        vault_unlocked: true,
        gate_open: true,
    };

    #[test]
    fn ticket_allows_exactly_one_capture() {
        let mut ticket = CaptureTicket::issue(DeviceKind::Screen);
        assert!(matches!(
            authorize_capture(&OPEN, &mut ticket, DeviceKind::Screen),
            Decision::Allow(_)
        ));
        assert!(ticket.is_used());
        assert!(matches!(
            authorize_capture(&OPEN, &mut ticket, DeviceKind::Screen),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn locked_vault_denies_and_keeps_ticket() {
        let mut ticket = CaptureTicket::issue(DeviceKind::Microphone);
        let locked = CaptureContext {
            vault_unlocked: false,
            ..OPEN
        };
        assert!(matches!(
            authorize_capture(&locked, &mut ticket, DeviceKind::Microphone),
            Decision::Deny(_)
        ));
        assert!(!ticket.is_used());
    }

    #[test]
    fn closed_gate_denies() {
        let mut ticket = CaptureTicket::issue(DeviceKind::Microphone);
        let closed = CaptureContext {
            gate_open: false,
            ..OPEN
        };
        assert!(matches!(
            authorize_capture(&closed, &mut ticket, DeviceKind::Microphone),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn microphone_ticket_cannot_capture_screen() {
        let mut ticket = CaptureTicket::issue(DeviceKind::Microphone);
        assert!(matches!(
            authorize_capture(&OPEN, &mut ticket, DeviceKind::Screen),
            Decision::Deny(_)
        ));
    }
}
