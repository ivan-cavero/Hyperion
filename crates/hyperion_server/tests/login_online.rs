//! Online-mode login flow: real RSA key exchange, real AES/CFB8 on the wire,
//! and a mock Mojang session server — over a real TCP socket.

mod common;

use hyperion_protocol::{
    ENCRYPTION_REQUEST_PACKET_ID, LOGIN_DISCONNECT_PACKET_ID, LOGIN_SUCCESS_PACKET_ID,
    SET_COMPRESSION_PACKET_ID, decode_var_i32, encode_var_i32, server_id_hash,
};
use rand::RngCore;
use rand::rngs::OsRng;
use rsa::RsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use uuid::Uuid;

use common::{
    MockClient, PROFILE_JSON, encode_string, handshake_payload, mock_session_server,
    parse_encryption_request, parse_login_success, read_string, rsa_encrypt,
};
use hyperion_server::config::ServerConfig;
use hyperion_server::network::{ConnectionError, handle_connection};

/// Full online-mode login: real RSA key exchange, real AES/CFB8 on the
/// wire, and a mock Mojang session server verifying the player.
#[tokio::test]
async fn logs_in_online_with_encryption_and_session_verification() {
    let (session_address, mut requests) = mock_session_server(200, PROFILE_JSON).await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let config = ServerConfig {
        online_mode: true,
        session_server_url: format!("http://{session_address}"),
        ..ServerConfig::default()
    };
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config).await
    });

    let mut client = MockClient::connect(server_address).await;

    // Handshake with intent Login (2).
    client.write_packet(0, &handshake_payload(2)).await;

    // Login Start with the profile name the mock session server knows.
    let login_start_payload = [encode_string("Notch"), Uuid::nil().as_bytes().to_vec()].concat();
    client.write_packet(0, &login_start_payload).await;

    // Encryption Request arrives in plaintext.
    let request = client.read_packet().await;
    assert_eq!(request.packet_id, ENCRYPTION_REQUEST_PACKET_ID);
    let (public_key_der, verify_token) = parse_encryption_request(&request.payload);
    let public_key = RsaPublicKey::from_public_key_der(&public_key_der)
        .expect("public key should parse from DER");

    // Client-side key exchange, exactly like vanilla: random 16-byte
    // secret, both secret and token RSA-encrypted with the server key.
    let mut shared_secret = [0u8; 16];
    OsRng.fill_bytes(&mut shared_secret);
    let response_payload = {
        let encrypted_secret = rsa_encrypt(&public_key, &shared_secret);
        let encrypted_token = rsa_encrypt(&public_key, &verify_token);
        [
            hyperion_protocol::encode_var_i32(encrypted_secret.len() as i32),
            encrypted_secret,
            encode_var_i32(encrypted_token.len() as i32),
            encrypted_token,
        ]
        .concat()
    };
    client.write_packet(1, &response_payload).await;
    client.enable_encryption(&shared_secret);

    // The server must query the session server with our server hash.
    let request_line = requests
        .recv()
        .await
        .expect("session server should be queried");
    let expected_hash = server_id_hash("", &shared_secret, &public_key_der);
    assert!(request_line.contains(&format!("username=Notch&serverId={expected_hash}")));

    // Set Compression arrives encrypted but uncompressed.
    let set_compression = client.read_packet().await;
    assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
    let (threshold, _) =
        decode_var_i32(&set_compression.payload, 0, 5).expect("threshold should decode");
    assert_eq!(threshold, 256);
    client.compression_threshold = Some(threshold as usize);

    // Login Success arrives encrypted and compressed, carrying the
    // Mojang-verified profile (UUID, canonical name, skin properties).
    let login_success = client.read_packet().await;
    assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);
    let (profile_uuid, username, properties) = parse_login_success(&login_success.payload);
    assert_eq!(
        profile_uuid,
        Uuid::parse_str("069a79f444e94726a5befca90e38aaf5").expect("uuid should parse")
    );
    assert_eq!(username, "Notch");
    assert_eq!(
        properties,
        vec![(
            "textures".to_owned(),
            "eyJ0ZXh0dXJlcyI6e319".to_owned(),
            Some("c2ln".to_owned()),
        )]
    );
    // 16 (uuid) + 6 (name) + 1 (count) + 36 (property) + 16 (session id).
    assert_eq!(login_success.payload.len(), 75);

    // Login Acknowledged (encrypted + compressed).
    client.write_packet(3, &[]).await;

    drop(client);
    server_task
        .await
        .expect("server task should finish")
        .expect("connection should end cleanly");
}

