// SPDX-License-Identifier: Apache-2.0
//! Parse Design sketch placements, headers, relations, and geometry.

use crate::bytes::{lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
use cadmpeg_core::decode::u64_from_index;

use crate::records::sketch_placement::{
    DesignSketchFrame, DesignSketchFrameForm, SketchPlacementMatrix,
};

use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{f64s_at, take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::text::design_record_id_charged;
use crate::design::decode::{record_streams, reference_runs};

use crate::ids;
use crate::layout::sketch_container_visibility_member_prefix as visibility_member;
use crate::records::{
    admission::RecordAdmission,
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, DESIGN_MODULE_SKETCH},
    feature::scope::DesignParameterScope,
    references::{LostEdgeReference, PersistentReference, PersistentReferenceKind},
    sketch_geometry::{
        SketchCurveGeometry, SketchCurveIdentity, SketchGeometryError, SketchPoint,
        SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
        SketchPointRecordForm, SketchSurface, SketchSurfaceGeometry, SketchText,
        SketchTextAlignment, SketchTextLayout,
    },
    sketch_placement::{DesignSketchPlacement, DesignSketchVisibility},
    sketch_relations::{SketchGlyphTransform, SketchRelation, SketchRelationOperand},
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal, NonNegativeLength, NonNegativeReal, PositiveLength};
use cadmpeg_ir::sketches::TextPlacement;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};
use std::collections::HashMap;

use super::meta::{
    decode_types, design_primary_frames, metadata_for_bulk_stream, stream_types_by_class_tag,
};

const EPS_SKETCH_DECODE_LINE_COMPONENTS_E12: f64 = 1.0e-12;

/// Byte offsets of every indexed-record header in one `BulkStream`, grouped by
/// the record index carried at header offset seven.
pub(in crate::design) struct IndexedRecordOffsets {
    by_record_index: HashMap<u32, Vec<usize>>,
    /// Record indexes in the byte order of their first header.
    record_indexes: Vec<u32>,
}

impl IndexedRecordOffsets {
    /// Index every exact indexed-record header in `bytes` in one forward pass.
    pub(in crate::design) fn build(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
    ) -> Result<Self, CodecError> {
        let mut by_record_index = HashMap::<u32, Vec<usize>>::new();
        let mut record_indexes = Vec::new();
        for header in indexed_record_offsets(ctx, bytes)? {
            if !ctx.contains_key_hash_map(
                &by_record_index,
                &header.record_index,
                "f3d indexed record key",
            )? {
                ctx.push_vec(
                    &mut record_indexes,
                    header.record_index,
                    "f3d indexed record order",
                )?;
            }
            ctx.push_hash_group(
                &mut by_record_index,
                header.record_index,
                header.offset,
                "f3d indexed record key",
                "f3d indexed record offset",
            )?;
        }
        Ok(Self {
            by_record_index,
            record_indexes,
        })
    }

    /// Ascending header offsets carrying `record_index`.
    pub(in crate::design) fn offsets(&self, record_index: u32) -> &[usize] {
        self.by_record_index
            .get(&record_index)
            .map_or(&[], Vec::as_slice)
    }

    /// Record indexes and their ascending header offsets, in the byte order of
    /// each index's first header.
    pub(in crate::design) fn records(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = (u32, &[usize])> + '_, CodecError> {
        Ok(ctx
            .admit_iter(&self.record_indexes, "scan F3D indexed record groups")?
            .map(|record_index| (*record_index, self.offsets(*record_index))))
    }

    /// The first header that carries `record_index`.
    pub(in crate::design) fn first_offset(&self, record_index: u32) -> Option<usize> {
        self.offsets(record_index).first().copied()
    }

    /// The first header at or after `position` that carries `record_index`.
    /// The ascending offsets are bisected, so the search visits a logarithmic
    /// number of them.
    pub(super) fn first_at_or_after(
        &self,
        ctx: &DecodeContext<'_>,
        position: usize,
        record_index: u32,
    ) -> Result<Option<usize>, CodecError> {
        let offsets = self.offsets(record_index);
        let index = ctx.partition_point(
            offsets,
            |offset| Ok(*offset < position),
            "find F3D indexed record at or after offset",
        )?;
        Ok(offsets.get(index).copied())
    }

    /// Headers carrying `record_index`, `record_index + 1`, and so on for `N`
    /// indexes: each is the first header of its index at or after `position`
    /// or, after the first, at least one header length past its predecessor.
    pub(in crate::design) fn consecutive_headers<const N: usize>(
        &self,
        ctx: &DecodeContext<'_>,
        position: usize,
        record_index: u32,
    ) -> Result<Option<[usize; N]>, CodecError> {
        let mut offsets = [0; N];
        let mut search = Some(position);
        for (delta, slot) in offsets.iter_mut().enumerate() {
            let expected = u32::try_from(delta)
                .ok()
                .and_then(|delta| record_index.checked_add(delta));
            let (Some(at), Some(expected)) = (search, expected) else {
                return Ok(None);
            };
            let Some(offset) = self.first_at_or_after(ctx, at, expected)? else {
                return Ok(None);
            };
            *slot = offset;
            search = offset.checked_add(11);
        }
        Ok(Some(offsets))
    }

    /// The only frame of `record_index`: exactly two headers carry it.
    pub(in crate::design) fn only_frame(&self, record_index: u32) -> Option<(usize, usize)> {
        match self.offsets(record_index) {
            [start, paired] => Some((*start, *paired)),
            _ => None,
        }
    }

    /// The first frame of `record_index`: its first two headers.
    pub(in crate::design) fn first_frame(&self, record_index: u32) -> Option<(usize, usize)> {
        let [start, paired] = self.offsets(record_index).first_chunk::<2>()?;
        Some((*start, *paired))
    }

    /// The frame of `record_index` that opens at `start`: the header after the
    /// one at `start`. The ascending offsets are bisected.
    pub(in crate::design) fn frame_at(
        &self,
        ctx: &DecodeContext<'_>,
        record_index: u32,
        start: usize,
    ) -> Result<Option<usize>, CodecError> {
        let offsets = self.offsets(record_index);
        let Ok(index) = ctx.binary_search(offsets, &start, "find F3D indexed record frame")? else {
            return Ok(None);
        };
        Ok(index
            .checked_add(1)
            .and_then(|next| offsets.get(next))
            .copied())
    }

    /// Consecutive header offsets carrying `record_index`, each pair delimiting
    /// one frame of that record.
    pub(in crate::design) fn frames(
        &self,
        ctx: &DecodeContext<'_>,
        record_index: u32,
    ) -> Result<impl Iterator<Item = (usize, usize)> + '_, CodecError> {
        let width = std::num::NonZeroUsize::new(2)
            .ok_or_else(|| CodecError::malformed("F3D indexed frame width is zero"))?;
        Ok(ctx
            .admit_iter(self.offsets(record_index), "scan F3D indexed record frames")?
            .windows(width)
            .map(|pair| (pair[0], pair[1])))
    }
}

/// Build the retained native stream key with the identity component's percent encoding.
pub(in crate::design) fn native_scope_charged(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<String, CodecError> {
    let encoded_len = native_scope_encoded_len(ctx, name)?;

    let mut out = ctx.retained_string(encoded_len, "f3d native stream key")?;
    append_native_scope(ctx, name, &mut out)?;
    Ok(out)
}

pub(in crate::design) fn native_scope_scoped<'a>(
    ctx: &'a DecodeContext<'_>,
    name: &str,
) -> Result<(cadmpeg_core::decode::ScopedReservation<'a>, String), CodecError> {
    let encoded_len = native_scope_encoded_len(ctx, name)?;

    let mut reservation = ctx.reserve_scoped(0, "f3d scoped native stream key")?;
    let mut out = String::new();
    ctx.reserve_scoped_string(
        &mut reservation,
        &mut out,
        encoded_len,
        "f3d scoped native stream key",
    )?;
    append_native_scope(ctx, name, &mut out)?;
    Ok((reservation, out))
}

fn native_scope_encoded_len(ctx: &DecodeContext<'_>, name: &str) -> Result<usize, CodecError> {
    percent_encoded_len(ctx, name, "measure F3D native stream key")?
        .checked_add(ids::SCHEME_PREFIX.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("measure F3D native stream key", u64::MAX - 1, u64::MAX)
        })
}

fn append_native_scope(
    ctx: &DecodeContext<'_>,
    name: &str,
    out: &mut String,
) -> Result<(), CodecError> {
    ctx.append_retained(out, ids::SCHEME_PREFIX, "write F3D native stream key")?;
    append_percent_encoded(ctx, name, out, "write F3D native stream key")
}

/// Whether the identity component's percent encoding escapes `character`.
fn is_percent_escaped(character: char) -> bool {
    matches!(character, ':' | '#' | '%') || character.is_whitespace()
}

/// Byte length of `name` under the identity component's percent encoding.
pub(in crate::design::decode) fn percent_encoded_len(
    ctx: &DecodeContext<'_>,
    name: &str,
    operation: &'static str,
) -> Result<usize, CodecError> {
    ctx.admit_iter(name, operation)?
        .try_fold(0usize, |length, character| {
            let width = if is_percent_escaped(character) {
                character.len_utf8().checked_mul(3)?
            } else {
                character.len_utf8()
            };
            length.checked_add(width)
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

/// Append `name` under the identity component's percent encoding. Each
/// written character is charged; a caller that reserved the encoded length
/// grows `out` no further.
pub(in crate::design::decode) fn append_percent_encoded(
    ctx: &DecodeContext<'_>,
    name: &str,
    out: &mut String,
    operation: &'static str,
) -> Result<(), CodecError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for character in ctx.admit_iter(name, operation)? {
        if !is_percent_escaped(character) {
            ctx.push_retained_char(out, character, operation)?;
            continue;
        }
        let mut buffer = [0; 4];
        let length = character.encode_utf8(&mut buffer).len();
        for byte in buffer.into_iter().take(length) {
            for escaped in [
                '%',
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 0x0f)]),
            ] {
                ctx.push_retained_char(out, escaped, operation)?;
            }
        }
    }
    Ok(())
}

fn clone_sketch_entity_id_charged(
    ctx: &DecodeContext<'_>,
    entity_id: &crate::records::identity::DesignEntityId,
) -> Result<crate::records::identity::DesignEntityId, CodecError> {
    let text = ctx.copy_retained_text(entity_id.as_str(), "f3d sketch placement entity ID")?;
    crate::records::identity::DesignEntityId::try_from(text).map_err(CodecError::Malformed)
}

/// Cache a stream index under its borrowed identity. A new entry charges its
/// map slot before the stream is indexed.
pub(in crate::design) fn cached_borrowed_record_offsets<'a, 's>(
    ctx: &DecodeContext<'_>,
    cache: &'a mut HashMap<&'s str, IndexedRecordOffsets>,
    stream: &'s str,
    bytes: &[u8],
) -> Result<&'a IndexedRecordOffsets, CodecError> {
    match ctx.entry_hash_map(cache, stream, "f3d indexed stream cache entry")? {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            Ok(entry.insert(IndexedRecordOffsets::build(ctx, bytes)?))
        }
    }
}

/// Cache a stream index under owned text. Only a new entry copies its key, and
/// it charges the key and its map slot before the stream is indexed.
pub(in crate::design) fn cached_owned_record_offsets<'a>(
    ctx: &DecodeContext<'_>,
    cache: &'a mut HashMap<String, IndexedRecordOffsets>,
    stream: &str,
    bytes: &[u8],
) -> Result<&'a IndexedRecordOffsets, CodecError> {
    if ctx.contains_key_hash_map(cache, stream, "find F3D indexed stream cache entry")? {
        return ctx
            .get_hash_map(cache, stream, "find F3D indexed stream cache entry")?
            .ok_or_else(|| CodecError::malformed("F3D indexed stream cache lost an entry"));
    }
    let key = ctx.copy_retained_text(stream, "f3d indexed stream cache key")?;
    match ctx.entry_hash_map(cache, key, "f3d indexed stream cache entry")? {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            Ok(entry.insert(IndexedRecordOffsets::build(ctx, bytes)?))
        }
    }
}

/// Whether `scope` is a sketch scope under any of its localized kind names.
fn is_sketch_scope(scope: &DesignParameterScope) -> bool {
    use crate::records::feature::scope::DesignScopePayload;
    matches!(
        scope.payload(),
        DesignScopePayload::Sketch(_)
            | DesignScopePayload::Esquisse(_)
            | DesignScopePayload::Skizze(_)
            | DesignScopePayload::Esboco(_)
    )
}

/// A design `BulkStream` entry with its native scope and record index.
struct SketchStream<'scan, 'ctx> {
    name: &'scan str,
    bytes: &'scan [u8],
    scope: String,
    _scope_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    records: IndexedRecordOffsets,
}

/// Decode the unique local-to-model placement frame referenced by every
/// parameter-owning sketch scope, and every member-run head placement. A
/// localized Sketch scope follows its entity container within the same
/// stream interval even though its generic reference table does not repeat
/// the entity suffix.
pub(crate) fn decode_sketch_placements(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    // The stream indexes and the lookup tables below live until the
    // placements are assembled.
    let mut storage = ctx.reserve_scoped(0, "f3d sketch placement stream index")?;
    let mut streams = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        ctx.reserve_scoped_vec(
            &mut storage,
            &mut streams,
            1,
            "f3d sketch placement stream index",
        )?;
        let bytes = scan.entry_bytes(&entry.name)?;
        let (scope_storage, scope) = native_scope_scoped(ctx, &entry.name)?;
        let records = storage.with_storage(|| IndexedRecordOffsets::build(ctx, bytes))?;
        streams.push(SketchStream {
            name: &entry.name,
            bytes,
            scope,
            _scope_storage: scope_storage,
            records,
        });
    }
    let mut stream_by_scope = HashMap::new();
    for (position, stream) in ctx
        .admit_iter(&streams, "scan F3D sketch placement streams")?
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut stream_by_scope,
                stream.scope.as_str(),
                position,
                "f3d sketch placement stream index",
            )
        })?;
    }
    let stream_of = |id: &str| -> Result<Option<usize>, CodecError> {
        let Some(scope) = record_streams::record_stream(ctx, id)? else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(&stream_by_scope, scope, "find F3D sketch placement stream")?
            .copied())
    };

    let mut visibilities = HashMap::new();
    for (position, stream) in ctx
        .admit_iter(&streams, "scan F3D sketch placement streams")?
        .enumerate()
    {
        let Some(metadata) = metadata_for_bulk_stream(ctx, scan, stream.name)? else {
            continue;
        };
        let mut decoded_storage = ctx.reserve_scoped(0, "f3d sketch visibility records")?;
        let decoded = decoded_storage
            .with_storage(|| decode_sketch_visibilities_in_stream(ctx, stream.bytes, &metadata))?;
        for &(entity_suffix, visibility) in
            ctx.admit_iter(&decoded, "scan F3D decoded sketch visibilities")?
        {
            let key = (position, entity_suffix);
            if ctx.contains_key_hash_map(
                &visibilities,
                &key,
                "f3d sketch placement visibility index",
            )? {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design stream {} repeats sketch visibility for entity {entity_suffix}",
                        stream.name
                    ),
                ));
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut visibilities,
                    key,
                    visibility,
                    "f3d sketch placement visibility index",
                )
            })?;
        }
    }

    let mut out = Vec::new();
    // Sketch scopes by stream and byte offset, for the member-run pass.
    let mut sketch_scopes = Vec::new();
    // Streams and entity suffixes that a scope placement already covers.
    let mut scope_placed = std::collections::HashSet::new();
    for scope in ctx.admit_iter(scopes, "scan F3D sketch placement scopes")? {
        if !is_sketch_scope(scope) {
            continue;
        }
        let Some(position) = stream_of(&scope.id)? else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut sketch_scopes,
            (position, scope.byte_offset(), scope.record_index),
            "f3d sketch placement scope index",
        )?;
        let (Some(binding), Some(stream)) = (scope.sketch_entity(), streams.get(position)) else {
            continue;
        };
        let Some(mut placement) = scope_placement(ctx, stream, scope, &binding.entity_id)? else {
            continue;
        };
        placement.id = design_record_id_charged(
            ctx,
            stream.name,
            ":design-sketch-placement#",
            placement.byte_offset(),
            "f3d sketch placement ID",
        )?;
        placement.visibility = ctx
            .get_hash_map(
                &visibilities,
                &(position, placement.entity_id.suffix()),
                "find F3D sketch placement visibility",
            )?
            .copied();
        storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut scope_placed,
                (position, placement.entity_id.suffix()),
                "f3d sketch scope placement index",
            )
        })?;
        ctx.push_vec(&mut out, placement, "f3d sketch placement output")?;
    }

    // A sketch entity header pairs with a same-index member-run record whose
    // leading marked reference names a head record carrying the row-major
    // 4×4 placement. A localized Sketch scope belongs to the preceding sketch
    // entity interval: it follows that entity and precedes the next sketch
    // entity in the same stream. Some member-run sketches have no scope.
    let mut sketch_entities = Vec::new();
    for entity in ctx.admit_iter(entities, "scan F3D sketch entities")? {
        if !entity.in_sketch_module() {
            continue;
        }
        let Some(position) = stream_of(&entity.id)? else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut sketch_entities,
            (position, entity),
            "f3d sketch placement entity index",
        )?;
    }
    let mut entity_offsets = Vec::new();
    for &(position, entity) in ctx.admit_iter(&sketch_entities, "scan F3D sketch entities")? {
        ctx.push_scoped_vec(
            &mut storage,
            &mut entity_offsets,
            (position, entity.byte_offset),
            "f3d sketch placement entity index",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut entity_offsets[..],
        |key| *key,
        Ord::cmp,
        "sort F3D sketch entity offsets",
    )?;
    ctx.stable_sort_by_key(
        &mut sketch_scopes[..],
        |(position, offset, _)| (*position, *offset),
        Ord::cmp,
        "sort F3D sketch scope offsets",
    )?;
    for &(position, entity) in ctx.admit_iter(&sketch_entities, "scan F3D sketch entities")? {
        let suffix = entity.entity_id.suffix();
        if ctx.contains_hash_set(
            &scope_placed,
            &(position, suffix),
            "find F3D sketch scope placement",
        )? {
            continue;
        }
        let Some(stream) = streams.get(position) else {
            continue;
        };
        let Some(head) = member_run_head(
            ctx,
            stream.bytes,
            entity.byte_offset,
            suffix,
            &stream.records,
        )?
        else {
            continue;
        };
        let mut placement = member_run_placement(ctx, stream.bytes, &entity.entity_id, head)?;
        let key = (position, entity.byte_offset);
        let next = ctx.partition_point(
            &entity_offsets,
            |candidate| Ok(*candidate <= key),
            "find F3D next sketch entity",
        )?;
        let next_entity_offset = entity_offsets
            .get(next)
            .filter(|(next_position, _)| *next_position == position)
            .map(|(_, offset)| *offset);
        let first = ctx.partition_point(
            &sketch_scopes,
            |(scope_position, offset, _)| Ok((*scope_position, *offset) <= key),
            "find F3D sketch entity scope",
        )?;
        let in_interval = |(scope_position, offset, _): &&(usize, u64, u32)| {
            *scope_position == position && next_entity_offset.is_none_or(|end| *offset < end)
        };
        if let (Some(scope), None) = (
            sketch_scopes.get(first).filter(in_interval),
            sketch_scopes.get(first + 1).filter(in_interval),
        ) {
            placement.scope_record_index = Some(scope.2);
        }
        placement.id = design_record_id_charged(
            ctx,
            stream.name,
            ":design-sketch-placement#",
            placement.byte_offset(),
            "f3d sketch placement ID",
        )?;
        placement.visibility = ctx
            .get_hash_map(
                &visibilities,
                &(position, suffix),
                "find F3D sketch placement visibility",
            )?
            .copied();
        ctx.push_vec(&mut out, placement, "f3d sketch placement output")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch 1",
    )?;
    Ok(out)
}

/// The placement a sketch scope's frame names: the only scope-placement frame
/// among the records that the scope frame's marked references name.
fn scope_placement(
    ctx: &DecodeContext<'_>,
    stream: &SketchStream<'_, '_>,
    scope: &DesignParameterScope,
    entity_id: &crate::records::identity::DesignEntityId,
) -> Result<Option<DesignSketchPlacement>, CodecError> {
    let start = usize::try_from(scope.byte_offset()).ok();
    let end = usize::try_from(scope.paired_byte_offset()).ok();
    let Some(frame) = start
        .zip(end)
        .and_then(|(start, end)| stream.bytes.get(start..end))
    else {
        return Ok(None);
    };
    let (referenced, _referenced_storage) = marked_record_indices(ctx, frame)?;
    let mut only = None;
    let mut ambiguous = false;
    for &record_index in
        ctx.admit_iter(&referenced, "scan F3D sketch placement record candidates")?
    {
        for candidate in scope_placement_frames(ctx, stream.bytes, record_index, &stream.records)? {
            ambiguous |= only.replace(candidate).is_some();
        }
    }
    let (Some(candidate), false) = (only, ambiguous) else {
        return Ok(None);
    };
    Ok(Some(scope_placement_from_frame(
        ctx,
        stream.bytes,
        scope.record_index,
        entity_id,
        candidate,
    )?))
}

/// Distinct record indices, ascending, of every marked reference in `frame`:
/// a `1` byte, a u32 record index and six zero bytes at any byte position.
fn marked_record_indices<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    frame: &[u8],
) -> Result<(Vec<u32>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d sketch placement reference index")?;
    let mut indices = Vec::new();
    let width = std::num::NonZeroUsize::new(11)
        .ok_or_else(|| CodecError::malformed("F3D marked reference width is zero"))?;
    for window in ctx
        .admit_iter(frame, "scan F3D sketch placement references")?
        .windows(width)
    {
        if window.first() != Some(&1) || !zeros_at::<6>(window, 5) {
            continue;
        }
        let Some(record_index) = View::u32_le_at(window, 1) else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut indices,
            record_index,
            "f3d sketch placement reference index",
        )?;
    }
    sorted_distinct(
        ctx,
        &mut indices,
        "order F3D sketch placement reference indices",
    )?;
    Ok((indices, storage))
}

const CURRENT_SKETCH_CONTAINER_VERSION: u32 = 18;
const SKETCH_CONTAINER_MEMBER_TYPE_GUID: &str = "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9";
const SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const SKETCH_CONTAINER_MEMBER_VERSION: u32 = 4;

