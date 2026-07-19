// SPDX-License-Identifier: Apache-2.0
//! Source-fidelity tiling for Rhino 3DM containers.

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::source_fidelity::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerSpan, SerializedOrigin, SerializedRange,
};
use cadmpeg_ir::SourceFidelity;

use crate::accounting::partition;
use crate::container::Scan;

/// Owner label attributed to every span this codec emits.
const OWNER: &str = "rhino";

/// Builds and validates the source-fidelity sidecar for a Rhino container.
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
    let sidecar = SourceFidelity::new(vec![source]);
    sidecar
        .validate()
        .expect("Rhino source-fidelity ledger tiles the source space completely");
    sidecar
}
