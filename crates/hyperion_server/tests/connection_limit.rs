//! Connection cap: with `max_connections` reached, new connections wait
//! for a free slot (backpressure) instead of piling up unlimited tasks.

mod common;

use std::time::Duration;

use tokio::net::TcpListener;

use common::{MockClient, handshake_payload};
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::{STATUS_RESPONSE_PACKET_ID, serve_with_listener};

#[tokio::test]
async fn excess_connections_wait_for_a_free_slot() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    // One slot only: the second connection must wait for the first.
    let config = ServerConfig {
        online_mode: false,
        max_connections: 1,
        ..ServerConfig::default()
    };
    let key_pool = KeyPool::new(1);
    let server_task =
        tokio::spawn(async move { serve_with_listener(listener, config, key_pool).await });

    // First client takes the only slot and completes a status exchange.
    let mut first = MockClient::connect(server_address).await;
    first.write_packet(0, &handshake_payload(1)).await;
    first.write_packet(0, &[]).await;
    let status_frame = first.read_packet().await;
    assert_eq!(status_frame.packet_id, STATUS_RESPONSE_PACKET_ID);

    // Second client connects but must be held in the backlog: no response
    // within the grace period.
    let mut second = MockClient::connect(server_address).await;
    second.write_packet(0, &handshake_payload(1)).await;
    second.write_packet(0, &[]).await;
    let held_back = tokio::time::timeout(Duration::from_millis(300), second.read_packet()).await;
    assert!(
        held_back.is_err(),
        "the second connection must wait for a free slot"
    );

    // The first client leaves; the second now gets its response.
    drop(first);
    let status_frame = tokio::time::timeout(Duration::from_secs(5), second.read_packet())
        .await
        .expect("the second connection must proceed once a slot frees");
    assert_eq!(status_frame.packet_id, STATUS_RESPONSE_PACKET_ID);

    drop(second);
    server_task.abort();
}
