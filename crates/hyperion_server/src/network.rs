//! Conexiones de red y máquina de estados del protocolo (Fase 1).
//!
//! Atiende el flujo Handshake → Status (server list + ping) y el flujo
//! Handshake → Login en modo offline: Login Start, Set Compression, Login
//! Success y Login Acknowledged. El cifrado de sesión (online mode) llega en
//! el siguiente hito; la criptografía ya está lista en
//! `hyperion_protocol::crypto`.

use std::io;

use bytes::{Buf, BytesMut};
use hyperion_protocol::{
    compress_body, decode_handshake, decode_login_acknowledged, decode_login_start,
    decode_packet_data, decode_ping_request, decode_status_request, decompress_body,
    encode_login_success_payload, encode_status_response_payload, encode_var_i32, split_frame,
    Cfb8Stream, GameProfile, HandshakeIntent, LoginSuccess, PacketFrame, ProtocolError,
    StatusDescription, StatusPlayers, StatusResponse, StatusVersion, SUPPORTED_PROTOCOL_VERSION,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

/// Umbral de compresión por defecto (como el vanilla: 256).
const COMPRESSION_THRESHOLD: usize = 256;

/// IDs de los paquetes clientbound de Status y Login usados aquí.
const STATUS_RESPONSE_PACKET_ID: i32 = 0;
const PONG_RESPONSE_PACKET_ID: i32 = 1;
const SET_COMPRESSION_PACKET_ID: i32 = 3;
const LOGIN_SUCCESS_PACKET_ID: i32 = 2;

/// Nombre de versión anunciado en el server list.
const PROTOCOL_VERSION_NAME: &str = "26.2";
/// Número máximo de jugadores anunciado.
const MAX_PLAYERS: i32 = 20;
/// MOTD del server list.
const SERVER_MOTD: &str = "A Hyperion server";

/// Error que termina la conexión actual.
#[derive(Debug)]
enum ConnectionError {
    /// El cliente cerró la conexión.
    Disconnected,
    /// El flujo de red falló.
    Io(io::Error),
    /// La trama recibida no respeta el protocolo.
    Protocol(ProtocolError),
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

/// Una conexión con su buffer de lectura, umbral de compresión y cifrado
/// de sesión (si está activo).
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

    /// Lee la siguiente trama, aplicando descifrado y descompresión según el
    /// estado de la conexión.
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

    /// Escribe un paquete (ID + payload) aplicando compresión y cifrado.
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

    /// Lee bytes del socket, los descifra si hace falta y los acumula.
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
}

/// Acepta conexiones en `bind_address` y despacha cada una a su propia tarea.
pub async fn serve(bind_address: &str) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address).await?;
    println!("Escuchando en {bind_address}");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, COMPRESSION_THRESHOLD).await {
                match error {
                    ConnectionError::Disconnected => {}
                    ConnectionError::Io(io_error) => eprintln!("Error de red: {io_error}"),
                    ConnectionError::Protocol(protocol_error) => {
                        eprintln!("Violación de protocolo: {protocol_error}");
                    }
                }
            }
        });
    }
}

/// Atiende una conexión: Handshake y luego el flujo del intent elegido.
async fn handle_connection(
    stream: TcpStream,
    compression_threshold: usize,
) -> Result<(), ConnectionError> {
    let mut connection = Connection::new(stream);

    let handshake_frame = connection.read_frame().await?;
    let handshake = decode_handshake(&handshake_frame)?;

    match handshake.intent {
        HandshakeIntent::Status => serve_status(&mut connection).await,
        HandshakeIntent::Login => serve_login(&mut connection, compression_threshold).await,
        // Transfer llega en un hito posterior.
        HandshakeIntent::Transfer => Ok(()),
    }
}

