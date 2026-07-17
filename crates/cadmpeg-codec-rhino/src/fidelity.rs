// SPDX-License-Identifier: Apache-2.0
//! Source-fidelity sidecar for the Rhino 3DM container.
//!
//! [`ledger`] projects the single [`partition`](crate::accounting::partition) of
//! a completed [`Scan`] into a v2 source-fidelity sidecar
//! ([`SourceFidelity`]). The root `source` space is tiled so every physical byte
//! is classified — the 32-byte archive header `Typed`, the leading comment and
//! each dissected table record one `Opaque` span, and all container framing
//! (table headers, end-of-table markers, checksums, the end-of-file chunk)
//! `Structural`. There is no derived space: the 3DM container stores its tables
//! uncompressed in the root stream, so record granularity is reached without
//! any transform.
//!
//! The declared [`LedgerLevel`] tracks the coarsest table in the partition. When
//! every table is dissected to record granularity the sidecar is
//! [`LedgerLevel::L2`] (complete *refined* tiling). A table whose records the
//! scanner does not retain individually (e.g. the user table) is emitted as one
//! undissected `TableRecordStream` `Opaque` span — coarse (L1) granularity — and
//! caps the whole sidecar at [`LedgerLevel::L1`], since the level is a single
//! scalar and one coarse table means not every payload is refined.
//!
//! The tiling is total and canonical by construction: [`partition`] walks the
//! scan in archive order and yields a gap-free ascending partition of the whole
//! image, so [`SourceFidelity::new`] returns byte-identical sidecars for repeat
//! decodes of the same input. The capability is [`LedgerCapability::Accounted`]:
//! every span carries a digest, and byte recovery rides the codec's native
//! opaque-record store rather than the platform retained store (see
//! `issue_object_tickets`).

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SourceFidelity,
};

use crate::accounting::{partition, TileRole};
use crate::container::Scan;

/// Owner label attributed to every span this codec emits.
const OWNER: &str = "rhino";

/// Build the source-fidelity sidecar for a scanned Rhino container.
///
/// The returned sidecar is canonicalized and has passed
/// [`SourceFidelity::validate`]; its single `source` space tiles `[0, length)`
/// exactly. The declared level is [`LedgerLevel::L2`] when every table is
/// dissected to record granularity, or [`LedgerLevel::L1`] when any table is
/// emitted as a single undissected record stream. The tiling is total by
/// construction, so a validation failure would be a builder defect and panics
/// rather than being serialized.
pub(crate) fn ledger(scan: &Scan<'_>) -> SourceFidelity {
    let data = scan.data;
    let tiles = partition(scan);
    // A table whose records the scanner does not retain individually is emitted
    // as one undissected `TableRecordStream` span, which is coarse (L1)
    // granularity. The level is a single scalar for the whole sidecar, so one
    // such table caps the ledger at L1; only a partition that reaches record
    // granularity everywhere earns L2.
    let level = if tiles
        .iter()
        .any(|tile| matches!(tile.role, TileRole::TableRecordStream { .. }))
    {
        LedgerLevel::L1
    } else {
        LedgerLevel::L2
    };
    let spans = tiles
        .into_iter()
        .map(|tile| LedgerSpan {
            range: SerializedRange {
                start: tile.range.start as u64,
                end: tile.range.end as u64,
            },
            class: tile.class,
            owner: OWNER.to_string(),
            meaning: tile.meaning,
            digest: sha256_hex(&data[tile.range]),
            retained: None,
        })
        .collect();
    let source = AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length: data.len() as u64,
        origin: SerializedOrigin::Root,
        spans,
    };
    let sidecar = SourceFidelity::new(level, LedgerCapability::Accounted, vec![source]);
    sidecar
        .validate()
        .expect("Rhino source-fidelity ledger tiles the source space completely");
    sidecar
}
