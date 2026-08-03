//! Protocol cryptography: AES-128/CFB8 session encryption and RSA.
//!
//! Minecraft encrypts the packet stream with AES-128/CFB8 using the shared
//! secret as both key and IV, and protects the key exchange with RSA
//! (PKCS#1 v1.5). The RSA key is 1024 bits and the public key is sent as
//! DER SubjectPublicKeyInfo.

use aes::Aes128;
use aes::cipher::{Block, BlockEncrypt, KeyInit};
use rand::rngs::OsRng;
use rsa::pkcs8::EncodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey};
use sha1::{Digest, Sha1};

use crate::ProtocolError;

/// Session shared secret size: 16 bytes serving as both AES-128 key and IV.
pub const SHARED_SECRET_LENGTH: usize = 16;

/// Size of the random challenge token sent with the Encryption Request.
pub const VERIFY_TOKEN_LENGTH: usize = 4;

/// Stream de AES-128/CFB8.
///
/// Mantiene el registro de desplazamiento de 16 bytes entre llamadas, de modo
/// que el cifrado/descifrado es continuo a lo largo de la conexión. Se usa una
/// instancia para cada dirección (cifrado de salida, descifrado de entrada).
pub struct Cfb8Stream {
    cipher: Aes128,
    feedback: Block<Aes128>,
}

impl Cfb8Stream {
    /// Crea el stream con la clave (= IV) dada.
    pub fn new(key: &[u8; SHARED_SECRET_LENGTH]) -> Self {
        let cipher = Aes128::new_from_slice(key).expect("AES-128 requires a 16-byte key");
        let feedback = Block::<Aes128>::clone_from_slice(key);
        Self { cipher, feedback }
    }

    /// Cifra `data` in-place, actualizando el registro de desplazamiento.
    pub fn encrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let ciphertext_byte = self.keystream_byte() ^ *byte;
            self.shift_feedback(ciphertext_byte);
            *byte = ciphertext_byte;
        }
    }

    /// Descifra `data` in-place, actualizando el registro de desplazamiento.
    pub fn decrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let ciphertext_byte = *byte;
            *byte = self.keystream_byte() ^ ciphertext_byte;
            self.shift_feedback(ciphertext_byte);
        }
    }

    fn keystream_byte(&mut self) -> u8 {
        let mut block = self.feedback;
        self.cipher.encrypt_block(&mut block);
        block[0]
    }

    fn shift_feedback(&mut self, byte: u8) {
        self.feedback.copy_within(1..SHARED_SECRET_LENGTH, 0);
        self.feedback[SHARED_SECRET_LENGTH - 1] = byte;
    }
}

/// Genera un keypair RSA de 1024 bits y devuelve la clave pública en DER
/// SubjectPublicKeyInfo junto con la clave privada.
pub fn generate_rsa_keypair() -> Result<(Vec<u8>, RsaPrivateKey), ProtocolError> {
    let private_key = RsaPrivateKey::new(&mut OsRng, 1024)
        .map_err(|error| ProtocolError::Crypto(error.to_string()))?;
    let public_key_der = private_key
        .to_public_key()
        .to_public_key_der()
        .map_err(|error| ProtocolError::Crypto(error.to_string()))?
        .as_bytes()
        .to_vec();

    Ok((public_key_der, private_key))
}

/// Descifra con RSA PKCS#1 v1.5 (lo que el cliente envía con la clave pública).
pub fn decrypt_pkcs1v15(
    private_key: &RsaPrivateKey,
    ciphertext: &[u8],
) -> Result<Vec<u8>, ProtocolError> {
    private_key
        .decrypt(Pkcs1v15Encrypt, ciphertext)
        .map_err(|error| ProtocolError::Crypto(error.to_string()))
}

/// Computes the "server hash" that the client sends to the session server for
/// authentication: SHA-1 of (server_id + shared_secret + public_key) formatted
/// as `new BigInteger(digest).toString(16)` (signed hex, no leading zeros,
/// `-` prefix when the high bit is set).
pub fn server_id_hash(server_id: &str, shared_secret: &[u8], public_key: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(server_id.as_bytes());
    hasher.update(shared_secret);
    hasher.update(public_key);
    let digest = hasher.finalize();

    signed_hex(&digest)
}

