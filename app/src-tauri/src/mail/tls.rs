//! Verschlüsselte Verbindung zu Gmail.
//!
//! Unter Windows übernimmt SChannel (in Windows eingebaut) die Verschlüsselung und prüft Kette
//! und Hostnamen; die Crate `schannel` bringt dafür keinen eigenen Kryptocode mit. Auf anderen
//! Systemen gibt es bewusst noch keinen Client (Phase 4), genau wie beim HTTPS-Client in `net`.

use std::time::Duration;

use super::{endpoint_allowed, MailError, MailStream, MailTransport};

/// Der Transport des Systems.
pub struct SystemMailTransport;

#[cfg(windows)]
impl MailTransport for SystemMailTransport {
    fn connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Box<dyn MailStream>, MailError> {
        use std::net::{TcpStream, ToSocketAddrs};

        use schannel::schannel_cred::{Direction, SchannelCred};
        use schannel::tls_stream::Builder;

        if !endpoint_allowed(host, port) {
            return Err(MailError::Blocked(format!(
                "Das Ziel {host}:{port} ist für Mail nicht erlaubt"
            )));
        }
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|error| MailError::Connect(error.to_string()))?
            .next()
            .ok_or_else(|| MailError::Connect("Adresse nicht gefunden".to_owned()))?;
        let tcp = TcpStream::connect_timeout(&address, timeout)
            .map_err(|error| MailError::Connect(error.to_string()))?;
        tcp.set_read_timeout(Some(timeout))?;
        tcp.set_write_timeout(Some(timeout))?;
        let credentials = SchannelCred::builder()
            .acquire(Direction::Outbound)
            .map_err(|error| MailError::Connect(error.to_string()))?;
        // Kette und Hostname werden geprüft (accept_invalid_hostnames ist standardmäßig aus).
        let stream = Builder::new()
            .domain(host)
            .connect(credentials, tcp)
            .map_err(|error| MailError::Connect(error.to_string()))?;
        Ok(Box::new(stream))
    }
}

#[cfg(not(windows))]
impl MailTransport for SystemMailTransport {
    fn connect(
        &self,
        host: &str,
        port: u16,
        _timeout: Duration,
    ) -> Result<Box<dyn MailStream>, MailError> {
        if !endpoint_allowed(host, port) {
            return Err(MailError::Blocked(format!(
                "Das Ziel {host}:{port} ist für Mail nicht erlaubt"
            )));
        }
        Err(MailError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_targets_are_refused_before_any_connection() {
        let transport = SystemMailTransport;
        let result = transport.connect("mail.example.com", 993, Duration::from_secs(1));
        assert!(matches!(result, Err(MailError::Blocked(_))));
    }
}
