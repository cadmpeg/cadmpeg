// SPDX-License-Identifier: Apache-2.0
//! L1 coarse source-fidelity tiling for the `V5_CFV2` container (doc §6.1,
//! §10 Phase 3C).
//!
//! [`container_ledger`] turns a parsed [`ContainerScan`] into a validated v2
//! [`SourceFidelity`] sidecar at [`LedgerLevel::L1`]: every physical byte of
//! the root input is covered by one span, container framing classed
//! [`SpanClass::Structural`], catalogued stream extents and everything else
//! [`SpanClass::Opaque`]. The reconstructed BREP logical stream is serialized
//! as a separate [`SerializedOrigin::Concat`] space whose segments name the
//! source extents it assembles, mirroring the runtime `Concat` derived space
//! [`crate::container::scan_view`] registers.
//!
//! The tiling is *coarse*: it classifies the framing bytes the container parse
//! confidently identifies and leaves every unclassified interior byte as one
//! opaque padding span, so `[0, length)` tiles exactly without refining record
//! structure. Completeness is the invariant, not granularity.
//!
//! Only spaces with canonical serialized ids (§6.1) are emitted: the root
//! `source` space and the reconstructed `stream:brep#0` space. The runtime
//! per-extent `Slice` spaces have no canonical id spelling; their bytes are
//! tiled inside `source` as opaque extent spans and referenced from the BREP
//! stream's `Concat` segments, so no byte is duplicated across spaces.
//!
//! The ledger is built at [`LedgerCapability::Accounted`]: spans carry digests,
//! not retained bytes. [`container_ledger`] validates before returning, so an
//! accounting-enabled result never escapes with a ledger that fails the
//! conservation invariant.

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{
    AddressSpaceLedger, CanonicalSpaceId, FidelityError, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SourceFidelity, SpaceExtent, SpanClass,
};

use crate::container::{self, ContainerScan, InnerDir};

/// The outer and inner `V5_CFV2` header length: the 8-byte magic plus the
/// big-endian directory offset/length pair.
const HEADER_LEN: u64 = 16;

/// A classified byte interval claimed by a known container structure, before
/// overlap resolution and gap filling.
struct Claim {
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &'static str,
    meaning: String,
}

/// Builds and validates the L1 source-fidelity sidecar for a scanned
/// `.CATPart`.
///
/// The returned [`SourceFidelity`] is canonicalized and has passed
/// [`SourceFidelity::validate`]; its spaces tile `[0, length)` exactly. Returns
/// the validation [`FidelityError`] if the built tiling ever violates the
/// conservation invariant — a defect in this builder, surfaced rather than
/// serialized.
pub(crate) fn container_ledger(scan: &ContainerScan<'_>) -> Result<SourceFidelity, FidelityError> {
    let mut spaces = vec![source_space(scan)];
    if let Some(stream) = brep_stream_space(scan) {
        spaces.push(stream);
    }
    let sidecar = SourceFidelity::new(LedgerLevel::L1, LedgerCapability::Accounted, spaces);
    sidecar.validate()?;
    Ok(sidecar)
}

/// Tiles the root input space: framing structural, stream extents opaque, every
/// remaining byte one opaque padding span.
fn source_space(scan: &ContainerScan<'_>) -> AddressSpaceLedger {
    let data = scan.data;
    let length = data.len() as u64;
    let mut claims = Vec::new();

    // Outer container header: magic plus the big-endian directory pointer pair.
    push_claim(
        &mut claims,
        0,
        HEADER_LEN,
        SpanClass::Structural,
        "container",
        "outer V5_CFV2 header".to_string(),
        length,
    );

    // Outer directory region, when the header points to a non-empty tail.
    //
    // The offset/length pair is read raw from the outer header and is not
    // trusted here: a hostile directory extent that reaches back over the inner
    // container would sort ahead of every inner claim and, because `tile` drops
    // any later claim overlapping already-tiled bytes, silently relabel the
    // catalogued opaque stream extents as discardable container framing. The
    // legitimate outer directory sits at or past the end of the inner
    // container's content, so the claim is emitted only when its start clears
    // that boundary; otherwise those bytes fall through to opaque padding or the
    // inner claims rather than being over-claimed as structural.
    let outer_dir_start = u64::from(scan.outer_dir_offset);
    let outer_dir_end = outer_dir_start.saturating_add(u64::from(scan.outer_dir_length));
    if outer_dir_start >= inner_content_end(scan) {
        push_claim(
            &mut claims,
            outer_dir_start,
            outer_dir_end,
            SpanClass::Structural,
            "container",
            "outer directory".to_string(),
            length,
        );
    }

    if let Some(dir) = &scan.inner {
        let inner = dir.inner as u64;
        // Inner nested V5_CFV2 header.
        push_claim(
            &mut claims,
            inner,
            inner.saturating_add(HEADER_LEN),
            SpanClass::Structural,
            "container",
            "inner V5_CFV2 header".to_string(),
            length,
        );
        // Inner stream directory magic (CATIA_V5 CB0001).
        let dir_off = dir.dir_offset as u64;
        push_claim(
            &mut claims,
            dir_off,
            dir_off.saturating_add(container::DIR_MAGIC.len() as u64),
            SpanClass::Structural,
            "container",
            "inner stream directory".to_string(),
            length,
        );
        push_extent_claims(&mut claims, dir, length);
    }

    let spans = tile(data, length, claims);
    AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length,
        origin: SerializedOrigin::Root,
        spans,
    }
}