/// Decode the direct display flag in every current sketch container's typed
/// Geometry member. Other sketch-container versions do not expose this member
/// layout and therefore leave neutral visibility unknown.
fn decode_sketch_visibilities_in_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> Result<Vec<(u64, DesignSketchVisibility)>, CodecError> {
    let mut out = Vec::new();
    for frame in ctx.admit_iter(
        &super::meta::typed_primary_frames(
            ctx,
            bytes,
            metadata,
            SKETCH_CONTAINER_TYPE_GUID,
            "sketch-container",
        )?,
        "scan F3D sketch visibility frames",
    )? {
        if frame.design_type.version != CURRENT_SKETCH_CONTAINER_VERSION {
            continue;
        }
        let base_matches = match frame
            .design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
        {
            Some(base) => ctx.eq_ignore_ascii_case(
                base,
                SKETCH_CONTAINER_MEMBER_TYPE_GUID,
                "match F3D sketch container base type",
            )?,
            None => false,
        };
        let module_matches = ctx.equal_bytes(
            frame.design_type.module.as_bytes(),
            DESIGN_MODULE_SKETCH.as_bytes(),
            "match F3D sketch container module",
        )?;
        if !module_matches || !base_matches {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} has incompatible registration metadata",
                    frame.entity_id
                ),
            ));
        }
        let frame_bytes = &bytes[..frame.end];
        let Some(NamedEntityHeader {
            entity_id,
            end: header_end,
            ..
        }) = (match parse_settled_entity_header(ctx, frame_bytes, frame.start)? {
            Some(header) => Some(header),
            None => parse_genesis_entity_header(ctx, frame_bytes, frame.start)?,
        })
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} has an invalid entity header",
                    frame.entity_id
                ),
            ));
        };
        let entity_suffix = entity_id.suffix();
        if entity_suffix != frame.entity_id {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} disagrees with its entity header {entity_suffix}",
                    frame.entity_id
                ),
            ));
        }
        let Some(member) = next_indexed_record_header(ctx, frame_bytes, header_end, |_| true)?
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("F3D sketch container {entity_suffix} has no typed Geometry member"),
            ));
        };
        let member_type = member
            .class_code
            .checked_sub(256)
            .and_then(|ordinal| usize::try_from(ordinal).ok())
            .and_then(|ordinal| metadata.types.get(ordinal));
        if !sketch_container_member_type_matches(ctx, member_type)? {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {entity_suffix} has an incompatible Geometry member"
                ),
            ));
        }
        let Some(visibility) =
            decode_sketch_visibility_member(frame_bytes, member.offset, entity_suffix)
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {entity_suffix} has an invalid visibility member"
                ),
            ));
        };

        ctx.push_vec(
            &mut out,
            (entity_suffix, visibility),
            "f3d sketch visibility records",
        )?;
    }
    Ok(out)
}

/// Whether a sketch container's Geometry member registers the typed
/// visibility member class.
fn sketch_container_member_type_matches(
    ctx: &DecodeContext<'_>,
    member_type: Option<&crate::records::entity_header::SegmentTypeData>,
) -> Result<bool, CodecError> {
    let Some(member_type) = member_type else {
        return Ok(false);
    };
    let operation = "match F3D sketch container member type";
    let Some(base) = member_type
        .base_type_guid
        .value()
        .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
    else {
        return Ok(false);
    };
    Ok(member_type.version == SKETCH_CONTAINER_MEMBER_VERSION
        && ctx.eq_ignore_ascii_case(
            member_type.type_guid.as_str(),
            SKETCH_CONTAINER_MEMBER_TYPE_GUID,
            operation,
        )?
        && ctx.equal_bytes(member_type.module.as_bytes(), b"Geometry", operation)?
        && ctx.eq_ignore_ascii_case(base, SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID, operation)?)
}

fn decode_sketch_visibility_member(
    bytes: &[u8],
    member_at: usize,
    entity_suffix: u64,
) -> Option<DesignSketchVisibility> {
    let record_index = View::u64_le_at(bytes, member_at + visibility_member::ENTITY_SUFFIX)?;
    if record_index != entity_suffix
        || !zeros_at::<4>(bytes, member_at + visibility_member::ZERO_RUN)
    {
        return None;
    }
    let mut cursor = member_at + visibility_member::OWNER_REFERENCE;
    let owner = take_reference(bytes, &mut cursor)?;
    if cursor != member_at + visibility_member::STREAM_ORDINAL
        || !matches!(owner.local(), Some((target, None)) if target != 0)
    {
        return None;
    }
    let stream_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, cursor)?)?;
    if bytes.get(member_at + visibility_member::RESERVED_ZERO) != Some(&0) {
        return None;
    }
    let stream_ordinal_offset = cursor;
    let visible_offset = member_at + visibility_member::VISIBLE;
    let visible = match bytes.get(visible_offset) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    if bytes.get(member_at + visibility_member::TAIL_MARKER) != Some(&1) {
        return None;
    }
    DesignSketchVisibility::new(
        stream_ordinal,
        u64_from_index(stream_ordinal_offset),
        visible,
    )
    .ok()
}

/// Byte length of a member-run head carrying an explicit 4×4 transform.
const MEMBER_RUN_HEAD_FRAME: usize = 162;

/// A placement frame and the indexed records that carry it.
struct PlacementFrame {
    record_index: u32,
    start: usize,
    paired_at: usize,
    frame: DesignSketchFrame,
}

/// Locate a member-run head placement: the paired same-index record after the
/// sketch's entity header opens with a marked reference naming a head record.
/// A 34-byte head denotes the identity placement. A 162-byte head stores
/// eleven zero bytes and the row-major 4×4 local-to-model transform at offset
/// 22.
fn member_run_head(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    entity_byte_offset: u64,
    entity_suffix: u64,
    records: &IndexedRecordOffsets,
) -> Result<Option<PlacementFrame>, CodecError> {
    let (Ok(start), Ok(entity_index)) = (
        usize::try_from(entity_byte_offset),
        u32::try_from(entity_suffix),
    ) else {
        return Ok(None);
    };
    let Some(after_entity) = start.checked_add(1) else {
        return Ok(None);
    };
    let Some(paired_at) = records.first_at_or_after(ctx, after_entity, entity_index)? else {
        return Ok(None);
    };
    // The paired record's prologue: the u32 index, zero bytes to offset 19,
    // then a marked u32 reference naming the head record.
    if !zeros_at::<8>(bytes, paired_at + 11)
        || bytes.get(paired_at + 19) != Some(&1)
        || !zeros_at::<4>(bytes, paired_at + 24)
    {
        return Ok(None);
    }
    let Some(head_index) = View::u32_le_at(bytes, paired_at + 20) else {
        return Ok(None);
    };
    let Some(head_at) = records.first_offset(head_index) else {
        return Ok(None);
    };
    // Only a head of one of the two frame lengths is a placement, so the
    // search for its end stops once a longer head is certain.
    let search_end = bytes.len().min(head_at + MEMBER_RUN_HEAD_FRAME + 11);
    let head_end =
        next_indexed_record_offset(ctx, &bytes[..search_end], head_at + 11)?.unwrap_or(bytes.len());
    let Some(form) = member_run_head_form(bytes, paired_at, head_at, head_end) else {
        return Ok(None);
    };
    let Ok(frame) = DesignSketchFrame::new(u64_from_index(head_at), form) else {
        return Ok(None);
    };
    Ok(Some(PlacementFrame {
        record_index: head_index,
        start: head_at,
        paired_at,
        frame,
    }))
}

/// The placement form of a member-run head that spans `head_at..head_end`.
fn member_run_head_form(
    bytes: &[u8],
    paired_at: usize,
    head_at: usize,
    head_end: usize,
) -> Option<DesignSketchFrameForm> {
    match head_end.checked_sub(head_at)? {
        34 if zeros_at::<10>(bytes, head_at + 11)
            && bytes_at::<3>(bytes, head_at + 21) == Some(&[1, 0, 1])
            && zeros_at::<6>(bytes, head_at + 28) =>
        {
            Some(DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: u64_from_index(paired_at),
            })
        }
        MEMBER_RUN_HEAD_FRAME if zeros_at::<11>(bytes, head_at + 11) => {
            if bytes_at::<2>(bytes, head_at + 150) != Some(&[0, 1]) {
                return None;
            }
            Some(DesignSketchFrameForm::MemberExplicit {
                paired_byte_offset: u64_from_index(paired_at),
                transform: placement_matrix_at(bytes, head_at + 22)?,
            })
        }
        _ => None,
    }
}

/// The row-major 4×4 placement matrix stored at `at`.
fn placement_matrix_at(bytes: &[u8], at: usize) -> Option<SketchPlacementMatrix> {
    let values = f64s_at::<16>(bytes, at)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.iter().copied().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    SketchPlacementMatrix::try_from(transform).ok()
}

/// The placement of the sketch entity `entity_id` at a member-run head.
fn member_run_placement(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    entity_id: &crate::records::identity::DesignEntityId,
    head: PlacementFrame,
) -> Result<DesignSketchPlacement, CodecError> {
    Ok(DesignSketchPlacement {
        id: String::new(),
        scope_record_index: None,
        entity_id: clone_sketch_entity_id_charged(ctx, entity_id)?,
        visibility: None,
        class_tag: retain_header_class_tag(ctx, bytes, head.start)?,
        record_index: head.record_index,
        paired_class_tag: retain_header_class_tag(ctx, bytes, head.paired_at)?,
        frame: head.frame,
    })
}

/// Copy the class tag of the indexed-record header at `at` into the output.
fn retain_header_class_tag(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<crate::records::references::DesignClassTag, CodecError> {
    indexed_record_header_at(bytes, at)
        .ok_or_else(|| CodecError::malformed("F3D sketch placement header is not indexed"))?
        .retain_class_tag(ctx, "copy F3D sketch placement class tag")
}

/// Every frame of `record_index` whose length and fixed layout form a
/// parameter-scope placement.
fn scope_placement_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    record_index: u32,
    records: &'a IndexedRecordOffsets,
) -> Result<impl Iterator<Item = PlacementFrame> + 'a, CodecError> {
    Ok(records
        .frames(ctx, record_index)?
        .filter_map(move |(start, paired_at)| {
            Some(PlacementFrame {
                record_index,
                start,
                paired_at,
                frame: scope_placement_frame(bytes, start, paired_at)?,
            })
        }))
}

/// The parameter-scope placement in the frame `start..paired_at`.
fn scope_placement_frame(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
) -> Option<DesignSketchFrame> {
    let frame_length = paired_at.checked_sub(start)?;
    let form = match frame_length {
        201 => DesignSketchFrameForm::ScopeCompact,
        305 => DesignSketchFrameForm::ScopeLegacy305(placement_matrix_at(bytes, start + 48)?),
        325 => DesignSketchFrameForm::ScopeLegacy325(placement_matrix_at(bytes, start + 48)?),
        329 => DesignSketchFrameForm::ScopeExplicit(placement_matrix_at(bytes, start + 55)?),
        // The `EntityGenesis`-flavor frame: `0x01` at offset 55, nine
        // zero bytes, and a form byte at offset 65. Form `0x01` is the
        // identity transform; form `0x00` is followed by the row-major
        // 4×4 f64 matrix at offset 66. The WorkPlane sibling of this
        // record class carries a marked record reference at offset 57
        // and fails the zero-run check.
        213 | 341 => {
            if bytes.get(start + 55) != Some(&1) || !zeros_at::<9>(bytes, start + 56) {
                return None;
            }
            match (frame_length, bytes.get(start + 65)) {
                (213, Some(&1)) => DesignSketchFrameForm::ScopeGenesisCompact,
                (341, Some(&0)) => DesignSketchFrameForm::ScopeGenesisExplicit(
                    placement_matrix_at(bytes, start + 66)?,
                ),
                _ => return None,
            }
        }
        _ => return None,
    };
    DesignSketchFrame::new(u64_from_index(start), form).ok()
}

/// The placement of the sketch entity `entity_id` that its parameter scope
/// `scope_record_index` names through `candidate`.
fn scope_placement_from_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &crate::records::identity::DesignEntityId,
    candidate: PlacementFrame,
) -> Result<DesignSketchPlacement, CodecError> {
    Ok(DesignSketchPlacement {
        id: String::new(),
        scope_record_index: Some(scope_record_index),
        entity_id: clone_sketch_entity_id_charged(ctx, entity_id)?,
        visibility: None,
        class_tag: retain_header_class_tag(ctx, bytes, candidate.start)?,
        record_index: candidate.record_index,
        paired_class_tag: retain_header_class_tag(ctx, bytes, candidate.paired_at)?,
        frame: candidate.frame,
    })
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub(crate) fn decode_persistent_references(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D persistent reference streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_persistent_references_from_stream(ctx, &entry.name, bytes, &mut out)?;
    }
    Ok(out)
}

fn decode_persistent_references_from_stream(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<PersistentReference>,
) -> Result<(), CodecError> {
    const TYPE_NAME: &[u8; 23] = b"IntrinsicMetaTypeuint64";
    let stream_start = out.len();
    for (name, kind) in [
        (b"pt_tag".as_slice(), PersistentReferenceKind::Point),
        (
            b"crv_primary_id".as_slice(),
            PersistentReferenceKind::CurvePrimary,
        ),
        (
            b"crv_secondary_id".as_slice(),
            PersistentReferenceKind::CurveSecondary,
        ),
    ] {
        for offset in ctx.find_bytes_iter(bytes, name, "find F3D persistent reference")? {
            let compact_type_offset = offset + name.len();
            let type_offset = if View::u32_le_at(bytes, compact_type_offset) == Some(23) {
                compact_type_offset
            } else if View::u32_le_at(bytes, compact_type_offset) == Some(2)
                && View::u32_le_at(bytes, compact_type_offset + 4) == Some(14)
                && View::u32_le_at(bytes, compact_type_offset + 22) == Some(23)
            {
                compact_type_offset + 22
            } else {
                continue;
            };
            if bytes_at::<23>(bytes, type_offset + 4) != Some(TYPE_NAME) {
                continue;
            }
            let value_offset = type_offset + 4 + TYPE_NAME.len();
            let Some(value) = View::u64_le_at(bytes, value_offset) else {
                continue;
            };
            let relative_value_offset = value_offset - offset;
            ctx.push_vec(
                out,
                PersistentReference {
                    id: design_record_id_charged(
                        ctx,
                        entry_name,
                        ":persistent-reference#",
                        u64_from_index(offset),
                        "f3d persistent reference ID",
                    )?,
                    byte_offset: u64_from_index(offset),
                    value_offset: u32::try_from(relative_value_offset).map_err(|_| {
                        ctx.refuse_codec_limit(
                            "f3d persistent reference relative offset",
                            u64::from(u32::MAX),
                            u64_from_index(relative_value_offset),
                        )
                    })?,
                    kind,
                    value,
                },
                "f3d persistent reference index",
            )?;
        }
    }
    // Each key's matches are ascending; order the stream's references by
    // offset across the three keys.
    ctx.stable_sort_by_key(
        &mut out[stream_start..],
        |reference| reference.byte_offset,
        Ord::cmp,
        "sort f3d design sketch 2",
    )
}

/// Decode every indexed `EDGE_REFERENCE_LOST` record from each design
/// `BulkStream` entry in `scan`.
pub(crate) fn decode_lost_edge_references(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_lost_edge_references_from_stream(ctx, &entry.name, bytes, &mut out)?;
    }
    Ok(out)
}

fn decode_lost_edge_references_from_stream(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<LostEdgeReference>,
) -> Result<(), CodecError> {
    const MARKER: &[u8; 19] = b"EDGE_REFERENCE_LOST";
    for offset in ctx.find_bytes_iter(bytes, MARKER, "scan F3D lost edge markers")? {
        let Some(header_offset) = offset.checked_sub(29) else {
            continue;
        };
        let Some(header) = indexed_record_header_at(bytes, header_offset) else {
            continue;
        };
        if !zeros_at::<14>(bytes, header_offset + 11)
            || View::u32_le_at(bytes, header_offset + 25) != u32::try_from(MARKER.len()).ok()
        {
            continue;
        }
        let Some(next) = indexed_record_header_at(bytes, offset + MARKER.len()) else {
            continue;
        };

        ctx.reserve_vec(out, 1, "f3d lost edge reference output")?;
        let id = design_record_id_charged(
            ctx,
            entry_name,
            ":lost-edge-reference#",
            u64_from_index(header_offset),
            "f3d lost edge reference ID",
        )?;
        let class_tag = header.retain_class_tag(ctx, "copy F3D lost-edge primary class tag")?;
        let next_class_tag = next.retain_class_tag(ctx, "copy F3D lost-edge paired class tag")?;
        let Ok(reference) = LostEdgeReference::new(
            id,
            u64_from_index(header_offset),
            class_tag.into(),
            header.record_index,
            next_class_tag.into(),
            next.record_index,
        ) else {
            continue;
        };
        out.push(reference);
    }
    Ok(())
}

/// Parse the fixed entity-header layout at `start`: a u64 entity suffix, five
/// zero bytes, an optional slot, and the UTF-16LE entity id whose numeric
/// suffix equals the header's entity suffix.
pub(super) fn parse_settled_entity_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<NamedEntityHeader>, CodecError> {
    let Some(entity_suffix) = View::u64_le_at(bytes, start + 7) else {
        return Ok(None);
    };
    if entity_suffix == 0 || !zeros_at::<5>(bytes, start + 15) {
        return Ok(None);
    }
    let (optional_slot_present, string_offset) = match bytes.get(start + 20) {
        Some(0) => (false, start + 21),
        Some(1) if zeros_at::<4>(bytes, start + 21) => (true, start + 25),
        _ => return Ok(None),
    };
    let Some((entity_id, end)) =
        lp_utf16_bounded_charged(ctx, bytes, string_offset, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return Ok(None);
    };
    Ok(
        (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
            entity_id,
            entity_id_offset: string_offset + 4,
            optional_slot_present,
            end,
        }),
    )
}

/// An admitted entity identity and its source header locations.
pub(super) struct NamedEntityHeader {
    pub(super) entity_id: crate::records::identity::DesignEntityId,
    pub(super) entity_id_offset: usize,
    optional_slot_present: bool,
    pub(super) end: usize,
}

/// Parse the `EntityGenesis` entity-header layout at `start`: the u32 record
/// index doubles as the entity suffix and is followed by a zero run, a
/// `0x01`-marked u32 1, the `EntityGenesis` and `IntrinsicMetaTypeuint64`
/// key strings, the u64 origin bitfield, and the UTF-16LE entity id whose
/// numeric suffix equals the record index.
pub(super) fn parse_genesis_entity_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<NamedEntityHeader>, CodecError> {
    let Some(entity_suffix) = View::u32_le_at(bytes, start + 7).map(u64::from) else {
        return Ok(None);
    };
    if entity_suffix == 0 {
        return Ok(None);
    }
    // The zero run holds at most 24 bytes.
    let zero_run = bytes.get(start + 11..).map_or(0, |tail| {
        tail.iter().take(24).take_while(|byte| **byte == 0).count()
    });
    let cursor = start + 11 + zero_run;
    if zero_run == 0
        || bytes.get(cursor) != Some(&1)
        || View::u32_le_at(bytes, cursor + 1) != Some(1)
    {
        return Ok(None);
    }
    let Some(after_key) = lp_ascii_matches(bytes, cursor + 5, b"EntityGenesis") else {
        return Ok(None);
    };
    let Some(after_type) = lp_ascii_matches(bytes, after_key, b"IntrinsicMetaTypeuint64") else {
        return Ok(None);
    };
    let Some((entity_id, end)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        after_type + 8,
        1..=256,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return Ok(None);
    };
    Ok(
        (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
            entity_id,
            entity_id_offset: after_type + 12,
            optional_slot_present: false,
            end,
        }),
    )
}

/// The offset after a u32-counted ASCII field at `at` that holds `expected`.
fn lp_ascii_matches<const N: usize>(bytes: &[u8], at: usize, expected: &[u8; N]) -> Option<usize> {
    if usize::try_from(View::u32_le_at(bytes, at)?).ok()? != N {
        return None;
    }
    let start = at.checked_add(4)?;
    (bytes_at::<N>(bytes, start)? == expected).then_some(start + N)
}

/// The run of marked member references `entries` in record order, each a
/// `1` byte, a u32 record index at `run_at + 11 * ordinal + 1` and six zero
/// bytes, after the `first` member. A malformed entry rejects the run; each
/// visited entry is charged before it is read.
fn collect_member_run(
    ctx: &DecodeContext<'_>,
    entries: &[[u8; 11]],
    run_at: usize,
    first: Option<crate::records::identity::Located<u32>>,
    operation: &'static str,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    let mut members = Vec::new();
    ctx.reserve_capacity(
        &mut members,
        entries.len() + usize::from(first.is_some()),
        operation,
    )?;
    if let Some(first) = first {
        ctx.push_vec(&mut members, first, operation)?;
    }
    let mut at = run_at;
    let malformed = ctx.position_by(
        entries,
        |entry| {
            let record_index = View::u32_le_at(entry, 1);
            let (1, true, Some(value)) = (entry[0], zeros_at::<6>(entry, 5), record_index) else {
                return Ok(true);
            };
            ctx.push_vec(
                &mut members,
                crate::records::identity::Located {
                    value,
                    offset: u64_from_index(at + 1),
                },
                operation,
            )?;
            at += 11;
            Ok(false)
        },
        operation,
    )?;
    Ok(malformed.is_none().then_some(members))
}

/// Parse the counted member-record run of the paired same-index container
/// record that follows an `EntityGenesis`-form sketch entity header: the u32
/// member count at paired-record offset 52, the marked reference to the
/// sketch's base-point record, and `count` entries of `0x01 + u32
/// record_index + six zero bytes` naming the sketch's owned records. The
/// base-point reference is returned as the first member.
fn parse_sketch_member_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    from: usize,
    entity_suffix: u64,
) -> Result<Vec<crate::records::identity::Located<u32>>, CodecError> {
    let Some(paired) = next_indexed_record_offset(ctx, bytes, from)? else {
        return Ok(Vec::new());
    };
    let Some(run) = sketch_member_run_layout(bytes, paired, entity_suffix) else {
        return Ok(Vec::new());
    };
    let base_point = crate::records::identity::Located {
        value: run.base_point_index,
        offset: u64_from_index(paired + 57),
    };
    Ok(collect_member_run(
        ctx,
        run.entries,
        paired + 67,
        Some(base_point),
        "f3d sketch member run",
    )?
    .unwrap_or_default())
}

/// The fixed layout around a sketch member run.
struct SketchMemberRunLayout<'a> {
    base_point_index: u32,
    entries: &'a [[u8; 11]],
}

fn sketch_member_run_layout(
    bytes: &[u8],
    paired: usize,
    entity_suffix: u64,
) -> Option<SketchMemberRunLayout<'_>> {
    if View::u32_le_at(bytes, paired + 7).map(u64::from) != Some(entity_suffix) {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(bytes, paired + 52)?).ok()?;
    if count == 0 || bytes.get(paired + 56) != Some(&1) || !zeros_at::<6>(bytes, paired + 61) {
        return None;
    }
    let base_point_index = View::u32_le_at(bytes, paired + 57)?;
    let run_start = paired.checked_add(67)?;
    let run_end = run_start.checked_add(count.checked_mul(11)?)?;
    let (entries, _) = bytes.get(run_start..run_end)?.as_chunks::<11>();
    Some(SketchMemberRunLayout {
        base_point_index,
        entries,
    })
}

