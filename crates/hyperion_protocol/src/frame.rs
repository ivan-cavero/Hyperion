use bytes::Bytes;
use uuid::Uuid;

use crate::ProtocolError;

/// Maximum uncompressed packet length, including packet ID and payload.
pub const MAX_PACKET_LENGTH: usize = 2_097_151;

const MAX_GENERAL_VARINT_BYTES: usize = 5;
const MAX_PACKET_LENGTH_VARINT_BYTES: usize = 3;

/// A decoded Minecraft packet frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketFrame {
    /// Packet identifier, interpreted according to the connection state.
    pub packet_id: i32,
    /// Packet data after the packet identifier.
    pub payload: Bytes,
}

/// The result of splitting a frame: byte offsets into the original buffer.
///
/// Returned by [`split_frame`] so callers can extract the body without
/// copying: the body spans `[body_offset..body_offset + body_length)`, and
/// the total bytes consumed (length prefix + body) is `total_consumed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSplit {
    /// Offset where the frame body (packet ID + payload) begins.
    pub body_offset: usize,
    /// Length of the frame body in bytes.
    pub body_length: usize,
    /// Total bytes consumed from the input buffer (length prefix + body).
    pub total_consumed: usize,
}

/// Separa el prefijo de longitud (VarInt de hasta 3 bytes) del cuerpo de la
/// trama. Devuelve `Ok(None)` si el buffer aún no contiene la trama completa.
pub fn split_frame(input: &[u8]) -> Result<Option<FrameSplit>, ProtocolError> {
    let (packet_length, length_bytes) =
        match decode_var_i32(input, 0, MAX_PACKET_LENGTH_VARINT_BYTES) {
            Ok(pair) => pair,
            Err(ProtocolError::UnexpectedEndOfInput) => return Ok(None),
            Err(error) => return Err(error),
        };
    let packet_length =
        usize::try_from(packet_length).map_err(|_| ProtocolError::InvalidPacketLength)?;

    if packet_length == 0 {
        return Err(ProtocolError::InvalidPacketLength);
    }
    if packet_length > MAX_PACKET_LENGTH {
        return Err(ProtocolError::PacketTooLarge);
    }

    let body_length = packet_length;
    let body_end = length_bytes
        .checked_add(body_length)
        .ok_or(ProtocolError::PacketTooLarge)?;
    if input.len() < body_end {
        return Ok(None);
    }

    Ok(Some(FrameSplit {
        body_offset: length_bytes,
        body_length,
        total_consumed: body_end,
    }))
}

/// Decodes a packet (ID + payload) from bytes without a length prefix,
/// as they appear after decompressing a frame body.
///
/// Takes ownership of the input as [`Bytes`] so the payload can be sliced
/// zero-copy (O(1)) instead of copied out of a borrowed buffer.
pub fn decode_packet_data(input: Bytes) -> Result<PacketFrame, ProtocolError> {
    let (packet_id, packet_id_length) = decode_var_i32(&input, 0, MAX_GENERAL_VARINT_BYTES)?;
    if packet_id_length > input.len() {
        return Err(ProtocolError::InvalidPacketPayload);
    }
    let payload = input.slice(packet_id_length..);

    Ok(PacketFrame { packet_id, payload })
}

/// Decodes a complete uncompressed frame.
pub fn decode_frame(input: &[u8]) -> Result<(PacketFrame, usize), ProtocolError> {
    let split = split_frame(input)?.ok_or(ProtocolError::UnexpectedEndOfInput)?;
    let packet_bytes = &input[split.body_offset..split.total_consumed];

    Ok((
        decode_packet_data(Bytes::copy_from_slice(packet_bytes))?,
        split.total_consumed,
    ))
}

