//! Play-state ping: the server must answer the serverbound `ping_request`
//! (ID 38) with the clientbound `ping` (ID 61) echoing the payload, exactly
//! like vanilla.

mod common;

use hyperion_protocol::{PING_PACKET_ID, PING_REQUEST_PACKET_ID};
use tokio::net::TcpListener;

use common::{MockClient, log_into_play};
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::{ConnectionError, handle_connection};

#[tokio::test]
async fn ping_request_is_answered_with_echoed_ping() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let config = ServerConfig {
        online_mode: false,
        ..ServerConfig::default()
    };
    let key_pool = KeyPool::new(1);
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config, &key_pool).await
    });

    let mut client = log_into_play(server_address).await;

    // Send a latency probe; vanilla echoes the exact payload back.
    let probe_payload = 0x0102_0304_0506_0708u64.to_be_bytes();
    client
        .write_packet(PING_REQUEST_PACKET_ID, &probe_payload)
        .await;
    let ping = client.read_packet().await;
    assert_eq!(ping.packet_id, PING_PACKET_ID);
    assert_eq!(&ping.payload[..], &probe_payload[..]);

    // Closing the client ends the keep-alive/chat loop cleanly.
    drop(client);
    let result = server_task
        .await
        .expect("server task should finish")
        .expect_err("disconnect must be reported after the client closes");
    assert!(matches!(result, ConnectionError::Disconnected));
}
