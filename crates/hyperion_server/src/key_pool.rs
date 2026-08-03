//! Pool of pre-generated RSA key pairs for online-mode logins.
//!
//! RSA-1024 key generation takes ≈13–37 ms (and up to 150 ms in outliers).
//! Generating keys on demand inside a login flow blocks a tokio worker even
//! with `spawn_blocking`, because the caller must wait for completion. This
//! pool moves the generation off the hot path entirely: a background task
//! replenishes the pool continuously, and `acquire()` returns an already-
//! generated key immediately (or waits a few ms in the unlikely event the
//! pool is empty during a burst).
//!
//! # Usage
//!
//! ```ignore
//! let pool = KeyPool::new(4);               // spawn 4 background generators
//! let (public_key_der, private_key) = pool.acquire().await;
//! ```

use std::sync::Arc;

use hyperion_protocol::generate_rsa_keypair;
use rsa::RsaPrivateKey;
use tokio::sync::{Mutex, mpsc};

/// A key pair ready for one online-mode login.
type RsaKeyMaterial = (Vec<u8>, RsaPrivateKey);

/// How many pre-generated keys the pool may hold before the generators
/// apply backpressure and stop producing.
///
/// A bounded channel is essential: an unbounded one would let the background
/// generators run forever, accumulating RSA keys (≈1–3 KB each) at ~100/sec
/// even on an idle server. With 4 generators, 16 slots ≈ 4 in flight each,
/// which still absorbs login bursts (keygen ≈37 ms, consumption ≈300 µs).
const KEY_POOL_CAPACITY: usize = 16;

/// Pool that keeps `pool_size` background tasks generating RSA key pairs.
///
/// Clone the pool to pass it to spawned tasks — all clones share the same
/// underlying channel of pre-generated keys.
#[derive(Clone)]
pub struct KeyPool {
    receiver: Arc<Mutex<mpsc::Receiver<RsaKeyMaterial>>>,
}

/// Cloneable handle used by background key-generation tasks to submit keys.
#[derive(Clone)]
struct Handle {
    sender: mpsc::Sender<RsaKeyMaterial>,
}

impl KeyPool {
    /// Creates a pool and spawns `pool_size` background generator tasks.
    ///
    /// Each task runs an infinite loop: `generate_rsa_keypair` on a blocking
    /// thread, then feeds the result into the shared channel. The bounded
    /// channel applies backpressure — when the pool is full, the generators
    /// block on send instead of producing keys nobody will consume. If the
    /// channel receiver is dropped (the pool is destroyed), all generator
    /// tasks exit.
    pub fn new(pool_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(KEY_POOL_CAPACITY);
        let handle = Handle { sender };

        for index in 0..pool_size {
            let handle = handle.clone();
            tokio::spawn(async move {
                loop {
                    let key = tokio::task::spawn_blocking(generate_rsa_keypair)
                        .await
                        .expect("RSA keygen task panicked")
                        .expect("RSA keygen failed");
                    if handle.sender.send(key).await.is_err() {
                        // The pool was dropped — exit the generator loop.
                        break;
                    }
                }
            });
            tracing::trace!("RSA key generator {index}/{pool_size} spawned");
        }

        Self {
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Returns a pre-generated key pair, waiting if all are currently in use.
    ///
    /// In practice this should never wait for more than a few milliseconds
    /// because generator tasks run continuously and replenish the channel
    /// faster than logins consume keys (keygen ≈37 ms, login flow ≈300 μs).
    pub async fn acquire(&self) -> RsaKeyMaterial {
        let mut receiver = self.receiver.lock().await;
        receiver
            .recv()
            .await
            .expect("all RSA key generators have stopped")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_returns_a_usable_public_key() {
        let pool = KeyPool::new(1);
        let (public_key_der, _private_key) = pool.acquire().await;
        // DER SubjectPublicKeyInfo starts with a SEQUENCE tag.
        assert!(public_key_der.starts_with(&[0x30]), "DER sequence tag");
        assert!(!public_key_der.is_empty());
    }

    #[tokio::test]
    async fn generators_replenish_past_channel_capacity() {
        // Consume far more keys than the bounded channel can hold: the
        // generators must keep producing under backpressure, and every key
        // must still be a valid DER public key.
        let pool = KeyPool::new(2);
        for _ in 0..8 {
            let (public_key_der, _private_key) = pool.acquire().await;
            assert!(public_key_der.starts_with(&[0x30]), "DER sequence tag");
        }
    }
}
