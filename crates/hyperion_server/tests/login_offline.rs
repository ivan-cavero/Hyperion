//! Offline-mode login flow: vanilla UUID, compression, and username
//! validation over a real TCP socket.

mod common;

use bytes::Bytes;
use hyperion_protocol::{
    ACCEPT_CODE_OF_CONDUCT_PACKET_ID, ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID,
    CODE_OF_CONDUCT_PACKET_ID, FINISH_CONFIGURATION_PACKET_ID, KNOWN_PACKS_PACKET_ID,
    LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_DISCONNECT_PACKET_ID, LOGIN_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, PLAYER_POSITION_PACKET_ID, ProtocolError, REGISTRY_DATA_PACKET_ID,
    SELECT_KNOWN_PACKS_PACKET_ID, SET_COMPRESSION_PACKET_ID, UPDATE_ENABLED_FEATURES_PACKET_ID,
    UPDATE_TAGS_PACKET_ID, decode_packet_data, decode_var_i32, decompress_body, encode_var_i32,
    offline_mode_uuid,
};
use tokio::net::TcpListener;
use uuid::Uuid;

use common::{MockClient, encode_string, handshake_payload, read_string};
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::{ConnectionError, handle_connection};

#[tokio::test]
async fn logs_in_offline_with_vanilla_uuid_and_compression() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    // Low threshold to force Login Success to travel compressed.
    let config = ServerConfig {
        online_mode: false,
        compression_threshold: 4,
        ..ServerConfig::default()
    };
    let key_pool = KeyPool::new(1);
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config, &key_pool).await
    });

    let mut client = MockClient::connect(server_address).await;

    // Handshake with intent Login (2).
    client.write_packet(0, &handshake_payload(2)).await;

    // Login Start. The client-declared UUID is ignored by the server,
    // which assigns the vanilla offline UUID instead.
    let login_start_payload =
        [encode_string("TestPlayer"), Uuid::nil().as_bytes().to_vec()].concat();
    client.write_packet(0, &login_start_payload).await;

    // Set Compression (not yet compressing).
    let set_compression = client.read_packet().await;
    assert_eq!(set_compression.packet_id, SET_COMPRESSION_PACKET_ID);
    let (threshold, _) =
        decode_var_i32(&set_compression.payload, 0, 5).expect("threshold should decode");
    assert_eq!(threshold, 4);
    client.compression_threshold = Some(4usize);

    // Login Success: must arrive compressed (data_length > 0).
    let raw_success_body = client.read_raw_body().await;
    let (data_length, _) =
        decode_var_i32(&raw_success_body, 0, 5).expect("data length should decode");
    assert!(data_length > 0, "Login Success should be compressed");
    let login_success = decode_packet_data(Bytes::from(
        decompress_body(&raw_success_body).expect("should decompress"),
    ))
    .expect("should parse");
    assert_eq!(login_success.packet_id, LOGIN_SUCCESS_PACKET_ID);
    let expected_uuid = offline_mode_uuid("TestPlayer");
    assert_eq!(&login_success.payload[0..16], expected_uuid.as_bytes());
    assert_eq!(login_success.payload[16], 10); // "TestPlayer" length
    assert_eq!(&login_success.payload[17..27], "TestPlayer".as_bytes());
    assert_eq!(login_success.payload[27], 0); // no properties
    assert_eq!(login_success.payload.len(), 44); // uuid + name + count + session id

    // Login Acknowledged (compressed): the client enters Configuration.
    client.write_packet(3, &[]).await;

    // --- Configuration state (vanilla order) ---
    // 1) Feature flags
    let features = client.read_packet().await;
    assert_eq!(features.packet_id, UPDATE_ENABLED_FEATURES_PACKET_ID);
    // 2) Known Packs (clientbound) — server initiates negotiation.
    let select = client.read_packet().await;
    assert_eq!(select.packet_id, SELECT_KNOWN_PACKS_PACKET_ID);
    // 3) Known Packs (serverbound) — client accepts minecraft:core@26.2.
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
    // 4) Registry Data (one packet per synchronized registry).
    let mut registry_packets = 0usize;
    let mut saw_tags = false;
    for _ in 0..64 {
        let packet = client.read_packet().await;
        if packet.packet_id == UPDATE_TAGS_PACKET_ID {
            saw_tags = true;
            break;
        }
        assert_eq!(packet.packet_id, REGISTRY_DATA_PACKET_ID);
        registry_packets += 1;
    }
    assert!(
        registry_packets >= 20,
        "expected full synchronized registry set, got {registry_packets}"
    );
    assert!(saw_tags, "update_tags never arrived");
    // 5) Code of Conduct (26.2).
    let conduct = client.read_packet().await;
    assert_eq!(conduct.packet_id, CODE_OF_CONDUCT_PACKET_ID);
    let mut offset = 0;
    assert_eq!(
        read_string(&conduct.payload, &mut offset),
        "https://aka.ms/MinecraftCodeOfConduct"
    );
    assert_eq!(offset, conduct.payload.len());
    client
        .write_packet(ACCEPT_CODE_OF_CONDUCT_PACKET_ID, &[])
        .await;
    // 6) Finish Configuration.
    let finish = client.read_packet().await;
    assert_eq!(finish.packet_id, FINISH_CONFIGURATION_PACKET_ID);
    client
        .write_packet(ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID, &[])
        .await;

    // --- Play state: the spawn sequence ---
    let mut saw_login = false;
    let mut saw_chunk = false;
    for _ in 0..15 {
        let packet = client.read_packet().await;
        if packet.packet_id == LOGIN_PACKET_ID {
            saw_login = true;
        }
        if packet.packet_id == LEVEL_CHUNK_WITH_LIGHT_PACKET_ID {
            saw_chunk = true;
        }
        if packet.packet_id == PLAYER_POSITION_PACKET_ID {
            break;
        }
    }
    assert!(saw_login, "spawn sequence must include Login (play)");
    assert!(saw_chunk, "spawn sequence must include the spawn chunk");

    // Closing the client ends the keep-alive/chat loop cleanly.
    drop(client);
    let result = server_task
        .await
        .expect("server task should finish")
        .expect_err("disconnect must be reported after the client closes");
    assert!(matches!(result, ConnectionError::Disconnected));
}

/// Usernames outside `[a-zA-Z0-9_]` are rejected at the protocol
/// boundary with a disconnect message, like vanilla.
#[tokio::test]
async fn login_rejects_invalid_username_with_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let key_pool = KeyPool::new(1);
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(
            stream,
            peer_address,
            ServerConfig {
                online_mode: false,
                ..ServerConfig::default()
            },
            &key_pool,
        )
        .await
    });

    let mut client = MockClient::connect(server_address).await;
    client.write_packet(0, &handshake_payload(2)).await;
    client
        .write_packet(
            0,
            &[encode_string("Bad-Name!"), Uuid::nil().as_bytes().to_vec()].concat(),
        )
        .await;

    let disconnect = client.read_packet().await;
    assert_eq!(disconnect.packet_id, LOGIN_DISCONNECT_PACKET_ID);
    let mut offset = 0usize;
    let reason = common::read_string(&disconnect.payload, &mut offset);
    assert!(reason.contains("Invalid characters in username"));

    assert!(matches!(
        server_task.await.expect("server task should finish"),
        Err(ConnectionError::Protocol(ProtocolError::InvalidUsername))
    ));
}