/// Parse the counted member-record run of a legacy sketch container's paired
/// same-index record. The paired record stores its head-placement reference
/// at offset 19, six zero bytes, a u32 sketch ordinal and seven bytes of
/// state, then the member count at offset 41. Each member is a padded marked
/// reference.
fn parse_legacy_sketch_member_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    let Some(paired) = next_indexed_record_header(ctx, bytes, primary_at + 11, |_| true)? else {
        return Ok(None);
    };
    let paired_at = paired.offset;
    if paired.record_index != entity_suffix
        || !zeros_at::<8>(bytes, paired_at + 11)
        || bytes.get(paired_at + 19) != Some(&1)
        || !zeros_at::<6>(bytes, paired_at + 24)
    {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, paired_at + 41)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count != 0)
    else {
        return Ok(None);
    };
    let Some(entries) = count
        .checked_mul(11)
        .and_then(|length| (paired_at + 45).checked_add(length))
        .and_then(|run_end| bytes.get(paired_at + 45..run_end))
    else {
        return Ok(None);
    };
    collect_member_run(
        ctx,
        entries.as_chunks::<11>().0,
        paired_at + 45,
        None,
        "f3d legacy sketch member run",
    )
}

/// Recognize either legacy sketch-container tail. A counted container owns
/// its complete member run. A localized container omits that run and is
/// accepted only when its paired record names an exact placement-head frame.
fn parse_legacy_sketch_container_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    records: &IndexedRecordOffsets,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    if let Some(members) = parse_legacy_sketch_member_run(ctx, bytes, primary_at, entity_suffix)? {
        return Ok(Some(members));
    }
    let head = member_run_head(
        ctx,
        bytes,
        u64_from_index(primary_at),
        u64::from(entity_suffix),
        records,
    )?;
    Ok(head.map(|_| Vec::new()))
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub(crate) fn decode_entity_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    const META_SUFFIX: &str = "MetaStream.dat";
    const BULK_SUFFIX: &str = "BulkStream.dat";
    let mut out = Vec::new();
    // Entity ids are unique per Design stream, not archive-wide. Both tables
    // are keyed by the segment's stream prefix, the text before its stream
    // file name.
    let mut tables = ctx.reserve_scoped(0, "f3d entity header tables")?;
    let mut entity_modules = HashMap::<&str, HashMap<u64, &str>>::new();
    let types = decode_types(ctx, scan)?;
    let mut legacy_sketch_candidates = HashMap::<&str, Vec<u32>>::new();
    for design_type in ctx.admit_iter(&types, "scan F3D sketch design types")? {
        let Some(stream) = record_streams::record_stream(ctx, design_type.id())? else {
            continue;
        };
        let Some(prefix) = ctx.strip_suffix(stream, META_SUFFIX, "match F3D type stream")? else {
            continue;
        };
        for &entity_id in reference_runs::admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D design type entity values",
        )? {
            tables.with_storage(|| {
                insert_entity_module(
                    ctx,
                    &mut entity_modules,
                    prefix,
                    entity_id,
                    &design_type.module,
                )
            })?;
        }
        if !ctx.equal_bytes(
            design_type.module.as_bytes(),
            DESIGN_MODULE_SKETCH.as_bytes(),
            "match F3D design type module",
        )? {
            continue;
        }
        let Some(legacy_prefix) =
            ctx.strip_prefix(prefix, ids::SCHEME_PREFIX, "match F3D type stream")?
        else {
            continue;
        };
        for &identity in reference_runs::admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D legacy design type entity values",
        )? {
            let Ok(identity) = u32::try_from(identity) else {
                continue;
            };
            tables.with_storage(|| {
                insert_legacy_candidate(ctx, &mut legacy_sketch_candidates, legacy_prefix, identity)
            })?;
        }
    }
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        // Modules come from the type table of this stream's own `MetaStream`.
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        let stream_modules = match ctx.strip_suffix(&scope, BULK_SUFFIX, "match F3D bulk stream")? {
            Some(prefix) => ctx.get_hash_map(&entity_modules, prefix, "find F3D entity modules")?,
            None => None,
        };
        // Legacy sketch candidates of this stream, which skip the suffixes its
        // entity headers already name.
        let entry_prefix = ctx.strip_suffix(&entry.name, BULK_SUFFIX, "match F3D bulk stream")?;
        let has_legacy_candidates = match entry_prefix {
            Some(prefix) => ctx
                .get_hash_map(
                    &legacy_sketch_candidates,
                    prefix,
                    "find F3D legacy sketch candidates",
                )?
                .is_some_and(|candidates| !candidates.is_empty()),
            None => false,
        };
        let mut existing_storage = ctx.reserve_scoped(0, "f3d existing entity index")?;
        let mut existing = Vec::new();
        for header in indexed_record_offsets(ctx, bytes)? {
            let start = header.offset;
            let settled = parse_settled_entity_header(ctx, bytes, start)?;
            let genesis_form = settled.is_none();
            let Some(NamedEntityHeader {
                entity_id,
                optional_slot_present,
                end,
                ..
            }) = (match settled {
                Some(header) => Some(header),
                None => parse_genesis_entity_header(ctx, bytes, start)?,
            })
            else {
                continue;
            };
            let entity_suffix = entity_id.suffix();
            let module = match stream_modules {
                Some(modules) => {
                    ctx.get_hash_map(modules, &entity_suffix, "find F3D entity module")?
                }
                None => None,
            };
            let in_sketch_module = match module {
                Some(module) => ctx.equal_bytes(
                    module.as_bytes(),
                    DESIGN_MODULE_SKETCH.as_bytes(),
                    "match F3D entity module",
                )?,
                None => false,
            };
            let module = module
                .map(|module| ctx.copy_retained_text(module, "f3d entity module text"))
                .transpose()?;
            let list = if in_sketch_module {
                decode_reference_list(ctx, bytes, end)?
            } else {
                None
            };
            let record_end = list.as_ref().map_or(end, |list| list.end);
            let references =
                list.map(
                    |list| crate::records::entity_header::SketchHeaderReferences {
                        record_reference: list.record_reference.value,
                        record_reference_offset: list.record_reference.offset,
                        references: list.references,
                    },
                );
            let members = if genesis_form && in_sketch_module {
                parse_sketch_member_run(ctx, bytes, record_end, entity_suffix)?
            } else {
                Vec::new()
            };
            if let (true, Ok(index)) = (has_legacy_candidates, u32::try_from(entity_suffix)) {
                ctx.push_scoped_vec(
                    &mut existing_storage,
                    &mut existing,
                    index,
                    "f3d existing entity index",
                )?;
            }
            ctx.push_vec(
                &mut out,
                DesignEntityHeader {
                    id: design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-entity-header#",
                        u64_from_index(start),
                        "f3d entity header ID",
                    )?,
                    byte_offset: u64_from_index(start),

                    entity_id,
                    class_tag: header.retain_class_tag(ctx, "copy F3D entity header class tag")?,
                    optional_slot_present,
                    registration: crate::records::entity_header::DesignEntityRegistration::new(
                        module,
                        references,
                        crate::records::identity::ReferenceRun::located(members),
                    )
                    .map_err(CodecError::Malformed)?,
                },
                "f3d entity header output",
            )?;
        }

        // Legacy Design streams do not carry textual entity headers. Their
        // MSketch metadata names candidate record indices; only actual sketch
        // containers have a consecutive same-index pair with the legacy
        // counted member run. Materialize the same ownership abstraction used
        // by later entity-header forms so downstream binding remains uniform.
        let (true, Some(entry_prefix)) = (has_legacy_candidates, entry_prefix) else {
            continue;
        };
        let Some(candidates) = ctx
            .get_mut_hash_map(
                &mut legacy_sketch_candidates,
                entry_prefix,
                "find F3D legacy sketch candidates",
            )?
            .filter(|candidates| !candidates.is_empty())
        else {
            continue;
        };
        sorted_distinct(ctx, candidates, "order F3D legacy sketch candidates")?;
        sorted_distinct(ctx, &mut existing, "order F3D existing entity indices")?;
        let mut records_storage = ctx.reserve_scoped(0, "f3d legacy sketch record index")?;
        let records = records_storage.with_storage(|| IndexedRecordOffsets::build(ctx, bytes))?;
        for (entity_suffix, offsets) in records.records(ctx)? {
            if ctx
                .binary_search(
                    candidates,
                    &entity_suffix,
                    "find F3D legacy sketch candidate",
                )?
                .is_err()
                || ctx
                    .binary_search(&existing, &entity_suffix, "find F3D existing entity")?
                    .is_ok()
            {
                continue;
            }
            // The first header of the index that forms a container names the
            // sketch.
            let container = ctx.find_map(
                offsets,
                |&start| {
                    Ok(parse_legacy_sketch_container_members(
                        ctx,
                        bytes,
                        start,
                        entity_suffix,
                        &records,
                    )?
                    .map(|members| (start, members)))
                },
                "scan F3D legacy sketch candidate headers",
            )?;
            let Some((start, members)) = container else {
                continue;
            };
            let entity_id = ctx.format_retained(
                format_args!("Sketch_{entity_suffix}"),
                "f3d legacy sketch entity ID",
            )?;
            ctx.push_vec(
                &mut out,
                DesignEntityHeader {
                    id: design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-entity-header#",
                        u64_from_index(start),
                        "f3d entity header ID",
                    )?,
                    byte_offset: u64_from_index(start),

                    entity_id: crate::records::identity::DesignEntityId::try_from(entity_id)
                        .map_err(CodecError::Malformed)?,
                    class_tag: retain_header_class_tag(ctx, bytes, start)?,
                    optional_slot_present: false,
                    registration: crate::records::entity_header::DesignEntityRegistration::new(
                        Some(ctx.copy_retained_text(
                            DESIGN_MODULE_SKETCH,
                            "f3d legacy sketch module text",
                        )?),
                        None,
                        crate::records::identity::ReferenceRun::located(members),
                    )
                    .map_err(CodecError::Malformed)?,
                },
                "f3d entity header output",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch 3",
    )?;
    Ok(out)
}

fn insert_entity_module<'a>(
    ctx: &DecodeContext<'_>,
    entity_modules: &mut HashMap<&'a str, HashMap<u64, &'a str>>,
    stream: &'a str,
    entity_id: u64,
    module: &'a str,
) -> Result<(), CodecError> {
    let stream_modules = ctx
        .entry_hash_map(entity_modules, stream, "f3d entity module stream")?
        .or_default();
    if !ctx.contains_key_hash_map(stream_modules, &entity_id, "f3d entity module index")? {
        ctx.insert_hash_map(stream_modules, entity_id, module, "f3d entity module index")?;
    }
    Ok(())
}

fn insert_legacy_candidate<'a>(
    ctx: &DecodeContext<'_>,
    streams: &mut HashMap<&'a str, Vec<u32>>,
    prefix: &'a str,
    identity: u32,
) -> Result<(), CodecError> {
    let candidates = ctx
        .entry_hash_map(streams, prefix, "f3d legacy sketch stream")?
        .or_default();
    ctx.push_vec(candidates, identity, "f3d legacy sketch candidate")
}

/// Sort `values` and drop repeated values.
pub(super) fn sorted_distinct(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<u32>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.stable_sort_by_key(&mut values[..], |value| *value, Ord::cmp, operation)?;
    ctx.dedup_vec(values, operation)
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub(crate) fn decode_record_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d wanted record index")?;
    let mut wanted = HashMap::new();
    for entity in ctx.admit_iter(entities, "scan F3D sketch reference entities")? {
        let Some(references) = entity.sketch_references() else {
            continue;
        };
        let Some(scope) = record_streams::record_stream(ctx, &entity.id)? else {
            continue;
        };
        for record in ctx.admit_iter(
            &references.references,
            "scan F3D sketch entity reference slots",
        )? {
            storage.with_storage(|| want_record_index(ctx, &mut wanted, scope, record.value))?;
        }
    }
    decode_headers_for_indices(ctx, scan, &mut wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub(crate) fn decode_related_record_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d wanted record index")?;
    let mut wanted = HashMap::new();
    for (scope, index) in ctx.admit_iter(indices, "scan F3D related sketch record indices")? {
        storage.with_storage(|| want_record_index(ctx, &mut wanted, scope, *index))?;
    }
    decode_headers_for_indices(ctx, scan, &mut wanted)
}

/// Record `index` as wanted in the stream `scope`.
fn want_record_index<'a>(
    ctx: &DecodeContext<'_>,
    wanted: &mut HashMap<&'a str, Vec<u32>>,
    scope: &'a str,
    index: u32,
) -> Result<(), CodecError> {
    let indices = ctx
        .entry_hash_map(wanted, scope, "f3d wanted record index")?
        .or_default();
    ctx.push_vec(indices, index, "f3d wanted record index")
}

fn decode_headers_for_indices(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    wanted: &mut HashMap<&str, Vec<u32>>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        let Some(indices) =
            ctx.get_mut_hash_map(wanted, scope.as_str(), "find F3D wanted record indices")?
        else {
            continue;
        };
        sorted_distinct(ctx, indices, "order F3D wanted record indices")?;
        decode_headers_for_indices_from_stream(ctx, &entry.name, bytes, indices, &mut out)?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch 4",
    )?;
    Ok(out)
}

/// Emit the first header of each record index in the ascending, distinct
/// `indices`.
fn decode_headers_for_indices_from_stream(
    ctx: &DecodeContext<'_>,
    stream_name: &str,
    bytes: &[u8],
    indices: &[u32],
    out: &mut Vec<DesignRecordHeader>,
) -> Result<(), CodecError> {
    let mut emitted_storage = ctx.reserve_scoped(0, "f3d record header emitted index")?;
    let mut emitted = emitted_storage.with_storage(|| {
        ctx.alloc_filled(indices.len(), false, "f3d record header emitted index")
    })?;
    for header in indexed_record_offsets(ctx, bytes)? {
        let Ok(position) = ctx.binary_search(
            indices,
            &header.record_index,
            "find F3D wanted record index",
        )?
        else {
            continue;
        };
        let Some(seen) = emitted.get_mut(position) else {
            continue;
        };
        if std::mem::replace(seen, true) {
            continue;
        }
        let position = header.offset;
        ctx.push_vec(
            out,
            DesignRecordHeader {
                id: design_record_id_charged(
                    ctx,
                    stream_name,
                    ":design-record-header#",
                    u64_from_index(position),
                    "f3d record header ID",
                )?,
                record_index: header.record_index,
                class_tag: header.retain_class_tag(ctx, "copy F3D record header class tag")?,
                byte_offset: u64_from_index(position),
            },
            "f3d record header output",
        )?;
    }
    Ok(())
}

/// A design `BulkStream` entry and the types its segment registers by class tag.
struct RelationStream<'scan, 'types, 'ctx> {
    name: &'scan str,
    bytes: &'scan [u8],
    scope: String,
    _scope_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    types_by_class_tag: HashMap<u32, &'types crate::records::entity_header::SegmentType>,
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]). Relations are emitted
/// stream by stream in entry order, each stream's in `records` order.
pub(crate) fn decode_sketch_relations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    // A record carries no class identity of its own: its class tag selects an
    // entry in its segment's own type table, and only that entry's GUID names
    // the class across segments.
    let types = decode_types(ctx, scan)?;
    let mut storage = ctx.reserve_scoped(0, "f3d sketch relation streams")?;
    let mut streams = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let types_by_class_tag =
            storage.with_storage(|| stream_types_by_class_tag(ctx, &types, &entry.name))?;
        let (scope_storage, scope) = native_scope_scoped(ctx, &entry.name)?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut streams,
            RelationStream {
                name: &entry.name,
                bytes: scan.entry_bytes(&entry.name)?,
                scope,
                _scope_storage: scope_storage,
                types_by_class_tag,
            },
            "f3d sketch relation streams",
        )?;
    }
    let mut stream_by_scope = HashMap::new();
    for (position, stream) in ctx
        .admit_iter(&streams, "scan F3D sketch relation streams")?
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut stream_by_scope,
                stream.scope.as_str(),
                position,
                "f3d sketch relation streams",
            )
        })?;
    }
    let mut located = Vec::new();
    for record in ctx.admit_iter(records, "scan F3D sketch relation record headers")? {
        let Some(scope) = record_streams::record_stream(ctx, &record.id)? else {
            continue;
        };
        let Some(&position) =
            ctx.get_hash_map(&stream_by_scope, scope, "find F3D sketch relation stream")?
        else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut located,
            (position, record),
            "f3d sketch relation records",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut located[..],
        |(position, _)| *position,
        Ord::cmp,
        "order F3D sketch relation records",
    )?;
    for &(position, record) in ctx.admit_iter(&located, "scan F3D sketch relation records")? {
        let Some(stream) = streams.get(position) else {
            continue;
        };
        let bytes = stream.bytes;
        let Ok(at) = usize::try_from(record.byte_offset) else {
            continue;
        };
        let record_end = next_indexed_record_offset(ctx, bytes, at + 11)?.unwrap_or(bytes.len());
        let Some(payload) = bytes.get(at..record_end) else {
            continue;
        };
        let class = match ctx.get_hash_map(
            &stream.types_by_class_tag,
            &record.class_tag.code(),
            "find F3D sketch relation type",
        )? {
            Some(design_type) => {
                SketchRelationClass::of(ctx, design_type.type_guid.as_str(), design_type.version)?
            }
            None => None,
        };
        let Some(class) = class else {
            continue;
        };
        let Some(mut parsed) = parse_classed_sketch_relation(ctx, payload, class)? else {
            continue;
        };
        let Some(padding) = payload.get(parsed.parsed_end..) else {
            continue;
        };
        if ctx.any_by(
            padding,
            |byte| Ok(*byte != 0),
            "scan F3D sketch relation trailing padding",
        )? {
            continue;
        }
        let pattern = decode_pattern_definition(ctx, payload, &mut parsed)?;
        let Ok(definition) =
            crate::records::sketch_relations::SketchRelationDefinition::new(parsed.state, pattern)
        else {
            continue;
        };
        admit_sketch_relation(
            ctx,
            &mut out,
            stream.name,
            record,
            payload,
            parsed,
            definition,
        )?;
    }
    Ok(out)
}

fn admit_sketch_relation(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<SketchRelation>,
    stream: &str,
    record: &DesignRecordHeader,
    payload: &[u8],
    parsed: ParsedSketchRelation,
    definition: crate::records::sketch_relations::SketchRelationDefinition,
) -> Result<(), CodecError> {
    ctx.reserve_vec(out, 1, "f3d sketch relation output")?;
    let rectangular_counted_reference_count = match &parsed.class_members {
        RelationClassMembers::Rectangular {
            reference_count, ..
        } => Some(*reference_count),
        _ => None,
    };
    let members = crate::records::sketch_relations::SketchRelationMembers::from_indices(
        ctx,
        ctx.admit_iter(&parsed.members, "scan F3D sketch relation members")?
            .map(|member| {
                (
                    member.reference.value,
                    member.reference.offset,
                    member.relation_ordinal,
                )
            }),
    )?;
    let return_members =
        crate::records::sketch_relations::SketchRelationReturnMembers::from_indices(
            ctx,
            ctx.admit_iter(
                &parsed.return_members,
                "scan F3D sketch relation return members",
            )?
            .map(|member| (member.value, member.offset)),
        )?;
    let auxiliary_count = parsed.auxiliary_references.len();

    let mut auxiliary_references = Vec::new();
    ctx.reserve_capacity(
        &mut auxiliary_references,
        auxiliary_count,
        "f3d sketch relation auxiliary output",
    )?;
    for row in ctx.admit_iter(
        &parsed.auxiliary_references,
        "scan F3D sketch relation auxiliary references",
    )? {
        ctx.push_vec(
            &mut auxiliary_references,
            crate::records::identity::Located {
                value: row.value,
                offset: u32::try_from(row.offset).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "f3d sketch relation auxiliary offset",
                        u64::from(u32::MAX),
                        u64_from_index(row.offset),
                    )
                })?,
            },
            "f3d sketch relation auxiliary output",
        )?;
    }
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: design_record_id_charged(
            ctx,
            stream,
            ":sketch-relation#",
            u64::from(record.record_index),
            "f3d sketch relation ID",
        )?,
        record_index: record.record_index,
        class_tag: record
            .class_tag
            .try_clone_for_decode(ctx, "copy F3D sketch relation class tag")?,
        byte_offset: record.byte_offset,
        state_offset: u32::try_from(parsed.state_offset).map_err(|_| {
            ctx.refuse_codec_limit(
                "f3d sketch relation state offset",
                u64::from(u32::MAX),
                u64_from_index(parsed.state_offset),
            )
        })?,
        owner_reference: parsed.owner_reference,
        owner_entity_id: None,
        owner_reference_offset: u32::try_from(parsed.owner_reference_offset).map_err(|_| {
            ctx.refuse_codec_limit(
                "f3d sketch relation owner offset",
                u64::from(u32::MAX),
                u64_from_index(parsed.owner_reference_offset),
            )
        })?,
        auxiliary_references: crate::records::identity::ReferenceRun::located(auxiliary_references),
        rectangular_counted_reference_count,
        members,
        definition,
        entity_genesis: parsed.entity_genesis,
        return_members,
        raw_bytes: ctx.copy_retained(payload, "f3d sketch relation raw bytes")?,
    })
    .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))?;
    out.push(relation);
    Ok(())
}

/// Decode the pattern definition a relation's class members carry, reading them
/// beside the auxiliary references the parse recorded. Circular patterns store
/// the angle- and count-parameter references, the evaluated f64 total angle six
/// zero bytes after the count-parameter reference, and the evaluated u32
/// instance count directly after it. Rectangular patterns store, per direction,
/// the evaluated u32 count, the count-parameter reference, a three-component
/// f64 unit direction six zero bytes after that reference, the evaluated f64
/// source distance, and the distance-parameter reference. A non-empty counted
/// reference run stores adjacent spacing; an empty run stores the total
/// seed-to-final span. Text-frame relations repeat the sketch-text member as an
/// auxiliary reference.
fn decode_pattern_definition(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parsed: &mut ParsedSketchRelation,
) -> Result<Option<crate::records::sketch_relations::SketchPatternDefinition>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    let text_reference = match parsed.auxiliary_references.as_slice() {
        [text] => Some(text.value),
        _ => None,
    };
    // A text relation names its text entity among its members.
    let names_text_member = |parsed: &ParsedSketchRelation| -> Result<bool, CodecError> {
        let Some(text_reference) = text_reference else {
            return Ok(false);
        };
        ctx.any_by(
            &parsed.members,
            |member| Ok(member.reference.value == text_reference),
            "scan F3D sketch pattern text references",
        )
    };
    match &parsed.class_members {
        RelationClassMembers::CircularPattern => Ok(circular_pattern_definition(payload, parsed)),
        RelationClassMembers::Rectangular { clauses, .. } => {
            if parsed.state != 0x2000_0000 {
                return Ok(None);
            }
            Ok(clauses.and_then(|clauses| rectangular_pattern_definition(payload, clauses)))
        }
        RelationClassMembers::TextFrame => {
            let Some(text_reference) = text_reference else {
                return Ok(None);
            };
            if parsed.state != 0x100_0000_0000 || !names_text_member(parsed)? {
                return Ok(None);
            }
            Ok(Some(SketchPatternDefinition::TextFrame { text_reference }))
        }
        RelationClassMembers::TextPath { .. } => {
            let Some(text_reference) = text_reference else {
                return Ok(None);
            };
            if parsed.state != 0x200_0000_0000 || !names_text_member(parsed)? {
                return Ok(None);
            }
            let RelationClassMembers::TextPath { glyph_transforms } = &mut parsed.class_members
            else {
                return Ok(None);
            };
            Ok(Some(SketchPatternDefinition::TextPath {
                text_reference,
                glyph_transforms: std::mem::take(glyph_transforms),
            }))
        }
        RelationClassMembers::Plain | RelationClassMembers::Tangent => Ok(None),
    }
}

