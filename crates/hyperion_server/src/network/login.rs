//! Login flows for both online and offline modes.
//!
//! **Offline mode** (vanilla `online-mode=false`):
//!   Login Start → Set Compression → Login Success → Login Acknowledged.
//!   No encryption, no authentication. UUID derived from the username
//!   (MD5 of `"OfflinePlayer:<name>"`, like vanilla).
//!
//! **Online mode** (vanilla `online-mode=true`):
//!   Login Start → Encryption Request → Encryption Response → validate
//!   verify token → enable AES/CFB8 → call Mojang `hasJoined` →
//!   Set Compression → Login Success → Login Acknowledged.

use std::time::Instant;

use hyperion_protocol::{
    ENCRYPTION_REQUEST_PACKET_ID, EncryptionRequest, GameProfile, LOGIN_DISCONNECT_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, LoginStart, LoginSuccess, ProtocolError, SET_COMPRESSION_PACKET_ID,
    SHARED_SECRET_LENGTH, VERIFY_TOKEN_LENGTH, decode_encryption_response,
    decode_login_acknowledged, decode_login_start, decrypt_pkcs1v15,
    encode_encryption_request_payload, encode_login_disconnect_payload,
    encode_login_success_payload, encode_var_i32, generate_rsa_keypair, offline_mode_uuid,
    server_id_hash,
};
use rand::RngCore;
use rand::rngs::OsRng;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use super::connection::{Connection, ConnectionError};
use crate::config::ServerConfig;
use crate::session::has_joined;

/// Handles the Login flow for both online and offline modes.
pub(super) async fn serve_login(
    connection: &mut Connection,
    config: &ServerConfig,
    peer_address: std::net::SocketAddr,
) -> Result<(), ConnectionError> {
    let login_start_frame = connection.read_frame().await?;
    let login_start = match decode_login_start(&login_start_frame) {
        Ok(login_start) => login_start,
        Err(ProtocolError::InvalidUsername) => {
            // Vanilla rejects bad usernames with a disconnect message.
            warn!(%peer_address, "login rejected: invalid characters in username");
            connection
                .write_frame(
                    LOGIN_DISCONNECT_PACKET_ID,
                    &encode_login_disconnect_payload("Invalid characters in username")?,
                )
                .await?;
            return Err(ConnectionError::Protocol(ProtocolError::InvalidUsername));
        }
        Err(error) => return Err(ConnectionError::Protocol(error)),
    };
    let username = login_start.username.clone();
    info!(
        %peer_address,
        %username,
        online_mode = config.online_mode,
        "login started"
    );

    let result = if config.online_mode {
        serve_login_online(connection, &login_start, config).await
    } else {
        serve_login_offline(connection, &login_start, config.compression_threshold).await
    };

    if result.is_ok() {
        info!(%peer_address, %username, "player connected");
    }
    result
}

/// Offline-mode login: no encryption, no Mojang verification. The profile
/// UUID is derived from the username exactly like vanilla
/// (`MD5("OfflinePlayer:" + name)`, version 3).
async fn serve_login_offline(
    connection: &mut Connection,
    login_start: &LoginStart,
    compression_threshold: usize,
) -> Result<(), ConnectionError> {
    // Set Compression (not yet compressing) then enable compression.
    connection
        .write_frame(
            SET_COMPRESSION_PACKET_ID,
            &encode_var_i32(compression_threshold as i32),
        )
        .await?;
    connection.compression_threshold = Some(compression_threshold);

    // Login Success (compressed) with the vanilla offline UUID.
    let profile_uuid = offline_mode_uuid(&login_start.username);
    debug!(
        username = %login_start.username,
        %profile_uuid,
        "offline profile assigned"
    );
    let success = LoginSuccess {
        profile: GameProfile {
            uuid: profile_uuid,
            username: login_start.username.clone(),
            properties: Vec::new(),
        },
        session_id: Uuid::new_v4(),
    };
    let success_payload = encode_login_success_payload(&success)?;
    connection
        .write_frame(LOGIN_SUCCESS_PACKET_ID, &success_payload)
        .await?;

    // Login Acknowledged: the client transitions to Configuration.
    let acknowledged_frame = connection.read_frame().await?;
    decode_login_acknowledged(&acknowledged_frame)?;
    trace!("login acknowledged");

    Ok(())
}

