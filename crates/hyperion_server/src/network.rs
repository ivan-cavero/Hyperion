//! Network connections and protocol state machine (Phase 1).
//!
//! Handles the Handshake → Status (server list + ping) flow and the
//! Handshake → Login flow in both offline and online modes. Online mode
//! follows the vanilla authentication protocol: Encryption Request,
//! shared-secret exchange via RSA, AES/CFB8 activation, and Mojang
//! session-server verification.

use std::io;

use bytes::{Buf, BytesMut};
use hyperion_protocol::{
    compress_body, decrypt_pkcs1v15, decode_handshake, decode_login_acknowledged,
    decode_login_start, decode_packet_data, decode_ping_request, decode_status_request,
    decompress_body, encode_login_success_payload, encode_status_response_payload,
    encode_var_i32, generate_rsa_keypair, server_id_hash, split_frame, Cfb8Stream, GameProfile,
    HandshakeIntent, LoginSuccess, PacketFrame, ProtocolError, StatusDescription, StatusPlayers,
    StatusResponse, StatusVersion, SUPPORTED_PROTOCOL_VERSION,
};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

/// Default compression threshold (same as vanilla: 256).
const COMPRESSION_THRESHOLD: usize = 256;

/// Clientbound Status and Login packet IDs used here.
const STATUS_RESPONSE_PACKET_ID: i32 = 0;
const PONG_RESPONSE_PACKET_ID: i32 = 1;
const SET_COMPRESSION_PACKET_ID: i32 = 3;
const LOGIN_SUCCESS_PACKET_ID: i32 = 2;

/// Version name advertised in the server list.
const PROTOCOL_VERSION_NAME: &str = "26.2";
/// Maximum player count advertised.
const MAX_PLAYERS: i32 = 20;
/// Server list MOTD.
const SERVER_MOTD: &str = "A Hyperion server";

/// Mojang session server base URL.
const SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com";

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
    /// key and IV. Called after successful online-mode authentication.
    fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.encrypt_cipher = Some(Cfb8Stream::new(shared_secret));
        self.decrypt_cipher = Some(Cfb8Stream::new(shared_secret));
    }
}

/// Accepts connections on `bind_address` and dispatches each to its own task.
pub async fn serve(bind_address: &str, online_mode: bool) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address).await?;
    let mode_label = if online_mode { "online" } else { "offline" };
    println!("Listening on {bind_address} (mode: {mode_label})");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) =
                handle_connection(stream, COMPRESSION_THRESHOLD, online_mode).await
            {
                match error {
                    ConnectionError::Disconnected => {}
                    ConnectionError::Io(io_error) => eprintln!("Network error: {io_error}"),
                    ConnectionError::Protocol(protocol_error) => {
                        eprintln!("Protocol violation: {protocol_error}");
                    }
                    ConnectionError::Auth(reason) => {
                        eprintln!("Auth failed: {reason}");
                    }
                }
            }
        });
    }
}

