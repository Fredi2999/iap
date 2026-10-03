//! Der einzige ausgehende HTTPS-Client von IAP.
//!
//! Warum WinHTTP statt einer Rust-TLS-Bibliothek: Das Betriebssystem bringt
//! TLS (Schannel) und den Zertifikatsspeicher mit; es kommt **keine neue
//! Bibliothek** in den Kern (keine 1–2 MB für rustls/ring, keine Zertifikate
//! im Programm). Das passt zum Größenbudget und hält die Angriffsfläche klein.
//!
//! Die Grenzen sind hier fest verdrahtet, nicht konfigurierbar:
//! - nur `GET` und `POST` (für den Kalender zusätzlich `PUT`, `PROPFIND` und `REPORT`), nur `https`, nur Hosts aus der festen Tabelle
//!   [`pa_policy::egress::Connector::hosts`];
//! - Weiterleitungen sind abgeschaltet (ein `3xx` ist ein Fehler);
//! - Antwortgröße und Laufzeit sind begrenzt.
//!
//! Wer diese Funktionen aufruft, muss vorher `pa_policy::egress::authorize_connector`
//! bestanden haben; dieses Modul prüft nur den Air Gap als zweite Sperre, keine Freigaben.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use pa_policy::egress::Connector;
use thiserror::Error;

/// Air Gap. Startet **an** und wird nur durch den Nutzer ausgeschaltet.
///
/// Warum hier und nicht nur in der Policy: Die Policy prüft den Air Gap vor jedem
/// Exa-Aufruf. Dieser Schalter sitzt zusätzlich im einzigen Code, der überhaupt ins
/// Netz sendet. Selbst ein Fehler oder Umweg weiter oben kann bei eingeschaltetem
/// Air Gap also nichts hinausschicken.
static AIR_GAP: AtomicBool = AtomicBool::new(true);

/// Tests teilen sich den globalen Air-Gap-Schalter; diese Sperre verhindert Wettläufe.
#[cfg(test)]
pub(crate) static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Setzt den Air Gap (`true` = gesperrt).
pub fn set_air_gap(on: bool) {
    AIR_GAP.store(on, Ordering::SeqCst);
}

/// Ob der Air Gap gerade sperrt.
pub fn air_gap_on() -> bool {
    AIR_GAP.load(Ordering::SeqCst)
}

/// Fehler des HTTPS-Clients.
#[derive(Debug, Error)]
pub enum NetError {
    /// Der Air Gap ist eingeschaltet: es wird nichts gesendet.
    #[error("Air Gap ist eingeschaltet: es wird nichts gesendet")]
    AirGap,
    /// Das Ziel ist nicht erlaubt.
    #[error("Ziel nicht erlaubt: {0}")]
    Blocked(String),
    /// Fehler des Betriebssystems (Verbindung, TLS, Zeitüberschreitung).
    #[error("Netzwerkfehler: {0}")]
    #[cfg_attr(not(windows), allow(dead_code))]
    Os(String),
    /// Die Antwort war größer als erlaubt.
    #[error("Die Antwort ist größer als erlaubt")]
    #[cfg_attr(not(windows), allow(dead_code))]
    TooLarge,
    /// Auf dieser Plattform gibt es noch keinen geprüften Client.
    #[cfg(not(windows))]
    #[error("HTTPS ist auf dieser Plattform noch nicht eingerichtet (Phase 4)")]
    Unsupported,
}

/// Antwort eines Aufrufs.
#[derive(Debug)]
pub struct HttpsResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// HTTP-Verb. Mehr gibt es bewusst nicht; die drei WebDAV-Verben prüft `request_with` nur für den Kalender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    /// Nur für den Kalender (CalDAV): Anlegen eines Termins.
    Put,
    /// Nur für den Kalender (CalDAV): Eigenschaften abfragen.
    Propfind,
    /// Nur für den Kalender (CalDAV): Termine abfragen.
    Report,
}

impl Method {
    #[cfg_attr(not(windows), allow(dead_code))]
    fn verb(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Propfind => "PROPFIND",
            Method::Report => "REPORT",
        }
    }
}

