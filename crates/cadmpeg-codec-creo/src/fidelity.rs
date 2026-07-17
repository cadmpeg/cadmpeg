// SPDX-License-Identifier: Apache-2.0
//! L1 coarse container accounting for the PSB `.prt` container (§10 Phase 3C).
//!
//! The Creo container is a single flat address space: the `#UGC:2` ASCII header
//! and table of contents, then a contiguous run of named binary sections to
//! end of file. There is no decompression and no reconstructed stream, so the
//! whole file is the `source` space with a [`SerializedOrigin::Root`] origin;
//! the serialized identity is `source` regardless of registration order.
//!
//! [`coarse_ledger`] tiles that space completely at container granularity:
//! the leading header/TOC framing is one [`SpanClass::Structural`] span and
//! every enumerated section is one [`SpanClass::Opaque`] span carrying a
//! SHA-256 digest but no retained bytes (accounted, not recoverable — §6.1).
//! Any byte a section walk leaves uncovered — an inter-section gap or a
//! trailing region — is filled with an explicit opaque padding span, so the
//! spans tile `[0, length)` with no hole. The builder validates the assembled
//! [`SourceFidelity`] before returning it: a coarse ledger that fails the
//! conservation invariant is a construction bug, not a shippable result, and
//! validation is mandatory for every accounting-enabled decode.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::source_fidelity::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SourceFidelity, SpanClass,
};

use crate::container::ContainerScan;

/// Owner label for the leading header/TOC framing span.
const OWNER_FRAMING: &str = "creo_framing";
/// Owner label for an enumerated binary section span.
const OWNER_SECTION: &str = "creo_section";
/// Owner label for bytes no section walk attributed.
const OWNER_UNATTRIBUTED: &str = "creo_unattributed";

/// One coarse tile before completeness filling and digesting: an absolute
/// half-open byte range in the source space with its class and labels.
struct CoarseSpan {
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &'static str,
    meaning: String,
}

/// Build the complete L1 coarse-tiling ledger for a scanned `.prt` file.
///
/// Produces one `source` [`AddressSpaceLedger`] whose spans tile
/// `[0, data.len())` exactly: the header/TOC framing as structural, each
/// section as an opaque digest span, and explicit opaque spans for any
/// unattributed gap or trailing region. The returned [`SourceFidelity`] is at
/// [`LedgerLevel::L1`] / [`LedgerCapability::Accounted`] and has already passed
/// [`SourceFidelity::validate`]; a validation failure surfaces as
/// [`CodecError::Malformed`] because it means the container framing the scan
/// reported cannot be reconciled into a conserving tiling.
pub fn coarse_ledger(scan: &ContainerScan<'_>) -> Result<SourceFidelity, CodecError> {
    let data = scan.data;
    let length = data.len() as u64;

    // Collect the container's known spans, then let `tile` fill every gap so
    // the result is complete by construction regardless of section layout.
    let mut coarse: Vec<CoarseSpan> = Vec::with_capacity(scan.sections.len() + 1);
    let first_section_offset = scan
        .sections
        .iter()
        .map(|section| section.offset as u64)
        .min()
        .unwrap_or(length);
    if first_section_offset > 0 {
        coarse.push(CoarseSpan {
            start: 0,
            end: first_section_offset.min(length),
            class: SpanClass::Structural,
            owner: OWNER_FRAMING,
            meaning: "PSB header and table of contents".to_string(),
        });
    }
    for section in &scan.sections {
        let start = section.offset as u64;
        let end = (section.offset as u64 + section.length as u64).min(length);
        if start >= end {
            continue;
        }
        coarse.push(CoarseSpan {
            start,
            end,
            class: SpanClass::Opaque,
            owner: OWNER_SECTION,
            meaning: format!("{}:{}", section.role, section.name),
        });
    }

    let spans = tile(coarse, length, data);
    let ledger = AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length,
        origin: SerializedOrigin::Root,
        spans,
    };
    let sidecar = SourceFidelity::new(LedgerLevel::L1, LedgerCapability::Accounted, vec![ledger]);
    sidecar
        .validate()
        .map_err(|error| CodecError::Malformed(format!("creo coarse ledger: {error}")))?;
    Ok(sidecar)
}

/// Sort the collected coarse spans, fill every uncovered byte with an explicit
/// opaque span, and digest each final span from its source bytes.
///
/// The container's sections are contiguous to end of file, so filling normally
/// adds nothing; it exists so a scan that ever reports a gap or a short final
/// section still yields a conserving tiling rather than a validation failure.
/// Overlaps cannot arise from the scan (section offsets strictly increase and
/// the framing prefix ends at the first section), but a clamped cursor keeps
/// the walk monotone if one ever did.
fn tile(mut coarse: Vec<CoarseSpan>, length: u64, data: &[u8]) -> Vec<LedgerSpan> {
    coarse.sort_by_key(|span| (span.start, span.end));
    let mut spans: Vec<LedgerSpan> = Vec::with_capacity(coarse.len() + 1);
    let mut cursor = 0_u64;
    for span in coarse {
        if span.start > cursor {
            spans.push(digest_span(
                cursor,
                span.start,
                SpanClass::Opaque,
                OWNER_UNATTRIBUTED,
                "unattributed container bytes".to_string(),
                data,
            ));
        }
        let start = span.start.max(cursor);
        if span.end <= start {
            continue;
        }
        spans.push(digest_span(
            start,
            span.end,
            span.class,
            span.owner,
            span.meaning,
            data,
        ));
        cursor = span.end;
    }
    if cursor < length {
        spans.push(digest_span(
            cursor,
            length,
            SpanClass::Opaque,
            OWNER_UNATTRIBUTED,
            "unattributed trailing bytes".to_string(),
            data,
        ));
    }
    spans
}

/// Build one ledger span over `[start, end)`, hashing the covered source bytes.
fn digest_span(
    start: u64,
    end: u64,
    class: SpanClass,
    owner: &'static str,
    meaning: String,
    data: &[u8],
) -> LedgerSpan {
    let lo = (start as usize).min(data.len());
    let hi = (end as usize).min(data.len());
    LedgerSpan {
        range: SerializedRange { start, end },
        class,
        owner: owner.to_string(),
        meaning,
        digest: sha256_hex(&data[lo..hi]),
        retained: None,
    }
}