fn signed_hex(digest: &[u8]) -> String {
    let negative = digest[0] & 0x80 != 0;

    // In the positive branch we borrow `digest` directly to avoid an
    // unnecessary allocation (SHA-1 is 20 bytes → 40 hex chars).
    let negative_magnitude;
    let magnitude: &[u8] = if negative {
        negative_magnitude = twos_complement(digest);
        &negative_magnitude
    } else {
        digest
    };

    // Pre-allocate capacity: 2 bytes per hex char (SHA-1: 20 → 40 chars).
    let mut hex_body = String::with_capacity(magnitude.len() * 2);
    for byte in magnitude {
        use std::fmt::Write;
        write!(hex_body, "{byte:02x}").expect("write to String is infallible");
    }

    let trimmed = hex_body.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".to_owned();
    }

    if negative {
        let mut result = String::with_capacity(trimmed.len() + 1);
        result.push('-');
        result.push_str(trimmed);
        result
    } else {
        trimmed.to_owned()
    }
}

fn twos_complement(bytes: &[u8]) -> Vec<u8> {
    let mut result = bytes.to_vec();
    let mut carry = 1u8;
    for byte in result.iter_mut().rev() {
        let (negated, overflow) = (!*byte).overflowing_add(carry);
        *byte = negated;
        carry = u8::from(overflow);
    }
    result
}

#[cfg(test)]
mod tests {
    use rsa::pkcs8::DecodePublicKey;
    use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

    use super::*;

    fn from_hex(input: &str) -> Vec<u8> {
        input
            .as_bytes()
            .chunks(2)
            .map(|pair| {
                let high = (pair[0] as char).to_digit(16).expect("hex digit") as u8;
                let low = (pair[1] as char).to_digit(16).expect("hex digit") as u8;
                (high << 4) | low
            })
            .collect()
    }

    #[test]
    fn cfb8_matches_reference_stream() {
        // Vector de referencia con clave == IV, como en Minecraft (el shared
        // secret es a la vez clave AES-128 e IV).
        let key: [u8; 16] = from_hex("2b7e151628aed2a6abf7158809cf4f3c")
            .try_into()
            .unwrap();
        let plaintext = from_hex(
            "68656c6c6f20776f726c642c20746869732069732061206c6f6e67657220434642382073747265616d2074657374203132333435363738393020616263646566",
        );
        let expected_ciphertext = from_hex(
            "175135cac569680ce764c4a6367616ea0cb388b94421785a0b2d3fee0c412b59050939e81df5124933440bf76eb64caa59d746760d75cf4c14f0bd29955c7649",
        );

        let mut encrypted = plaintext.clone();
        Cfb8Stream::new(&key).encrypt(&mut encrypted);
        assert_eq!(encrypted, expected_ciphertext);

        let mut decrypted = expected_ciphertext.clone();
        Cfb8Stream::new(&key).decrypt(&mut decrypted);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn rsa_keypair_round_trips_a_message() {
        let (public_key_der, private_key) =
            generate_rsa_keypair().expect("keypair should generate");
        let public_key =
            RsaPublicKey::from_public_key_der(&public_key_der).expect("public key should parse");
        let message = b"shared secret";
        let encrypted = public_key
            .encrypt(&mut OsRng, Pkcs1v15Encrypt, message)
            .expect("encrypt should work");
        let decrypted = decrypt_pkcs1v15(&private_key, &encrypted).expect("decrypt should work");

        assert_eq!(decrypted, message);
    }

    #[test]
    fn server_id_hash_matches_known_values() {
        let shared_secret = from_hex("00112233445566778899aabbccddeeff");
        let public_key = {
            let mut key =
                from_hex("30820122300d06092a864886f70d01010105000382010f003082010a0282010100");
            key.extend(1..80u8);
            key
        };
        assert_eq!(
            server_id_hash("", &shared_secret, &public_key),
            "5d5bd6e533cb847b128e247ac26c13f7642c988f"
        );

        // Hash con el bit alto puesto → representación negativa.
        let negative_secret = from_hex("ffeeddccbbaa99887766554433221100");
        let negative_public_key = {
            let mut key = vec![0x80];
            key.extend(0..150u8);
            key
        };
        assert_eq!(
            server_id_hash("", &negative_secret, &negative_public_key),
            "-52318f67e68f2980b189943a75d80dc2d5173899"
        );
    }
}
