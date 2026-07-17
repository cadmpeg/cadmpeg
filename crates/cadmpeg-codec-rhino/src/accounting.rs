// SPDX-License-Identifier: Apache-2.0
//! Exhaustive source-byte accounting and identity-preserving record retention.
//!
//! [`partition`] walks a completed [`Scan`] once and returns a gap-free ascending
//! tiling of the whole source image. Two consumers ride that single partition so
//! the native byte-census and the platform source-fidelity sidecar can never
//! diverge: [`install`] projects it into the `rhino` native namespace (byte spans
//! plus retained record bytes for recovery), and [`crate::fidelity::ledger`]
//! projects it into a source-fidelity sidecar whose
//! [`LedgerLevel`](cadmpeg_ir::LedgerLevel) is `L2` when every table reaches
//! record granularity, or `L1` when any table is emitted as one undissected
//! record stream.

use std::ops::Range;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::SpanClass;
use serde::Serialize;

use crate::container::Scan;

/// The object-record typecode; its retained bytes live under the object-record
/// identity, not the generic source-opaque namespace.
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;

/// The retention role of one partition tile.
///
/// Framing and the typed header carry no retained identity. Every opaque tile
/// names how its bytes are recovered so [`install`] can mint a stable id and,
/// where the codec owns the bytes, retain them for recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TileRole {
    /// Container framing; structural, not retained.
    Framing,
    /// The 32-byte archive header; typed, not retained.
    Header,
    /// The leading comment chunk, retained under a fixed identity.
    Comment,
    /// An object record; recovered under the object-record identity, not the
    /// generic opaque namespace.
    ObjectRecord {
        /// Global source order of the object record.
        source_order: usize,
    },
    /// A retained non-object table record.
    TableRecord {
        /// Source-order index of the owning table.
        table_index: usize,
        /// Source-order index of the record within its table.
        record_index: usize,
    },
    /// A whole undissected table record stream (a table whose records the
    /// scanner does not retain individually, e.g. the user table).
    TableRecordStream {
        /// Source-order index of the owning table.
        table_index: usize,
    },
}

/// One non-overlapping tile of the source image, in archive order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tile {
    /// Byte range within the source image.
    pub(crate) range: Range<usize>,
    /// Byte classification.
    pub(crate) class: SpanClass,
    /// Human-readable meaning label.
    pub(crate) meaning: String,
    /// How this tile's bytes are recovered.
    pub(crate) role: TileRole,
}

/// Build the complete gap-free partition of the source image in archive order.
///
/// The tiles tile `[0, data.len())` exactly: header, comment, then each table's
/// framing, records, and terminator, and finally any end-of-file chunk. The
/// caller may assert contiguity; both projections rely on it.
pub(crate) fn partition(scan: &Scan<'_>) -> Vec<Tile> {
    let data = scan.data;
    let mut tiles = Vec::new();
    let object_orders = scan
        .objects
        .iter()
        .enumerate()
        .map(|(source_order, object)| (object.range.start, source_order))
        .collect::<std::collections::BTreeMap<_, _>>();

    tiles.push(Tile {
        range: 0..32,
        class: SpanClass::Typed,
        meaning: "archive_header".to_string(),
        role: TileRole::Header,
    });
    tiles.push(Tile {
        range: scan.comment.range.clone(),
        class: SpanClass::Opaque,
        meaning: "comment_chunk".to_string(),
        role: TileRole::Comment,
    });

    for (table_index, table) in scan.tables.iter().enumerate() {
        let mut cursor = table.range.start;
        if table.records.is_empty() && table.record_count != 0 {
            if cursor < table.body.start {
                tiles.push(Tile {
                    range: cursor..table.body.start,
                    class: SpanClass::Structural,
                    meaning: format!("table_{table_index:02}_framing"),
                    role: TileRole::Framing,
                });
            }
            tiles.push(Tile {
                range: table.body.start..table.range.end,
                class: SpanClass::Opaque,
                meaning: format!("table_{table_index:02}_record_stream"),
                role: TileRole::TableRecordStream { table_index },
            });
            continue;
        }
        for (record_index, record) in table.records.iter().enumerate() {
            if cursor < record.range.start {
                tiles.push(Tile {
                    range: cursor..record.range.start,
                    class: SpanClass::Structural,
                    meaning: format!("table_{table_index:02}_framing"),
                    role: TileRole::Framing,
                });
            }
            let meaning = format!(
                "table_{table_index:02}_record_{record_index:06}_{:#010x}",
                record.typecode
            );
            let role = if record.typecode == TCODE_OBJECT_RECORD {
                let source_order = object_orders
                    .get(&record.range.start)
                    .copied()
                    .expect("scanned object record has a global source order");
                TileRole::ObjectRecord { source_order }
            } else {
                TileRole::TableRecord {
                    table_index,
                    record_index,
                }
            };
            tiles.push(Tile {
                range: record.range.clone(),
                class: SpanClass::Opaque,
                meaning,
                role,
            });
            cursor = record.range.end;
        }
        if cursor < table.range.end {
            tiles.push(Tile {
                range: cursor..table.range.end,
                class: SpanClass::Structural,
                meaning: format!("table_{table_index:02}_terminator_and_checksum"),
                role: TileRole::Framing,
            });
        }
    }
    if scan.eof_offset < data.len() {
        tiles.push(Tile {
            range: scan.eof_offset..data.len(),
            class: SpanClass::Structural,
            meaning: "end_of_file_chunk".to_string(),
            role: TileRole::Framing,
        });
    }

    let mut cursor = 0_usize;
    for tile in &tiles {
        assert_eq!(
            tile.range.start, cursor,
            "Rhino byte-accounting gap or overlap"
        );
        cursor = tile.range.end;
    }
    assert_eq!(cursor, data.len(), "Rhino byte-accounting suffix gap");
    tiles
}

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

