//! Mojang session-server client used by online-mode logins.
//!
//! Implements the server side of the Yggdrasil handshake: `GET
//! /session/minecraft/hasJoined?username=..&serverId=..` and parsing of the
//! returned `GameProfile`. The base URL is configurable so tests can point
//! at a local mock server.

use std::sync::OnceLock;

use hyperion_protocol::{GameProfile, GameProfileProperty};
use serde::Deserialize;
use tracing::{debug, trace};
use uuid::Uuid;

/// A shared HTTP client reused across logins: keeps the connection pool warm
/// and avoids a fresh TCP+TLS setup on every authentication.
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .build()
            .expect("HTTP client must build")
    })
}

/// Why an online-mode login could not be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The session server has no active join session for this player
    /// (HTTP 204 or 4xx): the client never logged in on its side.
    NotAuthenticated,
    /// The session server could not be reached or failed internally
    /// (network error or HTTP 5xx).
    Unavailable,
    /// The response could not be parsed or is missing required fields.
    InvalidResponse,
}

impl SessionError {
    /// Human-readable reason, matching the vanilla disconnect texts.
    pub fn reason(&self) -> &'static str {
        match self {
            SessionError::NotAuthenticated => "Failed to verify username!",
            SessionError::Unavailable => "Authentication servers are down",
            SessionError::InvalidResponse => "Failed to verify username!",
        }
    }
}

/// Asks the session server whether the player started a session for this
/// server hash. `username` is expected to already be validated
/// (`[a-zA-Z0-9_]`, enforced at `decode_login_start`), which makes the
/// query-string interpolation safe.
pub async fn has_joined(
    base_url: &str,
    username: &str,
    server_hash: &str,
) -> Result<GameProfile, SessionError> {
    let url = format!(
        "{base_url}/session/minecraft/hasJoined?username={username}&serverId={server_hash}"
    );
    trace!(session_url = %url, "querying session server");

    let response = http_client().get(&url).send().await.map_err(|error| {
        debug!(session_url = %url, %error, "session server unreachable");
        SessionError::Unavailable
    })?;

    let status = response.status();
    debug!(http_status = status.as_u16(), "session server responded");

    if status.as_u16() == 200 {
        let profile: HasJoinedProfile = response.json().await.map_err(|error| {
            debug!(%error, "session server response was not a valid profile");
            SessionError::InvalidResponse
        })?;
        return profile
            .into_game_profile()
            .ok_or(SessionError::InvalidResponse);
    }

    if status.as_u16() == 204 || status.is_client_error() {
        debug!("session server has no session for this player");
        Err(SessionError::NotAuthenticated)
    } else {
        Err(SessionError::Unavailable)
    }
}

/// The JSON payload of a successful `hasJoined` response.
#[derive(Debug, Deserialize)]
struct HasJoinedProfile {
    id: String,
    name: String,
    #[serde(default)]
    properties: Vec<HasJoinedProperty>,
}

#[derive(Debug, Deserialize)]
struct HasJoinedProperty {
    name: String,
    value: String,
    #[serde(default)]
    signature: Option<String>,
}

impl HasJoinedProfile {
    fn into_game_profile(self) -> Option<GameProfile> {
        Some(GameProfile {
            uuid: Uuid::parse_str(&self.id).ok()?,
            username: self.name,
            properties: self
                .properties
                .into_iter()
                .map(|property| GameProfileProperty {
                    name: property.name,
                    value: property.value,
                    signature: property.signature,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A one-shot mock session server used by the protocol tests in this
    //! crate: it answers every request with a canned status/body and records
    //! the request line so tests can assert on the URL.

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A valid `hasJoined` 200 response for the player "Notch".
    pub const PROFILE_JSON: &str = r#"{
        "id": "069a79f444e94726a5befca90e38aaf5",
        "name": "Notch",
        "properties": [
            {
                "name": "textures",
                "value": "eyJ0ZXh0dXJlcyI6e319",
                "signature": "c2ln"
            }
        ]
    }"#;

    /// Starts a mock session server. Returns its address and a receiver for
    /// the request line of each accepted request.
    pub async fn mock_session_server(
        status: u16,
        body: &'static str,
    ) -> (std::net::SocketAddr, tokio::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock session server should bind");
        let address = listener.local_addr().expect("mock session server address");
        let (request_tx, request_rx) = tokio::sync::mpsc::channel(4);
        let request_tx = std::sync::Arc::new(request_tx);

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let request_tx = std::sync::Arc::clone(&request_tx);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    loop {
                        let bytes_read = socket
                            .read(&mut chunk)
                            .await
                            .expect("session request should be readable");
                        if bytes_read == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..bytes_read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request_line = String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    let _ = request_tx.send(request_line).await;

                    let status_line = match status {
                        200 => "200 OK",
                        204 => "204 No Content",
                        _ => "503 Service Unavailable",
                    };
                    let response = if status == 204 {
                        format!(
                            "HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                    } else {
                        format!(
                            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("session response should be writable");
                });
            }
        });

        (address, request_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{mock_session_server, PROFILE_JSON};
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn has_joined_parses_verified_profile() {
        let (address, mut requests) = mock_session_server(200, PROFILE_JSON).await;

        let profile = has_joined(&format!("http://{address}"), "Notch", "abc123")
            .await
            .expect("profile should verify");

        assert_eq!(profile.username, "Notch");
        assert_eq!(
            profile.uuid,
            Uuid::parse_str("069a79f444e94726a5befca90e38aaf5").expect("uuid should parse")
        );
        assert_eq!(profile.properties.len(), 1);
        assert_eq!(profile.properties[0].name, "textures");
        assert_eq!(profile.properties[0].value, "eyJ0ZXh0dXJlcyI6e319");
        assert_eq!(profile.properties[0].signature.as_deref(), Some("c2ln"));

        let request_line = requests
            .recv()
            .await
            .expect("session server should be queried");
        assert!(request_line
            .starts_with("GET /session/minecraft/hasJoined?username=Notch&serverId=abc123 "));
    }

    #[tokio::test]
    async fn has_joined_rejects_missing_session() {
        let (address, _) = mock_session_server(204, "").await;

        assert_eq!(
            has_joined(&format!("http://{address}"), "Notch", "abc123").await,
            Err(SessionError::NotAuthenticated)
        );
    }

    #[tokio::test]
    async fn has_joined_reports_server_failure() {
        let (address, _) = mock_session_server(503, "{}").await;

        assert_eq!(
            has_joined(&format!("http://{address}"), "Notch", "abc123").await,
            Err(SessionError::Unavailable)
        );
    }

    #[tokio::test]
    async fn has_joined_reports_unreachable_server() {
        // Bind and drop so the port is guaranteed closed.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should have an address");
        drop(listener);

        assert_eq!(
            has_joined(&format!("http://{address}"), "Notch", "abc123").await,
            Err(SessionError::Unavailable)
        );
    }

    #[tokio::test]
    async fn has_joined_rejects_malformed_profile() {
        let (address, _) = mock_session_server(200, r#"{ "id": "not-a-uuid", "name": 5 }"#).await;

        assert_eq!(
            has_joined(&format!("http://{address}"), "Notch", "abc123").await,
            Err(SessionError::InvalidResponse)
        );
    }
}