/// The circular pattern a relation's two auxiliary references and the
/// evaluated values after them define.
fn circular_pattern_definition(
    payload: &[u8],
    parsed: &ParsedSketchRelation,
) -> Option<crate::records::sketch_relations::SketchPatternDefinition> {
    let [angle_parameter, count_parameter] = parsed.auxiliary_references.as_slice() else {
        return None;
    };
    if parsed.state != 0x1000_0000 {
        return None;
    }
    let angle_at = count_parameter.offset + 4 + 6;
    let evaluated_angle = FiniteReal::new(View::f64_le_at(payload, angle_at)?)?;
    let evaluated_count = crate::records::sketch_relations::SketchPatternCount::try_from(
        View::u32_le_at(payload, angle_at + 8)?,
    )
    .ok()?;
    Some(
        crate::records::sketch_relations::SketchPatternDefinition::Circular {
            angle_parameter: angle_parameter.value,
            count_parameter: count_parameter.value,
            evaluated_angle,
            evaluated_count,
        },
    )
}

/// The rectangular pattern the two direction clauses define.
fn rectangular_pattern_definition(
    payload: &[u8],
    clauses: [crate::records::identity::Located<u32, usize>; 4],
) -> Option<crate::records::sketch_relations::SketchPatternDefinition> {
    use crate::records::sketch_relations::SketchPatternDirection;
    let [first_count, first_distance, second_count, second_distance] = clauses;
    let direction = |count: crate::records::identity::Located<u32, usize>,
                     distance: crate::records::identity::Located<u32, usize>| {
        let count_at = count.offset.checked_sub(5)?;
        let evaluated_count = crate::records::sketch_relations::SketchPatternCount::try_from(
            View::u32_le_at(payload, count_at)?,
        )
        .ok()?;
        let direction_at = count.offset + 4 + 6;
        let direction = [
            View::f64_le_at(payload, direction_at)?,
            View::f64_le_at(payload, direction_at + 8)?,
            View::f64_le_at(payload, direction_at + 16)?,
        ];
        Some(SketchPatternDirection {
            evaluated_count,
            count_parameter: count.value,
            direction: direction.try_into().ok()?,
            evaluated_distance: FiniteReal::new(View::f64_le_at(payload, direction_at + 24)?)?,
            distance_parameter: distance.value,
        })
    };
    Some(
        crate::records::sketch_relations::SketchPatternDefinition::Rectangular {
            directions: [
                direction(first_count, first_distance)?,
                direction(second_count, second_distance)?,
            ],
        },
    )
}

fn trailing_sketch_owner_reference(record: &[u8]) -> Option<u32> {
    let tail = record.len().checked_sub(11)?;
    if record.get(tail) != Some(&1) || !zeros_at::<6>(record, tail + 5) {
        return None;
    }
    View::u32_le_at(record, tail + 1)
}

fn decode_sketch_points_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchPoint>, CodecError> {
    let frames = design_primary_frames(ctx, bytes, meta)?;
    // The last frame of each entity, for reference targets.
    let mut frames_storage = ctx.reserve_scoped(0, "f3d sketch point frame index")?;
    let mut frames_by_entity = HashMap::new();
    for frame in ctx.admit_iter(&frames, "scan F3D sketch point frames")? {
        let Ok(entity_id) = u32::try_from(frame.entity_id) else {
            continue;
        };
        frames_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut frames_by_entity,
                entity_id,
                frame,
                "f3d sketch point frame index",
            )
        })?;
    }
    let mut out = Vec::new();
    for frame in ctx.admit_iter(&frames, "scan F3D sketch point frames")? {
        let design_type = frame.design_type;
        if !ctx.eq_ignore_ascii_case(
            design_type.type_guid.as_str(),
            SKETCH_POINT_TYPE_GUID,
            "match F3D sketch point type",
        )? || !ctx.equal_bytes(
            design_type.module.as_bytes(),
            CURRENT_SKETCH_POINT_TYPE.2.as_bytes(),
            "match F3D sketch point module",
        )? || ![0, 8, 10, CURRENT_SKETCH_POINT_TYPE.1].contains(&design_type.version)
        {
            continue;
        }
        let payload = &bytes[frame.start..frame.end];
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-point entity ID exceeds u32".into()))?;
        let Some(decoded) = decode_sketch_point_record(ctx, payload, design_type.version)? else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch point {record_index} has an invalid version-{} member sequence",
                    design_type.version
                ),
            ));
        };
        let (u, v) = (decoded.coordinates[0] * 10.0, decoded.coordinates[1] * 10.0);
        if !point_target_has_guid(
            ctx,
            &frames_by_entity,
            decoded.trailing_reference(),
            SKETCH_CONTAINER_TYPE_GUID,
        )? {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch point {record_index} has an invalid trailing container reference"
                ),
            ));
        }
        let companion_frame = ctx
            .get_hash_map(
                &frames_by_entity,
                &decoded.paired_reference,
                "find F3D sketch point companion",
            )?
            .copied();
        let companion = match companion_frame {
            Some(companion_frame)
                if is_sketch_point_companion_type(ctx, companion_frame.design_type)? =>
            {
                decode_sketch_point_companion(
                    ctx,
                    &bytes[companion_frame.start..companion_frame.end],
                    record_index,
                    decoded.record_form,
                    |target| {
                        Ok(ctx
                            .get_hash_map(
                                &frames_by_entity,
                                &target,
                                "find F3D sketch point curve",
                            )?
                            .map(|frame| frame.design_type.type_guid.as_str()))
                    },
                )?
            }
            _ => None,
        };
        let Some((record_form, companion)) = companion else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("F3D sketch point {record_index} has no valid inverse companion"),
            ));
        };

        ctx.reserve_vec(&mut out, 1, "f3d sketch point output")?;
        out.push(SketchPoint::try_from_charged(
            ctx,
            crate::records::sketch_geometry::SketchPointDraft {
                id: design_record_id_charged(
                    ctx,
                    stream,
                    ":sketch-point#",
                    u64_from_index(frame.start),
                    "f3d sketch point ID",
                )?,
                record_index,
                owner_reference: decoded.owner_reference,
                class_tag: frame
                    .class_tag
                    .try_clone_for_decode(ctx, "copy F3D sketch point class tag")?,
                byte_offset: u64_from_index(frame.start),
                coordinate_offset: decoded.coordinate_offset,
                record_form,
                companion,
                paired_reference: decoded.paired_reference,
                coordinates: Point2::new(u, v),
            },
        )?);
    }
    Ok(out)
}

/// Whether `design_type` registers the sketch-point companion class.
fn is_sketch_point_companion_type(
    ctx: &DecodeContext<'_>,
    design_type: &crate::records::entity_header::SegmentTypeData,
) -> Result<bool, CodecError> {
    let operation = "match F3D sketch point companion type";
    Ok(design_type.version == SKETCH_POINT_COMPANION_TYPE.1
        && ctx.eq_ignore_ascii_case(
            design_type.type_guid.as_str(),
            SKETCH_POINT_COMPANION_TYPE.0,
            operation,
        )?
        && ctx.equal_bytes(
            design_type.module.as_bytes(),
            SKETCH_POINT_COMPANION_TYPE.2.as_bytes(),
            operation,
        )?)
}

/// Decode every sketch-point record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: its versioned identity, flags, coordinates,
/// closure, trailing reference, and paired reverse curve-incidence record,
/// converted centimetre→millimetre. Class version 0 supplies `(u,v)` and no
/// persistent identity; later forms supply `(u,v,w)` and `pt_tag`. A known
/// point record with a malformed or non-finite member sequence makes the
/// stream malformed.
pub(crate) fn decode_sketch_points(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_points_from_stream)
}

#[derive(Clone, Copy)]
struct SketchProperty<'a> {
    name: &'a str,
    value: u64,
}

/// A class property block: a presence byte, and when it is `01`, a u32 count
/// and that many `(key, type name, value)` triples.
///
/// Which keys a record carries varies by record, so a caller addresses a
/// property by name. Reading the block by fixed offset misframes every record
/// whose key set differs from the one the offsets were taken from.
struct SketchProperties<'a, 'ctx> {
    entries: Vec<SketchProperty<'a>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl SketchProperties<'_, '_> {
    /// The value of the first property named `key`.
    fn find(&self, ctx: &DecodeContext<'_>, key: &str) -> Result<Option<u64>, CodecError> {
        Ok(ctx
            .find_by(
                &self.entries,
                |property| {
                    ctx.equal_bytes(
                        property.name.as_bytes(),
                        key.as_bytes(),
                        "match F3D sketch property key",
                    )
                },
                "find F3D sketch property",
            )?
            .map(|property| property.value))
    }

    /// The first property and, in a two-property block, the second. Other
    /// block sizes have no such pair.
    fn first_two(&self) -> Option<(SketchProperty<'_>, Option<SketchProperty<'_>>)> {
        match self.entries.as_slice() {
            [first] => Some((*first, None)),
            [first, second] => Some((*first, Some(*second))),
            _ => None,
        }
    }
}

/// Read the property block at `cursor`, advancing it past the block.
fn read_property_block<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &'a [u8],
    cursor: &mut usize,
) -> Result<Option<SketchProperties<'a, 'ctx>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d sketch property block")?;
    let mut entries = Vec::new();
    let mut at = *cursor;
    match payload.get(at) {
        Some(0) => at += 1,
        Some(1) => {
            let Some(count) = View::u32_le_at(payload, at + 1)
                .and_then(|count| usize::try_from(count).ok())
                .filter(|count| *count <= MAX_RELATION_RUN)
            else {
                return Ok(None);
            };
            at += 5;
            ctx.reserve_scoped_vec(
                &mut storage,
                &mut entries,
                count,
                "f3d sketch property block",
            )?;
            for _ in ctx.admit_iter(&(0..count), "scan F3D sketch property block")? {
                let Some((name, after_key)) =
                    lp_ascii_filtered_view(payload, at, 0..=256, u8::is_ascii_graphic)
                else {
                    return Ok(None);
                };
                let Some((type_name, after_type)) =
                    lp_ascii_filtered_view(payload, after_key, 0..=256, u8::is_ascii_graphic)
                else {
                    return Ok(None);
                };
                let Some(value) = View::u64_le_at(payload, after_type) else {
                    return Ok(None);
                };
                if !ctx.equal_bytes(
                    type_name.as_bytes(),
                    b"IntrinsicMetaTypeuint64",
                    "match F3D sketch property type",
                )? {
                    return Ok(None);
                }
                entries.push(SketchProperty { name, value });
                at = after_type + 8;
            }
        }
        _ => return Ok(None),
    }
    *cursor = at;
    Ok(Some(SketchProperties {
        entries,
        _storage: storage,
    }))
}

const SKETCH_TEXT_TYPE_GUIDS: [&str; 2] = [
    "E0618268-3A06-450E-9E94-7CF4C2E66802",
    "F0B1AFA3-3BAF-42D0-B2F3-94B95662F2A9",
];

/// UTF-16 text read while a record's layout is still undetermined. It is held
/// in scoped storage and copied into the output only for the layout that
/// closes the record.
struct ScopedText<'ctx> {
    text: String,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn sketch_utf16_text<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    count_at: usize,
    count: usize,
) -> Result<Option<(ScopedText<'ctx>, usize)>, CodecError> {
    Ok(lp_utf16_bounded_scoped(
        ctx,
        payload,
        count_at,
        count..=count,
        "f3d scoped Design UTF-16 text",
    )?
    .map(|(text, end, storage)| {
        (
            ScopedText {
                text,
                _storage: storage,
            },
            end,
        )
    }))
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
fn decode_sketch_texts_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    let frames = design_primary_frames(ctx, bytes, meta)?;
    for frame in ctx.admit_iter(&frames, "scan F3D sketch text frames")? {
        let mut is_text = false;
        for type_guid in SKETCH_TEXT_TYPE_GUIDS {
            if ctx.eq_ignore_ascii_case(
                frame.design_type.type_guid.as_str(),
                type_guid,
                "match F3D sketch text type",
            )? {
                is_text = true;
                break;
            }
        }
        if !is_text {
            continue;
        }
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-text entity ID exceeds u32".into()))?;
        let class_tag = frame
            .class_tag
            .try_clone_for_decode(ctx, "copy F3D sketch text class tag")?;
        let payload = &bytes[frame.start..frame.end];
        if let Some(text) = decode_sketch_text_record(
            ctx,
            payload,
            stream,
            class_tag,
            frame.design_type.version,
            record_index,
            frame.start,
        )? {
            ctx.push_vec(&mut out, text, "f3d sketch text records")?;
        }
    }
    Ok(out)
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub(crate) fn decode_sketch_texts(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchText>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_texts_from_stream)
}

/// Whether a sketch-text record carries one of the two parameter-reference
/// members. A record either writes the member, whose own presence byte then
/// says whether it targets a parameter, or omits the member entirely; nothing
/// ahead of the slot distinguishes the two.
#[derive(Clone, Copy)]
enum TextReferenceSlot {
    Omitted,
    Written,
}

const TEXT_REFERENCE_SLOTS: [TextReferenceSlot; 2] =
    [TextReferenceSlot::Omitted, TextReferenceSlot::Written];

/// Bytes between a sketch-text record's last class member and its
/// owning-sketch reference, in either identity form.
const SKETCH_TEXT_TRAILING_RUN: usize = 30;

/// How far a placement-transform element may sit from the constant a planar
/// rigid placement writes there. The transform is composed in floating point,
/// so its constants arrive rounded; the bound is far tighter than a run of
/// misframed bytes would meet.
const TEXT_PLACEMENT_TOLERANCE: f64 = 1.0e-9;

/// Bytes between the end of the stored `txt_tag` rotation and the four f32 RGBA
/// components. The first byte is zero and four further bytes are unclassified.
const TXT_TAG_POST_ROTATION_RUN: usize = 5;

/// Bytes between the text-anchor coordinates and the u32 text count in the
/// `txt_tag` form, from [`TXT_TAG_ANCHOR_MEMBER_VERSION`] onward.
const TXT_TAG_ANCHOR_RUN: usize = 11;

/// The `txt_tag` class version that adds the eleventh byte of the run between
/// the text anchor and the text count. Below it the run is ten bytes.
const TXT_TAG_ANCHOR_MEMBER_VERSION: u32 = 4;

/// The `txt_tag` class version from which a record writes its persistent
/// identity as a property-block key. A property block carrying neither identity
/// key belongs to a `txt_tag` record below this version, which stores no
/// persistent identity at all.
const TXT_TAG_IDENTITY_KEY_VERSION: u32 = 4;

/// Three unknown bytes, one i32 font weight, and eight further bytes between
/// the `txt_tag` form's counted reference run and the thirty-byte class tail.
const TXT_TAG_MEMBER_RUN: usize = 15;
const TXT_TAG_FONT_WEIGHT_AT: usize = 3;

/// Which identity key a sketch-text record carries. The key selects the layout
/// the record uses from the property block onward.
#[derive(Clone, Copy, PartialEq)]
enum SketchTextIdentity {
    /// `textex_tag`: a `1` byte, the width factor, a zero byte between the font
    /// family and the height, and the anchor inside a placement transform.
    TextexTag { width_factor: NonNegativeReal },
    /// `txt_tag`: a `0` byte, no width factor, the height directly after the
    /// font family, and the anchor stored on its own.
    TxtTag { rotation: Angle },
}

/// Read one parameter-reference slot in the given form, advancing `cursor` by
/// what that form occupies. An omitted member reads as a null reference.
fn read_text_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
    slot: TextReferenceSlot,
) -> Option<Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>> {
    match slot {
        TextReferenceSlot::Omitted => Some(Reference::Null),
        TextReferenceSlot::Written => take_reference(payload, cursor),
    }
}

/// Read the four f32 RGBA colour components both sketch-text classes store,
/// advancing `cursor` past them. Every component is a fraction of full
/// intensity, so a run leaving `[0, 1]` says the record is misframed rather
/// than that the text carries an out-of-range colour.
fn read_sketch_text_color(payload: &[u8], cursor: &mut usize) -> Option<Color> {
    let mut view = View::over_retained(payload);
    view.seek(*cursor)?;
    let mut components = [0f32; 4];
    for component in &mut components {
        let value = view.f32_le()?;
        (0.0..=1.0).contains(&value).then_some(())?;
        *component = value;
    }
    *cursor = view.position();
    let [r, g, b, a] = components;
    Color::new(r, g, b, a)
}

/// Read the row-major 4×4 f64 placement transform a frame-text record stores,
/// advancing `cursor` past it, and reduce it to the anchor point in millimetres
/// and the rotation about that point in radians.
///
/// The transform is a planar rigid placement: its third row and column are the
/// identity's, its bottom row is `(0, 0, 0, 1)`, its translation lives in the
/// last column, and its 2×2 basis is a rotation of unit determinant carrying no
/// scale or shear. A run failing any of that is not a placement, so the record
/// is misframed.
fn read_text_placement(payload: &[u8], cursor: &mut usize) -> Option<TextPlacement<FinitePoint2>> {
    let elements = f64s_at::<16>(payload, *cursor)?;
    *cursor = cursor.checked_add(128)?;
    let at = |row: usize, column: usize| elements[row * 4 + column];
    let constant = |value: f64, expected: f64| (value - expected).abs() <= TEXT_PLACEMENT_TOLERANCE;
    let planar = [
        (0, 2, 0.0),
        (1, 2, 0.0),
        (2, 0, 0.0),
        (2, 1, 0.0),
        (2, 2, 1.0),
        (2, 3, 0.0),
        (3, 0, 0.0),
        (3, 1, 0.0),
        (3, 2, 0.0),
        (3, 3, 1.0),
    ]
    .into_iter()
    .all(|(row, column, expected)| constant(at(row, column), expected));
    let determinant = at(0, 0) * at(1, 1) - at(0, 1) * at(1, 0);
    (planar && constant(determinant, 1.0)).then_some(())?;
    let anchor = Point2::new(at(0, 3) * 10.0, at(1, 3) * 10.0);
    let anchor = FinitePoint2::new(anchor)?;
    Some(TextPlacement {
        anchor,
        rotation: Angle::new(at(1, 0).atan2(at(0, 0)))?,
    })
}

/// The record index a reference names, absent when the reference is null.
fn reference_index<G: AsRef<str>, L>(reference: &Reference<G, L>) -> Option<u32> {
    reference
        .target()
        .and_then(|target| u32::try_from(target).ok())
}

/// Class-level fields of a sketch-text record, ending at the text height.
struct SketchTextHead<'ctx> {
    entity_genesis: Option<u64>,
    persistent_id: Option<u64>,
    base_id: Option<u64>,
    font_family: ScopedText<'ctx>,
    height: PositiveLength,
    color: Color,
    cursor: usize,
}

/// Fields following the height, whose framing depends on the two slot forms.
struct SketchTextTail<'ctx> {
    layout: SketchTextLayout,
    text: ScopedText<'ctx>,
    font_weight: i32,
    owner_reference: u32,
}

/// Read the class-defined leading block: the presence byte, and when it is
/// `01`, a u32 count and that many `(reference, u32)` pairs. The `txt_tag` form
/// writes such a block; the `textex_tag` form writes the `00` byte alone.
fn read_sketch_text_leading_block(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
) -> Result<Option<()>, CodecError> {
    match payload.get(*cursor) {
        Some(0) => *cursor += 1,
        Some(1) => {
            let Some(count) = View::u32_le_at(payload, *cursor + 1)
                .and_then(|count| usize::try_from(count).ok())
                .filter(|count| *count <= MAX_RELATION_RUN)
            else {
                return Ok(None);
            };
            *cursor += 5;
            for _ in ctx.admit_iter(&(0..count), "scan F3D sketch text leading block")? {
                if take_reference(payload, cursor).is_none()
                    || View::u32_le_at(payload, *cursor).is_none()
                {
                    return Ok(None);
                }
                *cursor += 4;
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(()))
}

/// Read the font-family count and text at `cursor`.
fn read_font_family<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
) -> Result<Option<(ScopedText<'ctx>, usize)>, CodecError> {
    let Some(font_count) = View::u32_le_at(payload, cursor)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| (1..=1_024).contains(count))
    else {
        return Ok(None);
    };
    sketch_utf16_text(ctx, payload, cursor, font_count)
}

/// Read the text-content count and text at `cursor`.
fn read_text_content<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
) -> Result<Option<(ScopedText<'ctx>, usize)>, CodecError> {
    let Some(text_count) = View::u32_le_at(payload, cursor)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| (1..=1_048_576).contains(count))
    else {
        return Ok(None);
    };
    sketch_utf16_text(ctx, payload, cursor, text_count)
}

/// The f64 text height, scaled to millimetres, at `cursor`.
fn text_height_at(payload: &[u8], cursor: usize) -> Option<PositiveLength> {
    PositiveLength::new(View::f64_le_at(payload, cursor)? * 10.0)
}

/// Read the record prefix, property block, and the metrics up to the height.
fn decode_sketch_text_head<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    class_version: u32,
) -> Result<Option<(SketchTextHead<'ctx>, SketchTextIdentity)>, CodecError> {
    // Record prefix: the LP-ASCII class tag, the u64 entity ID, and the
    // LP-ASCII record name.
    let Some((_, after_tag)) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit) else {
        return Ok(None);
    };
    let Some((_, mut cursor)) =
        lp_ascii_filtered_view(payload, after_tag + 8, 0..=256, u8::is_ascii_graphic)
    else {
        return Ok(None);
    };
    if read_sketch_text_leading_block(ctx, payload, &mut cursor)?.is_none() {
        return Ok(None);
    }
    // The block carries the text identities under keys that vary by record, so
    // each is addressed by name. The identity key is `textex_tag` or `txt_tag`,
    // and which of the two the record carries selects the layout that follows.
    // A `txt_tag` record below TXT_TAG_IDENTITY_KEY_VERSION writes neither key
    // and carries no persistent identity, so its class version selects the
    // layout in place of a key.
    let Some(properties) = read_property_block(ctx, payload, &mut cursor)? else {
        return Ok(None);
    };
    let textex_tag = properties.find(ctx, "textex_tag")?;
    let txt_tag = properties.find(ctx, "txt_tag")?;
    let is_txt_tag = match (textex_tag, txt_tag) {
        (Some(_), _) => false,
        (None, Some(_)) => true,
        (None, None) if class_version < TXT_TAG_IDENTITY_KEY_VERSION => true,
        (None, None) => return Ok(None),
    };
    let (identity, color, persistent_id) = if is_txt_tag {
        let Some(rotation) = View::f64_le_at(payload, cursor).and_then(Angle::new) else {
            return Ok(None);
        };
        cursor += 8;
        if payload.get(cursor) != Some(&0) {
            return Ok(None);
        }
        cursor += TXT_TAG_POST_ROTATION_RUN;
        let Some(color) = read_sketch_text_color(payload, &mut cursor) else {
            return Ok(None);
        };
        (SketchTextIdentity::TxtTag { rotation }, color, txt_tag)
    } else {
        if payload.get(cursor) != Some(&1) {
            return Ok(None);
        }
        cursor += 1;
        let Some(width_factor) = View::f64_le_at(payload, cursor).and_then(NonNegativeReal::new)
        else {
            return Ok(None);
        };
        cursor += 8;
        let Some(color) = read_sketch_text_color(payload, &mut cursor) else {
            return Ok(None);
        };
        (
            SketchTextIdentity::TextexTag { width_factor },
            color,
            textex_tag,
        )
    };
    let Some((font_family, after_font)) = read_font_family(ctx, payload, cursor)? else {
        return Ok(None);
    };
    cursor = after_font;
    if matches!(identity, SketchTextIdentity::TextexTag { .. }) {
        if payload.get(cursor) != Some(&0) {
            return Ok(None);
        }
        cursor += 1;
    }
    let Some(height) = text_height_at(payload, cursor) else {
        return Ok(None);
    };
    cursor += 8;
    Ok(Some((
        SketchTextHead {
            entity_genesis: properties.find(ctx, "EntityGenesis")?,
            persistent_id,
            base_id: properties.find(ctx, "txt_tag_base")?,
            font_family,
            height,
            color,
            cursor,
        },
        identity,
    )))
}

