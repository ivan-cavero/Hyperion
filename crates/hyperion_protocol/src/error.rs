use thiserror::Error;

/// Errors returned when decoding or encoding Minecraft protocol data.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// The input ended before a complete value was available.
    #[error("unexpected end of protocol input")]
    UnexpectedEndOfInput,
    /// A VarInt exceeded the maximum number of bytes allowed in its context.
    #[error("VarInt exceeds the maximum length")]
    VarIntTooLong,
    /// A packet length was negative or otherwise invalid.
    #[error("invalid packet length")]
    InvalidPacketLength,
    /// A packet exceeded the protocol maximum.
    #[error("packet exceeds the maximum length")]
    PacketTooLarge,
    /// A length-prefixed string was not valid UTF-8.
    #[error("protocol string is not valid UTF-8")]
    InvalidUtf8,
    /// A protocol string exceeded its context-specific limit.
    #[error("protocol string exceeds its maximum length")]
    StringTooLong,
    /// A username contains characters outside the allowed set
    /// (`[a-zA-Z0-9_]`, as in vanilla).
    #[error("username contains invalid characters")]
    InvalidUsername,
    /// The packet ID does not match the expected packet.
    #[error("unexpected packet ID")]
    InvalidPacketId,
    /// The Handshake intent is not supported by this codec.
    #[error("invalid Handshake intent")]
    InvalidHandshakeIntent,
    /// A packet payload contains missing or trailing fields.
    #[error("invalid packet payload")]
    InvalidPacketPayload,
    /// Serializing a Status Response failed.
    #[error("could not serialize Status Response JSON: {0}")]
    JsonSerialization(String),
    /// An RSA or session-cipher operation failed.
    #[error("crypto operation failed: {0}")]
    Crypto(String),
    /// Decompressing a compressed packet failed.
    #[error("could not decompress packet: {0}")]
    Compression(String),
}
