// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! L1 container accounting: the coarse source-fidelity ledger for a `.f3d`.
//!
//! [`build_ledger`] tiles every physical byte of every space the container scan
//! produced. The root `source` space is tiled completely: each entry's
//! compressed payload becomes one [`SpanClass::Opaque`] span and every byte
//! between payloads — local file headers, the central directory, the end-of-
//! central-directory record, alignment padding — becomes a [`SpanClass::Structural`]
//! framing span. Each admitted entry then becomes its own space: a
//! [`SerializedOrigin::Slice`] of the root when stored, a decompression
//! [`SerializedOrigin::Transform`] when compressed, each tiled by a single
//! opaque span covering its unrefined payload.
//!
//! This is [`LedgerLevel::L1`] with [`LedgerCapability::Accounted`]: every byte
//! is classified and digested, but opaque spans carry no retained bytes. Spans
//! and spaces are emitted in registration order and the returned sidecar is
//! canonicalized, so two decodes of the same archive serialize identically.
//! Callers must run [`SourceFidelity::validate`] before trusting the ledger;
//! [`build_validated_ledger`] does so and is the accounting-enabled entry point.

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::source_fidelity::{
    AddressSpaceLedger, CanonicalSpaceId, FidelityError, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SerializedTransformKind, SourceFidelity, SpaceExtent,
    SpanClass,
};

use crate::container::{ContainerScan, EntryLayout};

/// Span owner label for container framing bytes in the root space.
const OWNER_FRAMING: &str = "f3d-zip";
/// Span owner label for an entry payload, in the root and its derived space.
const OWNER_ENTRY: &str = "f3d-entry";

/// Builds the L1 fidelity sidecar for a scanned `.f3d` archive.
///
/// The result is canonicalized but not validated; prefer
/// [`build_validated_ledger`] unless the caller validates separately.
pub fn build_ledger(scan: &ContainerScan<'_>) -> SourceFidelity {
    let mut spaces = vec![root_space(scan)];
    for entry in dedup_entries(scan) {
        if let Some(space) = entry_space(scan, entry) {
            spaces.push(space);
        }
    }
    SourceFidelity::new(LedgerLevel::L1, LedgerCapability::Accounted, spaces)
}

/// Builds and validates the L1 fidelity sidecar.
///
/// Validation is mandatory for an accounting-enabled result: a ledger that does
/// not tile every space exactly is not a level and must not be trusted.
pub fn build_validated_ledger(scan: &ContainerScan<'_>) -> Result<SourceFidelity, FidelityError> {
    let sidecar = build_ledger(scan);
    sidecar.validate()?;
    Ok(sidecar)
}

/// Tiles the root `source` space: opaque entry payloads, structural framing.
fn root_space(scan: &ContainerScan<'_>) -> AddressSpaceLedger {
    let source = scan.source_image;
    let length = source.len() as u64;

    // Payload extents in ascending order; a well-formed archive lays them out
    // disjointly, so gaps between them are pure container framing.
    let mut extents: Vec<&EntryLayout> = scan
        .layout
        .iter()
        .filter(|entry| entry.compressed.end > entry.compressed.start)
        .collect();
    extents.sort_by(|a, b| {
        a.compressed
            .start
            .cmp(&b.compressed.start)
            .then(a.compressed.end.cmp(&b.compressed.end))
    });

    let mut spans = Vec::new();
    let mut cursor = 0_u64;
    for entry in extents {
        let start = entry.compressed.start;
        let end = entry.compressed.end;
        // A hostile archive can overlap or nest payload extents; skip any that
        // would break the ascending tiling rather than emit a gap the validator
        // rejects. The bytes stay covered by the preceding span.
        if start < cursor {
            continue;
        }
        if start > cursor {
            spans.push(framing_span(source, cursor, start));
        }
        spans.push(opaque_span(
            source,
            start,
            end,
            OWNER_ENTRY,
            format!("payload: {}", entry.name),
        ));
        cursor = end;
    }
    if cursor < length {
        spans.push(framing_span(source, cursor, length));
    }

    AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length,
        origin: SerializedOrigin::Root,
        spans,
    }
}

/// Derives one entry's own space, tiled by a single unrefined opaque span.
///
/// Returns `None` for an entry whose payload the scan did not retain (it is
/// still accounted as an opaque span in the root, so no byte goes unclassified).
fn entry_space(scan: &ContainerScan<'_>, entry: &EntryLayout) -> Option<AddressSpaceLedger> {
    let bytes = scan.entry_bytes(&entry.name).ok()?;
    let length = bytes.len() as u64;
    let source_extent = SpaceExtent {
        space: CanonicalSpaceId::source(),
        range: SerializedRange {
            start: entry.compressed.start,
            end: entry.compressed.end,
        },
    };
    let origin = if entry.stored {
        SerializedOrigin::Slice {
            parent: CanonicalSpaceId::source(),
            range: source_extent.range,
        }
    } else {
        SerializedOrigin::Transform {
            inputs: vec![source_extent],
            transform: SerializedTransformKind::Decompress,
        }
    };

    // A zero-length entry tiles [0, 0) with no spans; an empty opaque span would
    // be a degenerate tile.
    let spans = if length > 0 {
        vec![LedgerSpan {
            range: SerializedRange {
                start: 0,
                end: length,
            },
            class: SpanClass::Opaque,
            owner: OWNER_ENTRY.to_string(),
            meaning: format!("unrefined payload: {}", entry.name),
            digest: sha256_hex(bytes),
            retained: None,
        }]
    } else {
        Vec::new()
    };

    Some(AddressSpaceLedger {
        id: CanonicalSpaceId::entry(&entry.name),
        length,
        origin,
        spans,
    })
}

/// Entry layouts with duplicate archive paths collapsed to the first seen, so a
/// derived space's canonical id stays unique.
fn dedup_entries<'s>(scan: &'s ContainerScan<'_>) -> Vec<&'s EntryLayout> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for entry in &scan.layout {
        if seen.insert(entry.name.as_str()) {
            out.push(entry);
        }
    }
    out
}

/// A structural framing span over `[start, end)` of the source.
fn framing_span(source: &[u8], start: u64, end: u64) -> LedgerSpan {
    span(
        source,
        start,
        end,
        SpanClass::Structural,
        OWNER_FRAMING,
        "container framing".to_string(),
    )
}

/// An opaque payload span over `[start, end)` of the source.
fn opaque_span(source: &[u8], start: u64, end: u64, owner: &str, meaning: String) -> LedgerSpan {
    span(source, start, end, SpanClass::Opaque, owner, meaning)
}

fn span(
    source: &[u8],
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &str,
    meaning: String,
) -> LedgerSpan {
    let lo = start.min(source.len() as u64) as usize;
    let hi = end.min(source.len() as u64) as usize;
    LedgerSpan {
        range: SerializedRange { start, end },
        class,
        owner: owner.to_string(),
        meaning,
        digest: sha256_hex(&source[lo..hi]),
        retained: None,
    }
}
