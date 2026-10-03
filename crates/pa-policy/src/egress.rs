//! Die Netzwerk-Ausnahmen: feste Konnektoren (Exa, Wikipedia, Open-Meteo, Brave,
//! Mail) pro Workflow-Lauf bzw. ausdrücklicher Aktivierung (Konzept 10.5).
//!
//! `capability::evaluate` verweigert `Network` weiterhin pauschal. Konnektoren
//! laufen über diesen eigenen, engeren Pfad, damit vier Dinge im Backend
//! erzwungen werden – nicht nur in der Oberfläche:
//!
//! 1. **Air Gap EIN sperrt wirklich**: jeder Aufruf fragt den aktuellen
//!    Zustand ab, auch mitten in einem Lauf.
//! 2. **Freigabe je Lauf**: [`ExaRunGrant`] ist an eine `run_id` gebunden und
//!    hat ein Aufrufbudget. Eine Fortsetzung braucht eine neue Freigabe.
//! 3. **Datenherkunft**: nur [`PublicText`] darf zu Exa. Er lässt sich nicht
//!    aus privater Herkunft bilden, auch nicht umformuliert.
//! 4. **Kein Umweg**: Hosts sind eine feste Tabelle ([`Connector::hosts`]); der
//!    Client hat keine Möglichkeit, sie zu ändern oder zu erweitern.

use serde::{Deserialize, Serialize};

use crate::{
    capability::{Capability, CapabilityAction, Decision},
    PolicyError,
};

/// Einziger erlaubter Host außer dem eigenen Inferenz-Port.
pub const EXA_HOST: &str = "api.exa.ai";

/// Ein externer Dienst, den IAP ansprechen darf. Alles andere bleibt gesperrt.
///
/// Warum ein Enum mit fester Host-Tabelle: Wer einen neuen Dienst anschließen will,
/// muss ihn hier eintragen, die Regeln (`AGENTS.md`, Konzept 10.5) ändern und Tests
/// bestehen. Es gibt keinen Weg, einen Host zur Laufzeit zu übergeben.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connector {
    /// Websuche über Exa.
    Exa,
    /// Wikipedia (Suche und Kurzfassung), keine Anmeldung.
    Wikipedia,
    /// Wetter und Ortssuche über Open-Meteo, keine Anmeldung.
    OpenMeteo,
    /// Websuche über Brave (eigener Schlüssel).
    Brave,
    /// Gmail über IMAP und SMTP (App-Passwort).
    Mail,
    /// Kalender: Apple iCloud (CalDAV) und Google (geheime iCal-Adresse, Kalender-Schnittstelle).
    Calendar,
}

impl Connector {
    /// Alle Konnektoren, für Tests und Anzeige.
    pub const ALL: [Connector; 6] = [
        Connector::Exa,
        Connector::Wikipedia,
        Connector::OpenMeteo,
        Connector::Brave,
        Connector::Mail,
        Connector::Calendar,
    ];

    /// Erlaubte Hosts des Dienstes.
    pub fn hosts(self) -> &'static [&'static str] {
        match self {
            Connector::Exa => &[EXA_HOST],
            Connector::Wikipedia => &["de.wikipedia.org", "en.wikipedia.org"],
            Connector::OpenMeteo => &["api.open-meteo.com", "geocoding-api.open-meteo.com"],
            Connector::Brave => &["api.search.brave.com"],
            Connector::Mail => &["imap.gmail.com", "smtp.gmail.com"],
            // iCloud verteilt Konten auf Server-Gruppen `pNN-caldav.icloud.com`; sie stehen nicht in
            // dieser Liste, sondern werden in `allows_host` über ein enges Muster erkannt.
            Connector::Calendar => &[
                "caldav.icloud.com",
                "calendar.google.com",
                "www.googleapis.com",
                "oauth2.googleapis.com",
            ],
        }
    }

    /// Ob der Dienst einen eigenen Schlüssel oder ein Passwort braucht.
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            Connector::Exa | Connector::Brave | Connector::Mail | Connector::Calendar
        )
    }

    /// Ob `host` genau einer der erlaubten Hosts ist (ohne Beachtung der Schreibweise).
    pub fn allows_host(self, host: &str) -> bool {
        self.hosts().iter().any(|h| h.eq_ignore_ascii_case(host))
            || (self == Connector::Calendar && is_icloud_shard(host))
    }

    /// Die Aktion, unter der Anfragen an diesen Dienst im Audit-Log stehen.
    pub fn action(self) -> CapabilityAction {
        match self {
            Connector::Exa => CapabilityAction::ExaRequest,
            Connector::Wikipedia | Connector::OpenMeteo | Connector::Brave => {
                CapabilityAction::WebRequest
            }
            Connector::Mail => CapabilityAction::MailRead,
            Connector::Calendar => CapabilityAction::CalendarRead,
        }
    }
}