/// Atiende el intercambio de Status: server list y ping/pong.
async fn serve_status(connection: &mut Connection) -> Result<(), ConnectionError> {
    loop {
        let frame = match connection.read_frame().await {
            Ok(frame) => frame,
            Err(ConnectionError::Disconnected) => return Ok(()),
            Err(error) => return Err(error),
        };
        match frame.packet_id {
            // Status Request (0): responder con el server list.
            0 => {
                decode_status_request(&frame)?;
                let payload = encode_status_response_payload(&default_status_response())?;
                connection
                    .write_frame(STATUS_RESPONSE_PACKET_ID, &payload)
                    .await?;
            }
            // Ping Request (1): Pong con el mismo payload.
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

/// Atiende el Login offline: Login Start, Set Compression, Login Success y
/// Login Acknowledged.
async fn serve_login(
    connection: &mut Connection,
    compression_threshold: usize,
) -> Result<(), ConnectionError> {
    let login_start_frame = connection.read_frame().await?;
    let login_start = decode_login_start(&login_start_frame)?;
    println!("Login de {} ({})", login_start.username, login_start.uuid);

    // Set Compression (aún sin comprimir) y activación de la compresión.
    connection
        .write_frame(
            SET_COMPRESSION_PACKET_ID,
            &encode_var_i32(compression_threshold as i32),
        )
        .await?;
    connection.compression_threshold = Some(compression_threshold);

    // Login Success (comprimido). El UUID de perfil es el declarado por el
    // cliente; vanilla en offline lo deriva de "OfflinePlayer:<name>" (pendiente).
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

    // Login Acknowledged: el cliente pasa a Configuration.
    let acknowledged_frame = connection.read_frame().await?;
    decode_login_acknowledged(&acknowledged_frame)?;
    println!(
        "Jugador {} conectado (Configuration llega en el siguiente hito)",
        login_start.username
    );

    Ok(())
}

/// El server list que anuncia Hyperion.
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

    /// Lee el cuerpo de una trama (sin descomprimir) del socket.
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

    /// Lee y decodifica un paquete, descomprimiendo si la conexión lo exige.
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
            handle_connection(stream, COMPRESSION_THRESHOLD).await
        });

        let mut client = TcpStream::connect(server_address)
            .await
            .expect("client should connect");
        let mut buffer = BytesMut::new();

        // Handshake con intent Status (1).
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

        // El servidor responde con el server list.
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

        // Ping Request → Pong Response con el mismo payload.
        let ping_payload = 12345i64.to_be_bytes().to_vec();
        client_write_packet(&mut client, 1, &ping_payload, None).await;
        let pong_frame = client_read_packet(&mut client, &mut buffer, None).await;
        assert_eq!(pong_frame.packet_id, PONG_RESPONSE_PACKET_ID);
        assert_eq!(pong_frame.payload, ping_payload);

        // Al cerrar el cliente, el servidor termina la conexión limpiamente.
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

        // Umbral bajo para forzar que el Login Success viaje comprimido.
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("client should connect");
            handle_connection(stream, 4).await
        });

        let mut client = TcpStream::connect(server_address)
            .await
            .expect("client should connect");
        let mut buffer = BytesMut::new();

        // Handshake con intent Login (2).
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

        // Set Compression (sin comprimir aún).
        let set_compression = client_read_packet(&mut client, &mut buffer, None).await;
        assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
        let (threshold, _) =
            decode_var_i32(&set_compression.payload, 0, 5).expect("threshold should decode");
        assert_eq!(threshold, 4);
        let client_compression = Some(4usize);

        // Login Success: debe llegar comprimido (data_length > 0).
        let raw_success_body = client_read_body(&mut client, &mut buffer).await;
        let (data_length, _) =
            decode_var_i32(&raw_success_body, 0, 5).expect("data length should decode");
        assert!(data_length > 0, "Login Success should be compressed");
        let login_success =
            decode_packet_data(&decompress_body(&raw_success_body).expect("should decompress"))
                .expect("should parse");
        assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);
        assert_eq!(&login_success.payload[0..16], uuid.as_bytes());
        assert_eq!(login_success.payload[16], 10); // longitud de "TestPlayer"
        assert_eq!(&login_success.payload[17..27], "TestPlayer".as_bytes());
        assert_eq!(login_success.payload[27], 0); // sin propiedades
        assert_eq!(login_success.payload.len(), 44); // uuid + nombre + count + session id

        // Login Acknowledged (comprimido).
        client_write_packet(&mut client, 3, &[], client_compression).await;

        drop(client);
        server_task
            .await
            .expect("server task should finish")
            .expect("connection should end cleanly");
    }
}
