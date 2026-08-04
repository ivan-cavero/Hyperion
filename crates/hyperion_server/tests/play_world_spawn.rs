//! Play spawn with a real Anvil world directory: brand + multi-chunk flat platform.

mod common;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_world::{BootstrapConfig, prepare_data_directory};
use tokio::net::TcpListener;

use common::log_into_play_with_stats;
use hyperion_server::config::ServerConfig;
use hyperion_server::key_pool::KeyPool;
use hyperion_server::network::{ConnectionError, handle_connection};

fn temp_data_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "hyperion-play-world-{}-{nanos}",
        std::process::id()
    ))
}

#[tokio::test]
async fn spawn_from_anvil_sends_brand_and_view_distance_chunks() {
    let root = temp_data_root();
    let _ = std::fs::remove_dir_all(&root);

    let paths = prepare_data_directory(&BootstrapConfig {
        root: root.clone(),
        level_name: "world".to_owned(),
        level_seed: 42,
        spawn_y: 64,
    })
    .expect("bootstrap world");

    // View distance 2 → (2*2+1)² = 25 chunks (keeps the test fast).
    let view_distance = 2;
    let expected_chunks = ((2 * view_distance + 1) as usize).pow(2);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let server_address = listener
        .local_addr()
        .expect("listener should have an address");

    let config = ServerConfig {
        online_mode: false,
        world_dir: paths.world_dir.clone(),
        view_distance,
        simulation_distance: view_distance,
        spawn_y: 64,
        ..ServerConfig::default()
    };
    let key_pool = KeyPool::new(1);
    let server_task = tokio::spawn(async move {
        let (stream, peer_address) = listener.accept().await.expect("client should connect");
        handle_connection(stream, peer_address, config, &key_pool).await
    });

    let (client, stats) = log_into_play_with_stats(server_address).await;
    assert!(stats.saw_login);
    assert!(stats.saw_brand, "F3 brand requires minecraft:brand payload");
    assert_eq!(
        stats.chunk_count, expected_chunks,
        "must stream a full square of view-distance chunks"
    );
    assert!(stats.saw_position);

    drop(client);
    let result = server_task
        .await
        .expect("server task should finish")
        .expect_err("disconnect must be reported after the client closes");
    assert!(matches!(result, ConnectionError::Disconnected));

    let _ = std::fs::remove_dir_all(&root);
}