/// Read the indexed Design form of a `textex_tag` record. It has no leading
/// relation block or record-name string: the indexed header is followed by a
/// nine-byte zero entity lane and the ordinary property block. Its one-byte
/// width prefix is zero, unlike the legacy class form's one-byte prefix of
/// one; the f64 width factor and the remaining metrics have the same roles.
fn decode_indexed_sketch_text_head<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(SketchTextHead<'ctx>, NonNegativeReal)>, CodecError> {
    let Some((_, after_tag)) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit) else {
        return Ok(None);
    };
    if after_tag != 7
        || View::u32_le_at(payload, after_tag).is_none()
        || !zeros_at::<9>(payload, 11)
    {
        return Ok(None);
    }
    let mut cursor = 20;
    let Some(properties) = read_property_block(ctx, payload, &mut cursor)? else {
        return Ok(None);
    };
    let Some(persistent_id) = properties.find(ctx, "textex_tag")? else {
        return Ok(None);
    };
    if payload.get(cursor) != Some(&0) {
        return Ok(None);
    }
    cursor += 1;
    let Some(width_factor) = View::f64_le_at(payload, cursor).and_then(NonNegativeReal::new) else {
        return Ok(None);
    };
    cursor += 8;
    let Some(color) = read_sketch_text_color(payload, &mut cursor) else {
        return Ok(None);
    };
    let Some((font_family, after_font)) = read_font_family(ctx, payload, cursor)? else {
        return Ok(None);
    };
    cursor = after_font;
    if payload.get(cursor) != Some(&0) {
        return Ok(None);
    }
    cursor += 1;
    let Some(height) = text_height_at(payload, cursor) else {
        return Ok(None);
    };
    cursor += 8;
    Ok(Some((
        SketchTextHead {
            entity_genesis: properties.find(ctx, "EntityGenesis")?,
            persistent_id: Some(persistent_id),
            base_id: properties.find(ctx, "txt_tag_base")?,
            font_family,
            height,
            color,
            cursor,
        },
        width_factor,
    )))
}

/// The alignment fields and text content shared by the legacy and indexed
/// `textex_tag` tails, which follow the first parameter-reference slot.
struct TextBody<'a, 'ctx> {
    first_reference: Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>,
    second_reference: Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>,
    horizontal_alignment: u32,
    vertical_alignment: u32,
    text: ScopedText<'ctx>,
    font_weight: i32,
    cursor: usize,
}

/// Read the first slot, the horizontal alignment enum and three flag bytes,
/// the text, the second slot, the vertical alignment enum, one flag byte and
/// the font weight.
fn read_text_body<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &'a [u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
) -> Result<Option<TextBody<'a, 'ctx>>, CodecError> {
    let Some(first_reference) = read_text_reference(payload, &mut cursor, first_slot) else {
        return Ok(None);
    };
    let Some(horizontal_alignment) = View::u32_le_at(payload, cursor) else {
        return Ok(None);
    };
    cursor += 7;
    let Some((text, after_text)) = read_text_content(ctx, payload, cursor)? else {
        return Ok(None);
    };
    cursor = after_text;
    let Some(second_reference) = read_text_reference(payload, &mut cursor, second_slot) else {
        return Ok(None);
    };
    let Some(vertical_alignment) = View::u32_le_at(payload, cursor) else {
        return Ok(None);
    };
    let Some(font_weight) = View::u32_le_at(payload, cursor + 5)
        .and_then(|weight| i32::try_from(weight).ok())
        .filter(|weight| matches!(weight, 400 | 500 | 750))
    else {
        return Ok(None);
    };
    Ok(Some(TextBody {
        first_reference,
        second_reference,
        horizontal_alignment,
        vertical_alignment,
        text,
        font_weight,
        cursor: cursor + 9,
    }))
}

/// Read the alignment fields, text content, and class tail under one pair of
/// slot forms, requiring the walk to end exactly on the owning-sketch
/// reference.
fn decode_sketch_text_tail<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail<'ctx>>, CodecError> {
    let Some(body) = read_text_body(ctx, payload, cursor, first_slot, second_slot)? else {
        return Ok(None);
    };
    Ok(
        close_sketch_text_tail(payload, body.cursor).and_then(|(placement, owner_reference)| {
            Some(SketchTextTail {
                layout: SketchTextLayout::TextexTag {
                    width_factor,
                    alignment: Some(SketchTextAlignment {
                        horizontal: body.horizontal_alignment,
                        vertical: body.vertical_alignment,
                    }),
                    first_reference: reference_index(&body.first_reference),
                    second_reference: reference_index(&body.second_reference),
                    placement,
                },
                text: body.text,
                font_weight: body.font_weight,
                owner_reference,
            })
        }),
    )
}

/// Read the legacy class tail at `cursor`: the text-type enum, the placement
/// it gates, the trailing run, and the owning-sketch reference that ends the
/// record. Returns the placement and the owner.
fn close_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
) -> Option<(Option<TextPlacement<FinitePoint2>>, u32)> {
    // The class tail opens with the text-type enum, which gates the placement
    // transform: frame text stores a 4x4 transform, path text stores none. One
    // flag byte follows the enum and repeats it, so a slot form that has
    // desynchronized fails here instead of framing a transform out of whatever
    // bytes the walk landed on.
    let text_type = View::u32_le_at(payload, cursor)?;
    (u32::from(*payload.get(cursor.checked_add(4)?)?) == text_type).then_some(())?;
    cursor = cursor.checked_add(5)?;
    let placement = match text_type {
        0 => Some(read_text_placement(payload, &mut cursor)?),
        1 => None,
        _ => return None,
    };
    cursor = cursor.checked_add(SKETCH_TEXT_TRAILING_RUN)?;
    let owner = take_reference(payload, &mut cursor)?;
    (cursor == payload.len()).then_some(())?;
    Some((placement, reference_index(&owner)?))
}

/// Read the `txt_tag` form's members from the height to the end of the record.
/// Two bytes separate the height from the anchor coordinates, which this form
/// stores directly rather than in a placement transform. The form writes no
/// parameter-reference slot: an eleven-byte unclassified member run, ten bytes
/// below [`TXT_TAG_ANCHOR_MEMBER_VERSION`], follows the anchor. The text string
/// is followed by a counted reference run, fifteen bytes, and the trailing run
/// and owning-sketch reference that close both forms.
fn decode_txt_tag_sketch_text_tail<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    class_version: u32,
    rotation: Angle,
) -> Result<Option<SketchTextTail<'ctx>>, CodecError> {
    let mut cursor = cursor + 2;
    let anchor = View::f64_le_at(payload, cursor)
        .zip(View::f64_le_at(payload, cursor + 8))
        .and_then(|(u, v)| FinitePoint2::new(Point2::new(u * 10.0, v * 10.0)));
    let Some(anchor) = anchor else {
        return Ok(None);
    };
    let anchor_run = if class_version < TXT_TAG_ANCHOR_MEMBER_VERSION {
        TXT_TAG_ANCHOR_RUN - 1
    } else {
        TXT_TAG_ANCHOR_RUN
    };
    cursor += 16 + anchor_run;
    let Some((text, after_text)) = read_text_content(ctx, payload, cursor)? else {
        return Ok(None);
    };
    cursor = after_text;
    let Some(references) = View::u32_le_at(payload, cursor)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count <= MAX_RELATION_RUN)
    else {
        return Ok(None);
    };
    cursor += 4;
    for _ in ctx.admit_iter(&(0..references), "scan F3D sketch text reference run")? {
        if take_reference(payload, &mut cursor).is_none() {
            return Ok(None);
        }
    }
    let Some(font_weight) = View::u32_le_at(payload, cursor + TXT_TAG_FONT_WEIGHT_AT)
        .and_then(|weight| i32::try_from(weight).ok())
        .filter(|weight| matches!(weight, 400 | 500 | 750))
    else {
        return Ok(None);
    };
    cursor += TXT_TAG_MEMBER_RUN + SKETCH_TEXT_TRAILING_RUN;
    let Some(owner) = take_reference(payload, &mut cursor) else {
        return Ok(None);
    };
    let Some(owner_reference) = reference_index(&owner).filter(|_| cursor == payload.len()) else {
        return Ok(None);
    };
    Ok(Some(SketchTextTail {
        layout: SketchTextLayout::TxtTag {
            placement: TextPlacement { anchor, rotation },
        },
        text,
        font_weight,
        owner_reference,
    }))
}

/// Read the indexed `textex_tag` tail. The member and alignment fields match
/// the common text body. The indexed class then writes a fixed 35-byte suffix:
/// a text-type u32, three fixed u32 values, a zero u32, a two-byte zero run,
/// two positive f32 scales, and a five-byte zero run before the owning-sketch
/// reference. The text-type values are the same frame (`0`) and path (`1`)
/// discriminators used by the legacy class tail. The indexed suffix carries no
/// neutral anchor or rotation; the source record is retained in full.
fn decode_indexed_sketch_text_tail<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail<'ctx>>, CodecError> {
    let Some(body) = read_text_body(ctx, payload, cursor, first_slot, second_slot)? else {
        return Ok(None);
    };
    let Some(owner_reference) = close_indexed_sketch_text_tail(payload, body.cursor) else {
        return Ok(None);
    };
    Ok(Some(SketchTextTail {
        layout: SketchTextLayout::TextexTag {
            width_factor,
            alignment: Some(SketchTextAlignment {
                horizontal: body.horizontal_alignment,
                vertical: body.vertical_alignment,
            }),
            first_reference: reference_index(&body.first_reference),
            second_reference: reference_index(&body.second_reference),
            placement: None,
        },
        text: body.text,
        font_weight: body.font_weight,
        owner_reference,
    }))
}

/// Read the indexed class's fixed 35-byte suffix at `cursor` and the
/// owning-sketch reference that ends the record.
fn close_indexed_sketch_text_tail(payload: &[u8], cursor: usize) -> Option<u32> {
    if !matches!(View::u32_le_at(payload, cursor)?, 0 | 1)
        || View::u32_le_at(payload, cursor.checked_add(4)?)? != 1
        || View::u32_le_at(payload, cursor.checked_add(8)?)? != 256
        || View::u32_le_at(payload, cursor.checked_add(12)?)? != 0
        || View::u32_le_at(payload, cursor.checked_add(16)?)? != 0
        || !zeros_at::<2>(payload, cursor.checked_add(20)?)
    {
        return None;
    }
    let scale_u = View::f32_le_at(payload, cursor.checked_add(22)?)?;
    let scale_v = View::f32_le_at(payload, cursor.checked_add(26)?)?;
    if !scale_u.is_finite() || !scale_v.is_finite() || scale_u <= 0.0 || scale_v <= 0.0 {
        return None;
    }
    if !zeros_at::<5>(payload, cursor.checked_add(30)?) {
        return None;
    }
    let mut cursor = cursor.checked_add(35)?;
    let owner = take_reference(payload, &mut cursor)?;
    (cursor == payload.len()).then_some(())?;
    reference_index(&owner)
}

/// The tail that exactly one pair of slot forms closes. Two forms that both
/// end on the owning-sketch reference leave the parameter references
/// undetermined.
fn only_closed_tail<'ctx>(
    mut decode: impl FnMut(
        TextReferenceSlot,
        TextReferenceSlot,
    ) -> Result<Option<SketchTextTail<'ctx>>, CodecError>,
) -> Result<Option<SketchTextTail<'ctx>>, CodecError> {
    let mut closed = None;
    for first_slot in TEXT_REFERENCE_SLOTS {
        for second_slot in TEXT_REFERENCE_SLOTS {
            let Some(tail) = decode(first_slot, second_slot)? else {
                continue;
            };
            if closed.replace(tail).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(closed)
}

struct SketchTextRecord<'a, 'ctx> {
    stream: &'a str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
    head: SketchTextHead<'ctx>,
    tail: SketchTextTail<'ctx>,
}

fn assemble_sketch_text(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    input: SketchTextRecord<'_, '_>,
) -> Result<SketchText, CodecError> {
    let SketchTextRecord {
        stream,
        class_tag,
        class_version,
        record_index,
        byte_offset,
        head,
        tail,
    } = input;
    let font_family = ctx.copy_retained_text(&head.font_family.text, "f3d Design UTF-16 text")?;
    let text = ctx.copy_retained_text(&tail.text.text, "f3d Design UTF-16 text")?;
    let id = design_record_id_charged(
        ctx,
        stream,
        ":sketch-text#",
        u64::try_from(byte_offset)
            .map_err(|_| ctx.refuse_codec_limit("f3d sketch text offset", 0, 1))?,
        "f3d sketch text identifier",
    )?;
    let raw_bytes = ctx.copy_retained(payload, "f3d sketch text raw bytes")?;
    Ok(SketchText {
        id,
        record_index,
        owner_reference: tail.owner_reference,
        class_tag,
        class_version,
        byte_offset: u64_from_index(byte_offset),
        entity_genesis: head.entity_genesis,
        persistent_id: head.persistent_id,
        base_id: head.base_id,
        text,
        font_family,
        font_weight: tail.font_weight,
        height: head.height,
        color: head.color,
        layout: tail.layout,
        raw_bytes,
    })
}

pub(crate) fn decode_sketch_text_record(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
) -> Result<Option<SketchText>, CodecError> {
    let mut decoded = None;
    if let Some((head, identity)) = decode_sketch_text_head(ctx, payload, class_version)? {
        let tail = match identity {
            SketchTextIdentity::TextexTag { width_factor } => {
                only_closed_tail(|first_slot, second_slot| {
                    decode_sketch_text_tail(
                        ctx,
                        payload,
                        head.cursor,
                        first_slot,
                        second_slot,
                        width_factor,
                    )
                })?
            }
            SketchTextIdentity::TxtTag { rotation } => {
                decode_txt_tag_sketch_text_tail(ctx, payload, head.cursor, class_version, rotation)?
            }
        };
        decoded = tail.map(|tail| (head, tail));
    }
    if decoded.is_none() {
        let Some((head, width_factor)) = decode_indexed_sketch_text_head(ctx, payload)? else {
            return Ok(None);
        };
        let tail = only_closed_tail(|first_slot, second_slot| {
            decode_indexed_sketch_text_tail(
                ctx,
                payload,
                head.cursor,
                first_slot,
                second_slot,
                width_factor,
            )
        })?;
        decoded = tail.map(|tail| (head, tail));
    }
    let Some((head, tail)) = decoded else {
        return Ok(None);
    };
    Ok(Some(assemble_sketch_text(
        ctx,
        payload,
        SketchTextRecord {
            stream,
            class_tag,
            class_version,
            record_index,
            byte_offset,
            head,
            tail,
        },
    )?))
}

#[derive(Debug)]
struct DecodedSketchPoint {
    owner_reference: Option<u32>,
    coordinate_offset: u32,
    record_form: SketchPointRecordForm,
    paired_reference: u32,
    coordinates: [f64; 2],
}

impl DecodedSketchPoint {
    fn trailing_reference(&self) -> Option<u32> {
        match self.record_form {
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            }
            | SketchPointRecordForm::Version11InlineTyped {
                trailing_reference, ..
            } => Some(trailing_reference),
            _ => self.owner_reference,
        }
    }
}

fn take_local_sketch_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
) -> Option<(u32, Option<&'a str>)> {
    let reference = take_reference(payload, cursor)?;
    let (target, inline_type_guid) = reference.into_local()?;
    Some((u32::try_from(target).ok()?, inline_type_guid))
}

fn take_same_segment_sketch_reference(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let (target, inline_type_guid) = take_local_sketch_reference(payload, cursor)?;
    inline_type_guid.is_none().then_some(target)
}

fn decode_version_zero_sketch_point(
    payload: &[u8],
    header_end: usize,
) -> Option<DecodedSketchPoint> {
    let mut cursor = header_end;
    if !zeros_at::<10>(payload, cursor) {
        return None;
    }
    cursor = cursor.checked_add(10)?;
    let paired_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    let flag = *payload.get(cursor)?;
    if flag > 1 {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let x = View::f64_le_at(payload, cursor)?;
    let y = View::f64_le_at(payload, cursor.checked_add(8)?)?;
    cursor = cursor.checked_add(16)?;
    if !zeros_at::<20>(payload, cursor)
        || View::f32_le_at(payload, cursor + 20) != Some(1.0)
        || !zeros_at::<12>(payload, cursor + 24)
        || View::f32_le_at(payload, cursor + 36) != Some(1.0)
        || View::f32_le_at(payload, cursor + 40) != Some(1.0)
        || bytes_at::<10>(payload, cursor + 44) != Some(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0])
    {
        return None;
    }
    cursor = cursor.checked_add(54)?;
    if take_same_segment_sketch_reference(payload, &mut cursor)? != paired_reference {
        return None;
    }
    let owner_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference: Some(owner_reference),
        coordinate_offset,
        record_form: SketchPointRecordForm::Version0 { flag: flag == 1 },
        paired_reference,
        coordinates: [x, y],
    })
}

/// Whether an inline type GUID names `expected`.
fn inline_guid_is(
    ctx: &DecodeContext<'_>,
    type_guid: Option<&str>,
    expected: &str,
) -> Result<bool, CodecError> {
    match type_guid {
        Some(type_guid) => {
            ctx.eq_ignore_ascii_case(type_guid, expected, "match F3D inline type GUID")
        }
        None => Ok(false),
    }
}

fn decode_sketch_point_record(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_version: u32,
) -> Result<Option<DecodedSketchPoint>, CodecError> {
    let Some((_, after_class_tag)) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)
    else {
        return Ok(None);
    };
    let header_end = after_class_tag + 4;
    if class_version == 0 {
        return Ok(decode_version_zero_sketch_point(payload, header_end));
    }
    if !matches!(class_version, 8 | 10 | 11) || !zeros_at::<9>(payload, header_end) {
        return Ok(None);
    }
    let mut cursor = header_end + 9;
    let Some(properties) = read_property_block(ctx, payload, &mut cursor)? else {
        return Ok(None);
    };
    let Some((first, second)) = properties.first_two() else {
        return Ok(None);
    };
    let is_key = |property: SketchProperty<'_>, key: &str| {
        ctx.equal_bytes(
            property.name.as_bytes(),
            key.as_bytes(),
            "match F3D sketch point property key",
        )
    };
    let (entity_genesis, persistent_id) = match second {
        None if is_key(first, "pt_tag")? => (None, first.value),
        Some(second)
            if class_version == 11
                && is_key(first, "EntityGenesis")?
                && is_key(second, "pt_tag")? =>
        {
            (Some(first.value), second.value)
        }
        _ => return Ok(None),
    };
    let Some(persistent_id) = std::num::NonZeroU64::new(persistent_id) else {
        return Ok(None);
    };
    let Some((paired_reference, paired_type_guid)) =
        take_local_sketch_reference(payload, &mut cursor)
    else {
        return Ok(None);
    };
    let inline_typed = match paired_type_guid {
        None => false,
        Some(_)
            if matches!(class_version, 10 | 11)
                && inline_guid_is(ctx, paired_type_guid, SKETCH_POINT_COMPANION_TYPE.0)? =>
        {
            true
        }
        Some(_) => return Ok(None),
    };
    let flag_count = if class_version == 11 { 8 } else { 7 };
    let Some(source_flags) = payload.get(cursor..cursor + flag_count) else {
        return Ok(None);
    };
    if source_flags.iter().any(|flag| *flag > 1) {
        return Ok(None);
    }
    let mut flags = [false; 8];
    for (slot, flag) in flags.iter_mut().zip(source_flags) {
        *slot = *flag == 1;
    }
    cursor += flag_count;
    let coordinate_offset = u32::try_from(cursor).ok();
    let coordinates = View::f64_le_at(payload, cursor).zip(View::f64_le_at(payload, cursor + 8));
    let depth = View::f64_le_at(payload, cursor + 16).map(|depth| depth * 10.0);
    let (Some(coordinate_offset), Some((u, v)), Some(depth)) =
        (coordinate_offset, coordinates, depth)
    else {
        return Ok(None);
    };
    cursor += 24;
    let selector = View::u64_le_at(payload, cursor);
    let state = payload.get(cursor + 8).copied();
    let reserved_zeros = if class_version == 8 {
        zeros_at::<8>(payload, cursor + 9)
    } else {
        zeros_at::<12>(payload, cursor + 9)
    };
    let floats_at = cursor + 9 + if class_version == 8 { 8 } else { 12 };
    let (Some(selector), Some(state), true) = (selector, state, reserved_zeros) else {
        return Ok(None);
    };
    if View::f32_le_at(payload, floats_at) != Some(1.0)
        || View::f32_le_at(payload, floats_at + 4) != Some(1.0)
        || bytes_at::<5>(payload, floats_at + 8) != Some(&[0, 1, 0, 0, 0])
    {
        return Ok(None);
    }
    cursor = floats_at + 13;
    let Some((repeated_reference, repeated_type_guid)) =
        take_local_sketch_reference(payload, &mut cursor)
    else {
        return Ok(None);
    };
    let repeated_encoding_matches = match repeated_type_guid {
        None => !inline_typed,
        Some(_) => {
            inline_typed && inline_guid_is(ctx, repeated_type_guid, SKETCH_POINT_COMPANION_TYPE.0)?
        }
    };
    if repeated_reference != paired_reference || !repeated_encoding_matches {
        return Ok(None);
    }
    let Some(closure) = SketchPointClosure::from_pair(selector, state) else {
        return Ok(None);
    };
    let mut seven = [false; 7];
    seven.copy_from_slice(&flags[..7]);
    let Some((record_form, owner_reference)) = (match (class_version, inline_typed) {
        (8, false) if closure == SketchPointClosure::Selector0State0 => {
            take_same_segment_sketch_reference(payload, &mut cursor).map(|owner| {
                (
                    SketchPointRecordForm::Version8 {
                        depth,
                        persistent_id,
                        flags: seven,
                    },
                    Some(owner),
                )
            })
        }
        (10, false) => take_same_segment_sketch_reference(payload, &mut cursor)
            .zip(crate::records::sketch_geometry::SketchPointClosure10::from_closure(closure))
            .map(|(owner, closure)| {
                (
                    SketchPointRecordForm::Version10 {
                        depth,
                        persistent_id,
                        flags: seven,
                        closure,
                    },
                    Some(owner),
                )
            }),
        (10, true) => {
            let Some((trailing_reference, type_guid)) =
                take_local_sketch_reference(payload, &mut cursor)
            else {
                return Ok(None);
            };
            if !inline_guid_is(ctx, type_guid, SKETCH_CONTAINER_TYPE_GUID)? {
                return Ok(None);
            }
            crate::records::sketch_geometry::SketchPointClosure10Inline::from_closure(closure).map(
                |closure| {
                    (
                        SketchPointRecordForm::Version10InlineTyped {
                            depth,
                            trailing_reference,
                            persistent_id,
                            flags: seven,
                            closure,
                        },
                        None,
                    )
                },
            )
        }
        (11, true) => {
            let Some((trailing_reference, type_guid)) =
                take_local_sketch_reference(payload, &mut cursor)
            else {
                return Ok(None);
            };
            if !inline_guid_is(ctx, type_guid, SKETCH_CONTAINER_TYPE_GUID)? {
                return Ok(None);
            }
            Some((
                SketchPointRecordForm::Version11InlineTyped {
                    depth,
                    entity_genesis,
                    trailing_reference,
                    companion_prefix_present_zero: false,
                    persistent_id,
                    flags,
                    closure,
                },
                None,
            ))
        }
        (11, false) => {
            let padded_paired_reference = match payload.len().checked_sub(cursor) {
                Some(11) => Some(false),
                Some(15) if zeros_at::<4>(payload, cursor) => {
                    cursor += 4;
                    Some(true)
                }
                _ => None,
            };
            padded_paired_reference
                .zip(take_same_segment_sketch_reference(payload, &mut cursor))
                .map(|(padded_paired_reference, owner)| {
                    (
                        SketchPointRecordForm::Version11 {
                            depth,
                            entity_genesis,
                            padded_paired_reference,
                            companion_prefix_present_zero: false,
                            persistent_id,
                            flags,
                            closure,
                        },
                        Some(owner),
                    )
                })
        }
        _ => None,
    }) else {
        return Ok(None);
    };
    if cursor != payload.len() {
        return Ok(None);
    }
    Ok(Some(DecodedSketchPoint {
        owner_reference,
        coordinate_offset,
        record_form,
        paired_reference,
        coordinates: [u, v],
    }))
}