/// Encode an uncompressed Minecraft packet frame.
pub fn encode_frame(packet_id: i32, payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let packet_id_bytes = encode_var_i32(packet_id);
    let packet_length = packet_id_bytes
        .len()
        .checked_add(payload.len())
        .ok_or(ProtocolError::PacketTooLarge)?;

    if packet_length > MAX_PACKET_LENGTH {
        return Err(ProtocolError::PacketTooLarge);
    }

    let packet_length_i32 =
        i32::try_from(packet_length).map_err(|_| ProtocolError::PacketTooLarge)?;
    let length_bytes = encode_var_i32(packet_length_i32);

    Ok([length_bytes, packet_id_bytes, payload.to_vec()].concat())
}

pub fn encode_var_i32(value: i32) -> Vec<u8> {
    encode_unsigned_var_i32(value as u32)
}

pub fn encode_string(value: &str, maximum_utf16_units: usize) -> Result<Vec<u8>, ProtocolError> {
    let utf16_units = value.encode_utf16().count();
    let maximum_bytes = maximum_utf16_units
        .checked_mul(3)
        .ok_or(ProtocolError::StringTooLong)?;
    if utf16_units > maximum_utf16_units || value.len() > maximum_bytes {
        return Err(ProtocolError::StringTooLong);
    }

    let value_length = i32::try_from(value.len()).map_err(|_| ProtocolError::StringTooLong)?;

    Ok([encode_var_i32(value_length), value.as_bytes().to_vec()].concat())
}

pub fn encode_boolean(value: bool) -> Vec<u8> {
    if value { vec![1] } else { vec![0] }
}

pub(crate) fn encode_bytes(value: &[u8], maximum_length: usize) -> Result<Vec<u8>, ProtocolError> {
    if value.len() > maximum_length {
        return Err(ProtocolError::InvalidPacketPayload);
    }
    let value_length =
        i32::try_from(value.len()).map_err(|_| ProtocolError::InvalidPacketPayload)?;

    Ok([encode_var_i32(value_length), value.to_vec()].concat())
}