/// Online-mode login: full vanilla authentication flow with encryption.
///
/// 1. Generate RSA keypair + random verify token
/// 2. Send Encryption Request (server_id="", public_key, verify_token, should_auth=true)
/// 3. Receive Encryption Response (RSA-encrypted shared_secret + verify_token)
/// 4. Decrypt both with the RSA private key
/// 5. Validate verify token matches
/// 6. Enable AES/CFB8 encryption (both directions) — from here the wire is encrypted
/// 7. Compute server_hash = sha1("" + shared_secret + public_key)
/// 8. Call the session server; on failure send an encrypted Login Disconnect
/// 9. Send Set Compression + Login Success with the verified profile
/// 10. Receive Login Acknowledged (encrypted)
async fn serve_login_online(
    connection: &mut Connection,
    login_start: &LoginStart,
    config: &ServerConfig,
) -> Result<(), ConnectionError> {
    let username = login_start.username.clone();

    // Step 1: RSA key pair and random verify token (cryptographically secure).
    // Key generation is CPU-bound (≈100–300 ms in release), so it runs on a
    // dedicated blocking thread instead of occupying one of tokio's limited
    // async worker threads. `spawn_blocking` returns the inner `Result`, so
    // both failure layers are mapped here.
    let keygen_started = Instant::now();
    let (public_key_der, private_key) = tokio::task::spawn_blocking(generate_rsa_keypair)
        .await
        .map_err(|join_error| ConnectionError::Auth(join_error.to_string()))?
        .map_err(|error| ConnectionError::Auth(error.to_string()))?;
    let mut verify_token = [0u8; VERIFY_TOKEN_LENGTH];
    OsRng.fill_bytes(&mut verify_token);
    debug!(
        username = %username,
        elapsed_ms = keygen_started.elapsed().as_millis() as u64,
        public_key_len = public_key_der.len(),
        "RSA key pair generated"
    );

    // Step 2: Send Encryption Request.
    let encryption_request_payload = encode_encryption_request_payload(&EncryptionRequest {
        server_id: String::new(), // always empty in modern servers
        public_key: public_key_der.clone(),
        verify_token: verify_token.to_vec(),
        should_authenticate: true,
    })?;
    connection
        .write_frame(ENCRYPTION_REQUEST_PACKET_ID, &encryption_request_payload)
        .await?;
    trace!(
        username = %username,
        verify_token = ?verify_token,
        "encryption request sent"
    );

    // Step 3: Receive Encryption Response.
    let response_frame = connection.read_frame().await?;
    let encryption_response = decode_encryption_response(&response_frame)?;
    debug!(
        username = %username,
        shared_secret_len = encryption_response.shared_secret.len(),
        verify_token_len = encryption_response.verify_token.len(),
        "encryption response received"
    );

    // Step 4: Decrypt shared secret and verify token with RSA.
    let decrypted_shared_secret =
        decrypt_pkcs1v15(&private_key, &encryption_response.shared_secret).map_err(|error| {
            ConnectionError::Auth(format!("shared secret decryption failed: {error}"))
        })?;
    let decrypted_verify_token = decrypt_pkcs1v15(&private_key, &encryption_response.verify_token)
        .map_err(|error| {
            ConnectionError::Auth(format!("verify token decryption failed: {error}"))
        })?;

    // Step 5: Validate the verify token and the shared secret length.
    // Vanilla closes the connection without a message on a nonce mismatch.
    if decrypted_verify_token != verify_token {
        warn!(username = %username, "verify token mismatch; closing connection");
        return Err(ConnectionError::Auth(
            "verify token does not match".to_owned(),
        ));
    }
    if decrypted_shared_secret.len() != SHARED_SECRET_LENGTH {
        warn!(
            username = %username,
            secret_len = decrypted_shared_secret.len(),
            "shared secret has an invalid length; closing connection"
        );
        return Err(ConnectionError::Auth(format!(
            "shared secret must be {SHARED_SECRET_LENGTH} bytes, got {}",
            decrypted_shared_secret.len()
        )));
    }
    let mut shared_secret = [0u8; SHARED_SECRET_LENGTH];
    shared_secret.copy_from_slice(&decrypted_shared_secret);

    // Step 6: Enable AES/CFB8 encryption (both directions). From this point
    // everything on the wire is encrypted, including disconnect packets.
    connection.enable_encryption(&shared_secret);
    info!(username = %username, "session encryption enabled (AES-128/CFB8)");

    // Step 7: Compute the server hash and verify with Mojang.
    let server_hash = server_id_hash("", &shared_secret, &public_key_der);
    debug!(
        username = %username,
        server_hash = %server_hash,
        session_server = %config.session_server_url,
        "requesting session verification"
    );
    let profile = match has_joined(&config.session_server_url, &username, &server_hash).await {
        Ok(profile) => profile,
        Err(error) => {
            let reason = error.reason();
            warn!(username = %username, %reason, "session verification failed");
            // The disconnect is encrypted, exactly like vanilla (the client
            // enabled encryption when it sent its Encryption Response).
            connection
                .write_frame(
                    LOGIN_DISCONNECT_PACKET_ID,
                    &encode_login_disconnect_payload(reason)?,
                )
                .await?;
            return Err(ConnectionError::Auth(reason.to_owned()));
        }
    };
    info!(
        username = %username,
        uuid = %profile.uuid,
        properties = profile.properties.len(),
        "session verified with Mojang"
    );

    // Step 8: Set Compression — encrypted, but not itself compressed.
    connection
        .write_frame(
            SET_COMPRESSION_PACKET_ID,
            &encode_var_i32(config.compression_threshold as i32),
        )
        .await?;
    connection.compression_threshold = Some(config.compression_threshold);

    // Step 9: Login Success with the Mojang-verified profile (encrypted + compressed).
    let success = LoginSuccess {
        profile,
        session_id: Uuid::new_v4(),
    };
    let success_payload = encode_login_success_payload(&success)?;
    connection
        .write_frame(LOGIN_SUCCESS_PACKET_ID, &success_payload)
        .await?;

    // Step 10: Login Acknowledged (encrypted from the client).
    let acknowledged_frame = connection.read_frame().await?;
    decode_login_acknowledged(&acknowledged_frame)?;
    trace!(username = %username, "login acknowledged");

    Ok(())
}
