// SPDX-License-Identifier: Apache-2.0
//! Source E5 edge operands and their allocation terminals.

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::e5::graph::{self, E5EdgeAllocationTerminal};
use crate::layout::e5_record_frame as header;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaE5EdgeRecord {
    pub(crate) id: String,
    byte_offset: u64,
    byte_len: u64,
    record_id: u32,
    object_references: [u32; 5],
    allocation_terminal: E5EdgeAllocationTerminal,
}

fn records(bytes: &[u8]) -> impl Iterator<Item = CatiaE5EdgeRecord> + '_ {
    crate::container::all_e5_record_spans(bytes).filter_map(|span| {
        let frame = bytes.get(span.clone())?;
        if frame.get(header::CLASS) != Some(&0xff) {
            return None;
        }
        let edge = graph::parse_edge_record(frame.get(header::LEN..)?)?;
        Some(CatiaE5EdgeRecord {
            id: format!("catia:e5:edge-use#{}", span.start),
            byte_offset: u64_from_index(span.start),
            byte_len: u64_from_index(span.len()),
            record_id: View::u32_le_at(frame, header::RECORD_ID)?,
            object_references: edge.references,
            allocation_terminal: edge.allocation_terminal,
        })
    })
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CatiaE5EdgeRecord>, CodecError> {
    let mut source = records(bytes);
    let Some(first) = source.next() else {
        return Ok(Vec::new());
    };
    let count = source.count() + 1;
    let minimum = header::LEN + 1 + 5 + 1;
    if bounded_len(u64_from_index(count), minimum, bytes.len()).is_none() {
        return Err(CodecError::malformed(
            "E5 edge count exceeds its source bytes",
        ));
    }
    ctx.charge_entities(u64_from_index(count), "admit CATIA E5 edge records")?;
    let mut output = ctx.alloc_filled(count, first, "CATIA E5 edge records")?;
    for (destination, source) in output.iter_mut().zip(records(bytes)) {
        *destination = source;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::derive;
    use crate::test_support::test_e5::append_e5_record;

    fn frame(terminal: &[u8], id: u32) -> Vec<u8> {
        let mut payload = vec![0x85, 0x8d, 0x8a, 0x90, 0x8e, 0x8f];
        payload.extend_from_slice(terminal);
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xff, id, &payload);
        bytes
    }

    #[test]
    fn unused_physical_edges_keep_repeated_persistent_ids_and_terminal_forms() {
        let mut bytes = Vec::new();
        for terminal in [0x01, 0x02, 0x21, 0x22, 0x25, 0x29, 0x2a] {
            bytes.extend(frame(&[terminal], 7));
        }
        let records = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes))
            .expect("bounded source records");
        assert_eq!(records.len(), 7);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record.id, format!("catia:e5:edge-use#{}", index * 20));
            assert_eq!(
                record.byte_offset,
                cadmpeg_core::decode::u64_from_index(index * 20)
            );
            assert_eq!(record.byte_len, 20);
            assert_eq!(record.record_id, 7);
            assert_eq!(record.object_references, [13, 10, 16, 14, 15]);
        }
        let wire = serde_json::to_value(&records).expect("stored edge records");
        for (row, (raw, value, encoding)) in wire.as_array().expect("record array").iter().zip([
            (1, 0, "backward_distance"),
            (2, 0, "selector2"),
            (33, 8, "backward_distance"),
            (34, 8, "selector2"),
            (37, 9, "backward_distance"),
            (41, 10, "backward_distance"),
            (42, 10, "selector2"),
        ]) {
            assert_eq!(
                row["allocation_terminal"],
                serde_json::json!({
                    "raw_byte": raw, "value": value, "encoding": encoding
                })
            );
        }
        assert_eq!(
            serde_json::from_value::<Vec<super::CatiaE5EdgeRecord>>(wire)
                .expect("load exact terminal forms"),
            records
        );
    }

    #[test]
    fn shifted_reference_and_unresolved_operands_remain_source_values() {
        let mut bytes = Vec::new();
        append_e5_record(
            &mut bytes,
            0xff,
            7,
            &[0x85, 0x30, 0x3e, 0xb9, 0x8a, 0x90, 0x8e, 0x8f, 0x21],
        );
        let records = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes))
            .expect("bounded source operands");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].object_references, [12_140_032, 10, 16, 14, 15]);
        assert_eq!(records[0].byte_len, 22);
    }

    #[test]
    fn allocation_field_mismatches_and_unsupported_forms_do_not_load() {
        let records =
            crate::test_support::with_service_context(|ctx| derive(ctx, &frame(&[0x21], 7)))
                .expect("source records");
        let wire = serde_json::to_value(&records).expect("stored records");
        for bad in [
            serde_json::json!({"raw_byte":33,"value":10,"encoding":"backward_distance"}),
            serde_json::json!({"raw_byte":33,"value":8,"encoding":"selector2"}),
            serde_json::json!({"raw_byte":38,"value":9,"encoding":"selector2"}),
        ] {
            let mut malformed = wire.clone();
            malformed[0]["allocation_terminal"] = bad;
            assert!(serde_json::from_value::<Vec<super::CatiaE5EdgeRecord>>(malformed).is_err());
        }
    }

    #[test]
    fn malformed_terminal_payloads_are_not_typed_edge_records() {
        let mut bytes = Vec::new();
        for terminal in [vec![], vec![0x26], vec![0x21, 0], vec![0x08, 8], vec![0x7f]] {
            bytes.extend(frame(&terminal, 7));
        }
        let records = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes))
            .expect("source retained by the container");
        assert!(records.is_empty());
    }

    #[test]
    fn delayed_first_record_keeps_the_exact_entity_quota_and_order() {
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xc0, 11, &[0; 64]);
        let first_offset = bytes.len();
        bytes.extend(frame(&[0x21], 7));
        bytes.extend(frame(&[0x22], 9));
        assert!(crate::test_support::with_entity_limit(1, |ctx| derive(ctx, &bytes)).is_err());
        let admitted = crate::test_support::with_entity_limit(2, |ctx| derive(ctx, &bytes))
            .expect("both stored records fit the entity quota");
        assert_eq!(admitted.len(), 2);
        assert_eq!(admitted[0].record_id, 7);
        assert_eq!(admitted[1].record_id, 9);
        assert_eq!(
            admitted[0].byte_offset,
            cadmpeg_core::decode::u64_from_index(first_offset)
        );
        assert_eq!(
            admitted[1].byte_offset,
            cadmpeg_core::decode::u64_from_index(first_offset + 20)
        );
        assert!(
            crate::test_support::with_entity_limit(0, |ctx| derive(ctx, &[]))
                .expect("no edge needs no entity quota")
                .is_empty()
        );
    }

    #[test]
    fn framed_payload_does_not_supply_an_embedded_edge_record() {
        let fake = frame(&[0x21], 7);
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xc0, 8, &fake);
        bytes.extend(frame(&[0x22], 9));
        let records = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes))
            .expect("bounded physical records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_id, 9);
        assert_eq!(records[0].byte_offset, 33);
    }
}