pub fn encode_var_i64(value: i64) -> Vec<u8> {
    let mut buffer = [0u8; 10];
    let mut remaining_value = value as u64;
    let mut index = 0;

    loop {
        let byte = (remaining_value as u8) & 0x7f;
        remaining_value >>= 7;
        if remaining_value == 0 {
            buffer[index] = byte;
            index += 1;
            break;
        } else {
            buffer[index] = byte | 0x80;
            index += 1;
        }
    }

    buffer[..index].to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_i32(value: i32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_i64(value: i64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_f32(value: f32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_f64(value: f64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_u16(value: u16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

#[allow(dead_code)] // used by hyperion_server
pub(crate) fn encode_byte(value: i8) -> Vec<u8> {
    vec![value as u8]
}

/// Encodes a block position: x in the 26 MSBs, z in the middle 26 bits,
/// y in the 12 LSBs.
pub(crate) fn encode_position(x: i32, y: i32, z: i32) -> Vec<u8> {
    let encoded = ((i64::from(x) & 0x3ff_ffff) << 38)
        | ((i64::from(z) & 0x3ff_ffff) << 12)
        | (i64::from(y) & 0xfff);
    encoded.to_be_bytes().to_vec()
}
pub(crate) fn encode_uuid(uuid: Uuid) -> Vec<u8> {
    uuid.as_bytes().to_vec()
}

pub(crate) struct PacketCursor<'input> {
    input: &'input [u8],
    offset: usize,
}

impl<'input> PacketCursor<'input> {
    pub(crate) const fn new(input: &'input [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub(crate) fn read_var_i32(&mut self) -> Result<i32, ProtocolError> {
        let (value, next_offset) =
            decode_var_i32_at(self.input, self.offset, 0, 0, 0, MAX_GENERAL_VARINT_BYTES)?;
        self.offset = next_offset;
        Ok(value)
    }

    pub(crate) fn read_string(
        &mut self,
        maximum_utf16_units: usize,
    ) -> Result<String, ProtocolError> {
        let byte_length = self.read_var_i32()?;
        let byte_length = usize::try_from(byte_length).map_err(|_| ProtocolError::StringTooLong)?;
        let maximum_bytes = maximum_utf16_units
            .checked_mul(3)
            .ok_or(ProtocolError::StringTooLong)?;
        if byte_length > maximum_bytes {
            return Err(ProtocolError::StringTooLong);
        }

        let string_bytes = self
            .input
            .get(self.offset..self.offset.saturating_add(byte_length))
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let string_value =
            std::str::from_utf8(string_bytes).map_err(|_| ProtocolError::InvalidUtf8)?;

        if string_value.encode_utf16().count() > maximum_utf16_units {
            return Err(ProtocolError::StringTooLong);
        }

        self.offset += byte_length;
        Ok(string_value.to_owned())
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(2)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let value_bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let value_bytes: [u8; 2] = value_bytes
            .try_into()
            .map_err(|_| ProtocolError::InvalidPacketPayload)?;

        self.offset = end_offset;
        Ok(u16::from_be_bytes(value_bytes))
    }

    pub(crate) fn read_uuid(&mut self) -> Result<Uuid, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(16)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let uuid_bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let uuid_bytes: [u8; 16] = uuid_bytes
            .try_into()
            .map_err(|_| ProtocolError::InvalidPacketPayload)?;

        self.offset = end_offset;
        Ok(Uuid::from_bytes(uuid_bytes))
    }

    pub(crate) fn read_byte_array(
        &mut self,
        maximum_length: usize,
    ) -> Result<Vec<u8>, ProtocolError> {
        let length = self.read_var_i32()?;
        let length = usize::try_from(length).map_err(|_| ProtocolError::InvalidPacketPayload)?;
        if length > maximum_length {
            return Err(ProtocolError::InvalidPacketPayload);
        }

        let end_offset = self
            .offset
            .checked_add(length)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;

        self.offset = end_offset;
        Ok(bytes.to_vec())
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool, ProtocolError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtocolError::InvalidPacketPayload),
        }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, ProtocolError> {
        let value = *self
            .input
            .get(self.offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        self.offset += 1;
        Ok(value)
    }

    pub(crate) fn read_i8(&mut self) -> Result<i8, ProtocolError> {
        Ok(self.read_u8()? as i8)
    }

    pub(crate) fn read_i64(&mut self) -> Result<i64, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(8)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let value_bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let value_bytes: [u8; 8] = value_bytes
            .try_into()
            .map_err(|_| ProtocolError::InvalidPacketPayload)?;

        self.offset = end_offset;
        Ok(i64::from_be_bytes(value_bytes))
    }

    pub(crate) fn read_f32(&mut self) -> Result<f32, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(4)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let value_bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let value_bytes: [u8; 4] = value_bytes
            .try_into()
            .map_err(|_| ProtocolError::InvalidPacketPayload)?;

        self.offset = end_offset;
        Ok(f32::from_be_bytes(value_bytes))
    }

    pub(crate) fn read_f64(&mut self) -> Result<f64, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(8)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let value_bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        let value_bytes: [u8; 8] = value_bytes
            .try_into()
            .map_err(|_| ProtocolError::InvalidPacketPayload)?;

        self.offset = end_offset;
        Ok(f64::from_be_bytes(value_bytes))
    }

    #[allow(dead_code)] // used by hyperion_server
    pub(crate) fn read_position(&mut self) -> Result<(i32, i32, i32), ProtocolError> {
        let value = self.read_i64()?;
        let x = (value >> 38) as i32;
        let y = (value << 52 >> 52) as i32;
        let z = (value << 26 >> 38) as i32;
        Ok((x, y, z))
    }

    pub(crate) fn read_fixed_bytes(&mut self, length: usize) -> Result<Vec<u8>, ProtocolError> {
        let end_offset = self
            .offset
            .checked_add(length)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let bytes = self
            .input
            .get(self.offset..end_offset)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;

        self.offset = end_offset;
        Ok(bytes.to_vec())
    }
    pub(crate) fn finish(&self) -> Result<(), ProtocolError> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidPacketPayload)
        }
    }
}

/// Buffered writer for building protocol payloads field by field.
///
/// Used by the Configuration and Play codecs, where packets contain many
/// heterogeneous fields (numbers, strings, positions, nested data).
#[derive(Debug, Default)]
pub(crate) struct ByteWriter {
    buffer: Vec<u8>,
}

