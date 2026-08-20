use crate::error::DecodeError;

/// IPFIX version number, per RFC 7011 §3.1. Any other value in the
/// version field means this is not an IPFIX message (it may be a
/// different protocol entirely, e.g. NetFlow v9, sent to the wrong port).
pub const IPFIX_VERSION: u16 = 0x000a;

/// The 16-byte IPFIX Message Header (RFC 7011 §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub version: u16,
    /// Total message length in octets, including this 16-byte header.
    pub length: u16,
    /// Export Time: seconds since the UNIX epoch, per the exporter's clock.
    pub export_time: u32,
    pub sequence_number: u32,
    pub observation_domain_id: u32,
}

pub const MESSAGE_HEADER_LEN: usize = 16;

impl MessageHeader {
    /// Parses the fixed 16-byte header from the start of `input` and
    /// validates the version and declared length against `input`'s
    /// actual size. Does not consume `input` — callers use `length` to
    /// slice out the message body themselves.
    pub fn parse(input: &[u8]) -> Result<Self, DecodeError> {
        if input.len() < MESSAGE_HEADER_LEN {
            return Err(DecodeError::TooShort {
                needed: MESSAGE_HEADER_LEN,
                have: input.len(),
            });
        }

        let version = u16::from_be_bytes([input[0], input[1]]);
        if version != IPFIX_VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }

        let length = u16::from_be_bytes([input[2], input[3]]);
        if (length as usize) < MESSAGE_HEADER_LEN {
            return Err(DecodeError::MessageTooShortForHeader { declared: length });
        }
        if length as usize > input.len() {
            return Err(DecodeError::MessageLengthExceedsInput {
                declared: length,
                available: input.len(),
            });
        }

        let export_time = u32::from_be_bytes([input[4], input[5], input[6], input[7]]);
        let sequence_number = u32::from_be_bytes([input[8], input[9], input[10], input[11]]);
        let observation_domain_id =
            u32::from_be_bytes([input[12], input[13], input[14], input[15]]);

        Ok(MessageHeader {
            version,
            length,
            export_time,
            sequence_number,
            observation_domain_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_header_bytes(length: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&IPFIX_VERSION.to_be_bytes());
        b.extend_from_slice(&length.to_be_bytes());
        b.extend_from_slice(&1_700_000_000u32.to_be_bytes()); // export_time
        b.extend_from_slice(&42u32.to_be_bytes()); // sequence_number
        b.extend_from_slice(&7u32.to_be_bytes()); // observation_domain_id
        b
    }

    #[test]
    fn parses_a_well_formed_header() {
        let bytes = valid_header_bytes(16);
        let header = MessageHeader::parse(&bytes).unwrap();
        assert_eq!(header.version, IPFIX_VERSION);
        assert_eq!(header.length, 16);
        assert_eq!(header.export_time, 1_700_000_000);
        assert_eq!(header.sequence_number, 42);
        assert_eq!(header.observation_domain_id, 7);
    }

    #[test]
    fn rejects_input_shorter_than_header() {
        let bytes = vec![0u8; 15];
        assert_eq!(
            MessageHeader::parse(&bytes),
            Err(DecodeError::TooShort {
                needed: 16,
                have: 15
            })
        );
    }

    #[test]
    fn rejects_wrong_version() {
        let mut bytes = valid_header_bytes(16);
        bytes[0] = 0x00;
        bytes[1] = 0x09; // version 9 = NetFlow v9, not IPFIX
        assert_eq!(
            MessageHeader::parse(&bytes),
            Err(DecodeError::UnsupportedVersion(9))
        );
    }

    #[test]
    fn rejects_declared_length_shorter_than_header() {
        let bytes = valid_header_bytes(10);
        assert_eq!(
            MessageHeader::parse(&bytes),
            Err(DecodeError::MessageTooShortForHeader { declared: 10 })
        );
    }

    #[test]
    fn rejects_declared_length_exceeding_input() {
        let bytes = valid_header_bytes(1000);
        assert_eq!(
            MessageHeader::parse(&bytes),
            Err(DecodeError::MessageLengthExceedsInput {
                declared: 1000,
                available: 16
            })
        );
    }

    #[test]
    fn never_panics_on_arbitrary_short_input() {
        for len in 0..20 {
            let bytes = vec![0xAAu8; len];
            let _ = MessageHeader::parse(&bytes);
        }
    }
}
