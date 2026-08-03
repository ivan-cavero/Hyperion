//! Mojang session-server client (`has_joined`) against a mock HTTP server.

mod common;

use tokio::net::TcpListener;
use uuid::Uuid;

use common::{PROFILE_JSON, mock_session_server};
use hyperion_server::session::{SessionError, has_joined};

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
    assert!(
        request_line
            .starts_with("GET /session/minecraft/hasJoined?username=Notch&serverId=abc123 ")
    );
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
