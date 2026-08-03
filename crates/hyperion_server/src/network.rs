//! Network connections and protocol state machine (Phase 1).
//!
//! Handles the Handshake → Status (server list + ping) flow and the
//! Handshake → Login flow in both offline and online modes. Online mode
//! follows the vanilla authentication protocol: Encryption Request,
//! shared-secret exchange via RSA, AES/CFB8 activation, and Mojang
//! session-server verification.

use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use bytes::{Buf, BytesMut};
use hyperion_protocol::{
    compress_body, decode_encryption_response, decode_handshake, decode_login_acknowledged,
    decode_login_start, decode_packet_data, decode_ping_request, decode_status_request,
    decompress_body, decrypt_pkcs1v15, encode_encryption_request_payload,
    encode_login_disconnect_payload, encode_login_success_payload, encode_status_response_payload,
    encode_var_i32, generate_rsa_keypair, offline_mode_uuid, server_id_hash, split_frame,
    Cfb8Stream, EncryptionRequest, GameProfile, HandshakeIntent, LoginStart, LoginSuccess,
    PacketFrame, ProtocolError, StatusDescription, StatusPlayers, StatusResponse, StatusVersion,
    ENCRYPTION_REQUEST_PACKET_ID, LOGIN_DISCONNECT_PACKET_ID, LOGIN_SUCCESS_PACKET_ID,
    SET_COMPRESSION_PACKET_ID, SHARED_SECRET_LENGTH, SUPPORTED_PROTOCOL_VERSION,
    VERIFY_TOKEN_LENGTH,
};
use rand::rngs::OsRng;
use rand::RngCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::session::has_joined;

/// Clientbound Status and Login packet IDs used in this module. (The Login
/// state IDs come from `hyperion_protocol`.)
const STATUS_RESPONSE_PACKET_ID: i32 = 0;
const PONG_RESPONSE_PACKET_ID: i32 = 1;

/// Version name advertised in the server list.
const PROTOCOL_VERSION_NAME: &str = "26.2";
/// Maximum player count advertised.
const MAX_PLAYERS: i32 = 20;
/// Server list MOTD.
const SERVER_MOTD: &str = "A Hyperion server";

/// An error that terminates the current connection.
#[derive(Debug)]
enum ConnectionError {
    /// The client closed the connection.
    Disconnected,
    /// The network I/O failed.
    Io(io::Error),
    /// The received frame violates the protocol.
    Protocol(ProtocolError),
    /// Authentication failed (online mode).
    Auth(String),
}

impl From<io::Error> for ConnectionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ProtocolError> for ConnectionError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

/// A connection with its read buffer, compression threshold, and optional
/// session encryption ciphers.
struct Connection {
    stream: TcpStream,
    buffer: BytesMut,
    compression_threshold: Option<usize>,
    decrypt_cipher: Option<Cfb8Stream>,
    encrypt_cipher: Option<Cfb8Stream>,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        // Minecraft traffic is small packets: disable Nagle so responses are
        // not held back waiting for more data (vanilla does the same).
        let _ = stream.set_nodelay(true);
        Self {
            stream,
            buffer: BytesMut::with_capacity(256),
            compression_threshold: None,
            decrypt_cipher: None,
            encrypt_cipher: None,
        }
    }

    /// Reads the next frame, applying decryption and decompression as needed.
    async fn read_frame(&mut self) -> Result<PacketFrame, ConnectionError> {
        let body = loop {
            match split_frame(&self.buffer[..]) {
                Ok(Some((body, consumed_bytes))) => {
                    self.buffer.advance(consumed_bytes);
                    break body;
                }
                Ok(None) => {
                    if self.read_and_decrypt_chunk().await? == 0 {
                        return Err(ConnectionError::Disconnected);
                    }
                }
                Err(error) => return Err(ConnectionError::Protocol(error)),
            }
        };

        let packet_body = if self.compression_threshold.is_some() {
            decompress_body(&body).map_err(ConnectionError::Protocol)?
        } else {
            body
        };

        decode_packet_data(&packet_body).map_err(ConnectionError::Protocol)
    }

    /// Writes a packet (ID + payload) applying compression and encryption.
    async fn write_frame(&mut self, packet_id: i32, payload: &[u8]) -> Result<(), ConnectionError> {
        let packet_body = [encode_var_i32(packet_id), payload.to_vec()].concat();
        let frame_body = if let Some(threshold) = self.compression_threshold {
            compress_body(&packet_body, threshold).map_err(ConnectionError::Protocol)?
        } else {
            packet_body
        };

        let mut frame = encode_var_i32(frame_body.len() as i32);
        frame.extend_from_slice(&frame_body);
        if let Some(cipher) = &mut self.encrypt_cipher {
            cipher.encrypt(&mut frame);
        }

        self.stream.write_all(&frame).await?;
        Ok(())
    }

    /// Reads bytes from the socket, decrypts if needed, and appends to the buffer.
    async fn read_and_decrypt_chunk(&mut self) -> Result<usize, ConnectionError> {
        let mut chunk = [0u8; 4096];
        let bytes_read = self.stream.read(&mut chunk).await?;
        if bytes_read == 0 {
            return Ok(0);
        }
        if let Some(cipher) = &mut self.decrypt_cipher {
            cipher.decrypt(&mut chunk[..bytes_read]);
        }
        self.buffer.extend_from_slice(&chunk[..bytes_read]);
        Ok(bytes_read)
    }

    /// Enables AES/CFB8 encryption using the given shared secret as both
    /// key and IV. From this point every byte on the wire is encrypted.
    fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.encrypt_cipher = Some(Cfb8Stream::new(shared_secret));
        self.decrypt_cipher = Some(Cfb8Stream::new(shared_secret));
    }
}