fn point_target_has_guid(
    ctx: &DecodeContext<'_>,
    frames_by_entity: &HashMap<u32, &super::meta::DesignPrimaryFrame<'_>>,
    target: Option<u32>,
    expected_guid: &str,
) -> Result<bool, CodecError> {
    let Some(target) = target else {
        return Ok(false);
    };
    match ctx.get_hash_map(frames_by_entity, &target, "find F3D sketch point target")? {
        Some(frame) => ctx.eq_ignore_ascii_case(
            frame.design_type.type_guid.as_str(),
            expected_guid,
            "match F3D sketch point target type",
        ),
        None => Ok(false),
    }
}

/// Decode a sketch point's inverse companion. `registered_type_guid` names the
/// type GUID its segment registers for a record index.
fn decode_sketch_point_companion<'t>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    point_record_index: u32,
    record_form: SketchPointRecordForm,
    registered_type_guid: impl Fn(u32) -> Result<Option<&'t str>, CodecError>,
) -> Result<Option<(SketchPointRecordForm, SketchPointCompanion)>, CodecError> {
    let reference_encoding = SketchPointCompanionReferenceEncoding::for_form(&record_form);
    let (prefix_present_zero, mut cursor) = if zeros_at::<10>(payload, 11) {
        (false, 21)
    } else if zeros_at::<9>(payload, 11) && bytes_at::<5>(payload, 20) == Some(&[1, 0, 0, 0, 0]) {
        (true, 25)
    } else {
        return Ok(None);
    };
    let Some(record_form) = record_form.with_companion_prefix_present_zero(prefix_present_zero)
    else {
        return Ok(None);
    };
    let Some(count) = View::u32_le_at(payload, cursor)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count <= MAX_RELATION_RUN)
    else {
        return Ok(None);
    };
    cursor += 4;

    let mut incident_curves = Vec::new();
    ctx.reserve_capacity(
        &mut incident_curves,
        count,
        "f3d sketch point incident curves",
    )?;
    for _ in ctx.admit_iter(&(0..count), "scan F3D sketch point incident curves")? {
        let Some((target, type_guid)) = take_local_sketch_reference(payload, &mut cursor) else {
            return Ok(None);
        };
        let Some(registered) = registered_type_guid(target)? else {
            return Ok(None);
        };
        let encoding_matches = match reference_encoding {
            SketchPointCompanionReferenceEncoding::SameSegment => type_guid.is_none(),
            SketchPointCompanionReferenceEncoding::InlineTyped => {
                inline_guid_is(ctx, type_guid, registered)?
            }
        };
        if !encoding_matches {
            return Ok(None);
        }
        ctx.push_vec(
            &mut incident_curves,
            target,
            "f3d sketch point incident curves",
        )?;
    }
    if payload.get(cursor) != Some(&0) {
        return Ok(None);
    }
    cursor += 1;
    let Some((inverse, inverse_type_guid)) = take_local_sketch_reference(payload, &mut cursor)
    else {
        return Ok(None);
    };
    let inverse_encoding_matches = match reference_encoding {
        SketchPointCompanionReferenceEncoding::SameSegment => inverse_type_guid.is_none(),
        SketchPointCompanionReferenceEncoding::InlineTyped => {
            inline_guid_is(ctx, inverse_type_guid, SKETCH_POINT_TYPE_GUID)?
        }
    };
    if inverse != point_record_index || !inverse_encoding_matches || cursor != payload.len() {
        return Ok(None);
    }
    Ok(Some((
        record_form,
        SketchPointCompanion { incident_curves },
    )))
}

const SKETCH_POINT_TYPE_GUID: &str = "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB";
pub(crate) const CURRENT_SKETCH_POINT_TYPE: (&str, u32, &str) =
    (SKETCH_POINT_TYPE_GUID, 11, "Geometry");
pub(crate) const SKETCH_POINT_COMPANION_TYPE: (&str, u32, &str) =
    ("362B7EC3-0F09-47C8-A3BE-DC066715CDAE", 0, "Geometry");
pub(crate) const SKETCH_CONTAINER_TYPE_GUID: &str = "44A64366-4BD3-4B24-881A-F94C206E8F2D";
pub(crate) const CURRENT_SKETCH_LINE_TYPE: (&str, u32, &str) =
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 2, "Geometry");
pub(crate) const CURRENT_SKETCH_CIRCULAR_TYPE: (&str, u32, &str) =
    ("F0130424-8B7E-4092-93C9-1CA807482534", 0, "Geometry");
pub(crate) const CURRENT_SKETCH_NURBS_TYPE: (&str, u32, &str) =
    ("D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B", 3, "MSketch");

const SKETCH_LINE_TYPES: [(&str, u32, &str); 6] = [
    CURRENT_SKETCH_LINE_TYPE,
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 1, "Geometry"),
    ("EA3B930A-3383-4AD3-BE25-4B2814EA3985", 0, "Geometry"),
    ("AE42BAB6-643F-4169-A33C-529C8E0A4D84", 0, "Geometry"),
    ("F279874A-17AB-43DA-BF8E-80259802D06E", 0, "Geometry"),
    ("58751243-FEA8-41E6-BBC9-37960EB8164B", 0, "Geometry"),
];
const SKETCH_CIRCULAR_TYPES: [(&str, u32, &str); 2] = [
    CURRENT_SKETCH_CIRCULAR_TYPE,
    ("FF23079A-D99C-47AB-940E-2F4E18F022AB", 2, "Geometry"),
];
const SKETCH_TEXT_FRAME_LINE_TYPE_GUID: &str = "16DEFC4D-1816-4FB0-8E39-9BDA23954248";

/// Geometry grammar selected by a sketch curve's stable type identity and
/// record version. Dynamic class tags are segment-local table ordinals and do
/// not identify a family without this resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SketchCurveClass {
    Line,
    Circular,
    Nurbs,
    TextFrameLine,
}

impl SketchCurveClass {
    fn of(
        ctx: &DecodeContext<'_>,
        type_guid: &str,
        version: u32,
        module: &str,
    ) -> Result<Option<Self>, CodecError> {
        let operation = "match F3D sketch curve type";
        let families = SKETCH_LINE_TYPES
            .iter()
            .map(|known| (*known, Self::Line))
            .chain(
                SKETCH_CIRCULAR_TYPES
                    .iter()
                    .map(|known| (*known, Self::Circular)),
            )
            .chain([
                (CURRENT_SKETCH_NURBS_TYPE, Self::Nurbs),
                (
                    (SKETCH_TEXT_FRAME_LINE_TYPE_GUID, 0, "MSketch"),
                    Self::TextFrameLine,
                ),
            ]);
        for ((known_guid, known_version, known_module), class) in families {
            if version == known_version
                && ctx.equal_bytes(module.as_bytes(), known_module.as_bytes(), operation)?
                && ctx.eq_ignore_ascii_case(type_guid, known_guid, operation)?
            {
                return Ok(Some(class));
            }
        }
        Ok(None)
    }
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
fn decode_sketch_curve_identities_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    let frames = design_primary_frames(ctx, bytes, meta)?;
    for frame in ctx.admit_iter(&frames, "scan F3D sketch curve frames")? {
        let payload = &bytes[frame.start..frame.end];
        let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
            decode_sketch_curve_identity(payload)
        else {
            continue;
        };
        let Some(primary_id) = std::num::NonZeroU64::new(primary_id) else {
            continue;
        };
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-curve entity ID exceeds u32".into()))?;
        let curve_class = SketchCurveClass::of(
            ctx,
            frame.design_type.type_guid.as_str(),
            frame.design_type.version,
            &frame.design_type.module,
        )?;
        let parsed_geometry = if let Some(curve_class) = curve_class {
            decode_sketch_curve_geometry(
                ctx,
                payload,
                geometry_shift,
                record_index,
                curve_class,
                frame.start,
            )?
        } else {
            None
        };
        let (geometry, geometry_offset) = parsed_geometry
            .map_or((None, geometry_shift + 133), |parsed| {
                (Some(parsed.geometry), parsed.geometry_offset)
            });

        ctx.reserve_vec(&mut out, 1, "f3d sketch curve output")?;
        out.push(SketchCurveIdentity {
            id: design_record_id_charged(
                ctx,
                stream,
                ":sketch-curve-identity#",
                u64_from_index(frame.start),
                "f3d sketch curve ID",
            )?,
            record_index,
            owner_reference: trailing_sketch_owner_reference(payload),
            class_tag: frame
                .class_tag
                .try_clone_for_decode(ctx, "copy F3D sketch curve class tag")?,
            byte_offset: u64_from_index(frame.start),
            geometry_offset: u32::try_from(geometry_offset).map_err(|_| {
                ctx.refuse_codec_limit(
                    "f3d sketch curve geometry offset",
                    u64::from(u32::MAX),
                    u64_from_index(geometry_offset),
                )
            })?,
            entity_genesis,
            primary_id,
            secondary_id,
            geometry,
        });
    }
    Ok(out)
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub(crate) fn decode_sketch_curve_identities(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_curve_identities_from_stream)
}

#[derive(Debug)]
struct ParsedSketchSurface {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    geometry: SketchSurfaceGeometry,
}

struct SketchSurfaceFrame {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    u_degree: u32,
    v_degree: u32,
    u_knots_at: usize,
    u_knot_count: usize,
    v_knots_at: usize,
    v_knot_count: usize,
    coordinate_count: usize,
    u_count: usize,
    v_count: usize,
}

fn parse_sketch_surface_frame(payload: &[u8]) -> Option<SketchSurfaceFrame> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(2)
        || View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 77) != Some(11)
        || payload.get(81..92) != Some(b"surface_tag")
        || View::u32_le_at(payload, 92) != Some(23)
        || payload.get(96..119) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69);
    let persistent_id = std::num::NonZeroU64::new(View::u64_le_at(payload, 119)?)?;
    let point_count = usize::try_from(View::u32_le_at(payload, 127)?).ok()?;
    if point_count == 0 || point_count > 100_000 {
        return None;
    }
    let coordinate_count = point_count.checked_mul(3)?;
    let coordinate_bytes = point_count.checked_mul(24)?;
    payload.get(131..131usize.checked_add(coordinate_bytes)?)?;
    let degrees_at = 131usize.checked_add(coordinate_bytes)?;
    let u_degree = View::u32_le_at(payload, degrees_at)?;
    let v_degree = View::u32_le_at(payload, degrees_at.checked_add(4)?)?;
    let u_knot_count =
        usize::try_from(View::u32_le_at(payload, degrees_at.checked_add(8)?)?).ok()?;
    let u_knots_at = degrees_at.checked_add(12)?;
    payload.get(u_knots_at..u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?)?;
    let v_count_at = u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?;
    let v_knot_count = usize::try_from(View::u32_le_at(payload, v_count_at)?).ok()?;
    let v_knots_at = v_count_at.checked_add(4)?;
    payload.get(v_knots_at..v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?)?;
    let grid_at = v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?;
    let u_count = usize::try_from(View::u32_le_at(payload, grid_at)?).ok()?;
    let v_count = usize::try_from(View::u32_le_at(payload, grid_at.checked_add(4)?)?).ok()?;
    // The grid count determines safe row framing. Geometry admission follows
    // after source coordinates are scaled to millimetres.
    if u_count.checked_mul(v_count) != Some(point_count) {
        return None;
    }
    Some(SketchSurfaceFrame {
        entity_genesis,
        persistent_id,
        u_degree,
        v_degree,
        u_knots_at,
        u_knot_count,
        v_knots_at,
        v_knot_count,
        coordinate_count,
        u_count,
        v_count,
    })
}

fn charged_sketch_scalar_values(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<f64>>, CodecError> {
    let Some(byte_len) = count.checked_mul(8) else {
        return Ok(None);
    };
    let Some(end) = offset.checked_add(byte_len) else {
        return Ok(None);
    };
    if payload.get(offset..end).is_none() {
        return Ok(None);
    }

    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, operation)?;
    for index in ctx.admit_iter(&(0..count), operation)? {
        let at = offset + index * 8;
        let value = View::f64_le_at(payload, at)
            .ok_or_else(|| CodecError::Malformed("F3D sketch scalar range changed".into()))?;
        values.push(value);
    }
    Ok(Some(values))
}

fn parse_sketch_surface(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<ParsedSketchSurface>, CodecError> {
    let Some(frame) = parse_sketch_surface_frame(payload) else {
        return Ok(None);
    };
    let Some(coordinates) = charged_sketch_scalar_values(
        ctx,
        payload,
        131,
        frame.coordinate_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let Some(u_knots) = charged_sketch_scalar_values(
        ctx,
        payload,
        frame.u_knots_at,
        frame.u_knot_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let Some(v_knots) = charged_sketch_scalar_values(
        ctx,
        payload,
        frame.v_knots_at,
        frame.v_knot_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let point_count = frame.coordinate_count / 3;

    let mut points = Vec::new();
    ctx.reserve_capacity(&mut points, point_count, "f3d sketch surface scaled points")?;
    let point_width = std::num::NonZeroUsize::new(3)
        .ok_or_else(|| CodecError::malformed("F3D coordinate width is zero"))?;
    for (ordinal, values) in ctx
        .admit_iter(&coordinates, "scan F3D sketch surface coordinates")?
        .chunks(point_width)
        .enumerate()
    {
        let Some(source) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
            return Ok(None);
        };
        let point = scaled_sketch_point(source).ok_or_else(|| {
            crate::design::text::malformed_design(ctx, format_args!(
                "F3D sketch surface at byte {record_at} control point {ordinal} overflows millimetres"
            ))
        })?;
        ctx.push_vec(&mut points, point, "f3d sketch surface scaled points")?;
    }

    let mut control_points = Vec::new();
    ctx.reserve_capacity(
        &mut control_points,
        frame.u_count,
        "f3d sketch surface rows",
    )?;
    for row in ctx
        .admit_iter(&points, "scan F3D sketch surface point rows")?
        .chunks(
            std::num::NonZeroUsize::new(frame.v_count)
                .ok_or_else(|| CodecError::malformed("F3D surface row width is zero"))?,
        )
    {
        let mut row_points = Vec::new();
        ctx.extend_from_slice(&mut row_points, row, "f3d sketch surface row points")?;
        ctx.push_vec(&mut control_points, row_points, "f3d sketch surface rows")?;
    }
    let geometry = match SketchSurfaceGeometry::from_checked_parts(
        RecordAdmission::Charged(ctx),
        frame.u_degree,
        frame.v_degree,
        u_knots,
        v_knots,
        control_points,
    ) {
        Ok(geometry) => geometry,
        Err(SketchGeometryError::Invalid(_)) => return Ok(None),
        Err(SketchGeometryError::Resource(error)) => return Err(error),
    };
    Ok(Some(ParsedSketchSurface {
        entity_genesis: frame.entity_genesis,
        persistent_id: frame.persistent_id,
        geometry,
    }))
}

/// Decode tensor-product surface entities owned by spatial Design sketches.
pub(crate) fn decode_sketch_surfaces(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchSurface>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for header in indexed_record_offsets(ctx, bytes)? {
            let record_at = header.offset;
            let payload = &bytes[record_at..];
            let Some(surface) = parse_sketch_surface(ctx, payload, record_at)? else {
                continue;
            };

            ctx.reserve_vec(&mut out, 1, "f3d sketch surface output")?;
            out.push(SketchSurface {
                id: design_record_id_charged(
                    ctx,
                    &entry.name,
                    ":sketch-surface#",
                    u64_from_index(record_at),
                    "f3d sketch surface ID",
                )?,
                record_index: header.record_index,
                owner_reference: None,
                class_tag: header.retain_class_tag(ctx, "copy F3D sketch surface class tag")?,
                byte_offset: u64_from_index(record_at),
                entity_genesis: surface.entity_genesis,
                persistent_id: surface.persistent_id,
                geometry: surface.geometry,
            });
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch 5",
    )?;
    Ok(out)
}

/// Stream scopes numbered in first-seen order, so the sketch-graph tables key
/// on fixed-width numbers instead of borrowing record identifiers.
struct ScopeNumbers<'ctx> {
    numbers: HashMap<String, u32>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl ScopeNumbers<'_> {
    /// The number of the stream scope of native record `id`.
    fn of(&mut self, ctx: &DecodeContext<'_>, id: &str) -> Result<Option<u32>, CodecError> {
        let Some(scope) = record_streams::record_stream(ctx, id)? else {
            return Ok(None);
        };
        if let Some(number) =
            ctx.get_hash_map(&self.numbers, scope, "find F3D sketch graph scope")?
        {
            return Ok(Some(*number));
        }
        let number = u32::try_from(self.numbers.len()).map_err(|_| {
            ctx.refuse_codec_limit(
                "f3d sketch graph scope",
                u64::from(u32::MAX),
                u64_from_index(self.numbers.len()),
            )
        })?;
        let numbers = &mut self.numbers;
        self.storage.with_storage(|| {
            let key = ctx.copy_retained_text(scope, "f3d sketch graph scope")?;
            ctx.insert_hash_map(numbers, key, number, "f3d sketch graph scope")
        })?;
        Ok(Some(number))
    }
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    ctx: &DecodeContext<'_>,
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    surfaces: &mut [SketchSurface],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    // Every table below is dropped before the bind returns.
    let mut storage = ctx.reserve_scoped(0, "f3d sketch graph tables")?;
    let mut scopes = ScopeNumbers {
        numbers: HashMap::new(),
        storage: ctx.reserve_scoped(0, "f3d sketch graph scope")?,
    };
    let mut sketch_owners = HashMap::new();
    for entity in ctx.admit_iter(entities, "scan F3D sketch entities")? {
        if !entity.in_sketch_module() {
            continue;
        }
        let Ok(suffix) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(scope) = scopes.of(ctx, &entity.id)? else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut sketch_owners,
                (scope, suffix),
                entity.entity_id.as_str(),
                "f3d sketch graph owner key",
            )
        })?;
    }

    // The scope number of each relation, in relation order.
    let mut relation_scopes = Vec::new();
    let relation_count = relations.len();
    for (relation, _) in relations
        .iter_mut()
        .zip(ctx.admit_iter(&(0..relation_count), "scan F3D sketch relation owners")?)
    {
        let Some(scope) = scopes.of(ctx, &relation.id)? else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion sketch relation {} has no Design stream identity",
                    relation.record_index
                ),
            ));
        };
        let Some(owner) = ctx.get_hash_map(
            &sketch_owners,
            &(scope, relation.owner_reference),
            "find F3D sketch relation owner",
        )?
        else {
            let stream = record_streams::record_stream(ctx, &relation.id)?.unwrap_or_default();
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion sketch relation {} in {stream} has no owning Design entity {}",
                    relation.record_index, relation.owner_reference,
                ),
            ));
        };
        let owner_text = ctx.copy_retained_text(owner, "f3d sketch relation owner text")?;
        relation.owner_entity_id = Some(
            cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                owner_text,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "Fusion sketch relation {} has an empty owner_entity_id",
                        relation.record_index,
                    ),
                )
            })?,
        );
        ctx.push_scoped_vec(
            &mut storage,
            &mut relation_scopes,
            scope,
            "f3d sketch graph scoped relations",
        )?;
    }

    // Typed records and the operand each resolves to, keyed by scope and
    // record index. The scope numbers of the typed records follow in input
    // order: points, curves, then surfaces.
    let mut operands = HashMap::new();
    let mut point_scopes = Vec::new();
    for point in ctx.admit_iter(&*points, "scan F3D sketch point operands")? {
        let scope = scopes.of(ctx, &point.id)?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut point_scopes,
            scope,
            "f3d sketch graph typed records",
        )?;
        if let Some(scope) = scope {
            let operand = SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut operands,
                    (scope, point.record_index),
                    operand,
                    "f3d sketch graph operand key",
                )
            })?;
        }
    }
    let mut curve_scopes = Vec::new();
    for curve in ctx.admit_iter(&*curves, "scan F3D sketch curves operands")? {
        let scope = scopes.of(ctx, &curve.id)?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut curve_scopes,
            scope,
            "f3d sketch graph typed records",
        )?;
        if let Some(scope) = scope {
            let operand = SketchRelationOperand::Curve {
                record_index: curve.record_index,
                primary_id: curve.primary_id.get(),
                secondary_id: curve.secondary_id,
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut operands,
                    (scope, curve.record_index),
                    operand,
                    "f3d sketch graph operand key",
                )
            })?;
        }
    }
    let mut surface_scopes = Vec::new();
    for surface in ctx.admit_iter(&*surfaces, "scan F3D sketch surfaces operands")? {
        let scope = scopes.of(ctx, &surface.id)?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut surface_scopes,
            scope,
            "f3d sketch graph typed records",
        )?;
        if let Some(scope) = scope {
            let operand = SketchRelationOperand::Surface {
                record_index: surface.record_index,
                persistent_id: surface.persistent_id.get(),
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut operands,
                    (scope, surface.record_index),
                    operand,
                    "f3d sketch graph operand key",
                )
            })?;
        }
    }

    // A direct backlink, a typed relation, and the sketch container's counted
    // member run are the three independent ownership joins. Payload-internal
    // references in an otherwise unowned Geometry record do not name a Sketch.
    // A direct backlink counts only when it names a sketch entity. The
    // relation and container joins apply after it and must agree.
    let mut owners = HashMap::new();
    let direct_owners = ctx
        .admit_iter(&*points, "scan F3D sketch direct point owners")?
        .zip(&point_scopes)
        .map(|(point, scope)| (*scope, point.record_index, point.owner_reference))
        .chain(
            ctx.admit_iter(&*curves, "scan F3D sketch curves direct owners")?
                .zip(&curve_scopes)
                .map(|(curve, scope)| (*scope, curve.record_index, curve.owner_reference)),
        )
        .chain(
            ctx.admit_iter(&*surfaces, "scan F3D sketch surfaces direct owners")?
                .zip(&surface_scopes)
                .map(|(surface, scope)| (*scope, surface.record_index, surface.owner_reference)),
        );
    for (scope, record_index, owner_reference) in direct_owners {
        let (Some(scope), Some(owner_reference)) = (scope, owner_reference) else {
            continue;
        };
        if ctx.contains_key_hash_map(
            &sketch_owners,
            &(scope, owner_reference),
            "find F3D sketch direct owner",
        )? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut owners,
                    (scope, record_index),
                    owner_reference,
                    "f3d sketch graph record owner",
                )
            })?;
        }
    }
    let mut join =
        |scope: u32, record_index: u32, owner: u32, id: &str| -> Result<(), CodecError> {
            if !ctx.contains_key_hash_map(
                &operands,
                &(scope, record_index),
                "find F3D sketch typed record",
            )? {
                return Ok(());
            }
            let previous = storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut owners,
                    (scope, record_index),
                    owner,
                    "f3d sketch graph record owner",
                )
            })?;
            if previous.is_some_and(|previous| previous != owner) {
                let stream = record_streams::record_stream(ctx, id)?.unwrap_or_default();
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                    "Fusion sketch record {record_index} in {stream} belongs to multiple sketches"
                ),
                ));
            }
            Ok(())
        };
    for (relation, &scope) in ctx
        .admit_iter(&*relations, "scan F3D scoped sketch relations")?
        .zip(&relation_scopes)
    {
        for member in ctx.admit_iter(
            &relation.members()[..],
            "scan F3D scoped sketch relation members",
        )? {
            join(
                scope,
                member.reference.record_index(),
                relation.owner_reference,
                &relation.id,
            )?;
        }
        for member in ctx.admit_iter(
            &relation.return_members()[..],
            "scan F3D scoped sketch relation return members",
        )? {
            join(
                scope,
                member.reference.record_index(),
                relation.owner_reference,
                &relation.id,
            )?;
        }
    }
    for entity in ctx.admit_iter(entities, "scan F3D sketch entities")? {
        let (Some(members), Ok(suffix)) = (
            entity.sketch_members(),
            u32::try_from(entity.entity_id.suffix()),
        ) else {
            continue;
        };
        let Some(scope) = scopes.of(ctx, &entity.id)? else {
            continue;
        };
        for &record_index in reference_runs::admit_reference_values(
            ctx,
            members,
            "scan F3D sketch entity member values",
        )? {
            join(scope, record_index, suffix, &entity.id)?;
        }
    }

    let owner_of = |scope: Option<u32>, record_index: u32| -> Result<Option<u32>, CodecError> {
        let Some(scope) = scope else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(
                &owners,
                &(scope, record_index),
                "find F3D sketch record owner",
            )?
            .copied())
    };
    for (point, scope) in points
        .iter_mut()
        .zip(ctx.admit_iter(&point_scopes, "assign F3D sketch point owners")?)
    {
        point.owner_reference = owner_of(*scope, point.record_index)?;
    }
    for (curve, scope) in curves
        .iter_mut()
        .zip(ctx.admit_iter(&curve_scopes, "assign F3D sketch curve owners")?)
    {
        curve.owner_reference = owner_of(*scope, curve.record_index)?;
    }
    for (surface, scope) in surfaces
        .iter_mut()
        .zip(ctx.admit_iter(&surface_scopes, "assign F3D sketch surface owners")?)
    {
        surface.owner_reference = owner_of(*scope, surface.record_index)?;
    }

    for (relation, &scope) in relations
        .iter_mut()
        .zip(ctx.admit_iter(&relation_scopes, "resolve F3D sketch relation operands")?)
    {
        // `resolve_members` visits the members, then the return members,
        // each in run order; resolve them in that order first.
        let mut resolved_storage = ctx.reserve_scoped(0, "f3d sketch relation operands")?;
        let mut resolved = Vec::new();
        let record_indices = ctx
            .admit_iter(&relation.members()[..], "scan F3D sketch relation members")?
            .map(|member| member.reference.record_index())
            .chain(
                ctx.admit_iter(
                    &relation.return_members()[..],
                    "scan F3D sketch relation return members",
                )?
                .map(|member| member.reference.record_index()),
            );
        for record_index in record_indices {
            let operand = ctx
                .get_hash_map(
                    &operands,
                    &(scope, record_index),
                    "find F3D sketch relation operand",
                )?
                .cloned()
                .unwrap_or(SketchRelationOperand::Record { record_index });
            ctx.push_scoped_vec(
                &mut resolved_storage,
                &mut resolved,
                operand,
                "f3d sketch relation operands",
            )?;
        }
        let mut resolved = resolved.into_iter();
        relation.resolve_members(ctx, |record_index| {
            resolved
                .next()
                .unwrap_or(SketchRelationOperand::Record { record_index })
        })?;
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69)?;
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(property_count)
        || View::u32_le_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || View::u32_le_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || View::u32_le_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        View::u64_le_at(payload, 70 + shift)?,
        View::u64_le_at(payload, 125 + shift)?,
    ))
}

