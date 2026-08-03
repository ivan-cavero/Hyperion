//! End-to-end Status flow: server list + ping/pong over a real TCP socket.

mod common;

use hyperion_protocol::SUPPORTED_PROTOCOL_VERSION;
use tokio::net::TcpListener;

use common::{MockClient, handshake_payload, read_string};
use hyperion_server::config::ServerConfig;
use hyperion_server::network::{
    PONG_RESPONSE_PACKET_ID, STATUS_RESPONSE_PACKET_ID, handle_connection,
};

#[tokio::test]
async fn serves_status_and_echoes_ping() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, ServerConfig::default()).await
    });

    let mut client = MockClient::connect(server_address).await;

    // Handshake with intent Status (1).
    client.write_packet(0, &handshake_payload(1)).await;

    // Status Request.
    client.write_packet(0, &[]).await;

    // The server responds with the server list.
    let status_frame = client.read_packet().await;
    assert_eq!(status_frame.packet_id, STATUS_RESPONSE_PACKET_ID);
    let mut offset = 0usize;
    let response_json = read_string(&status_frame.payload, &mut offset);
    assert!(response_json.contains("\"enforcesSecureChat\":false"));
    assert!(response_json.contains(&format!("\"protocol\":{SUPPORTED_PROTOCOL_VERSION}")));
    assert!(response_json.contains("\"text\":\"A Hyperion server\""));

    // Ping Request → Pong Response with the same payload.
    let ping_payload = 12345i64.to_be_bytes().to_vec();
    client.write_packet(1, &ping_payload).await;
    let pong_frame = client.read_packet().await;
    assert_eq!(pong_frame.packet_id, PONG_RESPONSE_PACKET_ID);
    assert_eq!(pong_frame.payload, ping_payload);

    // When the client closes, the server terminates cleanly.
    drop(client);
    server_task
        .await
        .expect("server task should finish")
        .expect("connection should end cleanly");
}