/// Accepts connections on `config.bind_address` and dispatches each to its
/// own task.
pub async fn serve(config: ServerConfig) -> io::Result<()> {
    let listener = TcpListener::bind(&config.bind_address).await?;
    info!(
        bind_address = %config.bind_address,
        online_mode = config.online_mode,
        compression_threshold = config.compression_threshold,
        session_server = %config.session_server_url,
        "server listening"
    );

    loop {
        let (stream, peer_address) = listener.accept().await?;
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, peer_address, config).await {
                match error {
                    ConnectionError::Disconnected => {}
                    ConnectionError::Io(io_error) => {
                        warn!(%peer_address, %io_error, "connection I/O error");
                    }
                    ConnectionError::Protocol(protocol_error) => {
                        warn!(%peer_address, %protocol_error, "protocol violation");
                    }
                    ConnectionError::Auth(reason) => {
                        warn!(%peer_address, %reason, "authentication failed");
                    }
                }
            }
        });
    }
}

/// Handles a single connection: Handshake then the chosen intent flow.
async fn handle_connection(
    stream: TcpStream,
    peer_address: SocketAddr,
    config: ServerConfig,
) -> Result<(), ConnectionError> {
    let mut connection = Connection::new(stream);
    debug!(%peer_address, "connection accepted");

    let handshake_frame = connection.read_frame().await?;
    let handshake = decode_handshake(&handshake_frame)?;
    debug!(
        %peer_address,
        intent = ?handshake.intent,
        protocol_version = handshake.protocol_version,
        server_address = %handshake.server_address,
        "handshake received"
    );

    match handshake.intent {
        HandshakeIntent::Status => serve_status(&mut connection).await,
        HandshakeIntent::Login => serve_login(&mut connection, &config, peer_address).await,
        // Transfer arrives in a later milestone.
        HandshakeIntent::Transfer => {
            warn!(%peer_address, "transfer intent is not implemented yet");
            Ok(())
        }
    }
}

/// Handles the Status exchange: server list and ping/pong.
async fn serve_status(connection: &mut Connection) -> Result<(), ConnectionError> {
    loop {
        let frame = match connection.read_frame().await {
            Ok(frame) => frame,
            Err(ConnectionError::Disconnected) => return Ok(()),
            Err(error) => return Err(error),
        };
        match frame.packet_id {
            // Status Request (0): respond with the server list.
            0 => {
                decode_status_request(&frame)?;
                trace!("status request received");
                let payload = encode_status_response_payload(&default_status_response())?;
                connection
                    .write_frame(STATUS_RESPONSE_PACKET_ID, &payload)
                    .await?;
            }
            // Ping Request (1): Pong with the same payload.
            1 => {
                let ping = decode_ping_request(&frame)?;
                trace!(ping_payload = ping.payload, "ping request received");
                connection
                    .write_frame(PONG_RESPONSE_PACKET_ID, &ping.payload.to_be_bytes())
                    .await?;
            }
            _ => return Ok(()),
        }
    }
}

