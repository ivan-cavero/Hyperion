//! Benchmarks for the server login path over a real loopback connection.
//!
//! Run with:
//!   cargo bench -p hyperion_server --bench login
//!
//! `offline_login_cycle` measures one full offline-mode login: a real TCP
//! client performs Handshake → Login Start → Set Compression → Login Success
//! → Login Acknowledged against a `handle_connection` task. This is the
//! per-connection cost every worker pays (sans the RSA key generation, which
//! is benchmarked separately in `hyperion_protocol`).

use std::io;

use bytes::{Buf, BytesMut};
use divan::black_box;
use hyperion_protocol::{
    LOGIN_ACKNOWLEDGED_PACKET_ID, LOGIN_SUCCESS_PACKET_ID, PacketFrame, SET_COMPRESSION_PACKET_ID,
    SUPPORTED_PROTOCOL_VERSION, compress_body, decode_packet_data, decode_var_i32, decompress_body,
    encode_var_i32, split_frame,
};
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::handle_connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use uuid::Uuid;

fn main() {
    divan::main();
}

/// A handshake packet with the Login intent.
fn handshake_payload() -> Vec<u8> {
    [
        encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
        encode_var_i32(9),
        b"localhost".to_vec(),
        25565u16.to_be_bytes().to_vec(),
        encode_var_i32(2),
    ]
    .concat()
}

/// A Login Start packet with a valid vanilla username.
fn login_start_payload() -> Vec<u8> {
    [
        encode_var_i32(12),
        b"BenchPlayerX".to_vec(),
        Uuid::nil().as_bytes().to_vec(),
    ]
    .concat()
}

/// Binary frame for `packet_id + payload`: `Data Length` prefix when the
/// client-side compression threshold is active, then length + body.
fn frame_for(packet_id: i32, payload: &[u8], compression_threshold: Option<usize>) -> Vec<u8> {
    let packet_body = [encode_var_i32(packet_id), payload.to_vec()].concat();
    let frame_body = match compression_threshold {
        Some(threshold) => compress_body(&packet_body, threshold).expect("should compress"),
        None => packet_body,
    };
    let mut frame = encode_var_i32(frame_body.len() as i32);
    frame.extend_from_slice(&frame_body);
    frame
}

async fn write_packet(
    stream: &mut TcpStream,
    packet_id: i32,
    payload: &[u8],
    compression_threshold: Option<usize>,
) -> io::Result<()> {
    stream
        .write_all(&frame_for(packet_id, payload, compression_threshold))
        .await
}

/// Reads one complete wire frame from the socket, buffering across reads.
async fn read_raw_frame(stream: &mut TcpStream, buffer: &mut BytesMut) -> io::Result<Vec<u8>> {
    loop {
        if let Some(split) = split_frame(buffer)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        {
            let body = buffer[split.body_offset..split.total_consumed].to_vec();
            buffer.advance(split.total_consumed);
            return Ok(body);
        }
        let mut chunk = [0u8; 4096];
        let bytes_read = stream.read(&mut chunk).await?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed the connection",
            ));
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }
}

/// Reads one packet. When `compression` is set, the frame body starts with a
/// Data Length prefix and the data may be zlib-compressed.
async fn read_packet(
    stream: &mut TcpStream,
    buffer: &mut BytesMut,
    compression_threshold: Option<usize>,
) -> io::Result<PacketFrame> {
    let raw_body = read_raw_frame(stream, buffer).await?;
    let packet_body: Vec<u8> = match compression_threshold {
        Some(_) => decompress_body(&raw_body)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => raw_body,
    };
    decode_packet_data(&packet_body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The full offline-mode login round trip over a real socket.
async fn login_cycle_once(key_pool: KeyPool) -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let server_address = listener.local_addr()?;
    let handle = key_pool.clone();

    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, ServerConfig::default(), &handle)
            .await
            .expect("server should complete the login")
    });

    let mut client = TcpStream::connect(server_address).await?;
    let mut read_buffer = BytesMut::with_capacity(1024);
    let mut client_compression = None;

    // Handshake with intent Login, then Login Start.
    write_packet(&mut client, 0, &handshake_payload(), client_compression).await?;
    write_packet(&mut client, 0, &login_start_payload(), client_compression).await?;

    // Set Compression arrives uncompressed and tells us the threshold, then
    // Login Success arrives with the Data Length prefix that the client
    // must start using immediately.
    let set_compression = read_packet(&mut client, &mut read_buffer, client_compression).await?;
    debug_assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
    let (threshold, _) = decode_var_i32(&set_compression.payload, 0, 5)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    client_compression = Some(threshold as usize);

    let login_success = read_packet(&mut client, &mut read_buffer, client_compression).await?;
    debug_assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);

    // Login Acknowledged: the server transitions to Configuration and returns.
    write_packet(
        &mut client,
        LOGIN_ACKNOWLEDGED_PACKET_ID,
        &[],
        client_compression,
    )
    .await?;
    server_task.await.expect("server task should join");

    Ok(())
}

#[divan::bench]
fn offline_login_cycle(bencher: divan::Bencher) {
    let runtime = Runtime::new().expect("tokio runtime");
    let key_pool = runtime.block_on(async { KeyPool::new(2) });
    bencher.bench(|| {
        runtime
            .block_on(login_cycle_once(key_pool.clone()))
            .expect("login cycle should succeed");
        black_box(())
    });
}
