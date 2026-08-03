//! Play-state ping: the server must answer the serverbound `ping_request`
//! (ID 38) with the clientbound `ping` (ID 61) echoing the payload, exactly
//! like vanilla.

mod common;

use hyperion_protocol::{
    ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID, ACCEPT_CODE_OF_CONDUCT_PACKET_ID,
    KNOWN_PACKS_PACKET_ID, LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, PING_PACKET_ID, PING_REQUEST_PACKET_ID, PLAYER_POSITION_PACKET_ID,
    REGISTRY_DATA_PACKET_ID, SET_COMPRESSION_PACKET_ID, UPDATE_TAGS_PACKET_ID, encode_var_i32,
};
use tokio::net::TcpListener;
use uuid::Uuid;

use common::{MockClient, encode_string, handshake_payload};
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::{ConnectionError, handle_connection};

/// Drives a client through the full offline login into Play, consuming the
/// spawn sequence, and returns the client ready for further exchanges.
async fn log_into_play(server_address: std::net::SocketAddr) -> MockClient {
    let mut client = MockClient::connect(server_address).await;

    client.write_packet(0, &handshake_payload(2)).await;
    let login_start_payload =
        [encode_string("PingPlayer"), Uuid::nil().as_bytes().to_vec()].concat();
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
        common::encode_string("minecraft"),
        common::encode_string("core"),
        common::encode_string("26.2"),
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
        if packet.packet_id == LOGIN_PACKET_ID {
            continue;
        }
        if packet.packet_id == LEVEL_CHUNK_WITH_LIGHT_PACKET_ID {
            continue;
        }
        if packet.packet_id == PLAYER_POSITION_PACKET_ID {
            break;
        }
    }

    client
}

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
