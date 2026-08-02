//! Conexiones de red y máquina de estados del protocolo (Fase 1).
//!
//! Por ahora se atiende únicamente el flujo Handshake → Status: responder el
//! server list y el ping/pong de un cliente vanilla. El Login (cifrado
//! RSA/AES y compresión) llega en el siguiente hito de la Fase 1.

use std::io;

use bytes::{Buf, BytesMut};
use hyperion_protocol::{
    decode_frame, decode_handshake, decode_ping_request, decode_status_request,
    encode_pong_response, encode_status_response, HandshakeIntent, PacketFrame, ProtocolError,
    StatusDescription, StatusPlayers, StatusResponse, StatusVersion, SUPPORTED_PROTOCOL_VERSION,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

/// Acepta conexiones en `bind_address` y despacha cada una a su propia tarea.
pub async fn serve(bind_address: &str) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address).await?;
    println!("Escuchando en {bind_address}");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream).await {
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

/// Lee la siguiente trama completa del stream, acumulando bytes entre llamadas.
async fn read_frame(
    stream: &mut TcpStream,
    buffer: &mut BytesMut,
) -> Result<PacketFrame, ConnectionError> {
    loop {
        match decode_frame(&buffer[..]) {
            Ok((frame, consumed_bytes)) => {
                buffer.advance(consumed_bytes);
                return Ok(frame);
            }
            Err(ProtocolError::UnexpectedEndOfInput) => {
                let bytes_read = stream.read_buf(buffer).await?;
                if bytes_read == 0 {
                    return Err(ConnectionError::Disconnected);
                }
            }
            Err(error) => return Err(ConnectionError::Protocol(error)),
        }
    }
}

/// Atiende una conexión: Handshake y, si el intent es Status, el intercambio
/// del server list (Status Request → Response, Ping Request → Pong Response).
async fn handle_connection(mut stream: TcpStream) -> Result<(), ConnectionError> {
    let mut buffer = BytesMut::with_capacity(256);

    let handshake_frame = read_frame(&mut stream, &mut buffer).await?;
    let handshake = decode_handshake(&handshake_frame)?;
    if handshake.intent != HandshakeIntent::Status {
        // Login y Transfer se implementan en el siguiente hito de la Fase 1.
        return Ok(());
    }

    loop {
        let frame = match read_frame(&mut stream, &mut buffer).await {
            Ok(frame) => frame,
            Err(ConnectionError::Disconnected) => return Ok(()),
            Err(error) => return Err(error),
        };
        match frame.packet_id {
            // Status Request (0): responder con el server list.
            0 => {
                decode_status_request(&frame)?;
                let response_frame = encode_status_response(&default_status_response())?;
                stream.write_all(&response_frame).await?;
            }
            // Ping Request (1): Pong con el mismo payload.
            1 => {
                let ping = decode_ping_request(&frame)?;
                let pong_frame = encode_pong_response(ping.payload)?;
                stream.write_all(&pong_frame).await?;
            }
            _ => return Ok(()),
        }
    }
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
    use super::*;
    use hyperion_protocol::encode_frame;

    fn encode_var_i32(value: i32) -> Vec<u8> {
        let mut encoded_bytes = Vec::new();
        let mut remaining_value = value as u32;

        loop {
            let mut current_byte = (remaining_value as u8) & 0x7f;
            remaining_value >>= 7;
            if remaining_value != 0 {
                current_byte |= 0x80;
            }
            encoded_bytes = [encoded_bytes, vec![current_byte]].concat();
            if remaining_value == 0 {
                return encoded_bytes;
            }
        }
    }

    fn encode_string(value: &str) -> Vec<u8> {
        [
            encode_var_i32(value.len() as i32),
            value.as_bytes().to_vec(),
        ]
        .concat()
    }

    /// Extrae el JSON del payload de una Status Response (String prefijado).
    fn status_response_json(payload: &[u8]) -> String {
        let length_byte_count = payload
            .iter()
            .position(|byte| byte & 0x80 == 0)
            .expect("string length should terminate");
        String::from_utf8(payload[length_byte_count + 1..].to_vec())
            .expect("response JSON should be UTF-8")
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
            handle_connection(stream).await
        });

        let mut client = TcpStream::connect(server_address)
            .await
            .expect("client should connect");

        // Handshake con intent Status.
        let handshake_payload = [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("localhost"),
            25565u16.to_be_bytes().to_vec(),
            encode_var_i32(1),
        ]
        .concat();
        let handshake_frame = encode_frame(0, &handshake_payload).expect("handshake should encode");
        client
            .write_all(&handshake_frame)
            .await
            .expect("handshake should write");

        // Status Request.
        let status_request = encode_frame(0, &[]).expect("status request should encode");
        client
            .write_all(&status_request)
            .await
            .expect("status request should write");

        // El servidor responde con el server list.
        let mut response_buffer = BytesMut::new();
        let status_frame = read_frame(&mut client, &mut response_buffer)
            .await
            .expect("status response should arrive");
        assert_eq!(status_frame.packet_id, 0);
        let response_json = status_response_json(&status_frame.payload);
        assert!(response_json.contains("\"enforcesSecureChat\":false"));
        assert!(response_json.contains(&format!("\"protocol\":{SUPPORTED_PROTOCOL_VERSION}")));
        assert!(response_json.contains("\"text\":\"A Hyperion server\""));

        // Ping Request → Pong Response con el mismo payload.
        let ping_payload = 12345i64.to_be_bytes().to_vec();
        let ping_frame = encode_frame(1, &ping_payload).expect("ping should encode");
        client
            .write_all(&ping_frame)
            .await
            .expect("ping should write");
        let pong_frame = read_frame(&mut client, &mut response_buffer)
            .await
            .expect("pong should arrive");
        assert_eq!(pong_frame.packet_id, 1);
        assert_eq!(pong_frame.payload, ping_payload);

        // Al cerrar el cliente, el servidor termina la conexión limpiamente.
        drop(client);
        server_task
            .await
            .expect("server task should finish")
            .expect("connection should end cleanly");
    }
}
