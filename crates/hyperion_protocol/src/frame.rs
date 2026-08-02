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
    pub payload: Vec<u8>,
}

/// Decode a complete uncompressed Minecraft packet frame.
pub fn decode_frame(input: &[u8]) -> Result<(PacketFrame, usize), ProtocolError> {
    let (packet_length, packet_data_offset) =
        decode_var_i32_at(input, 0, 0, 0, 0, MAX_PACKET_LENGTH_VARINT_BYTES)?;
    let packet_length =
        usize::try_from(packet_length).map_err(|_| ProtocolError::InvalidPacketLength)?;

    if packet_length == 0 {
        return Err(ProtocolError::InvalidPacketLength);
    }
    if packet_length > MAX_PACKET_LENGTH {
        return Err(ProtocolError::PacketTooLarge);
    }

    let packet_end = packet_data_offset
        .checked_add(packet_length)
        .ok_or(ProtocolError::PacketTooLarge)?;
    let packet_bytes = input
        .get(packet_data_offset..packet_end)
        .ok_or(ProtocolError::UnexpectedEndOfInput)?;
    let (packet_id, packet_id_length) =
        decode_var_i32_at(packet_bytes, 0, 0, 0, 0, MAX_GENERAL_VARINT_BYTES)?;
    let payload = packet_bytes
        .get(packet_id_length..)
        .ok_or(ProtocolError::InvalidPacketPayload)?
        .to_vec();

    Ok((PacketFrame { packet_id, payload }, packet_end))
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

pub(crate) fn encode_var_i32(value: i32) -> Vec<u8> {
    encode_unsigned_var_i32(value as u32)
}

pub(crate) fn encode_string(
    value: &str,
    maximum_utf16_units: usize,
) -> Result<Vec<u8>, ProtocolError> {
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

    pub(crate) fn finish(&self) -> Result<(), ProtocolError> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidPacketPayload)
        }
    }
}

fn encode_unsigned_var_i32(value: u32) -> Vec<u8> {
    let encoded_byte = (value as u8) & 0x7f;
    let remaining_value = value >> 7;

    if remaining_value == 0 {
        vec![encoded_byte]
    } else {
        [
            vec![encoded_byte | 0x80],
            encode_unsigned_var_i32(remaining_value),
        ]
        .concat()
    }
}

fn decode_var_i32_at(
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