struct DecodedSketchCurveGeometry {
    geometry: SketchCurveGeometry,
    geometry_offset: usize,
}

fn decode_sketch_curve_geometry(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
    record_at: usize,
) -> Result<Option<DecodedSketchCurveGeometry>, CodecError> {
    let Some(geometry_payload) = payload.get(geometry_shift..) else {
        return Ok(None);
    };
    let decoded = match class {
        SketchCurveClass::Line => {
            if let Some((geometry, _)) = decode_line_family(ctx, geometry_payload, record_at)? {
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                decode_line_family(ctx, referenced, record_at)?
                    .map(|(geometry, _)| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Circular => {
            if let Some(geometry) = decode_circular_arc(ctx, geometry_payload, record_at)? {
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                decode_circular_arc(ctx, referenced, record_at)?
                    .map(|geometry| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Nurbs => {
            let geometry = match decode_legacy_sketch_nurbs(ctx, geometry_payload, record_at)? {
                Some(geometry) => Some(geometry),
                None => decode_sketch_nurbs(ctx, geometry_payload, record_at)?,
            };
            geometry.map(|(geometry, _)| (geometry, 133))
        }
        SketchCurveClass::TextFrameLine => {
            decode_text_frame_line(ctx, payload, geometry_shift, record_index, record_at)?.and_then(
                |(geometry, end)| {
                    end.checked_sub(geometry_shift + 12 * 8)
                        .map(|offset| (geometry, offset))
                },
            )
        }
    };
    Ok(
        decoded.map(|(geometry, offset)| DecodedSketchCurveGeometry {
            geometry,
            geometry_offset: geometry_shift + offset,
        }),
    )
}

fn decode_circular_arc(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, 133 + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let (Some(center_cm), Some(normal), Some(reference_direction), Some(radius_cm)) = (
        FinitePoint3::new(Point3::new(values[0], values[1], values[2])),
        UnitVector3::new(Vector3::new(values[3], values[4], values[5])),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        PositiveLength::new(values[9]),
    ) else {
        return Ok(None);
    };
    let (Some(start_angle), Some(end_angle)) = (Angle::new(values[10]), Angle::new(values[11]))
    else {
        return Ok(None);
    };
    let center = scale_sketch_point(ctx, center_cm, record_at, "arc")?;
    let radius = PositiveLength::new(radius_cm.get() * 10.0);
    let Some(radius) = radius else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch arc at byte {record_at} overflows millimetres"),
        ));
    };
    Ok(SketchCurveGeometry::arc_from_parts(
        center,
        normal,
        reference_direction,
        radius,
        start_angle,
        end_angle,
    )
    .ok())
}

fn scale_sketch_point(
    ctx: &DecodeContext<'_>,
    point_centimetres: FinitePoint3,
    record_at: usize,
    kind: &str,
) -> Result<FinitePoint3, CodecError> {
    scaled_sketch_point(point_centimetres).ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch {kind} at byte {record_at} overflows millimetres"),
        )
    })
}

fn scaled_sketch_point(point_centimetres: FinitePoint3) -> Option<FinitePoint3> {
    let point = point_centimetres.get();
    FinitePoint3::new(Point3::new(point.x * 10.0, point.y * 10.0, point.z * 10.0))
}

fn referenced_analytic_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.get(133) != Some(&1) || !zeros_at::<6>(payload, 138) {
        return None;
    }
    payload.get(11..)
}

/// Decode a text-frame boundary line after its two point references and
/// inline analytic-curve record. The first point reference has a trailing
/// null-role byte in addition to its six-byte reference padding. The inline
/// record repeats the enclosing record index and carries eight zero bytes
/// before the line values.
fn decode_text_frame_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    let Some(values_at) = text_frame_line_values_at(payload, geometry_shift, record_index) else {
        return Ok(None);
    };
    let Some(end) = values_at.checked_add(12 * 8) else {
        return Ok(None);
    };
    Ok(decode_line_values(ctx, payload, values_at, record_at)?.map(|geometry| (geometry, end)))
}

/// The offset of a text-frame line's values: past the two padded point
/// references and the inline record header that repeats `record_index`.
fn text_frame_line_values_at(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
) -> Option<usize> {
    let mut cursor = geometry_shift.checked_add(133)?;
    let (_, end) = marked_u32(payload, cursor)?;
    if !zeros_at::<7>(payload, end) {
        return None;
    }
    cursor = end + 7;
    let (_, end) = marked_u32(payload, cursor)?;
    if !zeros_at::<6>(payload, end) {
        return None;
    }
    cursor = end + 6;
    let header = indexed_record_header_at(payload, cursor)?;
    if header.record_index != record_index || !zeros_at::<8>(payload, cursor + 11) {
        return None;
    }
    Some(cursor + 19)
}

/// Copy the three-digit NURBS subtype class tag at `at` into the output, when
/// the three bytes are digits.
fn nurbs_subtype_class_tag(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    at: usize,
    operation: &'static str,
) -> Result<Option<crate::records::references::DesignClassTag>, CodecError> {
    let Some(tag) = bytes_at::<3>(payload, at).filter(|tag| tag.iter().all(u8::is_ascii_digit))
    else {
        return Ok(None);
    };
    Ok(Some(crate::design::decode::text::retain_class_tag(
        ctx, tag, operation,
    )?))
}

/// A counted f64 lane: a u32 count, a u32 capacity, the u32 element width
/// eight, and `count` values.
struct ScalarLane {
    count: usize,
    capacity: usize,
    values_at: usize,
}

fn scalar_lane_at(payload: &[u8], at: usize) -> Option<ScalarLane> {
    let count = usize::try_from(View::u32_le_at(payload, at)?).ok()?;
    let capacity = usize::try_from(View::u32_le_at(payload, at.checked_add(4)?)?).ok()?;
    if View::u32_le_at(payload, at.checked_add(8)?)? != 8 {
        return None;
    }
    Some(ScalarLane {
        count,
        capacity,
        values_at: at.checked_add(12)?,
    })
}

/// The knot, weight and control-point lanes of a sketch NURBS whose knot
/// lane starts at `knots_at`, read through `accept`, which bounds each lane.
fn nurbs_lanes(
    payload: &[u8],
    knots_at: usize,
    accept: impl Fn(&ScalarLane) -> bool,
) -> Option<[ScalarLane; 3]> {
    let knots = scalar_lane_at(payload, knots_at).filter(&accept)?;
    let weights_at = knots.values_at.checked_add(knots.count.checked_mul(8)?)?;
    let weights = scalar_lane_at(payload, weights_at).filter(&accept)?;
    let points_at = weights
        .values_at
        .checked_add(weights.count.checked_mul(8)?)?;
    let points = scalar_lane_at(payload, points_at).filter(&accept)?;
    Some([knots, weights, points])
}

/// Read the three lanes of a sketch NURBS and admit its geometry. The knot
/// count equals the control-point count plus the degree plus one.
fn decode_nurbs_lanes(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
    degree: u32,
    fit_tolerance: f64,
    lanes: [ScalarLane; 3],
    operation: &'static str,
) -> Result<Option<(crate::records::sketch_geometry::SketchNurbsGeometry, usize)>, CodecError> {
    let [knots, weights, points] = lanes;
    let point_coordinates = points.count.checked_mul(3);
    let order = usize::try_from(degree)
        .ok()
        .and_then(|degree| points.count.checked_add(degree)?.checked_add(1));
    let (Some(point_coordinates), Some(order)) = (point_coordinates, order) else {
        return Ok(None);
    };
    if knots.count != order {
        return Ok(None);
    }
    let Some(knot_values) =
        charged_sketch_scalar_values(ctx, payload, knots.values_at, knots.count, operation)?
    else {
        return Ok(None);
    };
    let Some(weight_values) =
        charged_sketch_scalar_values(ctx, payload, weights.values_at, weights.count, operation)?
    else {
        return Ok(None);
    };
    let Some(coordinates) =
        charged_sketch_scalar_values(ctx, payload, points.values_at, point_coordinates, operation)?
    else {
        return Ok(None);
    };
    let Some(geometry) = admit_source_sketch_nurbs(
        ctx,
        degree,
        fit_tolerance,
        knot_values,
        weight_values,
        &coordinates,
        record_at,
    )?
    else {
        return Ok(None);
    };
    Ok(Some((geometry, points.values_at + points.count * 24)))
}

fn decode_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    let base = 133usize;
    let Some(carrier) = View::u64_le_at(payload, base) else {
        return Ok(None);
    };
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return Ok(None);
    }
    let (Some(degree), Some(fit_tolerance), Some(subtype_index)) = (
        View::u32_le_at(payload, base + 90),
        View::f64_le_at(payload, base + 94),
        View::u32_le_at(payload, base + 15),
    ) else {
        return Ok(None);
    };
    let Some(lanes) = nurbs_lanes(payload, base + 102, |lane| {
        lane.capacity == lane.count && lane.count <= 100_000
    }) else {
        return Ok(None);
    };
    let Some(subtype_class_tag) = nurbs_subtype_class_tag(
        ctx,
        payload,
        base + 12,
        "copy F3D sketch NURBS subtype class tag",
    )?
    else {
        return Ok(None);
    };
    let Some((geometry, end)) = decode_nurbs_lanes(
        ctx,
        payload,
        record_at,
        degree,
        fit_tolerance,
        lanes,
        "f3d sketch NURBS scalar values",
    )?
    else {
        return Ok(None);
    };
    Ok(Some((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            subtype_index,
            geometry,
        ),
        end,
    )))
}

fn decode_legacy_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    let base = 133usize;
    let Some(carrier) = View::u64_le_at(payload, base) else {
        return Ok(None);
    };
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3)
        || !zeros_at::<8>(payload, base + 19)
        || payload.get(base + 27) != Some(&1)
        || !zeros_at::<10>(payload, base + 32)
        || !zeros_at::<5>(payload, base + 50)
        || payload.get(base + 55) != Some(&1)
        || !zeros_at::<6>(payload, base + 60)
        || payload.get(base + 66) != Some(&1)
        || !zeros_at::<6>(payload, base + 71)
        || payload.get(base + 77) != Some(&1)
        || !zeros_at::<8>(payload, base + 80)
        || payload.get(base + 88).is_none_or(|value| *value > 1)
        || payload.get(base + 89).is_none_or(|value| *value > 1)
        || bytes_at::<8>(payload, base + 94)
            != Some(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e])
    {
        return Ok(None);
    }
    let (Some(degree), Some(fit_tolerance), Some(subtype_index)) = (
        View::u32_le_at(payload, base + 90).filter(|degree| *degree != 0),
        View::f64_le_at(payload, base + 42),
        View::u32_le_at(payload, base + 15),
    ) else {
        return Ok(None);
    };
    let Some(lanes) = nurbs_lanes(payload, base + 102, |lane| {
        lane.capacity >= lane.count && lane.capacity <= 100_000
    }) else {
        return Ok(None);
    };
    let Some(subtype_class_tag) = nurbs_subtype_class_tag(
        ctx,
        payload,
        base + 12,
        "copy F3D legacy sketch NURBS subtype class tag",
    )?
    else {
        return Ok(None);
    };
    let Some((geometry, end)) = decode_nurbs_lanes(
        ctx,
        payload,
        record_at,
        degree,
        fit_tolerance,
        lanes,
        "f3d legacy sketch NURBS scalar values",
    )?
    else {
        return Ok(None);
    };
    Ok(Some((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            subtype_index,
            geometry,
        ),
        end,
    )))
}

fn admit_source_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    degree: u32,
    fit_tolerance_cm: f64,
    knots: Vec<f64>,
    weights: Vec<f64>,
    coordinates: &[f64],
    record_at: usize,
) -> Result<Option<crate::records::sketch_geometry::SketchNurbsGeometry>, CodecError> {
    let Some(fit_tolerance_cm) = NonNegativeLength::new(fit_tolerance_cm) else {
        return Ok(None);
    };
    let fit_tolerance_mm = fit_tolerance_cm.get() * 10.0;
    let Some(fit_tolerance_mm) = NonNegativeLength::new(fit_tolerance_mm) else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!(
                "F3D sketch NURBS at byte {record_at} fit tolerance overflows millimetres"
            ),
        ));
    };
    let point_count = coordinates.len() / 3;

    let mut control_points = Vec::new();
    ctx.reserve_capacity(
        &mut control_points,
        point_count,
        "f3d sketch NURBS control points",
    )?;
    for point in ctx
        .admit_iter(coordinates, "scan F3D sketch NURBS coordinates")?
        .chunks(
            std::num::NonZeroUsize::new(3)
                .ok_or_else(|| CodecError::malformed("F3D coordinate width is zero"))?,
        )
        .filter(|point| point.len() == 3)
    {
        let Some(source) = FinitePoint3::new(Point3::new(point[0], point[1], point[2])) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut control_points,
            scale_sketch_point(ctx, source, record_at, "NURBS")?,
            "f3d sketch NURBS control points",
        )?;
    }
    let Ok(poles) = crate::records::sketch_geometry::SketchNurbsPoles::from_checked_points(
        control_points,
        weights,
    ) else {
        return Ok(None);
    };
    match crate::records::sketch_geometry::SketchNurbsGeometry::from_checked_parts(
        RecordAdmission::Charged(ctx),
        degree,
        fit_tolerance_mm,
        knots,
        poles,
    ) {
        Ok(geometry) => Ok(Some(geometry)),
        Err(SketchGeometryError::Invalid(_)) => Ok(None),
        Err(SketchGeometryError::Resource(error)) => Err(error),
    }
}

fn decode_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    decode_line_values(ctx, payload, 133, record_at)
}

fn decode_compact_planar_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let values_at = 133;
    let mut values = [0.0; 9];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    if values[2] != 0.0 || values[5] != 0.0 || values[8] != 0.0 {
        return Ok(None);
    }
    let Some((_, reference_end)) = marked_u32(payload, values_at + 9 * 8) else {
        return Ok(None);
    };
    if !zeros_at::<6>(payload, reference_end) {
        return Ok(None);
    }
    decode_line_components(ctx, &values, Vector3::new(0.0, 0.0, 1.0), record_at)
}

fn decode_line_family(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    if let Some(geometry) = decode_line(ctx, payload, record_at)? {
        return Ok(Some((geometry, 12)));
    }
    Ok(decode_compact_planar_line(ctx, payload, record_at)?.map(|geometry| (geometry, 9)))
}

fn decode_line_values(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    values_at: usize,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let stored_normal = Vector3::new(values[9], values[10], values[11]);
    decode_line_components(ctx, &values, stored_normal, record_at)
}

fn decode_line_components(
    ctx: &DecodeContext<'_>,
    values: &[f64],
    stored_normal: Vector3,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let (Some(_source_direction), Some(_stored_direction), Some(stored_normal)) = (
        UnitVector3::normalized(displacement),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        UnitVector3::new(stored_normal),
    ) else {
        return Ok(None);
    };
    // Start plus displacement carries the bounded line and is corroborated by
    // the persistent endpoint records. Imported sketches can retain a stale
    // auxiliary unit direction, so derive the neutral tangent from the
    // admitted endpoints just as the normal is orthogonalized below.
    let Some(start_cm) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
        return Ok(None);
    };
    let start = scale_sketch_point(ctx, start_cm, record_at, "line")?;
    let end = FinitePoint3::new(start.get().translated(displacement, 10.0)).ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch line at byte {record_at} overflows millimetres"),
        )
    })?;
    let start_raw = start.get();
    let end_raw = end.get();
    let displacement_mm = FiniteVector3::new(Vector3::new(
        end_raw.x - start_raw.x,
        end_raw.y - start_raw.y,
        end_raw.z - start_raw.z,
    ))
    .ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch line at byte {record_at} has an overflowing displacement"),
        )
    })?;
    let Some(direction) = UnitVector3::normalized(displacement_mm.get()) else {
        return Ok(None);
    };
    // The stored line normal is an auxiliary orientation vector. Imported
    // legacy sketches can retain a small component along the line direction;
    // remove that component so the typed carrier maintains its orthonormal
    // invariant without changing the line's endpoints or orientation side.
    let dot = direction.as_raw().dot(*stored_normal.as_raw());
    let projected_normal = *stored_normal.as_raw() - direction.as_raw().scale(dot);
    let projected_length = projected_normal.norm();
    let normal = if projected_length.is_finite()
        && projected_length > EPS_SKETCH_DECODE_LINE_COMPONENTS_E12
    {
        let Some(normal) = UnitVector3::normalized(projected_normal) else {
            return Ok(None);
        };
        normal
    } else {
        // Spatial line carriers can store a unit auxiliary vector parallel to
        // the line. The neutral spatial-line geometry has no plane normal;
        // retain the bounded carrier and choose a stable perpendicular basis
        // vector so the native geometry record still satisfies its typed
        // invariant.
        let basis = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .min_by(|left, right| {
            direction
                .as_raw()
                .dot(*left)
                .abs()
                .total_cmp(&direction.as_raw().dot(*right).abs())
        });
        let Some(basis) = basis else {
            return Ok(None);
        };
        let Some(normal) = UnitVector3::normalized(direction.as_raw().cross(basis)) else {
            return Ok(None);
        };
        normal
    };
    Ok(SketchCurveGeometry::line_from_parts(start, end, direction, normal).ok())
}

struct ParsedSketchRelationMember {
    reference: crate::records::identity::Located<u32, usize>,
    relation_ordinal: u32,
}

struct ParsedSketchRelation {
    members: Vec<ParsedSketchRelationMember>,
    auxiliary_references: Vec<crate::records::identity::Located<u32, usize>>,
    owner_reference: u32,
    owner_reference_offset: usize,
    state: u64,
    state_offset: usize,
    entity_genesis: Option<u64>,
    class_members: RelationClassMembers,
    return_members: Vec<crate::records::identity::Located<u32, usize>>,
    parsed_end: usize,
}

