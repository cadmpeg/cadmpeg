// SPDX-License-Identifier: Apache-2.0
//! L1 container accounting for the `.sldprt` outer container.
//!
//! [`container_ledger`] turns a completed [`ContainerScan`] into a v2
//! source-fidelity sidecar ([`SourceFidelity`]) at [`LedgerLevel::L1`]: complete
//! *coarse* tiling. The root `source` space is tiled so every physical byte is
//! classified — outer header and every block/cache-cell/directory frame as
//! `Structural`, each compressed block payload as one `Opaque` span, and any
//! unclaimed run between frames as an explicit `Opaque` padding span. Each block
//! also registers its decompressed payload as a child `Transform` space carrying
//! one `Opaque` span; that is the coarse boundary — sub-payload structure
//! (Parasolid streams, records) is L2 refinement and is not tiled here.
//!
//! Canonical ids derive only from the scan, never from runtime registration
//! order: the root is `source`, and a block's decompressed space is
//! `stream:<section>#<index>` keyed by the block's file-order index, so two
//! decodes of the same bytes serialize byte-identical sidecars. The result is
//! returned in canonical order via [`SourceFidelity::new`]; callers that require
//! the conservation invariant enforced call [`SourceFidelity::validate`].

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::le::u32_at as u32_le;
use cadmpeg_ir::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
    SerializedOrigin, SerializedRange, SerializedTransformKind, SourceFidelity, SpaceExtent,
    SpanClass,
};

use crate::container::{
    Block, CacheCell, ContainerScan, DirectoryEntry, BLOCK_HEADER_LEN, OUTER_HEADER_LEN,
};

/// Owner label attributed to every span this codec emits.
const OWNER: &str = "sldprt";

/// Byte length of a tail-directory frame header before its name: the block
/// header plus a 14-byte descriptor.
const DIRECTORY_HEADER_LEN: usize = BLOCK_HEADER_LEN + 14;

/// Build the L1 source-fidelity sidecar for a scanned `.sldprt` container.
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

    SourceFidelity::new(LedgerLevel::L1, LedgerCapability::Accounted, spaces)
}

/// One claimed run of the root space, before gaps are filled.
struct Claim {
    start: u64,
    end: u64,
    class: SpanClass,
    meaning: String,
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
        });
    }

    for block in &scan.blocks {
        let (payload_start, payload_end) = block_payload_range(source, block);
        claims.push(Claim {
            start: (block.offset as u64).min(length),
            end: payload_start,
            class: SpanClass::Structural,
            meaning: "block-frame".to_string(),
        });
        claims.push(Claim {
            start: payload_start,
            end: payload_end,
            class: SpanClass::Opaque,
            meaning: format!("block-payload:{}", block.family),
        });
    }

    for cell in &scan.cache_cells {
        claims.push(Claim {
            start: (cell.offset as u64).min(length),
            end: cache_cell_frame_end(source, cell),
            class: SpanClass::Structural,
            meaning: "cache-cell".to_string(),
        });
    }

    for entry in &scan.directory {
        claims.push(Claim {
            start: (entry.offset as u64).min(length),
            end: directory_frame_end(source, entry),
            class: SpanClass::Structural,
            meaning: "directory-entry".to_string(),
        });
    }

    claims.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    fill(source, claims, length)
}

/// Convert accepted claims into a complete tiling, inserting `Opaque` padding
/// spans over any unclaimed run and skipping claims that overlap an already
/// accepted span.
fn fill(source: &[u8], claims: Vec<Claim>, length: u64) -> Vec<LedgerSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0_u64;
    for claim in claims {
        if claim.end <= claim.start || claim.start < cursor {
            continue;
        }
        if claim.start > cursor {
            spans.push(span(
                source,
                cursor,
                claim.start,
                SpanClass::Opaque,
                "padding",
            ));
        }
        spans.push(span(
            source,
            claim.start,
            claim.end,
            claim.class,
            &claim.meaning,
        ));
        cursor = claim.end;
    }
    if cursor < length {
        spans.push(span(source, cursor, length, SpanClass::Opaque, "padding"));
    }
    spans
}

/// Build one block's decompressed child space: a `Transform`-from-`source`
/// origin plus a single `Opaque` span over the whole payload.
fn block_space(source: &[u8], block: &Block, index: u32) -> AddressSpaceLedger {
    let (payload_start, payload_end) = block_payload_range(source, block);
    let path = block.section.as_deref().unwrap_or("block");
    let length = block.payload.len() as u64;
    let spans = if length == 0 {
        Vec::new()
    } else {
        vec![LedgerSpan {
            range: SerializedRange {
                start: 0,
                end: length,
            },
            class: SpanClass::Opaque,
            owner: OWNER.to_string(),
            meaning: format!("block-payload:{}", block.family),
            digest: sha256_hex(&block.payload),
            retained: None,
        }]
    };
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
