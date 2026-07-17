// SPDX-License-Identifier: Apache-2.0
//! L1 coarse source-fidelity accounting for the NX SPLMSSTR container.
//!
//! [`install`] builds a complete coarse tiling of every physical space the
//! decode produces and stores it as the serialized v2 sidecar (§6.1). Two
//! space families are tiled:
//!
//! - the root `source` space, whose directory framing is [`SpanClass::Structural`]
//!   and whose catalogued file-entry payloads are one [`SpanClass::Opaque`] span
//!   each, with every remaining byte covered by a structural framing span; and
//! - one derived space per inflated stream, named `stream:<part>#<n>`, produced
//!   by a [`SerializedTransformKind::Decompress`] transform of the compressed
//!   member's source extent and tiled by a single opaque span.
//!
//! The ledger is [`LedgerLevel::L1`] and [`LedgerCapability::Accounted`]: every
//! byte of every space is classified, but opaque spans carry only digests, not
//! retained bytes. [`install`] validates the sidecar before serializing it, so
//! an accounting-enabled result never carries a ledger that violates the
//! conservation invariant.

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SerializedTransformKind, SourceFidelity, SpaceExtent,
    SpanClass,
};

use crate::decode::Scan;

/// The canonical part payload whose inflated members become derived spaces.
const PART_PATH: &str = "/Root/UG_PART/UG_PART";

/// Build the validated v2 sidecar for a parsed NX container.
///
/// The tiling is complete by construction, so validation is an invariant guard,
/// not input-dependent control flow; a failure is a decoder bug. The caller
/// rides the result on [`DecodeReport::source_fidelity`](cadmpeg_ir::report::DecodeReport::source_fidelity),
/// the platform's designated sidecar surface (§6.1), rather than a private
/// native arena, so a consumer reading the standard slot sees the L1 ledger.
pub(crate) fn ledger(scan: &Scan) -> SourceFidelity {
    let sidecar = build_sidecar(scan);
    sidecar
        .validate()
        .expect("nx source-fidelity ledger tiles completely and derives a valid origin DAG");
    sidecar
}

/// Build the complete coarse (L1) sidecar for a parsed NX container.
pub(crate) fn build_sidecar(scan: &Scan) -> SourceFidelity {
    let mut spaces = vec![source_space(scan)];
    spaces.extend(stream_spaces(scan));
    SourceFidelity::new(LedgerLevel::L1, LedgerCapability::Accounted, spaces)
}

/// Tile the root `source` space: catalogued payloads opaque, all else structural.
fn source_space(scan: &Scan) -> AddressSpaceLedger {
    let data = scan.container.data.as_slice();
    let length = data.len() as u64;

    // Every catalogued file-entry payload is one unrefined opaque region. Merge
    // only truly overlapping (or nested) extents so the result is a disjoint,
    // ascending cover; adjacent extents stay separate to keep per-payload
    // identity. Overlap is possible in hostile input, and a tiling cannot carry
    // overlapping spans, so coalescing is required for completeness.
    let mut payloads: Vec<(u64, u64)> = scan
        .container
        .entries
        .iter()
        .filter_map(|entry| entry.file_span)
        .filter_map(|(offset, size)| offset.checked_add(size).map(|end| (offset, end)))
        .filter(|&(start, end)| start < end && end <= length)
        .collect();
    payloads.sort_unstable();

    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(payloads.len());
    for (start, end) in payloads {
        match merged.last_mut() {
            Some(last) if start < last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    let mut spans = Vec::new();
    let mut cursor = 0_u64;
    for (start, end) in merged {
        if cursor < start {
            spans.push(structural(data, cursor, start, "nx_container_framing"));
        }
        spans.push(opaque(data, start, end, "nx_file_entry_payload"));
        cursor = end;
    }
    if cursor < length {
        spans.push(structural(data, cursor, length, "nx_container_framing"));
    }

    AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length,
        origin: SerializedOrigin::Root,
        spans,
    }
}

/// One derived decompression space per inflated stream, tiled by a single
/// opaque span over the whole inflated body.
fn stream_spaces(scan: &Scan) -> Vec<AddressSpaceLedger> {
    scan.streams
        .iter()
        .enumerate()
        .map(|(ordinal, stream)| {
            let length = stream.inflated.len() as u64;
            let input_start = stream.file_offset as u64;
            let input_end = input_start.saturating_add(stream.consumed);
            AddressSpaceLedger {
                id: CanonicalSpaceId::stream(PART_PATH, ordinal as u32),
                length,
                origin: SerializedOrigin::Transform {
                    inputs: vec![SpaceExtent {
                        space: CanonicalSpaceId::source(),
                        range: SerializedRange {
                            start: input_start,
                            end: input_end,
                        },
                    }],
                    transform: SerializedTransformKind::Decompress,
                },
                spans: vec![LedgerSpan {
                    range: SerializedRange {
                        start: 0,
                        end: length,
                    },
                    class: SpanClass::Opaque,
                    owner: "nx_parasolid_stream".to_string(),
                    meaning: format!("inflated {} stream", stream.kind.label()),
                    digest: sha256_hex(&stream.inflated),
                    retained: None,
                }],
            }
        })
        .collect()
}

fn structural(data: &[u8], start: u64, end: u64, owner: &str) -> LedgerSpan {
    span(
        data,
        start,
        end,
        SpanClass::Structural,
        owner,
        "container framing",
    )
}

fn opaque(data: &[u8], start: u64, end: u64, owner: &str) -> LedgerSpan {
    span(
        data,
        start,
        end,
        SpanClass::Opaque,
        owner,
        "catalogued file-entry payload",
    )
}

fn span(
    data: &[u8],
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &str,
    meaning: &str,
) -> LedgerSpan {
    let digest = match (usize::try_from(start), usize::try_from(end)) {
        (Ok(lo), Ok(hi)) if lo <= hi && hi <= data.len() => sha256_hex(&data[lo..hi]),
        _ => sha256_hex(&[]),
    };
    LedgerSpan {
        range: SerializedRange { start, end },
        class,
        owner: owner.to_string(),
        meaning: meaning.to_string(),
        digest,
        retained: None,
    }
}