impl ByteWriter {
    pub(crate) const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    #[allow(dead_code)] // used by hyperion_server
    pub(crate) fn len(&self) -> usize {
        self.buffer.len()
    }

    #[allow(dead_code)] // used by hyperion_server
    pub(crate) fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub(crate) fn push_var_i32(&mut self, value: i32) {
        self.buffer.extend_from_slice(&encode_var_i32(value));
    }

    pub(crate) fn push_var_i64(&mut self, value: i64) {
        self.buffer.extend_from_slice(&encode_var_i64(value));
    }

    pub(crate) fn push_i32(&mut self, value: i32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_i64(&mut self, value: i64) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_f32(&mut self, value: f32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_f64(&mut self, value: f64) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn push_bool(&mut self, value: bool) {
        self.buffer.push(u8::from(value));
    }

    pub(crate) fn push_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    pub(crate) fn push_byte(&mut self, value: i8) {
        self.buffer.push(value as u8);
    }

    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub(crate) fn push_position(&mut self, x: i32, y: i32, z: i32) {
        self.buffer.extend_from_slice(&encode_position(x, y, z));
    }

    pub(crate) fn push_string(
        &mut self,
        value: &str,
        maximum_utf16_units: usize,
    ) -> Result<(), ProtocolError> {
        let encoded = encode_string(value, maximum_utf16_units)?;
        self.buffer.extend_from_slice(&encoded);
        Ok(())
    }

    pub(crate) fn push_byte_array(
        &mut self,
        value: &[u8],
        maximum_length: usize,
    ) -> Result<(), ProtocolError> {
        let encoded = encode_bytes(value, maximum_length)?;
        self.buffer.extend_from_slice(&encoded);
        Ok(())
    }
}
fn encode_unsigned_var_i32(value: u32) -> Vec<u8> {
    let mut buffer = [0u8; MAX_GENERAL_VARINT_BYTES];
    let mut remaining_value = value;
    let mut index = 0;

    loop {
        let byte = (remaining_value as u8) & 0x7f;
        remaining_value >>= 7;
        if remaining_value == 0 {
            buffer[index] = byte;
            index += 1;
            break;
        } else {
            buffer[index] = byte | 0x80;
            index += 1;
        }
    }

    buffer[..index].to_vec()
}

/// Decodifica un VarInt desde `input[offset..]`, limitado a `maximum_bytes`.
pub fn decode_var_i32(
    input: &[u8],
    offset: usize,
    maximum_bytes: usize,
) -> Result<(i32, usize), ProtocolError> {
    decode_var_i32_at(input, offset, 0, 0, 0, maximum_bytes)
}

pub(crate) fn decode_var_i32_at(
    input: &[u8],
    offset: usize,
    accumulated_value: u32,
    shift: u32,
    bytes_read: usize,
    maximum_bytes: usize,
) -> Result<(i32, usize), ProtocolError> {
    if bytes_read == maximum_bytes {
        return Err(ProtocolError::VarIntTooLong);
    }

    let current_byte = *input
        .get(offset)
        .ok_or(ProtocolError::UnexpectedEndOfInput)?;
    let next_value = accumulated_value | (u32::from(current_byte & 0x7f) << shift);
    let next_offset = offset + 1;
    let next_bytes_read = bytes_read + 1;

    if current_byte & 0x80 == 0 {
        Ok((next_value as i32, next_offset))
    } else {
        decode_var_i32_at(
            input,
            next_offset,
            next_value,
            shift + 7,
            next_bytes_read,
            maximum_bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_int_encodes_negative_one_in_five_bytes() {
        assert_eq!(encode_var_i32(-1), vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    #[test]
    fn cursor_rejects_trailing_data() {
        let mut cursor = PacketCursor::new(&[0]);

        cursor.read_var_i32().expect("VarInt should decode");
        assert_eq!(cursor.finish(), Ok(()));
    }
}
