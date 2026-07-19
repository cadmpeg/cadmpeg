// SPDX-License-Identifier: Apache-2.0
//! container accounting for the `.sldprt` outer container.
//!
//! [`container_ledger`] turns a completed [`ContainerScan`] into a v2
//! source-fidelity sidecar ([`SourceFidelity`]) with complete
//! *coarse* tiling. The root `source` space is tiled so every physical byte is
//! classified — outer header and every block/cache-cell/directory frame as
//! `Structural`, each compressed block payload as one `Opaque` span, and any
//! unclaimed run between frames as an explicit `Opaque` padding span. Each block
//! also registers its decompressed payload as a child `Transform` space carrying
//! one `Opaque` span; that is the coarse boundary — sub-payload structure
//! (Parasolid streams, records) is finer refinement and is not tiled here.
//!
//! Canonical ids derive only from the scan, never from runtime registration
//! order: the root is `source`, and a block's decompressed space is
//! `stream:<section>#<index>` keyed by the block's file-order index, so two
//! decodes of the same bytes serialize byte-identical sidecars. The result is
//! returned in canonical order via [`SourceFidelity::new`]; callers that require
//! the conservation invariant enforced call [`SourceFidelity::validate`].

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::le::u32_at as u32_le;
use cadmpeg_ir::source_fidelity::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerSpan, SerializedOrigin, SerializedRange,
    SerializedTransformKind, SpaceExtent, SpanClass,
};
use cadmpeg_ir::SourceFidelity;

use crate::container::{
    Block, CacheCell, ContainerScan, DirectoryEntry, BLOCK_HEADER_LEN, OUTER_HEADER_LEN,
};

/// Owner label attributed to every span this codec emits.
const OWNER: &str = "sldprt";

/// Byte length of a tail-directory frame header before its name: the block
/// header plus a 14-byte descriptor.
const DIRECTORY_HEADER_LEN: usize = BLOCK_HEADER_LEN + 14;

/// Build the source-fidelity sidecar for a scanned `.sldprt` container.
///
/// The returned sidecar is in canonical order but not yet validated; call
/// [`SourceFidelity::validate`] to enforce complete tiling and origin
/// consistency. Tiling is complete by construction: unclaimed bytes become
/// explicit `Opaque` padding spans.
pub fn container_ledger(scan: &ContainerScan) -> SourceFidelity {
    let source = scan.source_image.as_slice();
    let length = source.len() as u64;

    let root = AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length,
        origin: SerializedOrigin::Root,
        spans: tile_root(scan),
    };

    let mut spaces = vec![root];
    for (index, block) in scan.blocks.iter().enumerate() {
        spaces.push(block_space(source, block, index as u32));
    }

    SourceFidelity::new(spaces)
}

/// Priority of a preserved-payload claim: it must win any overlap against
/// container framing so real bytes are never relabeled as discardable.
const PRIORITY_PAYLOAD: u8 = 1;
/// Priority of a container-framing claim.
const PRIORITY_FRAMING: u8 = 0;

/// One claimed run of the root space, before overlaps are resolved.
struct Claim {
    start: u64,
    end: u64,
    class: SpanClass,
    meaning: String,
    /// Overlap-resolution rank: a higher priority paints its bytes first, so a
    /// framing claim can never overwrite an overlapping payload claim.
    priority: u8,
}

/// Tile the root `source` space into a gap-free ascending span list.
fn tile_root(scan: &ContainerScan) -> Vec<LedgerSpan> {
    let source = scan.source_image.as_slice();
    let length = source.len() as u64;
    let mut claims: Vec<Claim> = Vec::new();

    if length > 0 {
        claims.push(Claim {
            start: 0,
            end: (OUTER_HEADER_LEN as u64).min(length),
            class: SpanClass::Structural,
            meaning: "outer-header".to_string(),
            priority: PRIORITY_FRAMING,
        });
    }

    for block in &scan.blocks {
        let (payload_start, payload_end) = block_payload_range(source, block);
        claims.push(Claim {
            start: (block.offset as u64).min(length),
            end: payload_start,
            class: SpanClass::Structural,
            meaning: "block-frame".to_string(),
            priority: PRIORITY_FRAMING,
        });
        claims.push(Claim {
            start: payload_start,
            end: payload_end,
            class: SpanClass::Opaque,
            meaning: format!("block-payload:{}", block.family),
            priority: PRIORITY_PAYLOAD,
        });
    }

    for cell in &scan.cache_cells {
        claims.push(Claim {
            start: (cell.offset as u64).min(length),
            end: cache_cell_frame_end(source, cell),
            class: SpanClass::Structural,
            meaning: "cache-cell".to_string(),
            priority: PRIORITY_FRAMING,
        });
    }

    for entry in &scan.directory {
        claims.push(Claim {
            start: (entry.offset as u64).min(length),
            end: directory_frame_end(source, entry),
            class: SpanClass::Structural,
            meaning: "directory-entry".to_string(),
            priority: PRIORITY_FRAMING,
        });
    }

    resolve(source, &claims, length)
}