/// Handles a single connection: Handshake then the chosen intent flow.
async fn handle_connection(
    stream: TcpStream,
    compression_threshold: usize,
    online_mode: bool,
) -> Result<(), ConnectionError> {
    let mut connection = Connection::new(stream);

    let handshake_frame = connection.read_frame().await?;
    let handshake = decode_handshake(&handshake_frame)?;

    match handshake.intent {
        HandshakeIntent::Status => serve_status(&mut connection).await,
        HandshakeIntent::Login => {
            serve_login(&mut connection, compression_threshold, online_mode).await
        }
        // Transfer arrives in a later milestone.
        HandshakeIntent::Transfer => Ok(()),
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
                let payload = encode_status_response_payload(&default_status_response())?;
                connection
                    .write_frame(STATUS_RESPONSE_PACKET_ID, &payload)
                    .await?;
            }
            // Ping Request (1): Pong with the same payload.
            1 => {
                let ping = decode_ping_request(&frame)?;
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
///   No encryption, no authentication. UUID derived from username.
///
/// **Online mode** (vanilla `online-mode=true`):
///   Login Start → Encryption Request → Encryption Response → validate
///   verify token → call Mojang `hasJoined` → enable AES/CFB8 →
///   Set Compression → Login Success → Login Acknowledged.
async fn serve_login(
    connection: &mut Connection,
    compression_threshold: usize,
    online_mode: bool,
) -> Result<(), ConnectionError> {
    // Step 1: Receive Login Start.
    let login_start_frame = connection.read_frame().await?;
    let login_start = decode_login_start(&login_start_frame)?;
    let username = login_start.username.clone();

    if online_mode {
        serve_login_online(connection, &login_start, compression_threshold).await?;
    } else {
        serve_login_offline(connection, &login_start, compression_threshold).await?;
    }

    println!("Player {username} connected");
    Ok(())
}

/// Offline-mode login: no encryption, no Mojang verification.
/// UUID is derived from the username (vanilla uses UUID v3 of
/// "OfflinePlayer:<name>").
async fn serve_login_offline(
    connection: &mut Connection,
    login_start: &hyperion_protocol::LoginStart,
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

    // Login Success (compressed). UUID from the client; if nil, generate random.
    // TODO: derive UUID v3 from "OfflinePlayer:<name>" like vanilla.
    let profile_uuid = if login_start.uuid.is_nil() {
        Uuid::new_v4()
    } else {
        login_start.uuid
    };
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

    Ok(())
}

/// Online-mode login: full vanilla authentication flow with encryption.
///
/// 1. Generate RSA keypair + random verify token
/// 2. Send Encryption Request (server_id="", public_key, verify_token, should_auth=true)
/// 3. Receive Encryption Response (RSA-encrypted shared_secret + verify_token)
/// 4. Decrypt both with RSA private key
/// 5. Validate verify token matches
/// 6. Compute server_hash = sha1("" + shared_secret + public_key)
/// 7. Call GET sessionserver.mojang.com/session/minecraft/hasJoined
/// 8. Enable AES/CFB8 encryption
/// 9. Send Set Compression + Login Success with Mojang's verified profile
/// 10. Receive Login Acknowledged (encrypted)
async fn serve_login_online(
    connection: &mut Connection,
    login_start: &hyperion_protocol::LoginStart,
    compression_threshold: usize,
) -> Result<(), ConnectionError> {
    let username = login_start.username.clone();

    // Step 1: Generate RSA keypair and random verify token.
    let (public_key_der, private_key) =
        generate_rsa_keypair().map_err(|e| ConnectionError::Auth(e.to_string()))?;
    let verify_token: [u8; 4] = rand::thread_rng().gen();

    // Step 2: Send Encryption Request (Login ID 1).
    // Build the payload manually since write_frame adds its own framing.
    let encryption_request_payload = [
        hyperion_protocol::encode_string("", 20)?, // server_id (always empty)
        hyperion_protocol::encode_bytes(&public_key_der, 512)?,
        hyperion_protocol::encode_bytes(&verify_token[..], 128)?,
        hyperion_protocol::encode_boolean(true), // should_authenticate
    ]
    .concat();
    connection
        .write_frame(1, &encryption_request_payload)
        .await?;

    // Step 3: Receive Encryption Response.
    let response_frame = connection.read_frame().await?;
    let encryption_response =
        hyperion_protocol::decode_encryption_response(&response_frame)
            .map_err(ConnectionError::Protocol)?;

    // Step 4: Decrypt shared secret and verify token with RSA.
    let decrypted_shared_secret =
        decrypt_pkcs1v15(&private_key, &encryption_response.shared_secret)
            .map_err(|e| ConnectionError::Auth(format!("shared secret decryption failed: {e}")))?;
    let decrypted_verify_token =
        decrypt_pkcs1v15(&private_key, &encryption_response.verify_token)
            .map_err(|e| ConnectionError::Auth(format!("verify token decryption failed: {e}")))?;

    // Step 5: Validate verify token.
    if decrypted_verify_token != verify_token {
        return Err(ConnectionError::Auth(
            "verify token does not match".to_owned(),
        ));
    }

    // Validate shared secret length.
    if decrypted_shared_secret.len() != 16 {
        return Err(ConnectionError::Auth(format!(
            "shared secret must be 16 bytes, got {}",
            decrypted_shared_secret.len()
        )));
    }

    let mut shared_secret = [0u8; 16];
    shared_secret.copy_from_slice(&decrypted_shared_secret);

    // Step 6: Compute server hash.
    let server_hash = server_id_hash("", &shared_secret, &public_key_der);

    // Step 7: Call Mojang hasJoined.
    let profile = has_joined(&username, &server_hash)
        .await
        .map_err(|e| ConnectionError::Auth(format!("Mojang session server: {e}")))?;

    println!("Authenticated {}: {}", username, profile.uuid);

    // Step 8: Enable AES/CFB8 encryption (both directions).
    connection.enable_encryption(&shared_secret);

    // Step 9: Set Compression (sent encrypted).
    connection
        .write_frame(
            SET_COMPRESSION_PACKET_ID,
            &encode_var_i32(compression_threshold as i32),
        )
        .await?;
    connection.compression_threshold = Some(compression_threshold);

    // Step 10: Login Success with Mojang-verified profile (sent encrypted).
    let success = LoginSuccess {
        profile,
        session_id: Uuid::new_v4(),
    };
    let success_payload = encode_login_success_payload(&success)?;
    connection
        .write_frame(LOGIN_SUCCESS_PACKET_ID, &success_payload)
        .await?;

    // Step 11: Login Acknowledged (encrypted from client).
    let acknowledged_frame = connection.read_frame().await?;
    decode_login_acknowledged(&acknowledged_frame)?;

    Ok(())
}

/// Calls Mojang's session server to verify the player's identity.
///
/// `GET sessionserver.mojang.com/session/minecraft/hasJoined?username=X&serverId=Y`
///
/// Returns the verified `GameProfile` (UUID, name, skin properties) on success,
/// or an error string on failure.
async fn has_joined(username: &str, server_hash: &str) -> Result<GameProfile, String> {
    let url = format!(
        "{SESSION_SERVER_URL}/session/minecraft/hasJoined?username={username}&serverId={server_hash}"
    );

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "authentication failed (HTTP {})",
            response.status().as_u16()
        ));
    }

    let body: HasJoinedResponse = response
        .json()
        .await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    Ok(GameProfile {
        uuid: Uuid::parse_str(&body.id)
            .map_err(|e| format!("invalid UUID in response: {e}"))?,
        username: body.name,
        properties: body
            .properties
            .into_iter()
            .map(|p| hyperion_protocol::GameProfileProperty {
                name: p.name,
                value: p.value,
                signature: p.signature,
            })
            .collect(),
    })
}

