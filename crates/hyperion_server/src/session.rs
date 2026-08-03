//! Mojang session-server client used by online-mode logins.
//!
//! Implements the server side of the Yggdrasil handshake: `GET
//! /session/minecraft/hasJoined?username=..&serverId=..` and parsing of the
//! returned `GameProfile`. The base URL is configurable so tests can point
//! at a local mock server.

use std::sync::OnceLock;
use std::time::Duration;

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
            // A stuck session server must not hold a login task forever.
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
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
