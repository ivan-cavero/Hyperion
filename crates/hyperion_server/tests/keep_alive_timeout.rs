//! Keep-alive timeout: a client that never answers a keep-alive must be
//! kicked with a "Timed out" disconnect, like vanilla.

mod common;

use hyperion_protocol::DISCONNECT_PACKET_ID;
use tokio::net::TcpListener;

use common::log_into_play;
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::handle_connection;

#[tokio::test]
async fn silent_client_is_kicked_after_keep_alive_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    // Fast timings so the test stays quick: the client stops answering
    // after one keep-alive and must be kicked one interval later.
    let config = ServerConfig {
        online_mode: false,
        keep_alive_interval_seconds: 1,
        keep_alive_timeout_seconds: 2,
        ..ServerConfig::default()
    };
    let key_pool = KeyPool::new(1);
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config, &key_pool).await
    });

    let mut client = log_into_play(server_address).await;
    // Acknowledged the first keep-alive (arrives 1s into Play)...
    let keep_alive = client.read_packet().await;
    assert_eq!(
        keep_alive.packet_id,
        hyperion_protocol::KEEP_ALIVE_PACKET_ID,
        "unexpected first packet id {}",
        keep_alive.packet_id
    );
    client.write_packet(hyperion_protocol::SERVERBOUND_KEEP_ALIVE_PACKET_ID, &keep_alive.payload).await;
    // ...then goes silent: probes keep arriving but the kick must follow
    // once the timeout elapses.
    let disconnect = loop {
        let packet = client.read_packet().await;
        if packet.packet_id == DISCONNECT_PACKET_ID {
            break packet;
        }
        assert_eq!(
            packet.packet_id,
            hyperion_protocol::KEEP_ALIVE_PACKET_ID,
            "expected keep-alives until the timeout kicks in"
        );
    };
    // Play-state disconnect reason is a network NBT string tag:
    // 0x08 + u16 length + UTF-8 bytes.
    assert_eq!(disconnect.payload[0], 0x08, "NBT string tag");
    let reason_length =
        u16::from_be_bytes([disconnect.payload[1], disconnect.payload[2]]) as usize;
    assert_eq!(reason_length, disconnect.payload.len() - 3);
    let reason = String::from_utf8(disconnect.payload[3..].to_vec()).expect("reason should be UTF-8");
    assert_eq!(reason, "Timed out");

    // The kick ends the session cleanly (no error to report).
    let _result = server_task
        .await
        .expect("server task should finish")
        .expect("a keep-alive kick must close the session cleanly");
}