/// The JSON response from Mojang's `hasJoined` endpoint.
#[derive(serde::Deserialize)]
struct HasJoinedResponse {
    id: String,
    name: String,
    #[serde(default)]
    properties: Vec<HasJoinedProperty>,
}

#[derive(serde::Deserialize)]
struct HasJoinedProperty {
    name: String,
    value: String,
    #[serde(default)]
    signature: Option<String>,
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
    use hyperion_protocol::decode_var_i32;

    use super::*;

    fn encode_string(value: &str) -> Vec<u8> {
        [
            encode_var_i32(value.len() as i32),
            value.as_bytes().to_vec(),
        ]
        .concat()
    }

    async fn client_write_packet(
        client: &mut TcpStream,
        packet_id: i32,
        payload: &[u8],
        compression_threshold: Option<usize>,
    ) {
        let packet_body = [encode_var_i32(packet_id), payload.to_vec()].concat();
        let frame_body = match compression_threshold {
            Some(threshold) => compress_body(&packet_body, threshold).expect("should compress"),
            None => packet_body,
        };
        let mut frame = encode_var_i32(frame_body.len() as i32);
        frame.extend_from_slice(&frame_body);
        client.write_all(&frame).await.expect("should write");
    }

    /// Reads the raw body of a frame (without decompressing) from the socket.
    async fn client_read_body(client: &mut TcpStream, buffer: &mut BytesMut) -> Vec<u8> {
        loop {
            match split_frame(&buffer[..]).expect("frame should split") {
                Some((body, consumed_bytes)) => {
                    buffer.advance(consumed_bytes);
                    return body;
                }
                None => {
                    let bytes_read = client.read_buf(buffer).await.expect("should read");
                    assert!(bytes_read > 0, "socket closed mid-frame");
                }
            }
        }
    }

