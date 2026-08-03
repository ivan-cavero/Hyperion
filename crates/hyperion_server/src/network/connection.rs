//! A single TCP connection: framing, encryption and compression.
//!
//! `Connection` owns the client socket and its read buffer, and exposes
//! `read_frame`/`write_frame` which transparently apply the session AES/CFB8
//! cipher and the zlib compression threshold.

use std::io;

use bytes::{Bytes, BytesMut};
use hyperion_protocol::{
    Cfb8Stream, PacketFrame, ProtocolError, SHARED_SECRET_LENGTH, compress_body,
    decode_packet_data, decompress_body, encode_var_i32, split_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// An error that terminates the current connection.
#[derive(Debug)]
pub enum ConnectionError {
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
pub(crate) struct Connection {
    stream: TcpStream,
    buffer: BytesMut,
    pub(crate) compression_threshold: Option<usize>,
    decrypt_cipher: Option<Cfb8Stream>,
    encrypt_cipher: Option<Cfb8Stream>,
}

impl Connection {
    pub(crate) fn new(stream: TcpStream) -> Self {
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
    ///
    /// This is a zero-copy path: the frame body is extracted from the read
    /// buffer via `BytesMut::split_to` and sliced into a reference-counted
    /// `Bytes` without copying the payload bytes.
    pub(crate) async fn read_frame(&mut self) -> Result<PacketFrame, ConnectionError> {
        let raw_body: Bytes = loop {
            match split_frame(&self.buffer[..]) {
                Ok(Some(split)) => {
                    // Split the consumed bytes off the buffer (zero-copy:
                    // `split_to` returns a `BytesMut` that shares the
                    // underlying allocation with `self.buffer`).
                    let mut frame_data = self.buffer.split_to(split.total_consumed);
                    // Discard the length-prefix portion, leaving only the
                    // body (packet ID + payload) as a reference-counted
                    // `Bytes` — no byte copy.
                    let _ = frame_data.split_to(split.body_offset);
                    break frame_data.freeze();
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
            // Decompression allocates; this is unavoidable with zlib.
            Bytes::from(decompress_body(&raw_body).map_err(ConnectionError::Protocol)?)
        } else {
            raw_body
        };

        decode_packet_data(&packet_body).map_err(ConnectionError::Protocol)
    }

    /// Writes a packet (ID + payload) applying compression and encryption.
    pub(crate) async fn write_frame(
        &mut self,
        packet_id: i32,
        payload: &[u8],
    ) -> Result<(), ConnectionError> {
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
    pub(crate) fn enable_encryption(&mut self, shared_secret: &[u8; SHARED_SECRET_LENGTH]) {
        self.encrypt_cipher = Some(Cfb8Stream::new(shared_secret));
        self.decrypt_cipher = Some(Cfb8Stream::new(shared_secret));
    }
}