/// Resolve possibly-overlapping claims into a complete gap-free tiling of
/// `[0, length)`.
///
/// A hostile container can place a framing marker inside a block's compressed
/// payload (the directory descriptor window carries no printability guard), so a
/// directory-entry frame claim can overlap a block-payload claim. A cursor sweep
/// that keeps whichever claim starts first would let the framing claim swallow
/// the payload and relabel preserved bytes as discardable framing. Instead every
/// coordinate boundary is walked and each segment takes the class of the
/// highest-priority claim covering it in full: payload claims outrank framing,
/// so real payload bytes always tile as `Opaque` even when a frame overlaps
/// them, and any byte no claim covers becomes an `Opaque` padding span.
fn resolve(source: &[u8], claims: &[Claim], length: u64) -> Vec<LedgerSpan> {
    if length == 0 {
        return Vec::new();
    }
    let mut coords: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    coords.insert(0);
    coords.insert(length);
    for claim in claims {
        if claim.start < claim.end {
            coords.insert(claim.start.min(length));
            coords.insert(claim.end.min(length));
        }
    }
    let coords: Vec<u64> = coords.into_iter().collect();

    // (start, end, class, meaning), merged with the previous segment when the
    // class and meaning match so adjacent same-kind runs stay one span.
    let mut segments: Vec<(u64, u64, SpanClass, String)> = Vec::new();
    for window in coords.windows(2) {
        let (a, b) = (window[0], window[1]);
        if a >= b {
            continue;
        }
        let mut best: Option<&Claim> = None;
        for claim in claims {
            if claim.start <= a
                && claim.end >= b
                && claim.start < claim.end
                && best.is_none_or(|current| claim.priority > current.priority)
            {
                best = Some(claim);
            }
        }
        let (class, meaning) = match best {
            Some(claim) => (claim.class, claim.meaning.clone()),
            None => (SpanClass::Opaque, "padding".to_string()),
        };
        if let Some(last) = segments.last_mut() {
            if last.1 == a && last.2 == class && last.3 == meaning {
                last.1 = b;
                continue;
            }
        }
        segments.push((a, b, class, meaning));
    }

    segments
        .into_iter()
        .map(|(start, end, class, meaning)| span(source, start, end, class, &meaning))
        .collect()
}

/// Build one block's decompressed child space: a `Transform`-from-`source`
/// origin plus a single `Opaque` span over the whole payload.
fn block_space(source: &[u8], block: &Block, index: u32) -> AddressSpaceLedger {
    let (payload_start, payload_end) = block_payload_range(source, block);
    let path = block.section.as_deref().unwrap_or("block");
    let length = block.payload.len() as u64;
    let spans = vec![LedgerSpan {
        range: SerializedRange {
            start: 0,
            end: length,
        },
        class: SpanClass::Opaque,
        owner: OWNER.to_string(),
        meaning: format!("block-payload:{}", block.family),
        digest: sha256_hex(&block.payload),
        retained: None,
    }];
    AddressSpaceLedger {
        id: CanonicalSpaceId::stream(path, index),
        length,
        origin: SerializedOrigin::Transform {
            inputs: vec![SpaceExtent {
                space: CanonicalSpaceId::source(),
                range: SerializedRange {
                    start: payload_start,
                    end: payload_end,
                },
            }],
            transform: SerializedTransformKind::Decompress,
        },
        spans,
    }
}

/// Compute a block's compressed-payload byte range in the root, clamped to the
/// source length.
fn block_payload_range(source: &[u8], block: &Block) -> (u64, u64) {
    let pre = u32_le(source, block.offset + 22).unwrap_or(0) as usize;
    let payload_start = block
        .offset
        .saturating_add(BLOCK_HEADER_LEN)
        .saturating_add(pre);
    let payload_end = payload_start.saturating_add(block.comp_sz as usize);
    let clamp = |v: usize| v.min(source.len()) as u64;
    (clamp(payload_start), clamp(payload_end))
}

/// Build one `LedgerSpan` over `[start, end)` of the source, digesting its bytes.
fn span(source: &[u8], start: u64, end: u64, class: SpanClass, meaning: &str) -> LedgerSpan {
    let bytes = source.get(start as usize..end as usize).unwrap_or(&[]);
    LedgerSpan {
        range: SerializedRange { start, end },
        class,
        owner: OWNER.to_string(),
        meaning: meaning.to_string(),
        digest: sha256_hex(bytes),
        retained: None,
    }
}

/// End offset of a cache-cell frame: marker header plus the swapped name.
fn cache_cell_frame_end(source: &[u8], cell: &CacheCell) -> u64 {
    let name_len = u32_le(source, cell.offset + 22).unwrap_or(0) as usize;
    cell.offset
        .saturating_add(BLOCK_HEADER_LEN)
        .saturating_add(name_len)
        .min(source.len()) as u64
}

/// End offset of a tail-directory frame: marker header, 14-byte descriptor, name.
fn directory_frame_end(source: &[u8], entry: &DirectoryEntry) -> u64 {
    let name_len = u32_le(source, entry.offset + 22).unwrap_or(0) as usize;
    entry
        .offset
        .saturating_add(DIRECTORY_HEADER_LEN)
        .saturating_add(name_len)
        .min(source.len()) as u64
}