    /// Reads and decodes a packet, decompressing if the connection requires it.
    async fn client_read_packet(
        client: &mut TcpStream,
        buffer: &mut BytesMut,
        compression_threshold: Option<usize>,
    ) -> PacketFrame {
        let body = client_read_body(client, buffer).await;
        let packet_body = if compression_threshold.is_some() {
            decompress_body(&body).expect("should decompress")
        } else {
            body
        };
        decode_packet_data(&packet_body).expect("should parse")
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
            let (stream, _) = listener.accept().await.expect("client should connect");
            handle_connection(stream, COMPRESSION_THRESHOLD, false).await
        });

        let mut client = TcpStream::connect(server_address)
            .await
            .expect("client should connect");
        let mut buffer = BytesMut::new();

        // Handshake with intent Status (1).
        let handshake_payload = [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("localhost"),
            25565u16.to_be_bytes().to_vec(),
            encode_var_i32(1),
        ]
        .concat();
        client_write_packet(&mut client, 0, &handshake_payload, None).await;

        // Status Request.
        client_write_packet(&mut client, 0, &[], None).await;

        // The server responds with the server list.
        let status_frame = client_read_packet(&mut client, &mut buffer, None).await;
        assert_eq!(status_frame.packet_id, STATUS_RESPONSE_PACKET_ID);
        let (string_length, length_bytes) =
            decode_var_i32(&status_frame.payload, 0, 5).expect("string length should decode");
        let response_json = String::from_utf8(
            status_frame.payload[length_bytes..length_bytes + string_length as usize].to_vec(),
        )
        .expect("response JSON should be UTF-8");
        assert!(response_json.contains("\"enforcesSecureChat\":false"));
        assert!(response_json.contains(&format!("\"protocol\":{SUPPORTED_PROTOCOL_VERSION}")));
        assert!(response_json.contains("\"text\":\"A Hyperion server\""));

        // Ping Request → Pong Response with the same payload.
        let ping_payload = 12345i64.to_be_bytes().to_vec();
        client_write_packet(&mut client, 1, &ping_payload, None).await;
        let pong_frame = client_read_packet(&mut client, &mut buffer, None).await;
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
    async fn logs_in_offline_with_compression() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let server_address = listener
            .local_addr()
            .expect("listener should have an address");

        // Low threshold to force Login Success to travel compressed.
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("client should connect");
            handle_connection(stream, 4, false).await
        });

        let mut client = TcpStream::connect(server_address)
            .await
            .expect("client should connect");
        let mut buffer = BytesMut::new();

        // Handshake with intent Login (2).
        let handshake_payload = [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("localhost"),
            25565u16.to_be_bytes().to_vec(),
            encode_var_i32(2),
        ]
        .concat();
        client_write_packet(&mut client, 0, &handshake_payload, None).await;

        // Login Start.
        let uuid = Uuid::from_u128(0x11111111_22222222_33333333_44444444);
        let login_start_payload = [encode_string("TestPlayer"), uuid.as_bytes().to_vec()].concat();
        client_write_packet(&mut client, 0, &login_start_payload, None).await;

        // Set Compression (not yet compressing).
        let set_compression = client_read_packet(&mut client, &mut buffer, None).await;
        assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
        let (threshold, _) =
            decode_var_i32(&set_compression.payload, 0, 5).expect("threshold should decode");
        assert_eq!(threshold, 4);
        let client_compression = Some(4usize);

        // Login Success: must arrive compressed (data_length > 0).
        let raw_success_body = client_read_body(&mut client, &mut buffer).await;
        let (data_length, _) =
            decode_var_i32(&raw_success_body, 0, 5).expect("data length should decode");
        assert!(data_length > 0, "Login Success should be compressed");
        let login_success =
            decode_packet_data(&decompress_body(&raw_success_body).expect("should decompress"))
                .expect("should parse");
        assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);
        assert_eq!(&login_success.payload[0..16], uuid.as_bytes());
        assert_eq!(login_success.payload[16], 10); // "TestPlayer" length
        assert_eq!(&login_success.payload[17..27], "TestPlayer".as_bytes());
        assert_eq!(login_success.payload[27], 0); // no properties
        assert_eq!(login_success.payload.len(), 44); // uuid + name + count + session id

        // Login Acknowledged (compressed).
        client_write_packet(&mut client, 3, &[], client_compression).await;

        drop(client);
        server_task
            .await
            .expect("server task should finish")
            .expect("connection should end cleanly");
    }
}