/// Eine Anfrage an einen Konnektor. Das Ziel ergibt sich aus Host-Tabelle, Pfad und Query;
/// der Aufrufer kann keine freie Adresse übergeben.
#[derive(Debug)]
pub struct Request<'a> {
    pub connector: Connector,
    pub host: &'a str,
    pub method: Method,
    pub path: &'a str,
    /// Query als Schlüssel-Wert-Paare; der Client kodiert sie selbst.
    pub query: &'a [(&'a str, &'a str)],
    /// Vollständige Kopfzeilen als Name-Wert-Paare.
    pub headers: &'a [(&'a str, &'a str)],
    pub body: &'a [u8],
    pub timeout: Duration,
    pub max_response: usize,
}

/// Transport unter der Prüfung. Das System nutzt WinHTTP; Tests setzen einen Fake ein, damit
/// Pfade, Header und Rümpfe ohne Netz geprüft werden können.
pub trait Transport: Send + Sync {
    /// Sendet die bereits geprüfte Anfrage. `target` ist Pfad plus kodierte Query.
    // Die Parameter spiegeln die WinHTTP-Aufrufe 1:1; ein Parameterobjekt würde nur umpacken.
    #[allow(clippy::too_many_arguments)]
    fn send(
        &self,
        host: &str,
        method: Method,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
        max_response: usize,
    ) -> Result<HttpsResponse, NetError>;
}

/// Der echte Transport des Betriebssystems.
pub struct SystemTransport;

impl Transport for SystemTransport {
    fn send(
        &self,
        host: &str,
        method: Method,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
        max_response: usize,
    ) -> Result<HttpsResponse, NetError> {
        imp::send(host, method, target, headers, body, timeout, max_response)
    }
}

/// Kodiert einen Query-Bestandteil nach RFC 3986 (nur Unreserved bleibt stehen).
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Kodiert einen Pfadbestandteil (etwa einen Artikeltitel), damit er nie `/`, `?` oder `#` einschleust.
pub fn encode_segment(text: &str) -> String {
    percent_encode(text)
}

/// Baut `path?k=v&k2=v2`. Schlüssel und Werte werden hier kodiert, nie vom Aufrufer.
pub fn build_target(path: &str, query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return path.to_owned();
    }
    let pairs: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect();
    format!("{path}?{}", pairs.join("&"))
}

/// Prüft die Anfrage und sendet sie über den Transport des Systems.
///
/// # Errors
/// Siehe [`NetError`].
#[cfg(test)]
pub fn request(req: &Request<'_>) -> Result<HttpsResponse, NetError> {
    request_with(&SystemTransport, req)
}