/// Handles the Login flow for both online and offline modes.
///
/// **Offline mode** (vanilla `online-mode=false`):
///   Login Start → Set Compression → Login Success → Login Acknowledged.
///   No encryption, no authentication. UUID derived from the username
///   (MD5 of `"OfflinePlayer:<name>"`, like vanilla).
///
/// **Online mode** (vanilla `online-mode=true`):
///   Login Start → Encryption Request → Encryption Response → validate
///   verify token → enable AES/CFB8 → call Mojang `hasJoined` →
///   Set Compression → Login Success → Login Acknowledged.
async fn serve_login(
    connection: &mut Connection,
    config: &ServerConfig,
    peer_address: SocketAddr,
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
/// 4. Decrypt both with RSA private key
/// 5. Validate verify token matches
/// 6. Enable AES/CFB8 encryption (both directions) — from here the wire is encrypted
/// 7. Compute server_hash = sha1("" + shared_secret + public_key)
/// 8. Call Mojang `hasJoined`; on failure send an encrypted Login Disconnect
/// 9. Send Set Compression + Login Success with Mojang's verified profile
/// 10. Receive Login Acknowledged (encrypted)
async fn serve_login_online(
    connection: &mut Connection,
    login_start: &LoginStart,
    config: &ServerConfig,
) -> Result<(), ConnectionError> {
    let username = login_start.username.clone();

    // Step 1: RSA keypair and random verify token (cryptographically secure).
    let keygen_started = Instant::now();
    let (public_key_der, private_key) =
        generate_rsa_keypair().map_err(|error| ConnectionError::Auth(error.to_string()))?;
    let mut verify_token = [0u8; VERIFY_TOKEN_LENGTH];
    OsRng.fill_bytes(&mut verify_token);
    debug!(
        username = %username,
        elapsed_ms = keygen_started.elapsed().as_millis() as u64,
        public_key_len = public_key_der.len(),
        "RSA keypair generated"
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
            // enabled encryption when it sent the Encryption Response).
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

/// The server list advertised by Hyperion.
fn default_status_response() -> StatusResponse {
    StatusResponse {
        version: StatusVersion {
            name: PROTOCOL_VERSION_NAME.to_owned(),
            protocol: SUPPORTED_PROTOCOL_VERSION,
        },
        players: StatusPlayers {
            max: MAX_PLAYERS,
            online: 0,
            sample: Vec::new(),
        },
        description: StatusDescription {
            text: SERVER_MOTD.to_owned(),
        },
        favicon: None,
        enforces_secure_chat: false,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use hyperion_protocol::{decode_var_i32, offline_mode_uuid};
    use rand::rngs::OsRng;
    use rand::RngCore;
    use rsa::pkcs8::DecodePublicKey;
    use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::session::test_support::{mock_session_server, PROFILE_JSON};

    fn encode_string(value: &str) -> Vec<u8> {
        [
            encode_var_i32(value.len() as i32),
            value.as_bytes().to_vec(),
        ]
        .concat()
    }

    fn handshake_payload(intent: i32) -> Vec<u8> {
        [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("localhost"),
            25565u16.to_be_bytes().to_vec(),
            encode_var_i32(intent),
        ]
        .concat()
    }

    /// Reads a length-prefixed string from a packet payload.
    fn read_string(data: &[u8], offset: &mut usize) -> String {
        let (length, next_offset) = decode_var_i32(data, *offset, 5).expect("length should decode");
        *offset = next_offset;
        let start = *offset;
        *offset += length as usize;
        String::from_utf8(data[start..*offset].to_vec()).expect("string should be UTF-8")
    }

    /// Reads a length-prefixed byte array from a packet payload.
    fn read_bytes(data: &[u8], offset: &mut usize) -> Vec<u8> {
        let (length, next_offset) = decode_var_i32(data, *offset, 5).expect("length should decode");
        *offset = next_offset;
        let start = *offset;
        *offset += length as usize;
        data[start..*offset].to_vec()
    }

    /// Parses an Encryption Request payload, returning (public_key, verify_token).
    fn parse_encryption_request(payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut offset = 0usize;
        let _server_id = read_string(payload, &mut offset);
        let public_key = read_bytes(payload, &mut offset);
        let verify_token = read_bytes(payload, &mut offset);
        (public_key, verify_token)
    }

    /// Parsed contents of a Login Success payload.
    type ProfileFields = (Uuid, String, Vec<(String, String, Option<String>)>);

    /// Parses a Login Success payload into its top-level fields.
    fn parse_login_success(payload: &[u8]) -> ProfileFields {
        let mut offset = 0usize;
        let uuid = Uuid::from_slice(&payload[offset..offset + 16]).expect("uuid should parse");
        offset += 16;
        let username = read_string(payload, &mut offset);
        let (property_count, after_count) =
            decode_var_i32(payload, offset, 5).expect("property count should decode");
        offset = after_count;
        let mut properties = Vec::new();
        for _ in 0..property_count as usize {
            let name = read_string(payload, &mut offset);
            let value = read_string(payload, &mut offset);
            let has_signature = payload[offset] == 1;
            offset += 1;
            let signature = if has_signature {
                Some(read_string(payload, &mut offset))
            } else {
                None
            };
            properties.push((name, value, signature));
        }
        (uuid, username, properties)
    }

    fn rsa_encrypt(public_key: &RsaPublicKey, data: &[u8]) -> Vec<u8> {
        public_key
            .encrypt(&mut OsRng, Pkcs1v15Encrypt, data)
            .expect("RSA encryption should work")
    }

    /// A synthetic Minecraft client that drives the server through a login:
    /// it speaks the real wire protocol, including AES/CFB8 encryption and
    /// zlib compression, and performs the RSA key exchange like the vanilla
    /// client does.
    struct MockClient {
        stream: TcpStream,
        read_buffer: BytesMut,
        compression_threshold: Option<usize>,
        encrypt_cipher: Option<Cfb8Stream>,
        decrypt_cipher: Option<Cfb8Stream>,
    }

    impl MockClient {
        async fn connect(server_address: SocketAddr) -> Self {
            let stream = TcpStream::connect(server_address)
                .await
                .expect("client should connect");
            Self {
                stream,
                read_buffer: BytesMut::new(),
                compression_threshold: None,
                encrypt_cipher: None,
                decrypt_cipher: None,
            }
        }

        /// Enables AES/CFB8 in both directions, like the vanilla client does
        /// right after sending its Encryption Response.
        fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
            self.encrypt_cipher = Some(Cfb8Stream::new(shared_secret));
            self.decrypt_cipher = Some(Cfb8Stream::new(shared_secret));
        }

        async fn write_packet(&mut self, packet_id: i32, payload: &[u8]) {
            let packet_body = [encode_var_i32(packet_id), payload.to_vec()].concat();
            let frame_body = match self.compression_threshold {
                Some(threshold) => compress_body(&packet_body, threshold).expect("should compress"),
                None => packet_body,
            };
            let mut frame = encode_var_i32(frame_body.len() as i32);
            frame.extend_from_slice(&frame_body);
            if let Some(cipher) = &mut self.encrypt_cipher {
                cipher.encrypt(&mut frame);
            }
            self.stream.write_all(&frame).await.expect("should write");
        }

        /// Reads the raw body of the next frame (before decompression).
        async fn read_raw_body(&mut self) -> Vec<u8> {
            loop {
                match split_frame(&self.read_buffer[..]).expect("frame should split") {
                    Some((body, consumed_bytes)) => {
                        self.read_buffer.advance(consumed_bytes);
                        return body;
                    }
                    None => {
                        let mut chunk = [0u8; 4096];
                        let bytes_read = self.stream.read(&mut chunk).await.expect("should read");
                        assert!(bytes_read > 0, "socket closed mid-frame");
                        if let Some(cipher) = &mut self.decrypt_cipher {
                            cipher.decrypt(&mut chunk[..bytes_read]);
                        }
                        self.read_buffer.extend_from_slice(&chunk[..bytes_read]);
                    }
                }
            }
        }

        /// Reads and decodes a packet, decrypting and decompressing as needed.
        async fn read_packet(&mut self) -> PacketFrame {
            let body = self.read_raw_body().await;
            let packet_body = if self.compression_threshold.is_some() {
                decompress_body(&body).expect("should decompress")
            } else {
                body
            };
            decode_packet_data(&packet_body).expect("should parse")
        }
    }

    #[tokio::test]
    async fn serves_status_and_echoes_ping() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let server_address = listener
            .local_addr()
            .expect("listener should have an address");

        let server_task = tokio::spawn(async move {
            let (stream, peer_address) = listener.accept().await.expect("client should connect");
            handle_connection(stream, peer_address, ServerConfig::default()).await
        });

        let mut client = MockClient::connect(server_address).await;

        // Handshake with intent Status (1).
        client.write_packet(0, &handshake_payload(1)).await;

        // Status Request.
        client.write_packet(0, &[]).await;

        // The server responds with the server list.
        let status_frame = client.read_packet().await;
        assert_eq!(status_frame.packet_id, STATUS_RESPONSE_PACKET_ID);
        let mut offset = 0usize;
        let response_json = read_string(&status_frame.payload, &mut offset);
        assert!(response_json.contains("\"enforcesSecureChat\":false"));
        assert!(response_json.contains(&format!("\"protocol\":{SUPPORTED_PROTOCOL_VERSION}")));
        assert!(response_json.contains("\"text\":\"A Hyperion server\""));

        // Ping Request → Pong Response with the same payload.
        let ping_payload = 12345i64.to_be_bytes().to_vec();
        client.write_packet(1, &ping_payload).await;
        let pong_frame = client.read_packet().await;
        assert_eq!(pong_frame.packet_id, PONG_RESPONSE_PACKET_ID);
        assert_eq!(pong_frame.payload, ping_payload);

        // When the client closes, the server terminates cleanly.
        drop(client);
        server_task
            .await
            .expect("server task should finish")
            .expect("connection should end cleanly");
    }

    #[tokio::test]
    async fn logs_in_offline_with_vanilla_uuid_and_compression() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let server_address = listener
            .local_addr()
            .expect("listener should have an address");

        // Low threshold to force Login Success to travel compressed.
        let config = ServerConfig {
            compression_threshold: 4,
            ..ServerConfig::default()
        };
        let server_task = tokio::spawn(async move {
            let (stream, peer_address) = listener.accept().await.expect("client should connect");
            handle_connection(stream, peer_address, config).await
        });

        let mut client = MockClient::connect(server_address).await;

        // Handshake with intent Login (2).
        client.write_packet(0, &handshake_payload(2)).await;

        // Login Start. The client-declared UUID is ignored by the server,
        // which assigns the vanilla offline UUID instead.
        let login_start_payload =
            [encode_string("TestPlayer"), Uuid::nil().as_bytes().to_vec()].concat();
        client.write_packet(0, &login_start_payload).await;

        // Set Compression (not yet compressing).
        let set_compression = client.read_packet().await;
        assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
        let (threshold, _) =
            decode_var_i32(&set_compression.payload, 0, 5).expect("threshold should decode");
        assert_eq!(threshold, 4);
        client.compression_threshold = Some(4usize);

        // Login Success: must arrive compressed (data_length > 0).
        let raw_success_body = client.read_raw_body().await;
        let (data_length, _) =
            decode_var_i32(&raw_success_body, 0, 5).expect("data length should decode");
        assert!(data_length > 0, "Login Success should be compressed");
        let login_success =
            decode_packet_data(&decompress_body(&raw_success_body).expect("should decompress"))
                .expect("should parse");
        assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);
        let expected_uuid = offline_mode_uuid("TestPlayer");
        assert_eq!(&login_success.payload[0..16], expected_uuid.as_bytes());
        assert_eq!(login_success.payload[16], 10); // "TestPlayer" length
        assert_eq!(&login_success.payload[17..27], "TestPlayer".as_bytes());
        assert_eq!(login_success.payload[27], 0); // no properties
        assert_eq!(login_success.payload.len(), 44); // uuid + name + count + session id

        // Login Acknowledged (compressed).
        client.write_packet(3, &[]).await;

        drop(client);
        server_task
            .await
            .expect("server task should finish")
            .expect("connection should end cleanly");
    }

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
        let login_start_payload =
            [encode_string("Notch"), Uuid::nil().as_bytes().to_vec()].concat();
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
                encode_var_i32(encrypted_secret.len() as i32),
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
                encode_var_i32(encrypted_secret.len() as i32),
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
                encode_var_i32(encrypted_secret.len() as i32),
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

    /// Usernames outside `[a-zA-Z0-9_]` are rejected at the protocol
    /// boundary with a disconnect message, like vanilla.
    #[tokio::test]
    async fn login_rejects_invalid_username_with_disconnect() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let server_address = listener
            .local_addr()
            .expect("listener should have an address");

        let server_task = tokio::spawn(async move {
            let (stream, peer_address) = listener.accept().await.expect("client should connect");
            handle_connection(stream, peer_address, ServerConfig::default()).await
        });

        let mut client = MockClient::connect(server_address).await;
        client.write_packet(0, &handshake_payload(2)).await;
        client
            .write_packet(
                0,
                &[encode_string("Bad-Name!"), Uuid::nil().as_bytes().to_vec()].concat(),
            )
            .await;

        let disconnect = client.read_packet().await;
        assert_eq!(disconnect.packet_id, LOGIN_DISCONNECT_PACKET_ID);
        let mut offset = 0usize;
        let reason = read_string(&disconnect.payload, &mut offset);
        assert!(reason.contains("Invalid characters in username"));

        assert!(matches!(
            server_task.await.expect("server task should finish"),
            Err(ConnectionError::Protocol(ProtocolError::InvalidUsername))
        ));
    }
}