/// When the session server has no session for the player (204), the
/// server must send an encrypted Login Disconnect with the vanilla
/// failure message.
#[tokio::test]
async fn online_login_disconnects_unverified_player() {
    let (session_address, _) = mock_session_server(204, "").await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let config = ServerConfig {
        online_mode: true,
        session_server_url: format!("http://{session_address}"),
        ..ServerConfig::default()
    };
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config).await
    });

    let mut client = MockClient::connect(server_address).await;
    client.write_packet(0, &handshake_payload(2)).await;
    client
        .write_packet(
            0,
            &[encode_string("Notch"), Uuid::nil().as_bytes().to_vec()].concat(),
        )
        .await;

    let request = client.read_packet().await;
    assert_eq!(request.packet_id, ENCRYPTION_REQUEST_PACKET_ID);
    let (public_key_der, verify_token) = parse_encryption_request(&request.payload);
    let public_key = RsaPublicKey::from_public_key_der(&public_key_der)
        .expect("public key should parse from DER");
    let mut shared_secret = [0u8; 16];
    OsRng.fill_bytes(&mut shared_secret);
    let response_payload = {
        let encrypted_secret = rsa_encrypt(&public_key, &shared_secret);
        let encrypted_token = rsa_encrypt(&public_key, &verify_token);
        [
            hyperion_protocol::encode_var_i32(encrypted_secret.len() as i32),
            encrypted_secret,
            encode_var_i32(encrypted_token.len() as i32),
            encrypted_token,
        ]
        .concat()
    };
    client.write_packet(1, &response_payload).await;
    client.enable_encryption(&shared_secret);

    // The disconnect is encrypted (the client enabled encryption when it
    // sent the Encryption Response, and so did the server).
    let disconnect = client.read_packet().await;
    assert_eq!(disconnect.packet_id, LOGIN_DISCONNECT_PACKET_ID);
    let mut offset = 0usize;
    let reason = read_string(&disconnect.payload, &mut offset);
    assert!(
        reason.contains("Failed to verify username!"),
        "unexpected reason: {reason}"
    );

    assert!(matches!(
        server_task.await.expect("server task should finish"),
        Err(ConnectionError::Auth(_))
    ));
}

/// On a verify-token mismatch the connection is closed without a
/// disconnect message, exactly like vanilla.
#[tokio::test]
async fn online_login_closes_on_verify_token_mismatch() {
    let (session_address, _) = mock_session_server(200, PROFILE_JSON).await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let config = ServerConfig {
        online_mode: true,
        session_server_url: format!("http://{session_address}"),
        ..ServerConfig::default()
    };
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config).await
    });

    let mut client = MockClient::connect(server_address).await;
    client.write_packet(0, &handshake_payload(2)).await;
    client
        .write_packet(
            0,
            &[encode_string("Notch"), Uuid::nil().as_bytes().to_vec()].concat(),
        )
        .await;

    let request = client.read_packet().await;
    let (public_key_der, mut verify_token) = parse_encryption_request(&request.payload);
    let public_key = RsaPublicKey::from_public_key_der(&public_key_der)
        .expect("public key should parse from DER");
    let mut shared_secret = [0u8; 16];
    OsRng.fill_bytes(&mut shared_secret);

    // Tamper with the token: the server must reject it.
    verify_token[0] ^= 0xff;
    let response_payload = {
        let encrypted_secret = rsa_encrypt(&public_key, &shared_secret);
        let encrypted_token = rsa_encrypt(&public_key, &verify_token);
        [
            hyperion_protocol::encode_var_i32(encrypted_secret.len() as i32),
            encrypted_secret,
            encode_var_i32(encrypted_token.len() as i32),
            encrypted_token,
        ]
        .concat()
    };
    client.write_packet(1, &response_payload).await;

    // Vanilla closes without sending anything: the socket hits EOF.
    let mut byte = [0u8; 1];
    let bytes_read = client
        .stream
        .read(&mut byte)
        .await
        .expect("read should not error");
    assert_eq!(bytes_read, 0, "server should close without a disconnect");

    assert!(matches!(
        server_task.await.expect("server task should finish"),
        Err(ConnectionError::Auth(_))
    ));
}