/// Ob `host` eine iCloud-Server-Gruppe ist: `p`, ein bis drei Ziffern, `-caldav.icloud.com`.
///
/// Warum ein Muster statt einer Liste: Apple teilt jedes Konto einer Gruppe zu (`p12-caldav…`) und
/// nennt sie erst in der Antwort auf die Anmeldung. Das Muster ist absichtlich eng (kein anderer
/// Anfang, keine Endung, kein Port, kein Pfad), damit daraus kein offenes Tor wird.
fn is_icloud_shard(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let Some(rest) = host.strip_prefix('p') else {
        return false;
    };
    let Some(digits) = rest.strip_suffix("-caldav.icloud.com") else {
        return false;
    };
    (1..=3).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Obergrenze für einen Suchtext; verhindert, dass längere Fließtexte (etwa
/// eingefügte Dokumente) als „Suchbegriff“ hinausgehen.
pub const MAX_QUERY_CHARS: usize = 1_000;

/// Herkunft eines Textes im Workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataOrigin {
    /// Vom Nutzer sichtbar als öffentlich eingegebene Suchfrage.
    UserPublic,
    /// Öffentliches Exa-Ergebnis (unvertrauenswürdig, aber öffentlich).
    WebContent,
    /// Datei, Chat, Memory, Code, Audio, Bildschirm – oder unbekannt.
    Private,
}

impl DataOrigin {
    /// Verbindet zwei Herkünfte; Privates dominiert immer.
    ///
    /// Warum: Ein Text, der aus privatem und öffentlichem Material entstanden
    /// ist (etwa eine Modell-Zusammenfassung), bleibt privat. So bleiben auch
    /// umformulierte private Inhalte privat.
    pub fn join(self, other: DataOrigin) -> DataOrigin {
        if self == DataOrigin::Private || other == DataOrigin::Private {
            DataOrigin::Private
        } else if self == DataOrigin::WebContent || other == DataOrigin::WebContent {
            DataOrigin::WebContent
        } else {
            DataOrigin::UserPublic
        }
    }

    /// Verbindet beliebig viele Herkünfte. Eine leere Liste gilt als
    /// **privat**: unbekannte Herkunft wird nie als öffentlich behandelt.
    pub fn join_all(origins: &[DataOrigin]) -> DataOrigin {
        let mut iter = origins.iter().copied();
        match iter.next() {
            Some(first) => iter.fold(first, DataOrigin::join),
            None => DataOrigin::Private,
        }
    }
}

/// Text, der nachweislich öffentlich ist und deshalb zu Exa gehen darf.
///
/// Es gibt keinen Konstruktor ohne Herkunftsangabe und keinen für
/// [`DataOrigin::Private`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicText(String);

impl PublicText {
    /// Erzeugt einen öffentlichen Text.
    ///
    /// # Errors
    /// `PolicyError::DataBoundary` bei privater Herkunft, leerem oder zu
    /// langem Text oder Steuerzeichen.
    pub fn new(text: impl Into<String>, origin: DataOrigin) -> Result<Self, PolicyError> {
        if origin == DataOrigin::Private {
            return Err(PolicyError::DataBoundary(
                "Private Inhalte dürfen nie an Exa gehen".to_owned(),
            ));
        }
        let text = text.into();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(PolicyError::DataBoundary("Leerer Suchtext".to_owned()));
        }
        if trimmed.chars().count() > MAX_QUERY_CHARS {
            return Err(PolicyError::DataBoundary(format!(
                "Suchtext länger als {MAX_QUERY_CHARS} Zeichen"
            )));
        }
        if trimmed.chars().any(|c| c.is_control() && c != '\n') {
            return Err(PolicyError::DataBoundary(
                "Steuerzeichen im Suchtext".to_owned(),
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Der geprüfte Text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Welcher Exa-Endpunkt aufgerufen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExaEndpoint {
    /// `POST /search`
    Search,
    /// `POST /contents`
    Contents,
}

impl ExaEndpoint {
    /// Fester Pfad; der Client baut die URL nur daraus.
    pub fn path(self) -> &'static str {
        match self {
            ExaEndpoint::Search => "/search",
            ExaEndpoint::Contents => "/contents",
        }
    }
}

/// Freigabe für genau einen Workflow-Lauf (gilt für alle Web-Konnektoren des Laufs).
///
/// Warum kein Zeitstempel-Ablauf: Der Lauf endet mit dem Hauptfenster; eine
/// Fortsetzung erzeugt einen neuen Lauf und damit eine neue Freigabe.
#[derive(Debug, Clone)]
pub struct RunGrant {
    run_id: String,
    calls_left: u32,
    active: bool,
}

impl RunGrant {
    /// Erteilt die Freigabe für `run_id` mit höchstens `max_calls` Aufrufen.
    pub fn new(run_id: impl Into<String>, max_calls: u32) -> Self {
        Self {
            run_id: run_id.into(),
            calls_left: max_calls,
            active: true,
        }
    }

    /// Lauf, für den die Freigabe gilt.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Verbleibende Aufrufe.
    pub fn calls_left(&self) -> u32 {
        self.calls_left
    }

    /// Entzieht die Freigabe (Laufende, Abbruch, Fenster schließen).
    pub fn revoke(&mut self) {
        self.active = false;
    }
}

/// Bisheriger Name der Freigabe.
pub type ExaRunGrant = RunGrant;

/// Eine konkrete Anfrage an einen Konnektor vor der Prüfung.
#[derive(Debug)]
pub struct ConnectorRequest<'a> {
    /// Lauf, aus dem die Anfrage stammt.
    pub run_id: &'a str,
    /// Ziel-Dienst.
    pub connector: Connector,
    /// Ziel-Host; muss in [`Connector::hosts`] stehen.
    pub host: &'a str,
    /// Geprüfter öffentlicher Text (Suchfrage, Ort oder URL-Liste).
    pub text: &'a PublicText,
}

/// Eine konkrete Exa-Anfrage vor der Prüfung.
#[derive(Debug)]
pub struct ExaRequest<'a> {
    /// Lauf, aus dem die Anfrage stammt.
    pub run_id: &'a str,
    /// Ziel-Endpunkt.
    pub endpoint: ExaEndpoint,
    /// Geprüfter öffentlicher Text (Suchfrage oder URL-Liste).
    pub text: &'a PublicText,
}

/// Entscheidet über eine Anfrage an einen Web-Konnektor und verbraucht bei Erfolg einen Aufruf.
///
/// Reihenfolge: Dienst erlaubt, Air Gap, Host, Freigabe aktiv, Laufbindung, Budget. Jede
/// Ablehnung hat einen sprechenden Grund für Audit und Oberfläche. Mail hat einen eigenen
/// Pfad ([`authorize_mail`]), weil es keinen Lauf gibt.
pub fn authorize_connector(
    grant: &mut RunGrant,
    air_gap: bool,
    request: &ConnectorRequest<'_>,
) -> Decision {
    if request.connector == Connector::Mail {
        return Decision::Deny("Mail läuft nicht über Workflow-Freigaben".to_owned());
    }
    if request.connector == Connector::Calendar {
        return Decision::Deny("Der Kalender läuft nicht über Workflow-Freigaben".to_owned());
    }
    if air_gap {
        return Decision::Deny(
            "Air Gap ist eingeschaltet: externe Anfragen sind gesperrt".to_owned(),
        );
    }
    if !request.connector.allows_host(request.host) {
        return Decision::Deny(format!(
            "Der Host `{}` gehört nicht zu diesem Dienst",
            request.host
        ));
    }
    if !grant.active {
        return Decision::Deny("Die Freigabe dieses Laufs wurde entzogen".to_owned());
    }
    if grant.run_id != request.run_id {
        return Decision::Deny("Die Freigabe gilt für einen anderen Lauf".to_owned());
    }
    if grant.calls_left == 0 {
        return Decision::Deny("Das Aufrufbudget für externe Dienste ist ausgeschöpft".to_owned());
    }
    grant.calls_left -= 1;
    Decision::Allow(Capability {
        action: request.connector.action(),
        canonical_path: None,
    })
}

/// Entscheidet über eine Exa-Anfrage und verbraucht bei Erfolg einen Aufruf.
pub fn authorize_exa(grant: &mut ExaRunGrant, air_gap: bool, request: &ExaRequest<'_>) -> Decision {
    authorize_connector(
        grant,
        air_gap,
        &ConnectorRequest {
            run_id: request.run_id,
            connector: Connector::Exa,
            host: EXA_HOST,
            text: request.text,
        },
    )
}

/// Entscheidet über eine Mail-Verbindung (Lesen oder Senden).
///
/// Es gibt keine Lauf-Freigabe, sondern die ausdrückliche Aktivierung durch den Nutzer in
/// dieser Sitzung; die App prüft sie vor dem Aufruf. Hier gelten nur die harten Regeln:
/// Air Gap, fester Host und eine Mail-Aktion. Limits und Absenderprüfung sitzen im Mail-Modul.
pub fn authorize_mail(air_gap: bool, host: &str, action: CapabilityAction) -> Decision {
    if air_gap {
        return Decision::Deny(
            "Air Gap ist eingeschaltet: externe Anfragen sind gesperrt".to_owned(),
        );
    }
    if !Connector::Mail.allows_host(host) {
        return Decision::Deny(format!("Der Host `{host}` ist für Mail nicht erlaubt"));
    }
    if !matches!(
        action,
        CapabilityAction::MailRead | CapabilityAction::MailSend
    ) {
        return Decision::Deny("Keine Mail-Aktion".to_owned());
    }
    Decision::Allow(Capability {
        action,
        canonical_path: None,
    })
}

/// Entscheidet über eine Kalender-Verbindung (Abrufen oder Termin anlegen).
///
/// Wie bei Mail gibt es keine Lauf-Freigabe: Auslöser ist immer ein Klick des Nutzers
/// („Aktualisieren“, „Termin speichern“), nie ein Takt. Hier gelten die harten Regeln: Air Gap,
/// feste Host-Tabelle und eine Kalender-Aktion.
pub fn authorize_calendar(air_gap: bool, host: &str, action: CapabilityAction) -> Decision {
    if air_gap {
        return Decision::Deny(
            "Air Gap ist eingeschaltet: externe Anfragen sind gesperrt".to_owned(),
        );
    }
    if !Connector::Calendar.allows_host(host) {
        return Decision::Deny(format!(
            "Der Host `{host}` ist für den Kalender nicht erlaubt"
        ));
    }
    if !matches!(
        action,
        CapabilityAction::CalendarRead | CapabilityAction::CalendarWrite
    ) {
        return Decision::Deny("Keine Kalender-Aktion".to_owned());
    }
    Decision::Allow(Capability {
        action,
        canonical_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> PublicText {
        PublicText::new(text, DataOrigin::UserPublic).unwrap()
    }

    #[test]
    fn private_origin_cannot_become_public_text() {
        assert!(matches!(
            PublicText::new("Gehaltsliste Q3", DataOrigin::Private),
            Err(PolicyError::DataBoundary(_))
        ));
    }

    #[test]
    fn rephrased_private_stays_private() {
        let joined = DataOrigin::join_all(&[DataOrigin::UserPublic, DataOrigin::Private]);
        assert_eq!(joined, DataOrigin::Private);
        assert!(PublicText::new("umformuliert", joined).is_err());
    }

    #[test]
    fn unknown_origin_is_private() {
        assert_eq!(DataOrigin::join_all(&[]), DataOrigin::Private);
    }

    #[test]
    fn web_content_joined_with_public_stays_public_ok() {
        let joined = DataOrigin::UserPublic.join(DataOrigin::WebContent);
        assert_eq!(joined, DataOrigin::WebContent);
        assert!(PublicText::new("präzisierte Suche", joined).is_ok());
    }

    #[test]
    fn overlong_or_empty_or_control_text_is_rejected() {
        assert!(PublicText::new("   ", DataOrigin::UserPublic).is_err());
        assert!(PublicText::new("a".repeat(MAX_QUERY_CHARS + 1), DataOrigin::UserPublic).is_err());
        assert!(PublicText::new("a\u{0007}b", DataOrigin::UserPublic).is_err());
    }

    #[test]
    fn air_gap_blocks_even_with_valid_grant() {
        let mut grant = ExaRunGrant::new("run-1", 3);
        let text = public("Rust Async");
        let request = ExaRequest {
            run_id: "run-1",
            endpoint: ExaEndpoint::Search,
            text: &text,
        };
        assert!(matches!(
            authorize_exa(&mut grant, true, &request),
            Decision::Deny(_)
        ));
        assert_eq!(
            grant.calls_left(),
            3,
            "gesperrter Aufruf verbraucht kein Budget"
        );
    }

    #[test]
    fn grant_of_other_run_is_denied() {
        let mut grant = ExaRunGrant::new("run-1", 3);
        let text = public("x");
        let request = ExaRequest {
            run_id: "run-2",
            endpoint: ExaEndpoint::Search,
            text: &text,
        };
        assert!(matches!(
            authorize_exa(&mut grant, false, &request),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn revoked_grant_is_denied() {
        let mut grant = ExaRunGrant::new("run-1", 3);
        grant.revoke();
        let text = public("x");
        let request = ExaRequest {
            run_id: "run-1",
            endpoint: ExaEndpoint::Search,
            text: &text,
        };
        assert!(matches!(
            authorize_exa(&mut grant, false, &request),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn budget_runs_out_honestly() {
        let mut grant = ExaRunGrant::new("run-1", 2);
        let text = public("x");
        let request = ExaRequest {
            run_id: "run-1",
            endpoint: ExaEndpoint::Search,
            text: &text,
        };
        assert!(matches!(
            authorize_exa(&mut grant, false, &request),
            Decision::Allow(_)
        ));
        assert!(matches!(
            authorize_exa(&mut grant, false, &request),
            Decision::Allow(_)
        ));
        assert!(matches!(
            authorize_exa(&mut grant, false, &request),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn endpoints_are_fixed_paths() {
        assert_eq!(ExaEndpoint::Search.path(), "/search");
        assert_eq!(ExaEndpoint::Contents.path(), "/contents");
        assert_eq!(EXA_HOST, "api.exa.ai");
    }

    #[test]
    fn every_connector_is_denied_under_air_gap_and_without_a_grant() {
        for connector in Connector::ALL {
            if matches!(connector, Connector::Mail | Connector::Calendar) {
                continue;
            }
            let host = connector.hosts()[0];
            let text = public("Rom");
            let request = ConnectorRequest {
                run_id: "run-1",
                connector,
                host,
                text: &text,
            };
            let mut grant = RunGrant::new("run-1", 2);
            assert!(matches!(
                authorize_connector(&mut grant, true, &request),
                Decision::Deny(_)
            ));
            assert_eq!(
                grant.calls_left(),
                2,
                "{connector:?}: Sperre verbraucht nichts"
            );
            grant.revoke();
            assert!(matches!(
                authorize_connector(&mut grant, false, &request),
                Decision::Deny(_)
            ));
            let mut other = RunGrant::new("run-2", 2);
            assert!(matches!(
                authorize_connector(&mut other, false, &request),
                Decision::Deny(_)
            ));
            let mut empty = RunGrant::new("run-1", 0);
            assert!(matches!(
                authorize_connector(&mut empty, false, &request),
                Decision::Deny(_)
            ));
            let mut ok = RunGrant::new("run-1", 1);
            match authorize_connector(&mut ok, false, &request) {
                Decision::Allow(cap) => assert_eq!(cap.action, connector.action()),
                other => panic!("{connector:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_host_of_another_connector_or_unknown_host_is_denied() {
        let text = public("x");
        for (connector, host) in [
            (Connector::Exa, "api.search.brave.com"),
            (Connector::Wikipedia, "evil.example.org"),
            (Connector::Wikipedia, "wikipedia.org.evil.example"),
            (Connector::OpenMeteo, "api.exa.ai"),
            (Connector::Brave, "api.exa.ai"),
        ] {
            let mut grant = RunGrant::new("run-1", 5);
            let request = ConnectorRequest {
                run_id: "run-1",
                connector,
                host,
                text: &text,
            };
            assert!(
                matches!(
                    authorize_connector(&mut grant, false, &request),
                    Decision::Deny(_)
                ),
                "{connector:?} -> {host}"
            );
            assert_eq!(grant.calls_left(), 5);
        }
    }

    #[test]
    fn the_host_table_is_exactly_the_agreed_list() {
        let mut all: Vec<&str> = Connector::ALL
            .iter()
            .flat_map(|c| c.hosts().iter().copied())
            .collect();
        all.sort_unstable();
        assert_eq!(
            all,
            [
                "api.exa.ai",
                "api.open-meteo.com",
                "api.search.brave.com",
                "caldav.icloud.com",
                "calendar.google.com",
                "de.wikipedia.org",
                "en.wikipedia.org",
                "geocoding-api.open-meteo.com",
                "imap.gmail.com",
                "oauth2.googleapis.com",
                "smtp.gmail.com",
                "www.googleapis.com",
            ]
        );
        assert!(
            Connector::Exa.needs_key()
                && Connector::Brave.needs_key()
                && Connector::Mail.needs_key()
                && Connector::Calendar.needs_key()
        );
        assert!(!Connector::Wikipedia.needs_key() && !Connector::OpenMeteo.needs_key());
    }

    #[test]
    fn calendar_hosts_are_a_fixed_table_with_one_pattern() {
        for host in [
            "caldav.icloud.com",
            "p12-caldav.icloud.com",
            "P103-CALDAV.ICLOUD.COM",
            "calendar.google.com",
            "www.googleapis.com",
            "oauth2.googleapis.com",
        ] {
            assert!(Connector::Calendar.allows_host(host), "{host}");
        }
        for host in [
            "icloud.com",
            "p-caldav.icloud.com",
            "pab-caldav.icloud.com",
            "p1234-caldav.icloud.com",
            "p12-caldav.icloud.com.evil.example",
            "evilp12-caldav.icloud.com",
            "p12-caldav.icloud.com:443",
            "p12-caldav.icloud.com/x",
            "google.com",
            "api.exa.ai",
            "imap.gmail.com",
            "",
        ] {
            assert!(!Connector::Calendar.allows_host(host), "{host}");
        }
        // Die Muster gelten nur für den Kalender, nicht für andere Dienste.
        assert!(!Connector::Wikipedia.allows_host("p12-caldav.icloud.com"));
        assert!(!Connector::Mail.allows_host("calendar.google.com"));
    }

    #[test]
    fn calendar_has_its_own_gate() {
        assert!(matches!(
            authorize_calendar(true, "caldav.icloud.com", CapabilityAction::CalendarRead),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_calendar(false, "caldav.evil.example", CapabilityAction::CalendarRead),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_calendar(false, "caldav.icloud.com", CapabilityAction::Network),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_calendar(
                false,
                "p5-caldav.icloud.com",
                CapabilityAction::CalendarWrite
            ),
            Decision::Allow(_)
        ));
        // Der allgemeine Pfad-Prüfer gibt Kalender-Aktionen nie frei.
        assert!(CapabilityAction::CalendarRead.has_dedicated_gate());
        assert!(CapabilityAction::CalendarWrite.has_dedicated_gate());
        // Kalender geht nie über die Workflow-Freigabe.
        let text = public("x");
        let mut grant = RunGrant::new("run-1", 3);
        let request = ConnectorRequest {
            run_id: "run-1",
            connector: Connector::Calendar,
            host: "caldav.icloud.com",
            text: &text,
        };
        assert!(matches!(
            authorize_connector(&mut grant, false, &request),
            Decision::Deny(_)
        ));
        assert_eq!(grant.calls_left(), 3);
    }

    #[test]
    fn mail_has_its_own_gate() {
        assert!(matches!(
            authorize_mail(true, "imap.gmail.com", CapabilityAction::MailRead),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_mail(false, "imap.evil.example", CapabilityAction::MailRead),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_mail(false, "imap.gmail.com", CapabilityAction::Network),
            Decision::Deny(_)
        ));
        assert!(matches!(
            authorize_mail(false, "smtp.gmail.com", CapabilityAction::MailSend),
            Decision::Allow(_)
        ));
        // Mail geht nie über die Workflow-Freigabe.
        let text = public("x");
        let mut grant = RunGrant::new("run-1", 3);
        let request = ConnectorRequest {
            run_id: "run-1",
            connector: Connector::Mail,
            host: "imap.gmail.com",
            text: &text,
        };
        assert!(matches!(
            authorize_connector(&mut grant, false, &request),
            Decision::Deny(_)
        ));
    }
}
