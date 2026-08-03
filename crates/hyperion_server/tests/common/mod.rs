//! Shared test infrastructure for the `hyperion_server` integration tests.
//!
//! This module is not compiled as a test itself (`tests/common/mod.rs` is a
//! Cargo convention for shared helpers): each integration test file declares
//! `mod common;` and uses anything defined here.
//!
//! Because every integration-test binary compiles this module but only uses a
//! subset of its helpers, allow `dead_code` per-binary.

#![allow(dead_code)]

use bytes::{Buf, Bytes, BytesMut};
use hyperion_protocol::{
    ACCEPT_CODE_OF_CONDUCT_PACKET_ID, ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID, Cfb8Stream,
    KNOWN_PACKS_PACKET_ID, LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, PLAYER_POSITION_PACKET_ID, PacketFrame, REGISTRY_DATA_PACKET_ID,
    SET_COMPRESSION_PACKET_ID, SUPPORTED_PROTOCOL_VERSION, UPDATE_TAGS_PACKET_ID, compress_body,
    decode_packet_data, decode_var_i32, decompress_body, encode_var_i32, split_frame,
};
use rand::rngs::OsRng;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

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

/// Encodes a length-prefixed string, as the vanilla client would send it.
pub fn encode_string(value: &str) -> Vec<u8> {
    [
        encode_var_i32(value.len() as i32),
        value.as_bytes().to_vec(),
    ]
    .concat()
}

/// Builds a Handshake payload (protocol version + address + port + intent).
pub fn handshake_payload(intent: i32) -> Vec<u8> {
    [
        encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
        encode_string("localhost"),
        25565u16.to_be_bytes().to_vec(),
        encode_var_i32(intent),
    ]
    .concat()
}

/// Reads a length-prefixed string from a packet payload.
pub fn read_string(data: &[u8], offset: &mut usize) -> String {
    let (length, next_offset) = decode_var_i32(data, *offset, 5).expect("length should decode");
    *offset = next_offset;
    let start = *offset;
    *offset += length as usize;
    String::from_utf8(data[start..*offset].to_vec()).expect("string should be UTF-8")
}

/// Reads a length-prefixed byte array from a packet payload.
pub fn read_bytes(data: &[u8], offset: &mut usize) -> Vec<u8> {
    let (length, next_offset) = decode_var_i32(data, *offset, 5).expect("length should decode");
    *offset = next_offset;
    let start = *offset;
    *offset += length as usize;
    data[start..*offset].to_vec()
}

/// Parses an Encryption Request payload, returning (public key, verify token).
pub fn parse_encryption_request(payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut offset = 0usize;
    let _server_id = read_string(payload, &mut offset);
    let public_key = read_bytes(payload, &mut offset);
    let verify_token = read_bytes(payload, &mut offset);
    (public_key, verify_token)
}

/// Parsed contents of a Login Success payload.
pub type ProfileFields = (Uuid, String, Vec<(String, String, Option<String>)>);

/// Parses a Login Success payload into its top-level fields.
pub fn parse_login_success(payload: &[u8]) -> ProfileFields {
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

/// Encrypts a message with RSA PKCS#1 v1.5, like the vanilla client does
/// when answering an Encryption Request.
pub fn rsa_encrypt(public_key: &RsaPublicKey, data: &[u8]) -> Vec<u8> {
    public_key
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, data)
        .expect("RSA encryption should work")
}

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

/// Drives a client through the full offline-mode login into Play, consuming
/// the Configuration handshake and the spawn sequence. Returns the client
/// ready for further exchanges.
pub async fn log_into_play(server_address: std::net::SocketAddr) -> MockClient {
    let mut client = MockClient::connect(server_address).await;

    client.write_packet(0, &handshake_payload(2)).await;
    let login_start_payload =
        [encode_string("TestPlayer"), Uuid::nil().as_bytes().to_vec()].concat();
    client.write_packet(0, &login_start_payload).await;

    let set_compression = client.read_packet().await;
    assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
    client.compression_threshold = Some(256usize);

    let login_success = client.read_packet().await;
    assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);

    // --- Configuration state ---
    client.write_packet(3, &[]).await; // Login Acknowledged
    let _features = client.read_packet().await; // Update Enabled Features
    let _select = client.read_packet().await; // Select Known Packs
    let known_packs_payload = [
        encode_var_i32(1).to_vec(),
        encode_string("minecraft"),
        encode_string("core"),
        encode_string("26.2"),
    ]
    .concat();
    client
        .write_packet(KNOWN_PACKS_PACKET_ID, &known_packs_payload)
        .await;
    loop {
        let packet = client.read_packet().await;
        if packet.packet_id == UPDATE_TAGS_PACKET_ID {
            break;
        }
        assert_eq!(packet.packet_id, REGISTRY_DATA_PACKET_ID);
    }
    let _conduct = client.read_packet().await; // Code of Conduct
    client
        .write_packet(ACCEPT_CODE_OF_CONDUCT_PACKET_ID, &[])
        .await;
    let _finish = client.read_packet().await; // Finish Configuration
    client
        .write_packet(ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID, &[])
        .await;

    // --- Play state: consume the spawn sequence ---
    for _ in 0..15 {
        let packet = client.read_packet().await;
        if packet.packet_id == LOGIN_PACKET_ID
            || packet.packet_id == LEVEL_CHUNK_WITH_LIGHT_PACKET_ID
        {
            continue;
        }
        if packet.packet_id == PLAYER_POSITION_PACKET_ID {
            break;
        }
    }

    client
}

/// A synthetic Minecraft client that drives the server through real network
/// flows: it speaks the wire protocol, including AES/CFB8 encryption and
/// zlib compression, and performs the RSA key exchange like the vanilla
/// client does.
pub struct MockClient {
    pub stream: TcpStream,
    read_buffer: bytes::BytesMut,
    pub compression_threshold: Option<usize>,
    encrypt_cipher: Option<Cfb8Stream>,
    decrypt_cipher: Option<Cfb8Stream>,
}

impl MockClient {
    pub async fn connect(server_address: std::net::SocketAddr) -> Self {
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
    pub fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.encrypt_cipher = Some(Cfb8Stream::new(shared_secret));
        self.decrypt_cipher = Some(Cfb8Stream::new(shared_secret));
    }

    pub async fn write_packet(&mut self, packet_id: i32, payload: &[u8]) {
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
    pub async fn read_raw_body(&mut self) -> Vec<u8> {
        loop {
            match split_frame(&self.read_buffer[..]).expect("frame should split") {
                Some(split) => {
                    let body = self.read_buffer[split.body_offset..split.total_consumed].to_vec();
                    self.read_buffer.advance(split.total_consumed);
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
    pub async fn read_packet(&mut self) -> PacketFrame {
        let body = self.read_raw_body().await;
        let packet_body = if self.compression_threshold.is_some() {
            decompress_body(&body).expect("should decompress")
        } else {
            body
        };
        decode_packet_data(Bytes::from(packet_body)).expect("should parse")
    }
}