/// Type GUID of the shared sketch-relation class, which adds no member of its
/// own between the property block and `ParentNode`.
const RELATION_TYPE_GUID: &str = "60403D47-0C49-49B0-BDE8-1679608164A2";
/// Type GUID of the offset class, whose relations carry mask `0x2000000000`.
const OFFSET_RELATION_TYPE_GUID: &str = "D3BD153B-EB8A-405E-9D29-69EE0C3D227C";
/// Type GUID of the spline-group class, whose relations carry mask `0x80000000`.
const SPLINE_RELATION_TYPE_GUID: &str = "73762C3B-82DC-4632-93B0-B8FE1CC5282F";
/// Type GUID of the tangency class, whose relations carry mask `0x100`.
const TANGENT_RELATION_TYPE_GUID: &str = "24DB790E-3DCD-4336-AFA3-6F119EF2239B";
/// Type GUID of the circular-pattern class, mask `0x10000000`.
const CIRCULAR_PATTERN_RELATION_TYPE_GUID: &str = "8269E861-0BB7-47E0-9911-5AE3EC475058";
/// Type GUID of the rectangular-pattern class, mask `0x20000000`.
const RECTANGULAR_PATTERN_RELATION_TYPE_GUID: &str = "40800FB9-C2BE-494E-A047-7D76E82B9F6C";
/// Type GUID of the text-frame class, mask `0x10000000000`.
const TEXT_FRAME_RELATION_TYPE_GUID: &str = "8B369926-123F-4F9D-878E-6D4C076128D3";
/// Type GUID of the text-path class, mask `0x20000000000`.
const TEXT_PATH_RELATION_TYPE_GUID: &str = "9D30FCDC-EA07-4141-93E2-918B1A59E962";

/// A sketch-relation record class, named by the type GUID its segment's type
/// table carries for the record's entity. The class fixes the members written
/// between the property block and the base class's `ParentNode`. A class tag
/// cannot name it: a tag is `256` plus an index into the segment's own type
/// table, so one tag names different relation classes in different segments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SketchRelationClass {
    /// A class whose most-derived level adds no member: the shared relation
    /// class, the offset class, and the spline-group class.
    Plain,
    /// Three `u8` flags, each `0` or `1`.
    Tangent,
    /// The angle-parameter reference, the count-parameter reference, the
    /// evaluated f64 total angle in radians, the evaluated u32 instance count,
    /// the pattern tables, and one `u8`.
    CircularPattern,
    /// Three `u8` flags, a u32-counted run of references, the pattern tables,
    /// and two direction clauses.
    RectangularPattern,
    /// Two references.
    TextFrame,
    /// The text-entity reference, a u32 character count, and that many glyph
    /// blocks. `leading_flag` is the `u8` the class writes before the
    /// reference, which it does from class version 1.
    TextPath {
        /// Whether the class version writes the leading `u8`.
        leading_flag: bool,
    },
}

impl SketchRelationClass {
    /// The relation class `type_guid` names at class version `version`, or
    /// `None` where the GUID is not a sketch-relation class.
    fn of(
        ctx: &DecodeContext<'_>,
        type_guid: &str,
        version: u32,
    ) -> Result<Option<Self>, CodecError> {
        let classes = [
            (RELATION_TYPE_GUID, Self::Plain),
            (OFFSET_RELATION_TYPE_GUID, Self::Plain),
            (SPLINE_RELATION_TYPE_GUID, Self::Plain),
            (TANGENT_RELATION_TYPE_GUID, Self::Tangent),
            (CIRCULAR_PATTERN_RELATION_TYPE_GUID, Self::CircularPattern),
            (
                RECTANGULAR_PATTERN_RELATION_TYPE_GUID,
                Self::RectangularPattern,
            ),
            (TEXT_FRAME_RELATION_TYPE_GUID, Self::TextFrame),
            (
                TEXT_PATH_RELATION_TYPE_GUID,
                Self::TextPath {
                    leading_flag: version >= 1,
                },
            ),
        ];
        for (known, class) in classes {
            if ctx.eq_ignore_ascii_case(type_guid, known, "match F3D sketch relation class")? {
                return Ok(Some(class));
            }
        }
        Ok(None)
    }
}

/// Largest plausible counted run inside a sketch relation; a larger count is a
/// misparse rather than a record that owns that many members.
const MAX_RELATION_RUN: usize = 4096;

/// Serialized width selected by a sketch relation's leading-block member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SketchRelationMaskWidth {
    U32,
    U64,
}

impl SketchRelationMaskWidth {
    fn from_leading_block(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::U32),
            1 => Some(Self::U64),
            _ => None,
        }
    }

    fn has_paired_member_run(self) -> bool {
        self == Self::U64
    }
}

/// Read the relation's mask width from its leading-block presence member.
pub(crate) fn relation_mask_width(record: &[u8]) -> Option<SketchRelationMaskWidth> {
    let (_, start) = lp_ascii_filtered_view(record, 15, 0..=256, u8::is_ascii_graphic)?;
    SketchRelationMaskWidth::from_leading_block(*record.get(start)?)
}

/// Take one reference member at `cursor`, returning its 32-bit target and the
/// byte offset of the target within the reference. Relation members address
/// records in the relation's own segment, so a reference whose target does not
/// fit a `u32` is a misparse.
fn take_relation_reference(
    payload: &[u8],
    cursor: &mut usize,
) -> Option<crate::records::identity::Located<u32, usize>> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    Some(crate::records::identity::Located {
        value: u32::try_from(reference.target()?).ok()?,
        offset: at + 1,
    })
}

#[derive(Clone, Copy)]
enum AuxiliaryRelationReference {
    Absent,
    Present(crate::records::identity::Located<u32, usize>),
}

/// Take one reference member that the class may leave absent, recording it in
/// the auxiliary run when it is present. An absent reference is one zero byte
/// and names nothing, so it contributes no entry.
fn take_auxiliary_relation_reference(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Result<Option<AuxiliaryRelationReference>, CodecError> {
    let at = *cursor;
    let Some(reference) = take_reference(payload, cursor) else {
        return Ok(None);
    };
    let Some(target) = reference.target() else {
        return Ok(Some(AuxiliaryRelationReference::Absent));
    };
    let Some(value) = u32::try_from(target).ok() else {
        return Ok(None);
    };
    let located = crate::records::identity::Located {
        value,
        offset: at + 1,
    };

    ctx.reserve_vec(
        auxiliary_references,
        1,
        "f3d sketch auxiliary relation references",
    )?;
    auxiliary_references.push(located);
    Ok(Some(AuxiliaryRelationReference::Present(located)))
}

/// Skip the two tables both pattern classes write after their own leading
/// members: a u32-counted map whose entry is a u64 key, a u32 count, and that
/// many u64 values; then a u32-counted run of u32.
fn skip_pattern_tables(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
) -> Result<Option<()>, CodecError> {
    let counted = |at: usize| {
        View::u32_le_at(payload, at)
            .and_then(|count| usize::try_from(count).ok())
            .filter(|count| *count <= MAX_RELATION_RUN)
    };
    let Some(entries) = counted(*cursor) else {
        return Ok(None);
    };
    *cursor += 4;
    for _ in ctx.admit_iter(&(0..entries), "skip F3D sketch pattern table")? {
        let Some(values) = counted(*cursor + 8) else {
            return Ok(None);
        };
        *cursor += 12 + values * 8;
    }
    let Some(ordinals) = counted(*cursor) else {
        return Ok(None);
    };
    *cursor += 4 + ordinals * 4;
    Ok((*cursor <= payload.len()).then_some(()))
}

/// What a sketch-relation subclass leaves behind after its own members.
enum RelationClassMembers {
    Plain,
    Tangent,
    CircularPattern,
    Rectangular {
        reference_count: u32,
        clauses: Option<[crate::records::identity::Located<u32, usize>; 4]>,
    },
    TextFrame,
    TextPath {
        glyph_transforms: Vec<SketchGlyphTransform>,
    },
}

/// Consume the members `class` writes between the property block and the base
/// class's `ParentNode`, advancing `cursor` past them and recording every
/// present reference among them in the auxiliary run
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn parse_relation_class_members(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
    class: SketchRelationClass,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Result<Option<RelationClassMembers>, CodecError> {
    let mut take = |cursor: &mut usize| {
        take_auxiliary_relation_reference(ctx, payload, cursor, auxiliary_references)
    };
    Ok(Some(match class {
        SketchRelationClass::Plain => RelationClassMembers::Plain,
        SketchRelationClass::Tangent => {
            // A flag outside `{0, 1}` is a misparse, not a third state.
            if !bytes_at::<3>(payload, *cursor)
                .is_some_and(|flags| flags.iter().all(|flag| *flag <= 1))
            {
                return Ok(None);
            }
            *cursor += 3;
            RelationClassMembers::Tangent
        }
        SketchRelationClass::TextFrame => {
            if take(cursor)?.is_none() || take(cursor)?.is_none() {
                return Ok(None);
            }
            RelationClassMembers::TextFrame
        }
        SketchRelationClass::CircularPattern => {
            if take(cursor)?.is_none() || take(cursor)?.is_none() {
                return Ok(None);
            }
            // The evaluated total angle and the evaluated instance count.
            *cursor += 12;
            if skip_pattern_tables(ctx, payload, cursor)?.is_none()
                || payload.get(*cursor) != Some(&0)
            {
                return Ok(None);
            }
            *cursor += 1;
            RelationClassMembers::CircularPattern
        }
        SketchRelationClass::RectangularPattern => {
            use AuxiliaryRelationReference::Present;

            // The three flags are not checked the way the tangency class's are:
            // the counted runs and the unit directions that follow them already
            // reject a misframed record, and the flags do not.
            *cursor += 3;
            let Some(reference_count) = View::u32_le_at(payload, *cursor) else {
                return Ok(None);
            };
            let Some(references) = usize::try_from(reference_count)
                .ok()
                .filter(|references| *references <= MAX_RELATION_RUN)
            else {
                return Ok(None);
            };
            *cursor += 4;
            for _ in ctx.admit_iter(&(0..references), "scan F3D sketch pattern references")? {
                if take(cursor)?.is_none() {
                    return Ok(None);
                }
            }
            if skip_pattern_tables(ctx, payload, cursor)?.is_none() {
                return Ok(None);
            }
            let mut clauses = [AuxiliaryRelationReference::Absent; 4];
            for pair in clauses.chunks_exact_mut(2) {
                *cursor += 4;
                let Some(count) = take(cursor)? else {
                    return Ok(None);
                };
                *cursor += 32;
                let Some(distance) = take(cursor)? else {
                    return Ok(None);
                };
                pair[0] = count;
                pair[1] = distance;
            }
            let clauses = match clauses {
                [Present(a), Present(b), Present(c), Present(d)] => Some([a, b, c, d]),
                _ => None,
            };
            RelationClassMembers::Rectangular {
                reference_count,
                clauses,
            }
        }

        SketchRelationClass::TextPath { leading_flag } => {
            if leading_flag {
                if payload.get(*cursor) != Some(&1) {
                    return Ok(None);
                }
                *cursor += 1;
            }
            let Some((text_reference, transforms, end)) =
                parse_text_glyph_run(ctx, payload, *cursor)?
            else {
                return Ok(None);
            };
            ctx.push_vec(
                auxiliary_references,
                crate::records::identity::Located {
                    value: text_reference,
                    offset: *cursor + 1,
                },
                "f3d sketch auxiliary relation references",
            )?;
            *cursor = end;
            RelationClassMembers::TextPath {
                glyph_transforms: transforms,
            }
        }
    }))
}

/// Parse one sketch-relation record body whose class is known
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
///
/// The payload is `u8 1`, a u32 count and that many `(reference, u32 relation
/// ordinal)` pairs, the property-block presence byte and its block, the
/// class-defined members, the `ParentNode` reference naming the owning sketch,
/// a u64 constraint mask, a u32 count and that many bare references, and one
/// zero byte. At relation base-class version 0 the leading byte is zero, the
/// pair list is absent, and the mask is a u32. Both reference runs hold the
/// same members; only the second is in semantic order.
fn parse_classed_sketch_relation(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class: SketchRelationClass,
) -> Result<Option<ParsedSketchRelation>, CodecError> {
    // The record header is the LP-ASCII class tag, the u64 entity id, and the
    // LP-ASCII record name; the member payload follows it.
    let Some((_, start)) = lp_ascii_filtered_view(payload, 15, 0..=256, u8::is_ascii_graphic)
    else {
        return Ok(None);
    };
    let Some(mask_width) = payload
        .get(start)
        .and_then(|value| SketchRelationMaskWidth::from_leading_block(*value))
    else {
        return Ok(None);
    };
    let mut cursor = start + 1;
    let mut members = Vec::new();
    if mask_width.has_paired_member_run() {
        let Some(member_count) = relation_run_count(payload, cursor) else {
            return Ok(None);
        };
        cursor += 4;
        ctx.reserve_capacity(
            &mut members,
            member_count,
            "f3d sketch relation paired members",
        )?;
        for _ in ctx.admit_iter(
            &(0..member_count),
            "scan F3D sketch relation paired members",
        )? {
            let Some(reference) = take_relation_reference(payload, &mut cursor) else {
                return Ok(None);
            };
            let Some(relation_ordinal) = View::u32_le_at(payload, cursor) else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut members,
                ParsedSketchRelationMember {
                    reference,
                    relation_ordinal,
                },
                "f3d sketch relation paired members",
            )?;
            cursor += 4;
        }
    }
    // The base class level opens with its property-block presence byte. The
    // block is `u32 count` and that many `(key, type name, value)` triples;
    // `EntityGenesis` is one such key.
    let Some(properties) = read_property_block(ctx, payload, &mut cursor)? else {
        return Ok(None);
    };
    let entity_genesis = properties.find(ctx, "EntityGenesis")?;
    let mut auxiliary_references = Vec::new();
    let Some(class_members) =
        parse_relation_class_members(ctx, payload, &mut cursor, class, &mut auxiliary_references)?
    else {
        return Ok(None);
    };
    let Some(owner) = take_relation_reference(payload, &mut cursor) else {
        return Ok(None);
    };
    let state_offset = cursor;
    // The constraint mask follows `ParentNode` directly. It is a u64 in the
    // paired-run form and a u32 at relation base-class version 0.
    let state = match mask_width {
        SketchRelationMaskWidth::U64 => {
            View::u64_le_at(payload, state_offset).map(|state| (state, state_offset + 8))
        }
        SketchRelationMaskWidth::U32 => {
            View::u32_le_at(payload, state_offset).map(|state| (u64::from(state), state_offset + 4))
        }
    };
    let Some((state, mut cursor)) = state else {
        return Ok(None);
    };
    let Some(return_count) = relation_run_count(payload, cursor) else {
        return Ok(None);
    };
    cursor += 4;

    let mut return_members = Vec::new();
    ctx.reserve_capacity(
        &mut return_members,
        return_count,
        "f3d sketch relation return members",
    )?;
    for _ in ctx.admit_iter(
        &(0..return_count),
        "scan F3D sketch relation return members",
    )? {
        let Some(reference) = take_relation_reference(payload, &mut cursor) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut return_members,
            reference,
            "f3d sketch relation return members",
        )?;
    }
    if payload.get(cursor) != Some(&0) {
        return Ok(None);
    }
    Ok(Some(ParsedSketchRelation {
        members,
        auxiliary_references,
        owner_reference: owner.value,
        owner_reference_offset: owner.offset,
        state,
        state_offset,
        entity_genesis,
        class_members,
        return_members,
        parsed_end: cursor + 1,
    }))
}

/// The u32 count of a counted relation run at `at`, when it is plausible.
fn relation_run_count(payload: &[u8], at: usize) -> Option<usize> {
    usize::try_from(View::u32_le_at(payload, at)?)
        .ok()
        .filter(|count| *count <= MAX_RELATION_RUN)
}

/// Parse a text-path glyph run at `at`: the marked text-entity reference,
/// six zero bytes, a u32 character count, and that many blocks of `u32 16`
/// followed by sixteen finite f64 values forming a row-major 4×4 character
/// placement transform. Returns the text reference, the transforms in
/// character order, and the offset directly after the last block.
type TextGlyphRun = (u32, Vec<SketchGlyphTransform>, usize);

fn parse_text_glyph_run(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    at: usize,
) -> Result<Option<TextGlyphRun>, CodecError> {
    let Some((text_reference, end)) = marked_u32(payload, at) else {
        return Ok(None);
    };
    let Some(count) = View::u32_le_at(payload, end + 6)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| zeros_at::<6>(payload, end) && (1..=4096).contains(count))
    else {
        return Ok(None);
    };
    let mut view = View::over_retained(payload);
    if view.seek(end + 10).is_none() {
        return Ok(None);
    }

    let mut transforms = Vec::new();
    ctx.reserve_capacity(&mut transforms, count, "f3d sketch text glyph transforms")?;
    for _ in ctx.admit_iter(&(0..count), "scan F3D sketch text glyph transforms")? {
        if view.u32_le() != Some(16) {
            return Ok(None);
        }
        let mut transform = [[0.0; 4]; 4];
        for row in &mut transform {
            for cell in row {
                let Some(value) = view.f64_le() else {
                    return Ok(None);
                };
                *cell = value;
            }
        }
        let Ok(transform) = SketchGlyphTransform::try_from(transform) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut transforms,
            transform,
            "f3d sketch text glyph transforms",
        )?;
    }
    Ok(Some((text_reference, transforms, view.position())))
}

/// Validated indexed-record identity and byte offset.
#[derive(Clone, Copy)]
pub(in crate::design::decode) struct IndexedRecordHeader<'bytes> {
    pub(super) offset: usize,
    pub(super) record_index: u32,
    /// Three ASCII digits borrowed from the header.
    pub(super) class_tag: &'bytes [u8; 3],
    /// Numeric value of the three-digit class tag.
    pub(super) class_code: u32,
}

impl IndexedRecordHeader<'_> {
    /// Copy the class tag into retained output storage.
    pub(super) fn retain_class_tag(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<crate::records::references::DesignClassTag, CodecError> {
        crate::design::decode::text::retain_class_tag(ctx, self.class_tag, operation)
    }
}

/// The indexed-record header at `at`: marker `3`, a three-digit class tag and
/// a record index. The test reads eleven bytes and allocates nothing.
pub(super) fn indexed_record_header_at(bytes: &[u8], at: usize) -> Option<IndexedRecordHeader<'_>> {
    if View::u32_le_at(bytes, at) != Some(3) {
        return None;
    }
    let class_tag = bytes_at::<3>(bytes, at.checked_add(4)?)?;
    if !class_tag.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let class_code = class_tag
        .iter()
        .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'));
    let record_index = View::u32_le_at(bytes, at.checked_add(7)?)?;
    Some(IndexedRecordHeader {
        offset: at,
        record_index,
        class_tag,
        class_code,
    })
}

/// The first indexed-record header at or after `position` that `accept`
/// selects. Each byte the search visits is admitted once before its marker
/// test, so a forward scan that resumes after each result pays for the bytes
/// it reads and no more.
pub(super) fn next_indexed_record_header<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    position: usize,
    mut accept: impl FnMut(&IndexedRecordHeader<'bytes>) -> bool,
) -> Result<Option<IndexedRecordHeader<'bytes>>, CodecError> {
    let mut cursor = position;
    loop {
        let Some(tail) = bytes.get(cursor..) else {
            return Ok(None);
        };
        let Some(relative) = ctx.position_by(
            tail,
            |byte| Ok(*byte == 3),
            "find F3D indexed record header",
        )?
        else {
            return Ok(None);
        };
        // `relative` indexes `tail`, so both sums stay within `bytes.len()`.
        let at = cursor + relative;
        if let Some(header) = indexed_record_header_at(bytes, at) {
            if accept(&header) {
                return Ok(Some(header));
            }
        }
        cursor = at + 1;
    }
}

pub(in crate::design) fn next_indexed_record_offset(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<usize>, CodecError> {
    Ok(next_indexed_record_header(ctx, bytes, position, |_| true)?.map(|header| header.offset))
}

/// Every indexed record whose class tag is three characters, in byte order.
///
/// A class tag is `256` plus an index into the segment's own type table, so a
/// tag reaches four characters only in a segment registering more than 744
/// types. No segment registers that many.
pub(super) fn indexed_record_offsets<'input>(
    ctx: &DecodeContext<'_>,
    bytes: &'input [u8],
) -> Result<impl Iterator<Item = IndexedRecordHeader<'input>> + 'input, CodecError> {
    Ok(ctx
        .admit_iter(bytes, "scan F3D indexed record headers")?
        .enumerate()
        .filter_map(move |(at, _)| indexed_record_header_at(bytes, at)))
}

pub(super) fn next_indexed_record_offset_with_index(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
    record_index: u32,
) -> Result<Option<usize>, CodecError> {
    Ok(next_indexed_record_header(ctx, bytes, position, |header| {
        header.record_index == record_index
    })?
    .map(|header| header.offset))
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1))
        .then_some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

struct SketchReferenceList {
    record_reference: crate::records::identity::Located<Option<u32>>,
    references: Vec<crate::records::identity::Located<u32>>,
    end: usize,
}

fn decode_reference_list(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<SketchReferenceList>, CodecError> {
    // The eight-byte base-record slot is either a u32 record reference with a
    // zero high half or the all-ones sentinel marking a sketch with no base
    // record; the list grammar is identical in both forms.
    let record_reference = match (
        View::u32_le_at(bytes, position),
        View::u32_le_at(bytes, position + 4),
    ) {
        (Some(u32::MAX), Some(u32::MAX)) => None,
        (Some(reference), Some(0)) => Some(reference),
        _ => return Ok(None),
    };
    let Some(declared_count) = View::u32_le_at(bytes, position + 9)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|_| bytes.get(position + 8) == Some(&1))
    else {
        return Ok(None);
    };
    let run_at = position + 13;
    let Some(entries) = declared_count
        .checked_mul(11)
        .and_then(|length| run_at.checked_add(length))
        .and_then(|run_end| bytes.get(run_at..run_end))
    else {
        return Ok(None);
    };
    let end = run_at + entries.len();
    // The run holds exactly the declared count: one more marked reference
    // after it means the count is wrong.
    if bytes_at::<11>(bytes, end).is_some_and(|entry| entry[0] == 1 && zeros_at::<6>(entry, 5)) {
        return Ok(None);
    }
    let Some(references) = collect_member_run(
        ctx,
        entries.as_chunks::<11>().0,
        run_at,
        None,
        "f3d sketch header references",
    )?
    else {
        return Ok(None);
    };
    Ok(Some(SketchReferenceList {
        record_reference: crate::records::identity::Located {
            value: record_reference,
            offset: u64_from_index(position),
        },
        references,
        end,
    }))
}

fn decode_sketch_streams<T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    decode: impl Fn(
        &DecodeContext<'_>,
        &[u8],
        &crate::metastream::MetaStream,
        &str,
    ) -> Result<Vec<T>, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D sketch design entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(ctx, scan, &entry.name)? else {
            continue;
        };
        ctx.append_vec(
            &mut out,
            &mut { decode(ctx, bytes, &meta, &entry.name)? },
            "f3d sketch stream output",
        )?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
