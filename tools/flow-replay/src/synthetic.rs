//! Builds well-formed, synthetic IPFIX messages for safely testing the
//! collector — see docs/security-principles.md: "Use only synthetic
//! telemetry, sanitized telemetry, lab networks, authorized
//! environments" and "Never generate real attack traffic." Nothing in
//! this module reads captured packets or real customer data; every
//! value is either a caller-supplied parameter or a fixed constant.

const IPFIX_VERSION: u16 = 0x000a;
const TEMPLATE_SET_ID: u16 = 2;

/// The template ID this module's synthetic messages use throughout —
/// arbitrary but fixed, so `template_message` and `data_message` always
/// agree with each other.
pub const TEMPLATE_ID: u16 = 256;

fn message_header(length: u16, sequence_number: u32, observation_domain_id: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&IPFIX_VERSION.to_be_bytes());
    b.extend_from_slice(&length.to_be_bytes());
    b.extend_from_slice(
        &(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0))
        .to_be_bytes(),
    );
    b.extend_from_slice(&sequence_number.to_be_bytes());
    b.extend_from_slice(&observation_domain_id.to_be_bytes());
    b
}

/// A Template Set defining [`TEMPLATE_ID`] with three fixed-length
/// fields: sourceIPv4Address (IE 8), destinationIPv4Address (IE 12), and
/// packetDeltaCount (IE 2, 8 bytes) — IE numbers per the public IANA
/// IPFIX Information Elements registry.
pub fn template_message(sequence_number: u32, observation_domain_id: u32) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&TEMPLATE_ID.to_be_bytes());
    record.extend_from_slice(&3u16.to_be_bytes()); // field_count
    record.extend_from_slice(&8u16.to_be_bytes()); // sourceIPv4Address
    record.extend_from_slice(&4u16.to_be_bytes());
    record.extend_from_slice(&12u16.to_be_bytes()); // destinationIPv4Address
    record.extend_from_slice(&4u16.to_be_bytes());
    record.extend_from_slice(&2u16.to_be_bytes()); // packetDeltaCount
    record.extend_from_slice(&8u16.to_be_bytes());

    let mut set = Vec::new();
    set.extend_from_slice(&TEMPLATE_SET_ID.to_be_bytes());
    set.extend_from_slice(&((4 + record.len()) as u16).to_be_bytes());
    set.extend_from_slice(&record);

    let mut message = message_header(
        (16 + set.len()) as u16,
        sequence_number,
        observation_domain_id,
    );
    message.extend_from_slice(&set);
    message
}

/// A Data Set of one record matching [`template_message`]'s template.
pub fn data_message(
    sequence_number: u32,
    observation_domain_id: u32,
    source_ipv4: [u8; 4],
    destination_ipv4: [u8; 4],
    packet_count: u64,
) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&source_ipv4);
    record.extend_from_slice(&destination_ipv4);
    record.extend_from_slice(&packet_count.to_be_bytes());

    let mut set = Vec::new();
    set.extend_from_slice(&TEMPLATE_ID.to_be_bytes());
    set.extend_from_slice(&((4 + record.len()) as u16).to_be_bytes());
    set.extend_from_slice(&record);

    let mut message = message_header(
        (16 + set.len()) as u16,
        sequence_number,
        observation_domain_id,
    );
    message.extend_from_slice(&set);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_message_has_a_well_formed_header() {
        let msg = template_message(1, 7);
        assert_eq!(u16::from_be_bytes([msg[0], msg[1]]), IPFIX_VERSION);
        let declared_len = u16::from_be_bytes([msg[2], msg[3]]);
        assert_eq!(declared_len as usize, msg.len());
    }

    #[test]
    fn data_message_carries_the_requested_sequence_number() {
        let msg = data_message(42, 7, [1, 2, 3, 4], [5, 6, 7, 8], 99);
        let seq = u32::from_be_bytes([msg[8], msg[9], msg[10], msg[11]]);
        assert_eq!(seq, 42);
    }

    /// The real payoff of a synthetic-fixture module: round-trip it
    /// through the actual decoder and confirm the values come back out
    /// unchanged. This exercises `wetechinetmon-protocol-ipfix` the same
    /// way a live exporter's traffic would, without ever touching a real
    /// network capture.
    #[test]
    fn round_trips_through_the_real_ipfix_decoder() {
        use wetechinetmon_protocol_ipfix::{decode_message, DecodedSet, TemplateCache};

        let mut cache = TemplateCache::new();

        let tmpl_msg = template_message(1, 7);
        let decoded_tmpl = decode_message(&tmpl_msg, &mut cache).unwrap();
        assert!(matches!(decoded_tmpl.sets[0], DecodedSet::Templates(_)));

        let data_msg = data_message(2, 7, [10, 0, 0, 1], [10, 0, 0, 2], 1234);
        let decoded_data = decode_message(&data_msg, &mut cache).unwrap();
        match &decoded_data.sets[0] {
            DecodedSet::Data { records, .. } => {
                assert_eq!(records.len(), 1);
                assert_eq!(
                    records[0].fields[0].as_ipv4(),
                    Some(std::net::Ipv4Addr::new(10, 0, 0, 1))
                );
                assert_eq!(
                    records[0].fields[1].as_ipv4(),
                    Some(std::net::Ipv4Addr::new(10, 0, 0, 2))
                );
                assert_eq!(records[0].fields[2].as_u64_be(), Some(1234));
            }
            other => panic!("expected a Data set, got {other:?}"),
        }
    }
}