/// Wie `request`, aber mit frei wählbarem Transport (Tests).
///
/// # Errors
/// Siehe [`NetError`].
pub fn request_with(
    transport: &dyn Transport,
    req: &Request<'_>,
) -> Result<HttpsResponse, NetError> {
    if air_gap_on() {
        return Err(NetError::AirGap);
    }
    if req.connector == Connector::Mail {
        return Err(NetError::Blocked("Mail läuft nicht über HTTPS".to_owned()));
    }
    if !req.connector.allows_host(req.host) {
        return Err(NetError::Blocked(format!("Host `{}`", req.host)));
    }
    if !req.path.starts_with('/') || req.path.contains(['?', '#', ' ', '\r', '\n']) {
        return Err(NetError::Blocked(format!("Pfad `{}`", req.path)));
    }
    if req
        .headers
        .iter()
        .any(|(k, v)| k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']))
    {
        return Err(NetError::Blocked("Kopfzeile mit Steuerzeichen".to_owned()));
    }
    // Die WebDAV-Verben gehören nur zum Kalender; kein anderer Dienst darf damit etwas anlegen.
    if matches!(req.method, Method::Put | Method::Propfind | Method::Report)
        && req.connector != Connector::Calendar
    {
        return Err(NetError::Blocked(format!(
            "{} ist für diesen Dienst nicht erlaubt",
            req.method.verb()
        )));
    }
    if req.method == Method::Get && !req.body.is_empty() {
        return Err(NetError::Blocked("GET mit Rumpf".to_owned()));
    }
    let target = build_target(req.path, req.query);
    transport.send(
        req.host,
        req.method,
        &target,
        req.headers,
        req.body,
        req.timeout,
        req.max_response,
    )
}

/// Sendet einen `POST` mit JSON-Rumpf an `https://api.exa.ai<path>`.
///
/// `headers` sind vollständige Kopfzeilen ohne Zeilenende; Werte mit
/// Zeilenumbruch werden abgelehnt (Header-Einschleusung).
///
/// # Errors
/// Siehe [`NetError`].
#[cfg(test)]
pub fn post_json_to_exa(
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
    max_response: usize,
) -> Result<HttpsResponse, NetError> {
    request(&Request {
        connector: Connector::Exa,
        host: pa_policy::egress::EXA_HOST,
        method: Method::Post,
        path,
        query: &[],
        headers,
        body,
        timeout,
        max_response,
    })
}

#[cfg(windows)]
mod imp {
    use std::{ffi::c_void, time::Duration};

    use windows::{
        core::PCWSTR,
        Win32::Networking::WinHttp::{
            WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
            WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
            WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
            INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3, WINHTTP_OPTION_DISABLE_FEATURE,
            WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
        },
    };
    use zeroize::Zeroize;

    use super::{HttpsResponse, Method, NetError};

    /// Schließt das Handle beim Verlassen, auch bei Fehlern.
    struct Handle(*mut c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: das Handle stammt aus WinHttp*-Aufrufen und wird genau einmal geschlossen.
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn os_error(what: &str) -> NetError {
        NetError::Os(format!(
            "{what}: {}",
            windows::core::Error::from_win32().message()
        ))
    }

    fn open(handle: *mut c_void, what: &str) -> Result<Handle, NetError> {
        if handle.is_null() {
            Err(os_error(what))
        } else {
            Ok(Handle(handle))
        }
    }

    pub fn send(
        host: &str,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
        max_response: usize,
    ) -> Result<HttpsResponse, NetError> {
        // Wikimedia verlangt einen aussagekräftigen User-Agent; er enthält keine Nutzerdaten.
        let agent = wide(concat!(
            "IAP/",
            env!("CARGO_PKG_VERSION"),
            " (local desktop app)"
        ));
        let host_w = wide(host);
        let path_w = wide(path);
        let verb = wide(method.verb());
        let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let body_len = u32::try_from(body.len()).map_err(|_| NetError::TooLarge)?;

        let mut header_text = String::new();
        for (name, value) in headers {
            header_text.push_str(name);
            header_text.push_str(": ");
            header_text.push_str(value);
            header_text.push_str("\r\n");
        }
        let mut header_w: Vec<u16> = header_text.encode_utf16().collect();
        header_text.zeroize();

        // SAFETY: alle Zeiger zeigen auf lokale Puffer, die bis zum Ende des Aufrufs leben;
        // Handles werden über `Handle` geschlossen.
        let result = unsafe {
            (|| {
                let session = open(
                    WinHttpOpen(
                        PCWSTR(agent.as_ptr()),
                        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                        PCWSTR::null(),
                        PCWSTR::null(),
                        0,
                    ),
                    "Sitzung",
                )?;
                WinHttpSetTimeouts(session.0, 10_000, 10_000, millis, millis)
                    .map_err(|_| os_error("Zeitgrenzen"))?;
                let both =
                    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
                if WinHttpSetOption(
                    Some(session.0),
                    WINHTTP_OPTION_SECURE_PROTOCOLS,
                    Some(&both.to_le_bytes()),
                )
                .is_err()
                {
                    WinHttpSetOption(
                        Some(session.0),
                        WINHTTP_OPTION_SECURE_PROTOCOLS,
                        Some(&WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_le_bytes()),
                    )
                    .map_err(|_| os_error("TLS-Version"))?;
                }
                let connection = open(
                    WinHttpConnect(
                        session.0,
                        PCWSTR(host_w.as_ptr()),
                        INTERNET_DEFAULT_HTTPS_PORT,
                        0,
                    ),
                    "Verbindung",
                )?;
                let request = open(
                    WinHttpOpenRequest(
                        connection.0,
                        PCWSTR(verb.as_ptr()),
                        PCWSTR(path_w.as_ptr()),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        std::ptr::null(),
                        WINHTTP_FLAG_SECURE,
                    ),
                    "Anfrage",
                )?;
                // Keine Weiterleitungen: eine Antwort von anderswo würde den festen Host umgehen.
                WinHttpSetOption(
                    Some(request.0),
                    WINHTTP_OPTION_DISABLE_FEATURE,
                    Some(&WINHTTP_DISABLE_REDIRECTS.to_le_bytes()),
                )
                .map_err(|_| os_error("Weiterleitungen"))?;
                WinHttpSendRequest(
                    request.0,
                    Some(&header_w),
                    if body.is_empty() {
                        None
                    } else {
                        Some(body.as_ptr().cast())
                    },
                    body_len,
                    body_len,
                    0,
                )
                .map_err(|_| os_error("Senden"))?;
                WinHttpReceiveResponse(request.0, std::ptr::null_mut())
                    .map_err(|_| os_error("Empfangen"))?;

                let mut status = 0_u32;
                let mut size = 4_u32;
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some((&raw mut status).cast()),
                    &raw mut size,
                    std::ptr::null_mut(),
                )
                .map_err(|_| os_error("Statuscode"))?;

                let mut out = Vec::new();
                loop {
                    let mut available = 0_u32;
                    WinHttpQueryDataAvailable(request.0, &raw mut available)
                        .map_err(|_| os_error("Daten"))?;
                    if available == 0 {
                        break;
                    }
                    if out.len() + available as usize > max_response {
                        return Err(NetError::TooLarge);
                    }
                    let start = out.len();
                    out.resize(start + available as usize, 0);
                    let mut read = 0_u32;
                    WinHttpReadData(
                        request.0,
                        out[start..].as_mut_ptr().cast(),
                        available,
                        &raw mut read,
                    )
                    .map_err(|_| os_error("Lesen"))?;
                    out.truncate(start + read as usize);
                    if read == 0 {
                        break;
                    }
                }
                Ok(HttpsResponse {
                    status: u16::try_from(status).unwrap_or(0),
                    body: out,
                })
            })()
        };
        header_w.zeroize();
        result
    }
}

#[cfg(not(windows))]
mod imp {
    use std::time::Duration;

    use super::{HttpsResponse, Method, NetError};

    pub fn send(
        _host: &str,
        _method: Method,
        _path: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
        _timeout: Duration,
        _max_response: usize,
    ) -> Result<HttpsResponse, NetError> {
        Err(NetError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::TEST_SERIAL as SERIAL;

    #[test]
    fn with_the_air_gap_on_nothing_leaves_the_process() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(true);
        // Gültige Anfrage, trotzdem gesperrt – und zwar vor jeder Betriebssystem-Funktion.
        assert!(matches!(
            post_json_to_exa(
                "/search",
                &[("x-api-key", "k")],
                b"{}",
                Duration::from_secs(1),
                10
            ),
            Err(NetError::AirGap)
        ));
    }

    #[test]
    fn paths_and_headers_cannot_smuggle_another_target() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(false);
        for path in ["search", "/search?x=1", "/a b", "/x\r\nHost: evil"] {
            assert!(matches!(
                post_json_to_exa(path, &[], b"{}", Duration::from_secs(1), 10),
                Err(NetError::Blocked(_))
            ));
        }
        assert!(matches!(
            post_json_to_exa(
                "/search",
                &[("x-api-key", "a\r\nHost: evil")],
                b"{}",
                Duration::from_secs(1),
                10
            ),
            Err(NetError::Blocked(_))
        ));
        assert!(matches!(
            post_json_to_exa(
                "/search",
                &[("a:b", "c")],
                b"{}",
                Duration::from_secs(1),
                10
            ),
            Err(NetError::Blocked(_))
        ));
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;
    use std::sync::Mutex;

    use super::TEST_SERIAL as SERIAL;

    /// Merkt sich die letzte Anfrage, die den Transport erreicht.
    type Seen = (String, Method, String, Vec<(String, String)>, Vec<u8>);

    #[derive(Default)]
    struct Fake {
        last: Mutex<Option<Seen>>,
    }

    impl Transport for Fake {
        fn send(
            &self,
            host: &str,
            method: Method,
            target: &str,
            headers: &[(&str, &str)],
            body: &[u8],
            _timeout: Duration,
            _max: usize,
        ) -> Result<HttpsResponse, NetError> {
            *self.last.lock().unwrap() = Some((
                host.to_owned(),
                method,
                target.to_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                body.to_vec(),
            ));
            Ok(HttpsResponse {
                status: 200,
                body: b"{}".to_vec(),
            })
        }
    }

    fn get<'a>(
        connector: Connector,
        host: &'a str,
        path: &'a str,
        query: &'a [(&'a str, &'a str)],
    ) -> Request<'a> {
        Request {
            connector,
            host,
            method: Method::Get,
            path,
            query,
            headers: &[],
            body: &[],
            timeout: Duration::from_secs(1),
            max_response: 10,
        }
    }

    #[test]
    fn the_query_is_encoded_by_the_client_and_cannot_break_out() {
        assert_eq!(
            build_target(
                "/w/api.php",
                &[("search", "Köln & Bonn=?#"), ("limit", "5")]
            ),
            "/w/api.php?search=K%C3%B6ln%20%26%20Bonn%3D%3F%23&limit=5"
        );
        assert_eq!(build_target("/x", &[]), "/x");
        assert_eq!(build_target("/x", &[("a b", "c")]), "/x?a%20b=c");
    }

    #[test]
    fn a_get_reaches_the_transport_with_the_fixed_host_and_encoded_target() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(false);
        let fake = Fake::default();
        request_with(
            &fake,
            &get(
                Connector::Wikipedia,
                "de.wikipedia.org",
                "/w/rest.php/v1/search/page",
                &[("q", "Rom & Co"), ("limit", "3")],
            ),
        )
        .expect("Antwort");
        let (host, method, target, _, body) = fake.last.lock().unwrap().clone().expect("gesendet");
        assert_eq!(host, "de.wikipedia.org");
        assert_eq!(method, Method::Get);
        assert_eq!(
            target,
            "/w/rest.php/v1/search/page?q=Rom%20%26%20Co&limit=3"
        );
        assert!(body.is_empty());
    }

    #[test]
    fn hosts_outside_the_table_never_reach_the_transport() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(false);
        let fake = Fake::default();
        for (connector, host) in [
            (Connector::Wikipedia, "evil.example.org"),
            (Connector::Exa, "de.wikipedia.org"),
            (Connector::Brave, "api.exa.ai"),
            (Connector::OpenMeteo, "api.open-meteo.com.evil.example"),
        ] {
            assert!(
                matches!(
                    request_with(&fake, &get(connector, host, "/x", &[])),
                    Err(NetError::Blocked(_))
                ),
                "{connector:?} {host}"
            );
        }
        assert!(fake.last.lock().unwrap().is_none());
    }

    #[test]
    fn the_air_gap_blocks_before_the_transport_and_mail_is_never_https() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fake = Fake::default();
        set_air_gap(true);
        assert!(matches!(
            request_with(
                &fake,
                &get(
                    Connector::OpenMeteo,
                    "api.open-meteo.com",
                    "/v1/forecast",
                    &[]
                )
            ),
            Err(NetError::AirGap)
        ));
        set_air_gap(false);
        assert!(matches!(
            request_with(&fake, &get(Connector::Mail, "imap.gmail.com", "/", &[])),
            Err(NetError::Blocked(_))
        ));
        assert!(fake.last.lock().unwrap().is_none());
    }

    #[test]
    fn webdav_verbs_are_for_the_calendar_only() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(false);
        let fake = Fake::default();
        for connector_host in [
            (Connector::Wikipedia, "de.wikipedia.org"),
            (Connector::Exa, "api.exa.ai"),
            (Connector::Brave, "api.search.brave.com"),
            (Connector::OpenMeteo, "api.open-meteo.com"),
        ] {
            for method in [Method::Put, Method::Propfind, Method::Report] {
                let mut req = get(connector_host.0, connector_host.1, "/x", &[]);
                req.method = method;
                assert!(
                    matches!(request_with(&fake, &req), Err(NetError::Blocked(_))),
                    "{:?} {method:?}",
                    connector_host.0
                );
            }
        }
        assert!(fake.last.lock().unwrap().is_none());
        // Der Kalender darf sie, auch gegen eine iCloud-Server-Gruppe, und nur dort.
        for (method, verb) in [
            (Method::Propfind, "PROPFIND"),
            (Method::Report, "REPORT"),
            (Method::Put, "PUT"),
        ] {
            let mut req = get(
                Connector::Calendar,
                "p12-caldav.icloud.com",
                "/1/calendars/",
                &[],
            );
            req.method = method;
            req.body = b"<x/>";
            request_with(&fake, &req).expect("erlaubt");
            let (host, sent, target, _, body) =
                fake.last.lock().unwrap().clone().expect("gesendet");
            assert_eq!(
                (host.as_str(), sent, target.as_str()),
                ("p12-caldav.icloud.com", method, "/1/calendars/")
            );
            assert_eq!(body, b"<x/>");
            assert_eq!(method.verb(), verb);
        }
        let mut other_host = get(
            Connector::Calendar,
            "p12-caldav.icloud.com.evil.example",
            "/",
            &[],
        );
        other_host.method = Method::Propfind;
        assert!(matches!(
            request_with(&fake, &other_host),
            Err(NetError::Blocked(_))
        ));
    }

    #[test]
    fn the_calendar_is_blocked_by_the_air_gap_before_the_transport() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fake = Fake::default();
        set_air_gap(true);
        for method in [
            Method::Get,
            Method::Propfind,
            Method::Report,
            Method::Put,
            Method::Post,
        ] {
            let mut req = get(Connector::Calendar, "caldav.icloud.com", "/", &[]);
            req.method = method;
            assert!(
                matches!(request_with(&fake, &req), Err(NetError::AirGap)),
                "{method:?}"
            );
        }
        assert!(fake.last.lock().unwrap().is_none());
        set_air_gap(false);
    }

    #[test]
    fn a_get_with_a_body_and_smuggled_paths_are_rejected() {
        let _guard = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_air_gap(false);
        let fake = Fake::default();
        let mut with_body = get(
            Connector::Brave,
            "api.search.brave.com",
            "/res/v1/web/search",
            &[],
        );
        with_body.body = b"x";
        assert!(matches!(
            request_with(&fake, &with_body),
            Err(NetError::Blocked(_))
        ));
        for path in ["/x?y=1", "/x#f", "x", "/a b"] {
            assert!(matches!(
                request_with(
                    &fake,
                    &get(Connector::Brave, "api.search.brave.com", path, &[])
                ),
                Err(NetError::Blocked(_))
            ));
        }
        assert!(fake.last.lock().unwrap().is_none());
    }
}

#[cfg(all(test, windows))]
mod live_tests {
    use super::*;

    /// Echter TLS-Aufruf mit absichtlich ungültigem Schlüssel: belegt Handshake,
    /// Kopfzeilen und Statuslesen. Nur von Hand: `cargo test -- --ignored live`.
    #[test]
    #[ignore = "sendet eine Anfrage an api.exa.ai (ungültiger Schlüssel, kein Nutzerinhalt)"]
    fn live_request_with_invalid_key_is_answered_with_an_error_status() {
        set_air_gap(false);
        let response = post_json_to_exa(
            "/search",
            &[
                ("Content-Type", "application/json"),
                ("x-api-key", "ungueltig"),
            ],
            br#"{"query":"test","numResults":1}"#,
            Duration::from_secs(30),
            1024 * 1024,
        )
        .expect("HTTPS-Aufruf");
        assert!(
            matches!(response.status, 400..=403),
            "Status {}",
            response.status
        );
    }
}