fn classification(class: SpanClass) -> &'static str {
    match class {
        SpanClass::Typed => "typed",
        SpanClass::Structural => "structural",
        SpanClass::Opaque => "opaque",
    }
}

fn opaque_record(data: &[u8], id: String, range: Range<usize>, typecode: u32) -> OpaqueRecord {
    OpaqueRecord {
        id,
        offset: range.start as u64,
        byte_len: range.len() as u64,
        typecode: format!("{typecode:#010x}"),
        sha256: sha256_hex(&data[range.clone()]),
        data: data[range].to_vec(),
    }
}

/// Installs a complete partition of the source archive and preserves every
/// direct non-object table record under a stable source identity.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let data = scan.data;
    let mut spans = Vec::new();
    let mut opaque = Vec::new();

    for tile in partition(scan) {
        let record_id = match &tile.role {
            TileRole::Framing | TileRole::Header => None,
            TileRole::Comment => {
                let id = "rhino:source:opaque#comment".to_string();
                opaque.push(opaque_record(
                    data,
                    id.clone(),
                    tile.range.clone(),
                    scan.comment.typecode,
                ));
                Some(id)
            }
            TileRole::ObjectRecord { source_order } => {
                Some(format!("rhino:object:record#{source_order:06}"))
            }
            TileRole::TableRecord {
                table_index,
                record_index,
            } => {
                let id =
                    format!("rhino:source:opaque#table-{table_index:02}-record-{record_index:06}");
                let typecode = scan.tables[*table_index].records[*record_index].typecode;
                opaque.push(opaque_record(
                    data,
                    id.clone(),
                    tile.range.clone(),
                    typecode,
                ));
                Some(id)
            }
            TileRole::TableRecordStream { table_index } => {
                let id = format!("rhino:source:opaque#table-{table_index:02}-record-stream");
                let typecode = scan.tables[*table_index].typecode;
                opaque.push(opaque_record(
                    data,
                    id.clone(),
                    tile.range.clone(),
                    typecode,
                ));
                Some(id)
            }
        };
        spans.push(ByteSpan {
            id: format!("rhino:source:span#{:012x}", tile.range.start),
            offset: tile.range.start as u64,
            byte_len: tile.range.len() as u64,
            classification: classification(tile.class),
            kind: tile.meaning,
            sha256: sha256_hex(&data[tile.range]),
            opaque_record: record_id,
        });
    }

    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("byte_spans", &spans)
        .expect("Rhino byte spans serialize");
    namespace
        .set_arena("opaque_records", &opaque)
        .expect("Rhino opaque records serialize");
}
