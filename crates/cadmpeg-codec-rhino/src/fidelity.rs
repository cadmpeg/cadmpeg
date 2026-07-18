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
//! The tiling is total and canonical by construction: [`partition`] walks the
//! scan in archive order and yields a gap-free ascending partition of the whole
//! image, so [`SourceFidelity::new`] returns byte-identical sidecars for repeat
//! decodes of the same input. The capability is [`LedgerCapability::Accounted`]:
//! every span carries a digest, and byte recovery rides the codec's native
//! opaque-record store rather than the platform retained store (see
//! `issue_object_tickets`).

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerSpan, SerializedOrigin,
    SerializedRange, SourceFidelity,
};

use crate::accounting::partition;
use crate::container::Scan;

/// Owner label attributed to every span this codec emits.
const OWNER: &str = "rhino";

/// Build the source-fidelity sidecar for a scanned Rhino container.
///
/// The returned sidecar is canonicalized and has passed
/// [`SourceFidelity::validate`]; its single `source` space tiles `[0, length)`
/// exactly. A validation failure is a builder defect and panics rather than
/// being serialized.
pub(crate) fn ledger(scan: &Scan<'_>) -> SourceFidelity {
    let data = scan.data;
    let tiles = partition(scan);
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
    let sidecar = SourceFidelity::new(LedgerCapability::Accounted, vec![source]);
    sidecar
        .validate()
        .expect("Rhino source-fidelity ledger tiles the source space completely");
    sidecar
}
