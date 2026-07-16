// SPDX-License-Identifier: Apache-2.0
//! Exhaustive source-byte accounting and identity-preserving record retention.

use std::ops::Range;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use serde::Serialize;

use crate::container::{Record, Scan};

/// One non-overlapping source span in archive order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ByteSpan {
    id: String,
    offset: u64,
    byte_len: u64,
    classification: &'static str,
    kind: String,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    opaque_record: Option<String>,
}

/// One complete named record retained independently of its typed projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct OpaqueRecord {
    id: String,
    offset: u64,
    byte_len: u64,
    typecode: String,
    sha256: String,
    #[serde(with = "cadmpeg_ir::bytes")]
    data: Vec<u8>,
}

fn span(
    data: &[u8],
    range: Range<usize>,
    classification: &'static str,
    kind: impl Into<String>,
    opaque_record: Option<String>,
) -> ByteSpan {
    ByteSpan {
        id: format!("rhino:source:span#{:012x}", range.start),
        offset: range.start as u64,
        byte_len: range.len() as u64,
        classification,
        kind: kind.into(),
        sha256: sha256_hex(&data[range]),
        opaque_record,
    }
}

fn retained(data: &[u8], record: &Record, id: String) -> OpaqueRecord {
    OpaqueRecord {
        id,
        offset: record.range.start as u64,
        byte_len: record.range.len() as u64,
        typecode: format!("{:#010x}", record.typecode),
        sha256: sha256_hex(&data[record.range.clone()]),
        data: data[record.range.clone()].to_vec(),
    }
}

/// Installs a complete partition of the source archive and preserves every
/// direct non-object table record under a stable source identity.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let data = scan.data;
    let mut spans = Vec::new();
    let mut opaque = Vec::new();
    let object_orders = scan
        .objects
        .iter()
        .enumerate()
        .map(|(source_order, object)| (object.range.start, source_order))
        .collect::<std::collections::BTreeMap<_, _>>();

    spans.push(span(data, 0..32, "typed", "archive_header", None));
    let comment_id = "rhino:source:opaque#comment".to_string();
    opaque.push(retained(data, &scan.comment, comment_id.clone()));
    spans.push(span(
        data,
        scan.comment.range.clone(),
        "opaque",
        "comment_chunk",
        Some(comment_id),
    ));

    for (table_index, table) in scan.tables.iter().enumerate() {
        let mut cursor = table.range.start;
        if table.records.is_empty() && table.record_count != 0 {
            if cursor < table.body.start {
                spans.push(span(
                    data,
                    cursor..table.body.start,
                    "structural",
                    format!("table_{table_index:02}_framing"),
                    None,
                ));
            }
            let id = format!("rhino:source:opaque#table-{table_index:02}-record-stream");
            let synthetic = Record {
                typecode: table.typecode,
                range: table.body.start..table.range.end,
                body: table.body.clone(),
                short: false,
                value: 0,
            };
            opaque.push(retained(data, &synthetic, id.clone()));
            spans.push(span(
                data,
                synthetic.range,
                "opaque",
                format!("table_{table_index:02}_record_stream"),
                Some(id),
            ));
            continue;
        }
        for (record_index, record) in table.records.iter().enumerate() {
            if cursor < record.range.start {
                spans.push(span(
                    data,
                    cursor..record.range.start,
                    "structural",
                    format!("table_{table_index:02}_framing"),
                    None,
                ));
            }
            let kind = format!(
                "table_{table_index:02}_record_{record_index:06}_{:#010x}",
                record.typecode
            );
            if record.typecode == 0x2000_8070 {
                let source_order = object_orders
                    .get(&record.range.start)
                    .copied()
                    .expect("scanned object record has a global source order");
                spans.push(span(
                    data,
                    record.range.clone(),
                    "opaque",
                    kind,
                    Some(format!("rhino:object:record#{source_order:06}")),
                ));
            } else {
                let id =
                    format!("rhino:source:opaque#table-{table_index:02}-record-{record_index:06}");
                opaque.push(retained(data, record, id.clone()));
                spans.push(span(data, record.range.clone(), "opaque", kind, Some(id)));
            }
            cursor = record.range.end;
        }
        if cursor < table.range.end {
            spans.push(span(
                data,
                cursor..table.range.end,
                "structural",
                format!("table_{table_index:02}_terminator_and_checksum"),
                None,
            ));
        }
    }
    if scan.eof_offset < data.len() {
        spans.push(span(
            data,
            scan.eof_offset..data.len(),
            "structural",
            "end_of_file_chunk",
            None,
        ));
    }

    spans.sort_by_key(|item| item.offset);
    let mut cursor = 0_u64;
    for item in &spans {
        assert_eq!(item.offset, cursor, "Rhino byte-accounting gap or overlap");
        cursor += item.byte_len;
    }
    assert_eq!(
        cursor,
        data.len() as u64,
        "Rhino byte-accounting suffix gap"
    );

    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("byte_spans", &spans)
        .expect("Rhino byte spans serialize");
    namespace
        .set_arena("opaque_records", &opaque)
        .expect("Rhino opaque records serialize");
}