/// The first byte at or past the end of the inner container's content.
///
/// A well-formed outer directory begins here or later. Returns `0` when there is
/// no nested container, so the non-nested variant keeps emitting its outer
/// directory claim unchanged. The bound is the maximum end of the inner header,
/// the inner stream directory magic, and every catalogued physical extent — the
/// spans whose opaque classification a hostile outer directory must not erase.
fn inner_content_end(scan: &ContainerScan<'_>) -> u64 {
    let Some(dir) = &scan.inner else {
        return 0;
    };
    let length = scan.data.len() as u64;
    let inner = dir.inner as u64;
    let mut end = inner.saturating_add(HEADER_LEN);
    let dir_off = dir.dir_offset as u64;
    end = end.max(dir_off.saturating_add(container::DIR_MAGIC.len() as u64));
    for descriptor in &dir.descriptors {
        for extent in &descriptor.extents {
            let start = inner.saturating_add(u64::from(extent.phys_off));
            let extent_end = start.saturating_add(u64::from(extent.phys_len));
            if extent_end > length || start >= extent_end {
                continue;
            }
            end = end.max(extent_end);
        }
    }
    end.min(length)
}

/// Adds one opaque claim per catalogued physical extent, applying the same
/// in-range filter as [`crate::container`] stream reconstruction so the ledger
/// and the reconstructed bytes agree.
fn push_extent_claims(claims: &mut Vec<Claim>, dir: &InnerDir, length: u64) {
    for descriptor in &dir.descriptors {
        for extent in &descriptor.extents {
            let start = (dir.inner as u64).saturating_add(u64::from(extent.phys_off));
            let end = start.saturating_add(u64::from(extent.phys_len));
            if end > length || start >= end {
                continue;
            }
            push_claim(
                claims,
                start,
                end,
                SpanClass::Opaque,
                "stream",
                format!("{} physical extent", descriptor.name),
                length,
            );
        }
    }
}

/// Serializes the reconstructed BREP logical stream as a `Concat` space, when
/// the directory catalogues one.
fn brep_stream_space(scan: &ContainerScan<'_>) -> Option<AddressSpaceLedger> {
    let bytes = scan.brep_bytes()?;
    if bytes.is_empty() {
        return None;
    }
    let dir = scan.inner.as_ref()?;
    let ranges = container::brep_extent_ranges(scan.data, dir)?;
    let segments: Vec<SpaceExtent> = ranges
        .into_iter()
        .map(|range| SpaceExtent {
            space: CanonicalSpaceId::source(),
            range: SerializedRange {
                start: range.start as u64,
                end: range.end as u64,
            },
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    let length = bytes.len() as u64;
    let span = LedgerSpan {
        range: SerializedRange {
            start: 0,
            end: length,
        },
        class: SpanClass::Opaque,
        owner: "stream".to_string(),
        meaning: "reconstructed BREP logical stream".to_string(),
        digest: sha256_hex(bytes),
        retained: None,
    };
    Some(AddressSpaceLedger {
        id: CanonicalSpaceId::stream("brep", 0),
        length,
        origin: SerializedOrigin::Concat { segments },
        spans: vec![span],
    })
}

/// Records a claim, clamping to `[0, length)` and dropping an empty or
/// out-of-range interval.
fn push_claim(
    claims: &mut Vec<Claim>,
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &'static str,
    meaning: String,
    length: u64,
) {
    let start = start.min(length);
    let end = end.min(length);
    if start >= end {
        return;
    }
    claims.push(Claim {
        start,
        end,
        class,
        owner,
        meaning,
    });
}

/// Resolves claims into an exact non-overlapping tiling of `[0, length)`.
///
/// Claims are swept in ascending start order; an interval that overlaps the
/// bytes already tiled is dropped (coarse framing never subdivides a payload),
/// and every uncovered gap becomes one opaque padding span. The result tiles
/// `[0, length)` exactly and deterministically for a given scan.
fn tile(data: &[u8], length: u64, mut claims: Vec<Claim>) -> Vec<LedgerSpan> {
    claims.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    let mut spans = Vec::new();
    let mut cursor = 0_u64;
    for claim in claims {
        if claim.start < cursor {
            continue;
        }
        if claim.start > cursor {
            spans.push(padding_span(data, cursor, claim.start));
        }
        spans.push(span(
            data,
            claim.start,
            claim.end,
            claim.class,
            claim.owner,
            &claim.meaning,
        ));
        cursor = claim.end;
    }
    if cursor < length {
        spans.push(padding_span(data, cursor, length));
    }
    spans
}

/// Builds one opaque padding span over an uncovered interval.
fn padding_span(data: &[u8], start: u64, end: u64) -> LedgerSpan {
    span(
        data,
        start,
        end,
        SpanClass::Opaque,
        "padding",
        "unclassified padding",
    )
}

/// Builds one ledger span, digesting the covered source bytes.
fn span(
    data: &[u8],
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &str,
    meaning: &str,
) -> LedgerSpan {
    let slice = &data[start as usize..end as usize];
    LedgerSpan {
        range: SerializedRange { start, end },
        class,
        owner: owner.to_string(),
        meaning: meaning.to_string(),
        digest: sha256_hex(slice),
        retained: None,
    }
}
