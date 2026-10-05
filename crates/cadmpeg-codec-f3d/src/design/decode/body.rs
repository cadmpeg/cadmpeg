// SPDX-License-Identifier: Apache-2.0
//! Parse body members, bounds, bindings, and visibility.

use crate::bytes::{lp_utf16_bounded_scoped, take_reference};
use crate::container::ContainerScan;
use crate::design::body::{
    BODY_MAP_CARRIER_BASE_TYPE_GUID, BODY_MAP_CARRIER_TYPE_GUID, BODY_MAP_CARRIER_TYPE_VERSION,
    BODY_MAP_ZERO_PREFIX_LENGTHS, SNAPSHOT_BODY_LIST_TYPE_GUID,
    SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID, SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS,
};
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::presentation::BrowserNodeRecord;
use crate::design::decode::record_streams::{record_stream, StreamOffsets};
use crate::design::decode::reference_runs::{admit_reference_values, reference_position};
use crate::design::decode::sketch::{
    native_scope_scoped, next_indexed_record_header, next_indexed_record_offset,
};
use crate::design::decode::text::{design_record_id_charged, rsplit_once_ascii};
use crate::design::presentation::{BODY_PRESENTATION_TYPE_GUID, BREP_CONTAINER_TYPE_GUID};
use crate::design::RECIPES;
use crate::layout::indexed_design_record_header;
use crate::metastream::{MetaStream, PrimaryRecordFrame};
use crate::records::{
    bodies::{DesignBodyBinding, DesignBodyBounds, DesignBodyMember},
    entity_header::{DesignEntityHeader, SegmentTypeData, DESIGN_MODULE_BODY},
    recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
};
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{
    index_from_u32, u64_from_index, DecodeContext, ScopedReservation, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{HashMap, HashSet};

/// The doubled `BodiesRoot` marker that opens the body-member list: two
/// counted ASCII names around a zero `u16`.
const BODY_MEMBER_MARKER: &[u8] = b"\x0a\x00\x00\x00BodiesRoot\x00\x00\x0a\x00\x00\x00BodiesRoot";
/// One body member: the marker byte `1`, the `u64` entity suffix and the
/// `u16` flags.
const BODY_MEMBER_LEN: usize = 11;
/// The largest member count a body-member list declares.
const MAX_BODY_MEMBERS: usize = 100_000;

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub(crate) fn decode_body_members(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D body-member streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(marker_at) =
            ctx.find_bytes(bytes, BODY_MEMBER_MARKER, "find F3D body-member marker")?
        else {
            continue;
        };
        let count_at = marker_at + BODY_MEMBER_MARKER.len();
        let Some(count) = View::u32_le_at(bytes, count_at).map(index_from_u32) else {
            continue;
        };
        if count > MAX_BODY_MEMBERS {
            continue;
        }
        let members_at = count_at + 4;
        let Some(members_end) = members_at.checked_add(count * BODY_MEMBER_LEN) else {
            continue;
        };
        let Some(member_bytes) = bytes.get(members_at..members_end) else {
            continue;
        };
        if bytes.get(members_end) != Some(&0) {
            continue;
        }
        let (members, _) = member_bytes.as_chunks::<BODY_MEMBER_LEN>();
        if !ctx.all_by(
            members,
            |member| Ok(member[0] == 1),
            "check F3D body-member markers",
        )? {
            continue;
        }
        ctx.reserve_vec(&mut out, members.len(), "f3d body members")?;
        for (ordinal, member) in ctx
            .admit_iter(members, "decode F3D body members")?
            .enumerate()
        {
            let byte_offset = u64_from_index(members_at + ordinal * BODY_MEMBER_LEN);
            let [_, fields @ ..] = member;
            let mut fields = View::over_retained(fields);
            let entity_suffix = fields.req_u64_le()?;
            let flags = fields.req_u16_le()?;
            let id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-body-member#",
                byte_offset,
                "f3d body member identifier",
            )?;
            out.push(
                DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberWire {
                    id,
                    byte_offset,
                    entity_suffix,
                    flags,
                })
                .map_err(CodecError::Malformed)?,
            );
        }
    }
    Ok(out)
}

/// One cached bounds frame: the marker byte `1` and six `f64` values, the
/// maximum corner before the minimum corner.
const BOUNDS_FRAME_LEN: usize = 49;

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
///
/// A body's records lie between its entity header and the next entity header
/// of the same stream. Each of the three records holds the same bounds frame
/// exactly once, and the first record holds exactly one frame that repeats.
pub(crate) fn decode_body_bounds(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    let mut stream_offsets = None;
    for entity in ctx.admit_iter(entities, "scan F3D body-bound entities")? {
        // A comparison with the module literal reads at most its bytes.
        if entity.module() != Some(DESIGN_MODULE_BODY) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &entity.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(start) = usize::try_from(entity.byte_offset) else {
            continue;
        };
        let (offsets, _) = match &mut stream_offsets {
            Some(index) => index,
            slot @ None => slot.insert(entity_offsets_by_stream(ctx, entities)?),
        };
        let end = next_entity_offset(ctx, offsets, stream, entity.byte_offset)?
            .and_then(|offset| usize::try_from(offset).ok())
            .unwrap_or(bytes.len());
        let Ok(record_index) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(record_indices) = record_index
            .checked_add(1)
            .zip(record_index.checked_add(2))
            .zip(record_index.checked_add(3))
            .map(|((first, second), third)| [first, second, third])
        else {
            continue;
        };
        let Some([first, second, third]) =
            body_bound_record_offsets(ctx, bytes, start, end, record_index)?
        else {
            continue;
        };
        if !(first < second && second < third) {
            continue;
        }
        let Some(search_at) = third.checked_add(indexed_design_record_header::LEN) else {
            continue;
        };
        // A header that opens at `end` still ends inside this window.
        let header_window = bytes
            .get(..end.saturating_add(indexed_design_record_header::LEN))
            .unwrap_or(bytes);
        let third_end = next_indexed_record_offset(ctx, header_window, search_at)?
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let Some((values, value_offsets)) = unique_body_bound_frame(
            ctx,
            bytes,
            [(first, second), (second, third), (third, third_end)],
        )?
        else {
            continue;
        };
        let corner = |start: usize| {
            FinitePoint3::new(Point3::new(
                values[start].get() * 10.0,
                values[start + 1].get() * 10.0,
                values[start + 2].get() * 10.0,
            ))
            .ok_or_else(|| {
                CodecError::Malformed("scene bounds maximum and minimum must be finite".into())
            })
        };
        let id = design_record_id_charged(
            ctx,
            &entry.name,
            ":design-body-bounds#",
            entity.byte_offset,
            "f3d body record identifier",
        )?;
        let record = DesignBodyBounds::from_parts(crate::records::bodies::DesignBodyBoundsWire {
            id,
            entity_suffix: entity.entity_id.suffix(),
            entity_byte_offset: entity.byte_offset,
            record_indices,
            record_byte_offsets: [
                u64_from_index(first),
                u64_from_index(second),
                u64_from_index(third),
            ],
            value_byte_offsets: value_offsets.map(u64_from_index),
            body_binding_ids: Vec::new(),
            maximum: corner(0)?,
            minimum: corner(3)?,
        })
        .map_err(CodecError::Malformed)?;
        ctx.push_vec(&mut out, record, "f3d body bounds")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design body 1",
    )?;
    Ok(out)
}

/// Every entity header offset with its stream scope, ordered by scope and
/// then by offset.
fn entity_offsets_by_stream<'entities, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    entities: &'entities [DesignEntityHeader],
) -> Result<StreamOffsets<'entities, 'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D entity offsets by stream")?;
    let mut offsets = Vec::new();
    for entity in ctx.admit_iter(entities, "index F3D entity offsets by stream")? {
        let Some(stream) = record_stream(ctx, &entity.id)? else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut offsets,
            (stream, entity.byte_offset),
            "index F3D entity offsets by stream",
        )?;
    }
    ctx.sort_unstable_by(
        &mut offsets,
        |entry| entry,
        Ord::cmp,
        "sort F3D entity offsets by stream",
    )?;
    Ok((offsets, storage))
}

/// The first entity header offset in `stream` after `offset`.
fn next_entity_offset(
    ctx: &DecodeContext<'_>,
    offsets: &[(&str, u64)],
    stream: &str,
    offset: u64,
) -> Result<Option<u64>, CodecError> {
    let key = (stream, offset);
    let index = ctx.partition_point(
        offsets,
        |entry| {
            Ok(ctx
                .compare(entry, &key, "find F3D next entity offset")?
                .is_le())
        },
        "find F3D next entity offset",
    )?;
    let Some((next_stream, next_offset)) = offsets.get(index) else {
        return Ok(None);
    };
    Ok(ctx
        .equal_bytes(
            next_stream.as_bytes(),
            stream.as_bytes(),
            "find F3D next entity offset",
        )?
        .then_some(*next_offset))
}

/// The header offsets of records `record_index + 1` through `record_index + 3`
/// when each has exactly one indexed header in `bytes[start..end]`. One forward
/// search visits the range once and stops at the first repeated index.
fn body_bound_record_offsets(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    record_index: u32,
) -> Result<Option<[usize; 3]>, CodecError> {
    let window = bytes.get(..end).unwrap_or(bytes);
    let mut offsets = [None; 3];
    let mut position = start;
    while let Some(header) = next_indexed_record_header(ctx, window, position, |header| {
        header
            .record_index
            .checked_sub(record_index)
            .is_some_and(|delta| (1..=3).contains(&delta))
    })? {
        let Some(slot) = header
            .record_index
            .checked_sub(record_index)
            .and_then(|delta| usize::try_from(delta).ok())
            .and_then(|delta| delta.checked_sub(1))
            .and_then(|ordinal| offsets.get_mut(ordinal))
        else {
            return Ok(None);
        };
        if slot.replace(header.offset).is_some() {
            return Ok(None);
        }
        position = header.offset + 1;
    }
    let [Some(first), Some(second), Some(third)] = offsets else {
        return Ok(None);
    };
    Ok(Some([first, second, third]))
}

/// The values of a repeated bounds frame and the offset of its first value in
/// each of the three records.
type BoundFrame = ([FiniteReal; 6], [usize; 3]);

/// The one bounds frame of the first record that repeats exactly once in each
/// later record: its values and the offset of its first value in each record.
/// A second repeated frame in the first record makes the cache ambiguous.
fn unique_body_bound_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: [(usize, usize); 3],
) -> Result<Option<BoundFrame>, CodecError> {
    let [(first_start, first_end), second, third] = records;
    let Some(last) = first_end.checked_sub(BOUNDS_FRAME_LEN - 1) else {
        return Ok(None);
    };
    let Some((at, (values, value_offsets))) =
        find_repeated_body_bound_frame(ctx, bytes, first_start, last, [second, third])?
    else {
        return Ok(None);
    };
    if find_repeated_body_bound_frame(ctx, bytes, at + 1, last, [second, third])?.is_some() {
        return Ok(None);
    }
    Ok(Some((values, value_offsets)))
}

/// The first frame opening in `bytes[from..to]` whose bytes repeat exactly
/// once in each of the `later` records. The search admits each opening it
/// visits and stops at the match.
fn find_repeated_body_bound_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    from: usize,
    to: usize,
    later: [(usize, usize); 2],
) -> Result<Option<(usize, BoundFrame)>, CodecError> {
    let Some(openings) = bytes.get(from..to) else {
        return Ok(None);
    };
    // `find_map` visits the openings in order, so `next` is the visited offset.
    let mut next = from;
    ctx.find_map(
        openings,
        |marker| {
            let at = next;
            next += 1;
            if *marker != 1 {
                return Ok(None);
            }
            let Some(frame) = bytes_at::<BOUNDS_FRAME_LEN>(bytes, at) else {
                return Ok(None);
            };
            let Some(values) = body_bound_values(frame) else {
                return Ok(None);
            };
            let [second, third] = later;
            let Some(second_at) = only_frame_in(ctx, bytes, frame, second)? else {
                return Ok(None);
            };
            let Some(third_at) = only_frame_in(ctx, bytes, frame, third)? else {
                return Ok(None);
            };
            Ok(Some((at, (values, [at + 1, second_at + 1, third_at + 1]))))
        },
        "find F3D body-bound frame",
    )
}

/// The offset of the only copy of `frame` inside `bytes[start..end]`.
/// Overlapping copies count, so the second search starts one byte later.
fn only_frame_in(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: &[u8; BOUNDS_FRAME_LEN],
    (start, end): (usize, usize),
) -> Result<Option<usize>, CodecError> {
    let Some(first) =
        ctx.find_bytes_in(bytes, frame, start, end, "find F3D repeated body bounds")?
    else {
        return Ok(None);
    };
    if ctx
        .find_bytes_in(
            bytes,
            frame,
            first + 1,
            end,
            "find F3D repeated body bounds",
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(first))
}

/// The six finite values of a bounds frame whose maximum corner is at least
/// its minimum corner on every axis and greater on one.
fn body_bound_values(frame: &[u8; BOUNDS_FRAME_LEN]) -> Option<[FiniteReal; 6]> {
    let [marker, fields @ ..] = frame;
    if *marker != 1 {
        return None;
    }
    let value = |index: usize| FiniteReal::new(View::f64_le_at(fields, index * 8)?);
    let values = [
        value(0)?,
        value(1)?,
        value(2)?,
        value(3)?,
        value(4)?,
        value(5)?,
    ];
    ((0..3).all(|axis| values[axis] >= values[axis + 3])
        && (0..3).any(|axis| values[axis] > values[axis + 3]))
    .then_some(values)
}

pub(super) fn decode_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    out: &mut Vec<ConstructionRecipe>,
) -> Result<(), CodecError> {
    for &(name, kind) in RECIPES {
        // Recipe indexes count per kind and Design ID. Each family name has
        // its own kind, so the counters of one family are the counters of its
        // kind. A Design ID is at most eight bytes.
        let mut counter_storage = ctx.reserve_scoped(0, "f3d construction recipe counters")?;
        let mut counters = HashMap::<Option<&str>, u32>::new();
        // No recipe family name overlaps itself, so the non-overlapping
        // matches are every match.
        for offset in ctx.find_bytes_iter(bytes, name, "find F3D construction recipe")? {
            if kind == ConstructionRecipeKind::Face
                && offset
                    .checked_sub(8)
                    .and_then(|at| bytes_at::<8>(bytes, at))
                    == Some(b"bounded_")
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| View::u32_le_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let parsed_design_id = recipe_design_id(bytes, offset, name);
            let design = parsed_design_id
                .map(|(value, design_id_at)| -> Result<_, CodecError> {
                    let selector = design_id_at
                        .checked_add(value.len())
                        .and_then(|selector_at| {
                            Some(ConstructionRecipeSelector {
                                value: View::u32_le_at(bytes, selector_at)?,
                                byte_offset: u64::try_from(selector_at).ok()?,
                            })
                        });
                    Ok(crate::records::recipes::ConstructionRecipeDesign {
                        id: crate::records::identity::RecordedValue {
                            value: ctx
                                .copy_retained_text(value, "f3d construction recipe design ID")?,
                            offset: u64_from_index(design_id_at),
                        },
                        selector,
                    })
                })
                .transpose()?;
            let key = parsed_design_id.map(|(value, _)| value);
            let recipe_index = match ctx.get_mut_hash_map(
                &mut counters,
                &key,
                "f3d construction recipe counters",
            )? {
                Some(counter) => {
                    let recipe_index = *counter;
                    *counter += 1;
                    recipe_index
                }
                None => {
                    counter_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut counters,
                            key,
                            1,
                            "f3d construction recipe counters",
                        )
                    })?;
                    0
                }
            };
            // The record index word precedes the family marker by sixteen
            // bytes. A marker within the first sixteen bytes of the stream has
            // no such word, so the stream states no record index for it; the
            // word itself always lies inside `bytes` when the marker does.
            let record_index = offset.checked_sub(16).and_then(|at| {
                Some(crate::records::identity::RecordedValue {
                    value: View::i32_le_at(bytes, at)?,
                    offset: u64_from_index(at),
                })
            });
            let recipe = ConstructionRecipe {
                id: design_record_id_charged(
                    ctx,
                    stream,
                    ":construction-recipe#",
                    u64_from_index(offset),
                    "f3d body record identifier",
                )?,
                byte_offset: u64_from_index(offset),
                kind,
                design,
                recipe_index,
                record_index,
            };
            ctx.push_vec(out, recipe, "f3d construction recipes")?;
        }
    }
    ctx.stable_sort_by_key(
        &mut out[..],
        |recipe| recipe.record_index.map(|index| index.value),
        Ord::cmp,
        "sort f3d design body 2",
    )?;
    Ok(())
}

/// The Design ID that names a construction recipe whose family marker opens
/// at `offset`, and the ID's byte offset. The ID is a counted ASCII field of
/// one to eight characters ending twenty bytes before the marker, three ASCII
/// digits at that place, or a counted field after the marker. Every test reads
/// a fixed number of bytes.
fn recipe_design_id<'bytes>(
    bytes: &'bytes [u8],
    offset: usize,
    name: &[u8],
) -> Option<(&'bytes str, usize)> {
    let id_end = offset.checked_sub(20)?;
    for length in 1..=8usize {
        let Some(length_at) = id_end.checked_sub(4 + length) else {
            continue;
        };
        if let Some((id, value_offset)) = ascii_id_at(bytes, length_at) {
            if value_offset.checked_add(id.len()) == Some(id_end) {
                return Some((id, value_offset));
            }
        }
    }
    if let Some(digits_at) = offset.checked_sub(23) {
        let digits = bytes_at::<3>(bytes, digits_at)?;
        if digits.iter().all(u8::is_ascii_digit) {
            return Some((std::str::from_utf8(digits).ok()?, digits_at));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8)
}

/// A counted ASCII alphanumeric field of one to eight bytes at
/// `length_offset`, and the offset of its first byte.
fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, length_offset)?).ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value_at = length_offset.checked_add(4)?;
    let value = bytes.get(value_at..value_at.checked_add(length)?)?;
    if !value.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some((std::str::from_utf8(value).ok()?, value_at))
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
#[derive(Clone, Copy)]
pub(crate) struct BodyBinding {
    /// The referenced ASM body key.
    pub(crate) asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub(crate) asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub(crate) entity_suffix: u64,
}

impl BodyBinding {
    /// Byte offset of `entity_suffix`, which follows `asm_key` in the pair.
    pub(crate) fn entity_suffix_offset(&self) -> usize {
        self.asm_key_offset + 8
    }
}

/// One body-map pair: the `u64` ASM body key and the `u64` entity suffix.
const BODY_MAP_PAIR_LEN: usize = 16;

/// One exactly framed Design body-map record.
///
/// The record owns the blob name and its location, and its ordered `bindings`
/// are the map's pairs: a binding's ordinal is its index and the pair count is
/// `bindings.len()`. The record is decode scratch: its storage is scoped and
/// released with it.
struct BodyMapRecord<'ctx> {
    blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    blob_name_offset: usize,
    bindings: Vec<BodyBinding>,
    _storage: ScopedReservation<'ctx>,
}

/// Body-map records of one Design stream and the scoped storage of the list.
type BodyMapRecords<'ctx> = (Vec<BodyMapRecord<'ctx>>, ScopedReservation<'ctx>);

/// Decode the pairs of a body-map record and attach them to its blob name.
fn body_map_record<'ctx>(
    ctx: &DecodeContext<'_>,
    pairs_start: usize,
    pairs: &[[u8; BODY_MAP_PAIR_LEN]],
    (name_at, blob_name, mut storage): (usize, String, ScopedReservation<'ctx>),
    operation: &'static str,
) -> Result<BodyMapRecord<'ctx>, CodecError> {
    let mut bindings = Vec::new();
    ctx.reserve_scoped_vec(&mut storage, &mut bindings, pairs.len(), operation)?;
    for (ordinal, pair) in ctx
        .admit_iter(pairs, "decode F3D body-map pairs")?
        .enumerate()
    {
        let mut fields = View::over_retained(pair);
        bindings.push(BodyBinding {
            asm_key: fields.req_u64_le()?,
            asm_key_offset: pairs_start + ordinal * BODY_MAP_PAIR_LEN,
            entity_suffix: fields.req_u64_le()?,
        });
    }
    Ok(BodyMapRecord {
        blob_name,
        blob_name_offset: name_at + 4,
        bindings,
        _storage: storage,
    })
}

/// The primary record frames of one Design stream and the frame ordinal of
/// each entity. The primary index names each entity at most once.
struct PrimaryFrames<'ctx> {
    frames: Vec<PrimaryRecordFrame>,
    by_entity: HashMap<u64, usize>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> PrimaryFrames<'ctx> {
    fn index(
        ctx: &'ctx DecodeContext<'_>,
        meta: &MetaStream,
        bulk_len: usize,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "f3d body-map primary index")?;
        let frames = storage
            .with_storage(|| crate::metastream::primary_record_frames(ctx, meta, bulk_len))?;
        let mut by_entity = HashMap::new();
        for (ordinal, frame) in ctx
            .admit_iter(&frames, "index F3D body-map primary frames")?
            .enumerate()
        {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut by_entity,
                    frame.entity_id,
                    ordinal,
                    "f3d body-map primary index",
                )
            })?;
        }
        Ok(Self {
            frames,
            by_entity,
            _storage: storage,
        })
    }

    fn frame(&self, entity: u64) -> Option<PrimaryRecordFrame> {
        self.by_entity
            .get(&entity)
            .and_then(|ordinal| self.frames.get(*ordinal))
            .copied()
    }
}

/// Whether a type registered under `type_guid` lists `entity`. The search
/// admits each type and each listed entity it visits and stops at the match;
/// a comparison with the GUID literal reads at most its bytes.
fn entity_has_type(
    ctx: &DecodeContext<'_>,
    meta: &MetaStream,
    entity: u64,
    type_guid: &'static str,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &meta.types,
        |design_type| {
            if !design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(type_guid)
            {
                return Ok(false);
            }
            Ok(reference_position(
                ctx,
                &design_type.entities,
                |registered| Ok(*registered == entity),
                "find F3D registered Design entity",
            )?
            .is_some())
        },
        "scan F3D Design type registrations",
    )
}

/// Every entity registered under `type_guid`, sorted, in scoped storage.
fn registered_entities<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    meta: &MetaStream,
    type_guid: &'static str,
) -> Result<(Vec<u64>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d registered Design entities")?;
    let mut entities = Vec::new();
    for design_type in ctx.admit_iter(&meta.types, "scan F3D Design type registrations")? {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        for entity in admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D registered Design entities",
        )? {
            ctx.push_scoped_vec(
                &mut storage,
                &mut entities,
                *entity,
                "f3d registered Design entities",
            )?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut entities,
        |entity| *entity,
        Ord::cmp,
        "sort F3D registered Design entities",
    )?;
    Ok((entities, storage))
}

/// Trailing zeros a local reference form carries after its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferencePadding {
    /// The doubled-marker form, `1 1` and the target, carries none.
    None,
    /// The ordinary reference form ends with two zeros.
    TwoZeros,
}

#[derive(Clone, Copy, Debug)]
struct LocalReferenceCandidate<'bytes> {
    target: u64,
    end: usize,
    inline_type_guid: Option<&'bytes str>,
    padding: ReferencePadding,
}

/// The local reference readings at `at`: the ordinary reference form and the
/// doubled-marker form, each optionally followed by one extra zero.
fn local_reference_candidates(
    bytes: &[u8],
    at: usize,
    allow_extra_zero: bool,
) -> [Option<LocalReferenceCandidate<'_>>; 4] {
    let mut candidates = [None; 4];
    let mut end = at;
    if let Some((target, inline_type_guid)) =
        take_reference(bytes, &mut end).and_then(crate::bytes::Reference::into_local)
    {
        let candidate = LocalReferenceCandidate {
            target,
            end,
            inline_type_guid,
            padding: ReferencePadding::TwoZeros,
        };
        candidates[0] = Some(candidate);
        if allow_extra_zero && bytes.get(end) == Some(&0) {
            candidates[1] = Some(LocalReferenceCandidate {
                end: end + 1,
                ..candidate
            });
        }
    }
    if bytes_at::<2>(bytes, at) == Some(&[1, 1]) {
        if let Some(target) = View::u64_le_at(bytes, at + 2) {
            let candidate = LocalReferenceCandidate {
                target,
                end: at + 10,
                inline_type_guid: None,
                padding: ReferencePadding::None,
            };
            candidates[2] = Some(candidate);
            if allow_extra_zero && bytes.get(candidate.end) == Some(&0) {
                candidates[3] = Some(LocalReferenceCandidate {
                    end: candidate.end + 1,
                    ..candidate
                });
            }
        }
    }
    candidates
}

/// Whether a local reference names an entity of the expected type, and its
/// inline type GUID, when present, is that type.
fn reference_has_type(
    ctx: &DecodeContext<'_>,
    meta: &MetaStream,
    reference: &LocalReferenceCandidate<'_>,
    expected_type_guid: &'static str,
) -> Result<bool, CodecError> {
    if reference
        .inline_type_guid
        .is_some_and(|guid| !guid.eq_ignore_ascii_case(expected_type_guid))
    {
        return Ok(false);
    }
    entity_has_type(ctx, meta, reference.target, expected_type_guid)
}

/// The three ASCII digits of the class tag the type at `type_ordinal`
/// assigns: 256 plus the ordinal, when that has three digits.
fn carrier_class_tag(type_ordinal: usize) -> Option<[u8; 3]> {
    let tag = type_ordinal.checked_add(256).filter(|tag| *tag <= 999)?;
    let digit = |value: usize| u8::try_from(value % 10).ok().map(|digit| b'0' + digit);
    Some([digit(tag / 100)?, digit(tag / 10)?, digit(tag)?])
}

/// Whether a body-map carrier type registers in the Body module over the
/// body-map base type. Each comparison with a literal reads at most its bytes.
fn carrier_registration_matches(design_type: &SegmentTypeData) -> bool {
    design_type.module == DESIGN_MODULE_BODY
        && design_type.base_type_guid.value().is_some_and(|base| {
            base.as_str()
                .eq_ignore_ascii_case(BODY_MAP_CARRIER_BASE_TYPE_GUID)
        })
}

/// Parse every exactly framed sibling body-map record that binds an `.smb`
/// snapshot. The carrier uses a bare entity header in every serializer band.
fn snapshot_body_map_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    primary: &PrimaryFrames<'_>,
) -> Result<BodyMapRecords<'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d snapshot body-map records")?;
    let mut out = Vec::new();
    let mut body_entities = None;
    for (type_ordinal, design_type) in ctx
        .admit_iter(&meta.types, "scan F3D snapshot body-map types")?
        .enumerate()
    {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if !SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS.contains(&design_type.version) {
            return Err(CodecError::NotImplemented(ctx.format_retained(
                format_args!(
                    "unsupported F3D Design snapshot body-map carrier version {}",
                    design_type.version
                ),
                "f3d Design unsupported diagnostic",
            )?));
        }
        if !carrier_registration_matches(design_type) {
            return Err(CodecError::malformed(
                "F3D Design snapshot body-map carrier has incompatible registration metadata",
            ));
        }
        let class_tag = carrier_class_tag(type_ordinal).ok_or_else(|| {
            CodecError::malformed("F3D Design snapshot body-map class tag is not three digits")
        })?;
        for entity in admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D snapshot body-map entities",
        )? {
            let entity = *entity;
            let Some(frame) = primary.frame(entity) else {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design snapshot body-map entity {entity} has no primary record"
                    ),
                ));
            };
            if View::u32_le_at(bytes, frame.start) != Some(3)
                || bytes_at::<3>(bytes, frame.start + 4) != Some(&class_tag)
                || View::u64_le_at(bytes, frame.start + 7) != Some(entity)
                || !zeros_at::<6>(bytes, frame.start + 15)
            {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design snapshot body-map entity {entity} has an invalid entity header"
                    ),
                ));
            }
            let (body_entities, _) = match &mut body_entities {
                Some(entities) => entities,
                slot @ None => {
                    slot.insert(registered_entities(ctx, meta, BODY_PRESENTATION_TYPE_GUID)?)
                }
            };
            if let Some(record) =
                parse_snapshot_body_map_frame(ctx, bytes, meta, frame, entity, body_entities)?
            {
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut out,
                    record,
                    "f3d snapshot body-map records",
                )?;
            }
        }
    }
    Ok((out, storage))
}

/// The snapshot body map in one carrier frame: the companion body-list
/// reference, a reserved zero run, the counted pairs whose body entities are
/// body presentations, the typed BREP container reference, a reserved zero
/// run, and the blob name, which ends the frame.
fn parse_snapshot_body_map_frame<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    frame: PrimaryRecordFrame,
    entity: u64,
    body_entities: &[u64],
) -> Result<Option<BodyMapRecord<'ctx>>, CodecError> {
    let Some(companion_entity) = entity.checked_add(1) else {
        return Ok(None);
    };
    let Some(companion_at) = frame.start.checked_add(21) else {
        return Ok(None);
    };
    for companion in local_reference_candidates(bytes, companion_at, true)
        .into_iter()
        .flatten()
    {
        // A two-byte reserved field follows; the ordinary form supplies it.
        let count_at = match companion.padding {
            ReferencePadding::None => {
                zeros_at::<2>(bytes, companion.end).then_some(companion.end + 2)
            }
            ReferencePadding::TwoZeros => Some(companion.end),
        };
        let Some(count_at) = count_at else {
            continue;
        };
        if companion.target != companion_entity
            || !reference_has_type(ctx, meta, &companion, SNAPSHOT_BODY_LIST_TYPE_GUID)?
        {
            continue;
        }
        let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
            continue;
        };
        let count = usize::try_from(pair_count)
            .map_err(|_| CodecError::malformed("F3D snapshot body-map count exceeds usize"))?;
        let pairs_start = count_at + 4;
        let Some(pairs_end) = count
            .checked_mul(BODY_MAP_PAIR_LEN)
            .and_then(|span| pairs_start.checked_add(span))
        else {
            continue;
        };
        let Some(pair_bytes) = bytes.get(pairs_start..pairs_end) else {
            continue;
        };
        let (pairs, _) = pair_bytes.as_chunks::<BODY_MAP_PAIR_LEN>();
        if !ctx.all_by(
            pairs,
            |pair| {
                let Some(body) = View::u64_le_at(pair, 8) else {
                    return Ok(false);
                };
                Ok(ctx
                    .binary_search(
                        body_entities,
                        &body,
                        "check F3D snapshot body-map pair type",
                    )?
                    .is_ok())
            },
            "check F3D snapshot body-map pair types",
        )? {
            continue;
        }
        for container in local_reference_candidates(bytes, pairs_end, false)
            .into_iter()
            .flatten()
        {
            // A three-byte reserved field follows; the ordinary form supplies
            // two of its zeros.
            let name_at = match container.padding {
                ReferencePadding::None => {
                    zeros_at::<3>(bytes, container.end).then_some(container.end + 3)
                }
                ReferencePadding::TwoZeros => {
                    zeros_at::<1>(bytes, container.end).then_some(container.end + 1)
                }
            };
            let Some(name_at) = name_at else {
                continue;
            };
            if !reference_has_type(ctx, meta, &container, BREP_CONTAINER_TYPE_GUID)? {
                continue;
            }
            let Some(name) = blob_name_at(ctx, bytes, name_at, frame.end)? else {
                continue;
            };
            let accepted = if name.1.is_empty() {
                pair_count == 0
            } else {
                brep_blob_kind(ctx, &name.1)? == Some(BrepBlobKind::Snapshot)
            };
            if !accepted {
                continue;
            }
            return body_map_record(ctx, pairs_start, pairs, name, "f3d snapshot body-map pairs")
                .map(Some);
        }
    }
    Ok(None)
}

/// A counted UTF-16LE blob name at `name_at` that ends exactly at `end`, with
/// its offset and the scoped storage that holds it.
fn blob_name_at<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    name_at: usize,
    end: usize,
) -> Result<Option<(usize, String, ScopedReservation<'ctx>)>, CodecError> {
    let Some(max_chars) = name_at
        .checked_add(4)
        .and_then(|payload| end.checked_sub(payload))
        .map(|remaining| remaining / 2)
    else {
        return Ok(None);
    };
    let Some((name, name_end, storage)) =
        lp_utf16_bounded_scoped(ctx, bytes, name_at, 0..=max_chars, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    Ok((name_end == end).then_some((name_at, name, storage)))
}

/// Parse every exactly indexed BREP body-map record in a Design `BulkStream`.
///
/// The type GUID names a family with more than one record frame. The
/// `MetaStream` entity list and primary record index select exact candidate
/// extents. A candidate is a body map only when one supported reserved-zero
/// width makes its count, pair run, tail, and basename consume that extent.
fn body_map_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    primary: &PrimaryFrames<'_>,
) -> Result<BodyMapRecords<'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d body-map records")?;
    let mut out = Vec::new();
    let mut typed_entities = HashSet::new();
    for (type_ordinal, design_type) in ctx
        .admit_iter(&meta.types, "scan F3D body-map carrier types")?
        .enumerate()
    {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if design_type.version != BODY_MAP_CARRIER_TYPE_VERSION {
            return Err(CodecError::NotImplemented(ctx.format_retained(
                format_args!(
                    "unsupported F3D Design body-map carrier version {}",
                    design_type.version
                ),
                "f3d Design unsupported diagnostic",
            )?));
        }
        if !carrier_registration_matches(design_type) {
            return Err(CodecError::Malformed(
                "F3D Design body-map carrier type has incompatible registration metadata".into(),
            ));
        }
        let class_tag = carrier_class_tag(type_ordinal).ok_or_else(|| {
            CodecError::Malformed(
                "F3D Design body-map carrier class tag is not three digits".into(),
            )
        })?;
        for entity_id in admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D body-map carrier entities",
        )? {
            let entity_id = *entity_id;
            if typed_entities.contains(&entity_id) {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design body-map carrier entity {entity_id} is registered more than once"
                    ),
                ));
            }
            storage.with_storage(|| {
                ctx.reserve_set(&mut typed_entities, 1, "f3d body-map typed entities")
            })?;
            typed_entities.insert(entity_id);
            let Some(frame) = primary.frame(entity_id) else {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design body-map carrier entity {entity_id} has no primary record"
                    ),
                ));
            };
            let record_index = u32::try_from(entity_id).map_err(|_| {
                crate::design::text::malformed_design(
                    ctx,
                    format_args!("F3D Design body-map carrier entity {entity_id} exceeds u32"),
                )
            })?;
            if View::u32_le_at(bytes, frame.start) != Some(3)
                || bytes_at::<3>(bytes, frame.start + indexed_design_record_header::CLASS_TAG)
                    != Some(&class_tag)
                || View::u32_le_at(
                    bytes,
                    frame.start + indexed_design_record_header::RECORD_INDEX,
                ) != Some(record_index)
            {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design body-map carrier entity {entity_id} has an invalid indexed header"
                    ),
                ));
            }

            let mut matched = None;
            for prefix_len in BODY_MAP_ZERO_PREFIX_LENGTHS {
                let Some(record) = parse_body_map_frame(ctx, bytes, meta, frame, prefix_len)?
                else {
                    continue;
                };
                if matched.replace(record).is_some() {
                    return Err(crate::design::text::malformed_design(
                        ctx,
                        format_args!(
                            "F3D Design body-map carrier entity {entity_id} has an ambiguous frame"
                        ),
                    ));
                }
            }
            if let Some(record) = matched {
                ctx.push_scoped_vec(&mut storage, &mut out, record, "f3d body-map records")?;
            }
        }
    }
    Ok((out, storage))
}

/// The body map in one carrier frame: the indexed header, `prefix_len` zero
/// bytes, the counted pairs, and a tail that is either a typed BREP container
/// reference or an eight-byte key and a zero `u32`, followed by the blob name
/// that ends the frame.
fn parse_body_map_frame<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    frame: PrimaryRecordFrame,
    prefix_len: usize,
) -> Result<Option<BodyMapRecord<'ctx>>, CodecError> {
    let PrimaryRecordFrame { start, end, .. } = frame;
    let Some(prefix_at) = start.checked_add(indexed_design_record_header::LEN) else {
        return Ok(None);
    };
    let Some(count_at) = prefix_at.checked_add(prefix_len) else {
        return Ok(None);
    };
    // The prefix is one of the fixed widths in `BODY_MAP_ZERO_PREFIX_LENGTHS`.
    if !bytes
        .get(prefix_at..count_at)
        .is_some_and(|prefix| prefix.iter().all(|byte| *byte == 0))
    {
        return Ok(None);
    }
    let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
        return Ok(None);
    };
    let count = usize::try_from(pair_count).map_err(|_| {
        crate::design::text::malformed_design(
            ctx,
            format_args!(
                "F3D Design body map at byte {start} pair count does not fit this platform"
            ),
        )
    })?;
    let Some(pairs_start) = count_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(pairs_end) = count
        .checked_mul(BODY_MAP_PAIR_LEN)
        .and_then(|span| pairs_start.checked_add(span))
    else {
        return Ok(None);
    };
    let body_map_name = |name_at: usize| -> Result<_, CodecError> {
        let Some(name) = blob_name_at(ctx, bytes, name_at, end)? else {
            return Ok(None);
        };
        let accepted = if pair_count == 0 {
            name.1.is_empty()
        } else {
            brep_blob_kind(ctx, &name.1)? == Some(BrepBlobKind::Modern)
        };
        Ok(accepted.then_some(name))
    };
    let mut typed_name = None;
    for reference in local_reference_candidates(bytes, pairs_end, true)
        .into_iter()
        .flatten()
    {
        if !reference_has_type(ctx, meta, &reference, BREP_CONTAINER_TYPE_GUID)? {
            continue;
        }
        if let Some(name) = body_map_name(reference.end)? {
            if typed_name.replace(name).is_some() {
                return Err(CodecError::malformed(
                    "F3D Design body-map frame has ambiguous typed reference tails",
                ));
            }
        }
    }
    let name = match typed_name {
        Some(name) => Some(name),
        None if View::u64_le_at(bytes, pairs_end).is_some()
            && View::u32_le_at(bytes, pairs_end + 8) == Some(0) =>
        {
            match pairs_end.checked_add(12) {
                Some(name_at) => body_map_name(name_at)?,
                None => None,
            }
        }
        None => None,
    };
    let Some(name) = name else {
        return Ok(None);
    };
    let Some(pair_bytes) = bytes.get(pairs_start..pairs_end) else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!("F3D Design body map at byte {start} has a truncated pair run"),
        ));
    };
    let (pairs, _) = pair_bytes.as_chunks::<BODY_MAP_PAIR_LEN>();
    body_map_record(ctx, pairs_start, pairs, name, "f3d body-map pairs").map(Some)
}

/// The two BREP blob kinds a body map names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrepBlobKind {
    /// A `.smb` snapshot blob.
    Snapshot,
    /// A `.smbh` blob.
    Modern,
}

/// The kind of a BREP blob basename: `BREP.` and a name with the extension
/// `smb` or `smbh`, without a path separator. The separator test and the
/// extension search read the name; the comparisons with literals read at most
/// their bytes.
fn brep_blob_kind(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<Option<BrepBlobKind>, CodecError> {
    const OPERATION: &str = "classify F3D BREP blob basename";
    if !value.starts_with("BREP.") {
        return Ok(None);
    }
    if ctx.any_by(
        value.as_bytes(),
        |byte| Ok(matches!(*byte, b'/' | b'\\')),
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((_, extension)) = rsplit_once_ascii(ctx, value, b'.', OPERATION)? else {
        return Ok(None);
    };
    Ok(match extension.as_bytes() {
        b"smb" => Some(BrepBlobKind::Snapshot),
        b"smbh" => Some(BrepBlobKind::Modern),
        _ => None,
    })
}

/// Every pair of the modern body maps in one Design `BulkStream`, in record
/// and pair order. The pairs are decode scratch held under the returned
/// reservation.
pub(crate) fn body_bindings<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
) -> Result<(Vec<BodyBinding>, ScopedReservation<'ctx>), CodecError> {
    let primary = PrimaryFrames::index(ctx, meta, bytes.len())?;
    let (records, _records_storage) = body_map_records(ctx, bytes, meta, &primary)?;
    let mut storage = ctx.reserve_scoped(0, "f3d flattened body-map pairs")?;
    let mut bindings = Vec::new();
    for record in ctx.admit_iter(&records, "flatten F3D body-map records")? {
        storage.with_storage(|| {
            ctx.extend_from_slice(
                &mut bindings,
                &record.bindings,
                "f3d flattened body-map pairs",
            )
        })?;
    }
    Ok((bindings, storage))
}

/// The body-map records one Design stream selects: the modern maps, or the
/// snapshot maps when the stream has no modern map.
fn selected_body_map_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
) -> Result<BodyMapRecords<'ctx>, CodecError> {
    let primary = PrimaryFrames::index(ctx, meta, bytes.len())?;
    let modern = body_map_records(ctx, bytes, meta, &primary)?;
    if modern.0.is_empty() {
        snapshot_body_map_records(ctx, bytes, meta, &primary)
    } else {
        Ok(modern)
    }
}

/// Return the typed model-blob set selected independently in each Design
/// stream. The modern `.smbh` map takes precedence over snapshot `.smb` maps.
///
/// Every named body map, modern or snapshot, must name a distinct binary BREP
/// entry of the Design asset: the multiset of map names equals the multiset of
/// BREP entry basenames. Without any named map the result is every distinct
/// BREP entry basename.
pub(crate) fn design_model_blob_names(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<String>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d body-map carrier names")?;
    // Every named carrier and whether its stream selects it.
    let mut carriers = Vec::new();
    let mut saw_design_stream = false;
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D Design body-map streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        saw_design_stream = true;
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(ctx, scan, &entry.name)?
        else {
            continue;
        };
        let primary = PrimaryFrames::index(ctx, &metadata, bytes.len())?;
        let (modern, _modern_storage) = body_map_records(ctx, bytes, &metadata, &primary)?;
        let (snapshots, _snapshot_storage) =
            snapshot_body_map_records(ctx, bytes, &metadata, &primary)?;
        let modern_selected = !modern.is_empty();
        for (records, selected, operation) in [
            (&modern, modern_selected, "scan F3D modern body-map records"),
            (
                &snapshots,
                !modern_selected,
                "scan F3D snapshot body-map records",
            ),
        ] {
            for record in ctx.admit_iter(records, operation)? {
                if record.blob_name.is_empty() {
                    continue;
                }
                let name = ctx.copy_scoped_text(
                    &record.blob_name,
                    &mut storage,
                    "f3d body-map carrier name",
                )?;
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut carriers,
                    (name, selected),
                    "f3d body-map carrier names",
                )?;
            }
        }
    }

    let mut archive_names = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D archive BREP entries")?
        .filter(|entry| {
            scan.belongs_to_design_asset(&entry.name)
                && matches!(entry.role, ContainerRole::BrepSmb | ContainerRole::BrepSmbh)
        })
    {
        let basename = rsplit_once_ascii(ctx, &entry.name, b'/', "find F3D archive BREP basename")?
            .map_or(entry.name.as_str(), |(_, basename)| basename);
        ctx.push_scoped_vec(
            &mut storage,
            &mut archive_names,
            basename,
            "f3d archive BREP names",
        )?;
    }
    ctx.sort_unstable_by(
        &mut archive_names,
        |name| *name,
        Ord::cmp,
        "sort F3D archive BREP names",
    )?;

    let mut names = Vec::new();
    if !saw_design_stream || carriers.is_empty() {
        ctx.dedup_by(
            &mut archive_names,
            |name, previous| {
                ctx.equal_bytes(
                    name.as_bytes(),
                    previous.as_bytes(),
                    "deduplicate F3D archive BREP names",
                )
            },
            "deduplicate F3D archive BREP names",
        )?;
        for name in ctx.admit_iter(&archive_names, "copy F3D archive BREP names")? {
            let name = ctx.copy_retained_text(name, "f3d archive BREP basename")?;
            ctx.push_vec(&mut names, name, "f3d design model blob names")?;
        }
        return Ok(names);
    }

    ctx.sort_unstable_by(
        &mut carriers,
        |(name, _)| name.as_str(),
        Ord::cmp,
        "sort F3D body-map carrier names",
    )?;
    let mut archive = archive_names.iter();
    let classifies_every_entry = carriers.len() == archive_names.len()
        && ctx.all_by(
            &carriers,
            |(name, _)| {
                let Some(archive_name) = archive.next() else {
                    return Ok(false);
                };
                ctx.equal_bytes(
                    name.as_bytes(),
                    archive_name.as_bytes(),
                    "match F3D body-map carrier names",
                )
            },
            "match F3D body-map carrier names",
        )?;
    if !classifies_every_entry {
        return Err(CodecError::malformed(
            "Design body-map carriers do not classify every binary BREP entry exactly once",
        ));
    }
    for (name, selected) in ctx.admit_iter(&carriers, "copy F3D selected body-map names")? {
        if !*selected {
            continue;
        }
        // Equal names are adjacent, so a repeated name repeats the last copy.
        if let Some(last) = names.last() {
            if ctx.equal_bytes(
                last.as_bytes(),
                name.as_bytes(),
                "deduplicate F3D selected body-map names",
            )? {
                continue;
            }
        }
        let name = ctx.copy_retained_text(name, "f3d selected body-map name")?;
        ctx.push_vec(&mut names, name, "f3d design model blob names")?;
    }
    Ok(names)
}

/// Decode every ordered Design BREP body-map pair and resolve each pair in its
/// named blob's body-selector namespace.
pub(crate) fn decode_design_body_bindings(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let active_basename = match active_brep_entry {
        Some(entry) => Some(
            rsplit_once_ascii(ctx, entry, b'/', "find F3D active BREP basename")?
                .map_or(entry, |(_, basename)| basename),
        ),
        None => None,
    };
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D Design body binding streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(ctx, scan, &entry.name)?
        else {
            continue;
        };
        let (records, _records_storage) = selected_body_map_records(ctx, bytes, &metadata)?;
        for record in ctx.admit_iter(&records, "scan F3D selected body-map records")? {
            let pair_count = u32::try_from(record.bindings.len())
                .map_err(|_| CodecError::malformed("F3D Design body map exceeds u32::MAX pairs"))?;
            let mut source_storage = ctx.reserve_scoped(0, "f3d source BREP body keys")?;
            let mut source_bodies = Vec::new();
            if pair_count != 0 {
                for key in ctx.admit_iter(body_keys, "scan F3D source BREP body keys")? {
                    let Some(source) = key.source_brep.as_deref().or(active_basename) else {
                        continue;
                    };
                    if !ctx.equal_bytes(
                        source.as_bytes(),
                        record.blob_name.as_bytes(),
                        "match F3D source BREP name",
                    )? {
                        continue;
                    }
                    ctx.push_scoped_vec(
                        &mut source_storage,
                        &mut source_bodies,
                        key,
                        "f3d source BREP body keys",
                    )?;
                }
            }
            for (ordinal, binding) in ctx
                .admit_iter(&record.bindings, "scan F3D body-map pair bindings")?
                .enumerate()
            {
                let ordinal = u32::try_from(ordinal).map_err(|_| {
                    CodecError::malformed("F3D Design body-map pair ordinal exceeds u32::MAX")
                })?;
                let body = crate::brep::resolve_body_selector(
                    ctx,
                    source_bodies.iter().copied(),
                    binding.asm_key,
                )?;
                let id = design_record_id_charged(
                    ctx,
                    &entry.name,
                    ":design-body-binding#",
                    u64_from_index(binding.asm_key_offset),
                    "f3d body record identifier",
                )?;
                let record =
                    DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
                        id,
                        stream: ctx.validate_nonblank_text(
                            ctx.copy_retained_text(&entry.name, "f3d body-binding stream")?,
                            "validate stream",
                        )?,
                        pair_count,
                        pair_ordinal: ordinal,
                        asm_body_key: binding.asm_key,
                        asm_body_key_offset: u64_from_index(binding.asm_key_offset),
                        entity_suffix: binding.entity_suffix,
                        entity_suffix_offset: u64_from_index(binding.entity_suffix_offset()),
                        blob_name: ctx
                            .copy_retained_text(&record.blob_name, "f3d body-binding blob name")?,
                        blob_name_offset: u64_from_index(record.blob_name_offset),
                        body: body
                            .map(|id| id.try_clone_for_decode(ctx, "copy F3D BREP body ID"))
                            .transpose()?,
                    })
                    .map_err(CodecError::Malformed)?;
                ctx.push_vec(&mut out, record, "f3d decoded body bindings")?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design body 5",
    )?;
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream, in pair-offset order.
pub(crate) fn bind_body_bounds(
    ctx: &DecodeContext<'_>,
    bounds: &mut [DesignBodyBounds],
    bindings: &[DesignBodyBinding],
) -> Result<(), CodecError> {
    if bounds.is_empty() {
        return Ok(());
    }
    // Pairs ordered by entity suffix and then by key offset.
    let mut storage = ctx.reserve_scoped(0, "f3d body bounds binding index")?;
    let mut by_suffix = Vec::new();
    ctx.reserve_scoped_vec(
        &mut storage,
        &mut by_suffix,
        bindings.len(),
        "f3d body bounds binding index",
    )?;
    for binding in ctx.admit_iter(bindings, "index F3D body bounds bindings")? {
        by_suffix.push(binding);
    }
    ctx.stable_sort_by_key(
        &mut by_suffix,
        |binding| (binding.entity_suffix, binding.asm_body_key_offset()),
        Ord::cmp,
        "sort f3d design body 6",
    )?;
    let bounds_len = bounds.len();
    for (bounds, _) in bounds
        .iter_mut()
        .zip(ctx.admit_iter(&(0..bounds_len), "scan F3D body bounds")?)
    {
        let suffix = bounds.entity_suffix();
        let first = ctx.partition_point(
            &by_suffix,
            |binding| Ok(binding.entity_suffix < suffix),
            "find F3D body bounds bindings",
        )?;
        let candidates = by_suffix.get(first..).unwrap_or_default();
        let run = ctx.partition_point(
            candidates,
            |binding| Ok(binding.entity_suffix == suffix),
            "find F3D body bounds bindings",
        )?;
        let candidates = candidates.get(..run).unwrap_or_default();
        let Some(stream) = record_stream(ctx, bounds.id())? else {
            continue;
        };
        let mut ids = Vec::new();
        for binding in ctx.admit_iter(candidates, "scan F3D body bounds bindings")? {
            let (_scope_storage, scope) = native_scope_scoped(ctx, binding.stream())?;
            if !ctx.equal_bytes(
                scope.as_bytes(),
                stream.as_bytes(),
                "match F3D body bounds binding stream",
            )? {
                continue;
            }
            let id = ctx.copy_retained_text(binding.id(), "f3d body bounds binding identifier")?;
            ctx.push_vec(
                &mut ids,
                crate::records::bodies::DesignBodyBindingId::try_from(id)
                    .map_err(CodecError::Malformed)?,
                "f3d body bounds binding identifiers",
            )?;
        }
        bounds.set_body_binding_ids(ids);
    }
    Ok(())
}

/// One body's display visibility: the Design stream, the byte offset of the
/// browser-node hidden flag, the body-map pair, and whether the body shows.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub(crate) stream: String,
    pub(crate) byte_offset: u64,
    pub(crate) asm_body_key_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) visible: bool,
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// Each BREP body-map record resolves blob-qualified body selectors to Design
/// entity suffixes, and each entity's browser-node record carries a hidden flag
/// directly after the node GUID
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
/// The result maps each blob and body selector to its display visibility;
/// bodies without records are absent. The map is decode scratch held under
/// the returned reservation.
pub(crate) fn decode_all_body_visibility<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        HashMap<(String, u64), DecodedBodyVisibility>,
        ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "f3d body visibility entries")?;
    let mut out = HashMap::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D body visibility streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(ctx, scan, &entry.name)?
        else {
            continue;
        };
        let (hidden_by_entity, _hidden_storage) =
            typed_browser_node_hidden_flags(ctx, bytes, &metadata)?;
        let (records, _records_storage) = selected_body_map_records(ctx, bytes, &metadata)?;
        for record in ctx.admit_iter(&records, "scan F3D visibility body-map records")? {
            for binding in
                ctx.admit_iter(&record.bindings, "scan F3D visibility body-map bindings")?
            {
                let Ok(index) = ctx.binary_search_by_key(
                    &hidden_by_entity,
                    &binding.entity_suffix,
                    |(entity_suffix, _)| Ok(*entity_suffix),
                    "find F3D body visibility node",
                )?
                else {
                    continue;
                };
                let Some((_, node)) = hidden_by_entity.get(index) else {
                    continue;
                };
                let key = ctx.copy_scoped_text(
                    &record.blob_name,
                    &mut storage,
                    "f3d visibility BREP name",
                )?;
                let stream =
                    ctx.copy_scoped_text(&entry.name, &mut storage, "f3d visibility stream")?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut out,
                        (key, binding.asm_key),
                        DecodedBodyVisibility {
                            stream,
                            byte_offset: node.byte_offset,
                            asm_body_key_offset: u64_from_index(binding.asm_key_offset),
                            entity_suffix: binding.entity_suffix,
                            visible: !node.hidden,
                        },
                        "f3d body visibility entries",
                    )
                })?;
            }
        }
    }
    Ok((out, storage))
}

/// Visibility selected from one typed browser-node record.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

/// The browser-node visibility of each entity, ordered by entity suffix.
///
/// An entity's node is the one browser node its body presentations link, when
/// they link exactly one node record. Without a linked node it is the entity's
/// only typed browser node.
fn typed_browser_node_hidden_flags<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
) -> Result<(Vec<(u64, BrowserNodeVisibility)>, ScopedReservation<'ctx>), CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "f3d browser visibility scratch")?;
    let nodes = scratch.with_storage(|| {
        crate::design::decode::presentation::browser_node_records(ctx, bytes, meta)
    })?;
    let presentations = scratch.with_storage(|| {
        crate::design::decode::presentation::body_presentations(ctx, bytes, meta)
    })?;

    // Typed nodes ordered by entity suffix, in record order within a suffix.
    let mut by_entity = Vec::new();
    for node in ctx.admit_iter(&nodes, "scan F3D browser visibility nodes")? {
        ctx.push_scoped_vec(
            &mut scratch,
            &mut by_entity,
            node,
            "f3d browser visibility candidates",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut by_entity,
        |node| node.entity_suffix,
        Ord::cmp,
        "sort F3D browser visibility candidates",
    )?;
    // Presentation-linked nodes ordered by the presentation's entity suffix
    // and then by node record index, in presentation order within one key.
    let mut linked: Vec<(u64, &BrowserNodeRecord)> = Vec::new();
    for presentation in ctx.admit_iter(&presentations, "scan F3D browser node presentations")? {
        let Some(node) = &presentation.browser_node else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut scratch,
            &mut linked,
            (presentation.entity_suffix, node),
            "f3d linked browser visibility nodes",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut linked,
        |(entity_suffix, node)| (*entity_suffix, node.record_index),
        Ord::cmp,
        "sort f3d design body 7",
    )?;

    let mut storage = ctx.reserve_scoped(0, "f3d selected browser visibility")?;
    let mut out = Vec::new();
    for (index, node) in ctx
        .admit_iter(&by_entity, "select F3D browser visibility")?
        .enumerate()
    {
        let entity_suffix = node.entity_suffix;
        let continues_run = index
            .checked_sub(1)
            .and_then(|previous| by_entity.get(previous))
            .is_some_and(|previous| previous.entity_suffix == entity_suffix);
        if continues_run {
            continue;
        }
        let only_candidate = by_entity
            .get(index + 1)
            .is_none_or(|next| next.entity_suffix != entity_suffix);
        let linked_start = ctx.partition_point(
            &linked,
            |(linked_suffix, _)| Ok(*linked_suffix < entity_suffix),
            "find F3D linked browser visibility nodes",
        )?;
        let linked_end = ctx.partition_point(
            &linked,
            |(linked_suffix, _)| Ok(*linked_suffix <= entity_suffix),
            "find F3D linked browser visibility nodes",
        )?;
        let selected = if linked_start == linked_end {
            only_candidate.then_some(*node)
        } else {
            // One record index links when the run's first and last agree.
            match (
                linked.get(linked_start),
                linked_end.checked_sub(1).and_then(|last| linked.get(last)),
            ) {
                (Some((_, first)), Some((_, last))) if first.record_index == last.record_index => {
                    Some(*first)
                }
                _ => None,
            }
        };
        if let Some(node) = selected {
            ctx.push_scoped_vec(
                &mut storage,
                &mut out,
                (
                    entity_suffix,
                    BrowserNodeVisibility {
                        byte_offset: node.hidden_offset,
                        hidden: node.hidden,
                    },
                ),
                "f3d selected browser visibility",
            )?;
        }
    }
    Ok((out, storage))
}

/// Map each browser-node GUID to its Design entity suffix, in scoped storage.
///
/// The GUID is the stable join between browser presentation records; the
/// adjacent entity suffix joins the node back to the Design body map. A GUID
/// that names two entity suffixes is absent.
pub(crate) fn scanned_browser_node_entities<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(HashMap<String, u64>, ScopedReservation<'ctx>), CodecError> {
    let (records, _records_storage) = scan_browser_node_identities(ctx, bytes)?;
    let mut storage = ctx.reserve_scoped(0, "index F3D browser node entities")?;
    let mut entities = HashMap::<String, u64>::new();
    let mut ambiguous_storage = ctx.reserve_scoped(0, "index F3D ambiguous browser nodes")?;
    let mut ambiguous = Vec::new();
    for record in ctx.admit_iter(&records, "index F3D browser node identities")? {
        let guid = record.guid();
        match ctx.get_hash_map(&entities, guid, "index F3D browser node entities")? {
            Some(previous) => {
                if *previous != record.entity_suffix {
                    ctx.push_scoped_vec(
                        &mut ambiguous_storage,
                        &mut ambiguous,
                        guid,
                        "index F3D ambiguous browser nodes",
                    )?;
                }
            }
            None => {
                let key = ctx.copy_scoped_text(guid, &mut storage, "copy F3D browser node GUID")?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut entities,
                        key,
                        record.entity_suffix,
                        "index F3D browser node entities",
                    )
                })?;
            }
        }
    }
    for guid in ctx.admit_iter(&ambiguous, "drop F3D ambiguous browser nodes")? {
        ctx.remove_hash_map(&mut entities, *guid, "drop F3D ambiguous browser nodes")?;
    }
    Ok((entities, storage))
}

/// The UTF-16LE code-unit count of a browser-node GUID.
const BROWSER_NODE_GUID_COUNT: [u8; 4] = 36u32.to_le_bytes();

/// One browser-node GUID and the entity suffix that follows it.
#[derive(Debug)]
struct ScannedBrowserNodeIdentity {
    /// Lowercase ASCII GUID.
    guid: [u8; 36],
    entity_suffix: u64,
}

impl ScannedBrowserNodeIdentity {
    fn guid(&self) -> &str {
        std::str::from_utf8(&self.guid).unwrap_or_default()
    }
}

/// Every browser-node identity in `bytes`: a 36-unit UTF-16LE GUID of hex
/// digits and hyphens, a hidden flag of `0` or `1`, the bytes `1 1`, and the
/// `u64` entity suffix.
fn scan_browser_node_identities<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(Vec<ScannedBrowserNodeIdentity>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "collect F3D browser node identities")?;
    let mut out = Vec::new();
    // The count `36, 0, 0, 0` does not overlap itself, so the non-overlapping
    // matches are every match.
    for at in ctx.find_bytes_iter(
        bytes,
        &BROWSER_NODE_GUID_COUNT,
        "find F3D browser node GUID count",
    )? {
        let Some(identity) = browser_node_identity_at(bytes, at) else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut out,
            identity,
            "collect F3D browser node identities",
        )?;
    }
    Ok((out, storage))
}

/// The browser-node identity whose GUID count is at `at`. The test reads a
/// fixed 87 bytes.
fn browser_node_identity_at(bytes: &[u8], at: usize) -> Option<ScannedBrowserNodeIdentity> {
    let units = bytes_at::<72>(bytes, at.checked_add(4)?)?;
    let flag_at = at + 4 + 72;
    let [hidden, 1, 1] = *bytes_at::<3>(bytes, flag_at)? else {
        return None;
    };
    if hidden > 1 {
        return None;
    }
    let entity_suffix = View::u64_le_at(bytes, flag_at + 3)?;
    let mut guid = [0; 36];
    for (slot, unit) in guid.iter_mut().zip(units.as_chunks::<2>().0) {
        let [byte, 0] = *unit else {
            return None;
        };
        if !(byte.is_ascii_hexdigit() || byte == b'-') {
            return None;
        }
        *slot = byte.to_ascii_lowercase();
    }
    Some(ScannedBrowserNodeIdentity {
        guid,
        entity_suffix,
    })
}

#[cfg(test)]
mod tests {
    mod map_limits;
    mod recipe_ids;
    mod reference_candidates;

    use cadmpeg_core::decode::u64_from_index;

    use std::io::{Cursor, Write};

    use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
    use cadmpeg_core::CodecError;
    use zip::CompressionMethod;

    use super::{
        body_bindings, body_bound_values, parse_body_map_frame, snapshot_body_map_records,
        typed_browser_node_hidden_flags, BodyMapRecord, BrowserNodeVisibility, PrimaryFrames,
    };
    use crate::bytes::lp_utf16_bytes;
    use crate::bytes::take_reference;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::records::entity_header::DESIGN_MODULE_BODY;
    use crate::records::entity_header::DESIGN_MODULE_FUSION;
    use crate::test_support::indexed_header;
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::push_reference_u64;
    use crate::test_support::zip_test::with_scan;

    /// Assert that `decode` refuses at `operation` in `dimension` once the
    /// budget admits every earlier request.
    fn assert_refuses_at<T>(
        dimension: ResourceDimension,
        operation: &str,
        decode: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    ) {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, decode);
        assert!(
            matches!(&error, CodecError::ResourceLimit(limit)
                if limit.dimension == dimension && limit.operation == operation),
            "{operation}: {error:?}"
        );
    }

    /// Every snapshot body-map record of one stream.
    fn snapshot_records<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        bytes: &[u8],
        meta: &crate::metastream::MetaStream,
    ) -> Result<Vec<BodyMapRecord<'ctx>>, CodecError> {
        let primary = PrimaryFrames::index(ctx, meta, bytes.len())?;
        let (records, _storage) = snapshot_body_map_records(ctx, bytes, meta, &primary)?;
        Ok(records)
    }

    /// The flattened modern body-map pairs of one stream.
    fn flat_pairs(bytes: &[u8], meta: &crate::metastream::MetaStream) -> Vec<super::BodyBinding> {
        let ctx = cadmpeg_test_support::service_decode_context();
        let (pairs, _storage) = body_bindings(&ctx, bytes, meta).expect("body-map pairs");
        pairs
    }

    fn body_member_archive(members: &[(u8, u64, u16)], terminator: u8) -> (Vec<u8>, usize) {
        let mut bulk = Vec::new();
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(&0_u16.to_le_bytes());
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(
            &u32::try_from(members.len())
                .expect("fixture count fits u32")
                .to_le_bytes(),
        );
        let member_offset = bulk.len();
        for (marker, entity_suffix, flags) in members {
            bulk.push(*marker);
            bulk.extend_from_slice(&entity_suffix.to_le_bytes());
            bulk.extend_from_slice(&flags.to_le_bytes());
        }
        bulk.push(terminator);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        (zip.finish().unwrap().into_inner(), member_offset)
    }

    #[test]
    fn body_member_collections_and_identity_refuse_caller_limits() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let (archive, member_offset) = body_member_archive(&[(1, 9, 2)], 0);
        with_scan(&archive, |scan| {
            for (dimension, operation) in [
                (ResourceDimension::CollectionItems, "f3d body members"),
                (ResourceDimension::RetainedBytes, "f3d native stream key"),
                (ResourceDimension::WorkUnits, "find F3D body-member marker"),
                (
                    ResourceDimension::WorkUnits,
                    "check F3D body-member markers",
                ),
            ] {
                assert_refuses_at(dimension, operation, |ctx| {
                    super::decode_body_members(ctx, scan)
                });
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                let members = super::decode_body_members(ctx, scan).unwrap();
                assert_eq!(members.len(), 1);
                assert_eq!(members[0].entity_suffix, 9);
                assert_eq!(members[0].flags, 2);
                assert_eq!(
                    members[0].id(),
                    &crate::ids::native_design_body_member_id(ENTRY, member_offset)
                );
            });
        });
    }

    #[test]
    fn body_member_list_requires_every_marker_and_the_zero_terminator() {
        for (members, terminator) in [
            (&[(1, 9, 2), (0, 10, 3)][..], 0),
            (&[(1, 9, 2), (1, 10, 3)][..], 1),
        ] {
            let (archive, _) = body_member_archive(members, terminator);
            with_scan(&archive, |scan| {
                let members = super::decode_body_members(
                    &cadmpeg_test_support::service_decode_context(),
                    scan,
                )
                .unwrap();
                assert!(members.is_empty());
            });
        }
        let (archive, _) = body_member_archive(&[(1, 9, 2), (1, 10, 3)], 0);
        with_scan(&archive, |scan| {
            let members =
                super::decode_body_members(&cadmpeg_test_support::service_decode_context(), scan)
                    .unwrap();
            assert_eq!(
                members
                    .iter()
                    .map(|member| (member.entity_suffix, member.flags))
                    .collect::<Vec<_>>(),
                [(9, 2), (10, 3)]
            );
        });
    }

    fn push_entity_header(out: &mut Vec<u8>, class_tag: &str, entity: u64) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend(
            lp_utf16_bytes(&format!("0_{entity}"))
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
    }

    fn body_map_bytes(prefix_len: usize, declared_count: u32, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(&declared_count.to_le_bytes());
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        out.extend_from_slice(&1793u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(
            lp_utf16_bytes(if declared_count == 0 {
                ""
            } else {
                "BREP.synthetic.smbh"
            })
            .expect("fixture UTF-16 code-unit count fits u32"),
        );
        out
    }

    fn body_map_bytes_with_typed_tail(prefix_len: usize, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(
            &(u32::try_from(pairs.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            1,
        );
        out.push(0);
        out.extend(
            lp_utf16_bytes("BREP.synthetic.smbh").expect("fixture UTF-16 code-unit count fits u32"),
        );
        out
    }

    fn body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                crate::records::entity_header::SegmentTypeData {
                    byte_offset: 0,
                    type_guid: crate::design::body::BODY_MAP_CARRIER_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .expect("type GUID"),
                    type_guid_offset: 0,
                    base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                        value: crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID
                            .to_owned()
                            .try_into()
                            .expect("base GUID"),
                        offset: 0,
                    },
                    version: crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    version_offset: 0,
                    module: DESIGN_MODULE_BODY.into(),
                    entities: crate::records::identity::ReferenceRun::located(vec![
                        crate::records::identity::Located {
                            value: 900,
                            offset: 0,
                        },
                    ]),
                },
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![crate::metastream::RecordIndexEntry {
                entity_id: 900,
                bulk_offset: 0,
            }],
            secondary_records: Vec::new(),
        }
    }

    fn push_snapshot_reference(out: &mut Vec<u8>, target: u64, target_type: &str, form: u8) {
        match form {
            1 => {
                out.push(1);
                out.extend_from_slice(&target.to_le_bytes());
                out.extend_from_slice(
                    &(u32::try_from(target_type.len()).expect("fixture value fits u32"))
                        .to_le_bytes(),
                );
                out.extend_from_slice(target_type.as_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            2 => {
                out.extend_from_slice(&[1, 1]);
                out.extend_from_slice(&target.to_le_bytes());
            }
            // Form 0 and every other stated form write the plain reference.
            _ => push_reference_u64(out, target),
        }
    }

    fn snapshot_body_map_bytes_with(
        form: u8,
        pair_count: u32,
        blob_name: &str,
        companion_type: &str,
    ) -> Vec<u8> {
        let entity = 900u64;
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"256");
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        push_snapshot_reference(&mut out, entity + 1, companion_type, form);
        if form == 2 {
            out.extend_from_slice(&[0, 0]);
        }
        out.extend_from_slice(&pair_count.to_le_bytes());
        if pair_count != 0 {
            out.extend_from_slice(&7u64.to_le_bytes());
            out.extend_from_slice(&500u64.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            form,
        );
        if form == 2 {
            out.extend_from_slice(&[0, 0, 0]);
        } else {
            out.push(0);
        }
        out.extend(lp_utf16_bytes(blob_name).expect("fixture UTF-16 code-unit count fits u32"));
        out
    }

    fn snapshot_body_map_bytes(form: u8) -> Vec<u8> {
        snapshot_body_map_bytes_with(
            form,
            1,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        )
    }

    fn snapshot_body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                design_type(
                    crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
                    Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID),
                    1,
                    DESIGN_MODULE_BODY,
                    vec![900],
                ),
                design_type(
                    crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![901],
                ),
                design_type(
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![500],
                ),
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![primary_record(900, 0)],
            secondary_records: Vec::new(),
        }
    }

    #[test]
    fn snapshot_body_map_accepts_every_reference_envelope() {
        let ctx = cadmpeg_test_support::service_decode_context();
        for form in 0..=2 {
            let records = snapshot_records(
                &ctx,
                &snapshot_body_map_bytes(form),
                &snapshot_body_map_metadata(),
            )
            .expect("typed snapshot body map");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
            assert_eq!(records[0].bindings.len(), 1);
            assert_eq!(records[0].bindings[0].asm_key, 7);
            assert_eq!(records[0].bindings[0].entity_suffix, 500);
        }
    }

    #[test]
    fn snapshot_body_map_accepts_three_zero_companion_variant() {
        let mut bytes = snapshot_body_map_bytes(0);
        let mut companion_end = 4 + 3 + 8 + 6;
        take_reference(&bytes, &mut companion_end).expect("ordinary companion reference");
        bytes.insert(companion_end, 0);

        let ctx = cadmpeg_test_support::service_decode_context();
        let records = snapshot_records(&ctx, &bytes, &snapshot_body_map_metadata())
            .expect("three-zero companion variant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_accepts_padded_doubled_companion() {
        let mut bytes = snapshot_body_map_bytes(2);
        let companion_end = 4 + 3 + 8 + 6 + 2 + 8;
        bytes.insert(companion_end, 0);

        let ctx = cadmpeg_test_support::service_decode_context();
        let records = snapshot_records(&ctx, &bytes, &snapshot_body_map_metadata())
            .expect("padded doubled companion");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_requires_typed_pair_targets() {
        let mut metadata = snapshot_body_map_metadata();
        metadata.types[2].entities = crate::records::identity::ReferenceRun::unlocated(Vec::new());
        let ctx = cadmpeg_test_support::service_decode_context();
        assert!(
            snapshot_records(&ctx, &snapshot_body_map_bytes(0), &metadata)
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn snapshot_body_map_retains_named_zero_pair_blob() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        let records = snapshot_records(&ctx, &bytes, &snapshot_body_map_metadata())
            .expect("named zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_accepts_empty_zero_pair_record() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        let records = snapshot_records(&ctx, &bytes, &snapshot_body_map_metadata())
            .expect("empty zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert!(records[0].blob_name.is_empty());
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_rejects_contradictory_inline_type() {
        let bytes = snapshot_body_map_bytes_with(
            1,
            1,
            "BREP.snapshot.smb",
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        assert!(
            snapshot_records(&ctx, &bytes, &snapshot_body_map_metadata())
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn body_map_count_is_bounded_by_the_stream_not_sixty_four_pairs() {
        let pairs = (0u64..65)
            .map(|ordinal| (1000 + ordinal, (1u64 << 40) + ordinal))
            .collect::<Vec<_>>();
        let bindings = flat_pairs(&body_map_bytes(10, 65, &pairs), &body_map_metadata());
        assert_eq!(bindings.len(), 65);
        assert_eq!(bindings[0].asm_key, 1000);
        assert_eq!(bindings[64].asm_key, 1064);
        assert_eq!(bindings[64].entity_suffix, (1u64 << 40) + 64);
    }

    #[test]
    fn body_map_accepts_typed_container_reference_tail() {
        let bytes = body_map_bytes_with_typed_tail(10, &[(2291, 7492), (2292, 7534)]);
        let bindings = flat_pairs(&bytes, &body_map_metadata());
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].asm_key, 2291);
        assert_eq!(bindings[0].entity_suffix, 7492);
        assert_eq!(bindings[1].asm_key, 2292);
        assert_eq!(bindings[1].entity_suffix, 7534);
    }

    #[test]
    fn both_empty_body_map_prefixes_have_no_pairs_or_brep_basename() {
        for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
            let bytes = body_map_bytes(prefix_len, 0, &[]);
            let ctx = cadmpeg_test_support::service_decode_context();
            let frame = parse_body_map_frame(
                &ctx,
                &bytes,
                &body_map_metadata(),
                crate::metastream::PrimaryRecordFrame {
                    entity_id: 900,
                    start: 0,
                    member_end: bytes.len(),
                    end: bytes.len(),
                },
                prefix_len,
            )
            .expect("empty body-map frame")
            .expect("supported empty body-map variant");
            assert!(frame.bindings.is_empty());
            assert!(flat_pairs(&bytes, &body_map_metadata()).is_empty());
        }
    }

    #[test]
    fn body_map_header_prevents_a_high_word_count_alias() {
        let bytes = body_map_bytes(10, 2, &[(10, (1u64 << 32) + 77), (20, 30)]);
        let bindings = flat_pairs(&bytes, &body_map_metadata());
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].entity_suffix, (1u64 << 32) + 77);
        assert_eq!(bindings[1].asm_key, 20);
    }

    #[test]
    fn truncated_body_map_frame_is_not_decoded() {
        let bytes = body_map_bytes(10, 2, &[(10, 20)]);
        assert!(flat_pairs(&bytes, &body_map_metadata()).is_empty());
    }

    #[test]
    fn body_map_parser_does_not_scan_an_unindexed_nested_header() {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 900);
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend(body_map_bytes(10, 1, &[(10, 20)]));

        assert!(flat_pairs(&bytes, &body_map_metadata()).is_empty());
    }

    #[test]
    fn composed_body_map_outputs_refuse_caller_limits() {
        const PREFIX: &str = "FusionAssetName[Active]/Design1/";
        const BREP: &str = "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh";
        let mut bulk = body_map_bytes(10, 1, &[(7, 42)]);
        let browser_offset = bulk.len();
        push_browser_node(
            &mut bulk,
            100,
            "11111111-2222-8333-A444-555555555555",
            false,
            42,
        );
        let metadata = crate::test_support::streams_test::design_metastream_with_records(
            &[
                (
                    crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    &[900],
                ),
                (
                    BROWSER_NODE_TYPE_GUID,
                    BROWSER_NODE_BASE_TYPE_GUID,
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    &[100],
                ),
            ],
            &[(900, 0), (100, u64_from_index(browser_offset))],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(format!("{PREFIX}BulkStream.dat"), stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file(format!("{PREFIX}MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(&metadata).unwrap();
        zip.start_file(BREP, stored).unwrap();
        zip.write_all(&[0]).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let ctx = cadmpeg_test_support::service_decode_context();
            let (visibility, _storage) = super::decode_all_body_visibility(&ctx, scan).unwrap();
            assert!(
                visibility
                    .get(&("BREP.synthetic.smbh".to_owned(), 7))
                    .unwrap()
                    .visible
            );
            let names = super::design_model_blob_names(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
            .unwrap();
            assert_eq!(names, ["BREP.synthetic.smbh"]);
            let bindings = super::decode_design_body_bindings(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                None,
                &[],
            )
            .unwrap();
            assert_eq!(bindings.len(), 1);
            let body_key = cadmpeg_asm::brep::records::BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: "f3d:brep:entity#7".try_into().unwrap(),
                record_index: 7,
                body_ordinal: 0,
                source_brep: Some("BREP.synthetic.smbh".into()),
                asm_body_key: Some(7),
            };
            let bound = super::decode_design_body_bindings(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                None,
                std::slice::from_ref(&body_key),
            )
            .unwrap();
            assert_eq!(bound.len(), 1);
            for (dimension, operation) in [
                (
                    ResourceDimension::CollectionItems,
                    "f3d body visibility entries",
                ),
                (
                    ResourceDimension::CollectionItems,
                    "f3d selected browser visibility",
                ),
            ] {
                assert_refuses_at(dimension, operation, |ctx| {
                    super::decode_all_body_visibility(ctx, scan).map(|_| ())
                });
            }
            // The blob name these decodes read ends the body-map frame. It is
            // scoped storage: a zero materialized limit refuses it, and a zero
            // retained limit does not.
            let name = lp_utf16_bytes("BREP.synthetic.smbh").unwrap();
            let name_at = browser_offset - name.len();
            for (retained, materialized) in [(u64::MAX, 0), (0, u64::MAX)] {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_retained_bytes = retained;
                policy.limits.max_materialized_bytes = materialized;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::blob_name_at(&ctx, &bulk, name_at, browser_offset);
                if materialized == 0 {
                    assert!(
                        matches!(&result, Err(CodecError::ResourceLimit(limit))
                            if limit.dimension == ResourceDimension::MaterializedBytes
                                && limit.operation == "f3d Design UTF-16 text"),
                        "{result:?}"
                    );
                } else {
                    let (at, text, _storage) = result.unwrap().expect("framed blob name");
                    assert_eq!((at, text.as_str()), (name_at, "BREP.synthetic.smbh"));
                }
            }
            for (dimension, operation) in [
                (
                    ResourceDimension::CollectionItems,
                    "f3d body-map carrier names",
                ),
                (ResourceDimension::CollectionItems, "f3d archive BREP names"),
                (
                    ResourceDimension::CollectionItems,
                    "f3d design model blob names",
                ),
                (
                    ResourceDimension::MaterializedBytes,
                    "f3d body-map carrier name",
                ),
                (
                    ResourceDimension::RetainedBytes,
                    "f3d selected body-map name",
                ),
            ] {
                assert_refuses_at(dimension, operation, |ctx| {
                    super::design_model_blob_names(ctx, scan)
                });
            }
            for (dimension, operation) in [
                (
                    ResourceDimension::CollectionItems,
                    "f3d source BREP body keys",
                ),
                (
                    ResourceDimension::CollectionItems,
                    "f3d decoded body bindings",
                ),
                (ResourceDimension::RetainedBytes, "f3d native stream key"),
                (
                    ResourceDimension::RetainedBytes,
                    "f3d body record identifier",
                ),
                (ResourceDimension::RetainedBytes, "f3d body-binding stream"),
                (
                    ResourceDimension::RetainedBytes,
                    "f3d body-binding blob name",
                ),
                (ResourceDimension::RetainedBytes, "copy F3D BREP body ID"),
            ] {
                assert_refuses_at(dimension, operation, |ctx| {
                    super::decode_design_body_bindings(
                        ctx,
                        scan,
                        None,
                        std::slice::from_ref(&body_key),
                    )
                });
            }
        });
    }

    #[test]
    fn archive_brep_names_without_body_maps_are_distinct_and_sorted() {
        const BLOBS: &str = "FusionAssetName[Active]/Breps.BlobParts/";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        for name in [
            format!("{BLOBS}BREP.b.smbh"),
            format!("{BLOBS}BREP.a.smb"),
            format!("{BLOBS}Copy/BREP.b.smbh"),
        ] {
            zip.start_file(name, stored).unwrap();
            zip.write_all(&[0]).unwrap();
        }
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let names = super::design_model_blob_names(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
            .unwrap();
            assert_eq!(names, ["BREP.a.smb", "BREP.b.smbh"]);
        });
    }

    fn push_browser_node(
        out: &mut Vec<u8>,
        record_index: u32,
        guid: &str,
        hidden: bool,
        entity: u64,
    ) -> u64 {
        indexed_header(out, *b"257", record_index);
        out.extend_from_slice(&[0; 10]);
        out.extend(lp_utf16_bytes(guid).expect("fixture UTF-16 code-unit count fits u32"));
        let hidden_offset = u64_from_index(out.len());
        out.push(u8::from(hidden));
        out.extend_from_slice(&[1, 1]);
        out.extend_from_slice(&entity.to_le_bytes());
        hidden_offset
    }

    fn visibility_of(
        selected: &[(u64, BrowserNodeVisibility)],
        entity: u64,
    ) -> Option<BrowserNodeVisibility> {
        selected
            .iter()
            .find(|(entity_suffix, _)| *entity_suffix == entity)
            .map(|(_, visibility)| *visibility)
    }

    #[test]
    fn presentation_guid_selects_visibility_when_suffix_repeats() {
        let entity = 42u64;
        let selected_guid = "11111111-2222-8333-A444-555555555555";
        let competing_guid = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_entity_header(&mut bytes, "256", entity);
        bytes.extend(
            lp_utf16_bytes(selected_guid).expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(
            lp_utf16_bytes("99999999-8888-8777-A666-555555555555")
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID)
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes("PrismMaterial-001").expect("fixture UTF-16 code-unit count fits u32"),
        );
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend(lp_utf16_bytes("Body").expect("fixture UTF-16 code-unit count fits u32"));
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend(
            lp_utf16_bytes("12345678-1234-8234-A234-123456789ABC")
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes(APPEARANCE_LIBRARY_ID).expect("fixture UTF-16 code-unit count fits u32"),
        );
        let selected_start = bytes.len();
        let selected_offset = push_browser_node(&mut bytes, 100, selected_guid, false, entity);
        let competing_start = bytes.len();
        push_browser_node(&mut bytes, 101, competing_guid, true, entity);

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![
                primary_record(entity, 0),
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let ctx = cadmpeg_test_support::service_decode_context();
        let (visibility, _storage) =
            typed_browser_node_hidden_flags(&ctx, &bytes, &meta).expect("typed presentation graph");
        let selected = visibility_of(&visibility, entity).expect("presentation-selected node");
        assert_eq!(selected.byte_offset, selected_offset);
        assert!(!selected.hidden);

        let mut nodes_only = Vec::new();
        let selected_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 100, selected_guid, false, entity);
        let competing_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 101, competing_guid, true, entity);
        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
            ],
            records: vec![
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let (visibility, _storage) =
            typed_browser_node_hidden_flags(&ctx, &nodes_only, &meta).expect("typed browser nodes");
        assert!(
            visibility_of(&visibility, entity).is_none(),
            "two unjoined typed nodes are ambiguous"
        );
    }

    #[test]
    fn browser_visibility_refuses_entity_candidate_and_output_limits() {
        let mut bytes = Vec::new();
        push_browser_node(
            &mut bytes,
            100,
            "11111111-2222-8333-A444-555555555555",
            true,
            42,
        );
        let metadata = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100],
                ),
            ],
            records: vec![primary_record(100, 0)],
            secondary_records: Vec::new(),
        };
        for (dimension, operation) in [
            (
                ResourceDimension::CollectionItems,
                "f3d browser visibility candidates",
            ),
            (
                ResourceDimension::CollectionItems,
                "f3d selected browser visibility",
            ),
        ] {
            assert_refuses_at(dimension, operation, |ctx| {
                typed_browser_node_hidden_flags(ctx, &bytes, &metadata).map(|_| ())
            });
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        let (selected, _storage) =
            typed_browser_node_hidden_flags(&ctx, &bytes, &metadata).unwrap();
        assert!(visibility_of(&selected, 42).unwrap().hidden);
    }

    #[test]
    fn scanned_browser_guids_refuse_map_and_ambiguity_limits() {
        const GUID: &str = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_browser_node(&mut bytes, 100, GUID, false, 42);
        push_browser_node(&mut bytes, 101, GUID, true, 43);
        for (items, operation) in [
            (2, "index F3D browser node entities"),
            (3, "index F3D ambiguous browser nodes"),
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_collection_items = items;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                super::scanned_browser_node_entities(&ctx, &bytes),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        let (entities, _storage) = super::scanned_browser_node_entities(&ctx, &bytes).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn scanned_browser_guids_are_lowercase_keys() {
        let mut bytes = Vec::new();
        push_browser_node(
            &mut bytes,
            100,
            "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE",
            false,
            42,
        );
        push_browser_node(
            &mut bytes,
            101,
            "aaaaaaaa-bbbb-8ccc-9ddd-eeeeeeeeeeee",
            true,
            42,
        );
        push_browser_node(
            &mut bytes,
            102,
            "11111111-2222-8333-A444-555555555555",
            true,
            7,
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        let (entities, _storage) = super::scanned_browser_node_entities(&ctx, &bytes).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities["aaaaaaaa-bbbb-8ccc-9ddd-eeeeeeeeeeee"], 42);
        assert_eq!(entities["11111111-2222-8333-a444-555555555555"], 7);
    }

    #[test]
    fn body_bound_frame_has_one_marker_and_six_ordered_f64_values() {
        let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
        let mut frame = [0u8; 49];
        frame[0] = 1;
        for (slot, value) in frame[1..].chunks_exact_mut(8).zip(values) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            body_bound_values(&frame).map(|values| values.map(cadmpeg_ir::scalar::FiniteReal::get)),
            Some(values)
        );

        let mut unmarked = frame;
        unmarked[0] = 0;
        assert!(body_bound_values(&unmarked).is_none());

        let mut degenerate = frame;
        for (slot, value) in degenerate[1..].chunks_exact_mut(8).zip([1.0f64; 6]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        assert!(body_bound_values(&degenerate).is_none());
    }

    #[test]
    fn body_bounds_binding_refuses_matching_and_identifier_limits() {
        use crate::records::bodies::{
            DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
        };
        const STREAM: &str = "Design/BulkStream.dat";
        let binding = DesignBodyBinding::try_from(DesignBodyBindingWire::<String> {
            id: "f3d:Design/BulkStream.dat:design-body-binding#20".into(),
            stream: STREAM.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 9,
            asm_body_key_offset: 20,
            entity_suffix: 7,
            entity_suffix_offset: 28,
            blob_name: "BREP.synthetic.smbh".into(),
            blob_name_offset: 40,
            body: None,
        })
        .unwrap();
        let make_bounds = || {
            DesignBodyBounds::try_from(DesignBodyBoundsWire {
                id: format!("{}:design-body-bounds#0", crate::ids::native_scope(STREAM)),
                entity_suffix: 7,
                entity_byte_offset: 0,
                record_indices: [8, 9, 10],
                record_byte_offsets: [10, 20, 30],
                value_byte_offsets: [11, 21, 31],
                body_binding_ids: Vec::new(),
                maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
                minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            })
            .unwrap()
        };
        for (dimension, operation) in [
            (
                ResourceDimension::CollectionItems,
                "f3d body bounds binding index",
            ),
            (
                ResourceDimension::CollectionItems,
                "f3d body bounds binding identifiers",
            ),
            (
                ResourceDimension::MaterializedBytes,
                "f3d scoped native stream key",
            ),
            (
                ResourceDimension::RetainedBytes,
                "f3d body bounds binding identifier",
            ),
        ] {
            assert_refuses_at(dimension, operation, |ctx| {
                let mut bounds = [make_bounds()];
                super::bind_body_bounds(ctx, &mut bounds, std::slice::from_ref(&binding))
            });
        }
        let mut bounds = [make_bounds()];
        super::bind_body_bounds(
            &cadmpeg_test_support::service_decode_context(),
            &mut bounds,
            &[binding],
        )
        .unwrap();
        assert_eq!(
            bounds[0].body_binding_ids().collect::<Vec<_>>(),
            ["f3d:Design/BulkStream.dat:design-body-binding#20"]
        );
    }

    #[test]
    fn body_bounds_bind_pairs_of_one_stream_in_key_offset_order() {
        use crate::records::bodies::{
            DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
        };
        let binding = |stream: &str, entity_suffix: u64, offset: u64| {
            DesignBodyBinding::try_from(DesignBodyBindingWire::<String> {
                id: format!(
                    "{}:design-body-binding#{offset}",
                    crate::ids::native_scope(stream)
                ),
                stream: stream.into(),
                pair_count: 1,
                pair_ordinal: 0,
                asm_body_key: 9,
                asm_body_key_offset: offset,
                entity_suffix,
                entity_suffix_offset: offset + 8,
                blob_name: "BREP.synthetic.smbh".into(),
                blob_name_offset: offset + 16,
                body: None,
            })
            .unwrap()
        };
        let bindings = [
            binding("Design/BulkStream.dat", 7, 60),
            binding("Other/BulkStream.dat", 7, 10),
            binding("Design/BulkStream.dat", 8, 30),
            binding("Design/BulkStream.dat", 7, 20),
        ];
        let mut bounds = [DesignBodyBounds::try_from(DesignBodyBoundsWire {
            id: format!(
                "{}:design-body-bounds#0",
                crate::ids::native_scope("Design/BulkStream.dat")
            ),
            entity_suffix: 7,
            entity_byte_offset: 0,
            record_indices: [8, 9, 10],
            record_byte_offsets: [10, 20, 30],
            value_byte_offsets: [11, 21, 31],
            body_binding_ids: Vec::new(),
            maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
            minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        })
        .unwrap()];
        super::bind_body_bounds(
            &cadmpeg_test_support::service_decode_context(),
            &mut bounds,
            &bindings,
        )
        .unwrap();
        assert_eq!(
            bounds[0].body_binding_ids().collect::<Vec<_>>(),
            [
                "f3d:Design/BulkStream.dat:design-body-binding#20",
                "f3d:Design/BulkStream.dat:design-body-binding#60",
            ]
        );
    }

    fn body_entity(
        entry: &str,
        byte_offset: u64,
        suffix: u64,
    ) -> crate::records::entity_header::DesignEntityHeader {
        crate::records::entity_header::DesignEntityHeader {
            id: format!(
                "{}:design-entity-header#{byte_offset}",
                crate::ids::native_scope(entry)
            ),
            byte_offset,
            entity_id: crate::records::identity::DesignEntityId::try_from(format!("0_{suffix}"))
                .unwrap(),
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            optional_slot_present: false,
            registration: crate::records::entity_header::DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.to_owned()),
                None,
                crate::records::identity::ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        }
    }

    /// An entity header for `suffix` followed by its three bounds records.
    fn push_body_bounds(bulk: &mut Vec<u8>, suffix: u32, values: [f64; 6]) {
        indexed_header(bulk, *b"256", suffix);
        bulk.extend_from_slice(&[0; 5]);
        for record_index in suffix + 1..=suffix + 3 {
            indexed_header(bulk, *b"257", record_index);
            bulk.push(1);
            for value in values {
                bulk.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    fn single_stream_archive(entry: &str, bulk: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(entry, stored).unwrap();
        zip.write_all(bulk).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn decoded_body_bounds_refuse_output_and_identifier_limits() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        push_body_bounds(&mut bulk, 7, [4.0, 6.0, 1.5, -1.0, 0.0, -0.25]);
        let entity = body_entity(ENTRY, 0, 7);
        let archive = single_stream_archive(ENTRY, &bulk);
        with_scan(&archive, |scan| {
            for (dimension, operation) in [
                (ResourceDimension::CollectionItems, "f3d body bounds"),
                (ResourceDimension::RetainedBytes, "f3d native stream key"),
                (
                    ResourceDimension::RetainedBytes,
                    "f3d body record identifier",
                ),
                (
                    ResourceDimension::WorkUnits,
                    "find F3D indexed record header",
                ),
                (
                    ResourceDimension::WorkUnits,
                    "find F3D repeated body bounds",
                ),
                (
                    ResourceDimension::MaterializedBytes,
                    "index F3D entity offsets by stream",
                ),
            ] {
                assert_refuses_at(dimension, operation, |ctx| {
                    super::decode_body_bounds(ctx, scan, std::slice::from_ref(&entity))
                });
            }
            let bounds = super::decode_body_bounds(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&entity),
            )
            .unwrap();
            assert_eq!(bounds.len(), 1);
            assert_eq!(bounds[0].entity_suffix(), 7);
        });
    }

    #[test]
    fn body_bound_records_end_at_the_next_entity_of_the_same_stream() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        push_body_bounds(&mut bulk, 7, [4.0, 6.0, 1.5, -1.0, 0.0, -0.25]);
        let second_offset = bulk.len();
        push_body_bounds(&mut bulk, 20, [2.0, 2.0, 2.0, 1.0, 1.0, 1.0]);
        let entities = [
            body_entity(ENTRY, u64_from_index(second_offset), 20),
            // An entity of another stream inside the first body's records
            // does not bound them.
            body_entity("FusionAssetName[Active]/Design2/BulkStream.dat", 10, 99),
            body_entity(ENTRY, 0, 7),
        ];
        let archive = single_stream_archive(ENTRY, &bulk);
        with_scan(&archive, |scan| {
            let bounds = super::decode_body_bounds(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                &entities,
            )
            .unwrap();
            assert_eq!(
                bounds
                    .iter()
                    .map(crate::records::bodies::DesignBodyBounds::entity_suffix)
                    .collect::<Vec<_>>(),
                [7, 20]
            );
        });
    }

    #[test]
    fn a_recipe_marker_before_the_record_index_word_states_no_record_index() {
        // The family marker opens at offset four, so the stream holds no index
        // word sixteen bytes before it.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &mut recipes,
        )
        .unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].record_index, None);
        let wire = serde_json::to_value(&recipes[0]).expect("recipe wire");
        assert_eq!(wire.get("record_index"), None);
        assert_eq!(wire.get("record_index_offset"), None);
    }

    #[test]
    fn construction_recipe_refuses_counter_output_and_identifier_limits() {
        const STREAM: &str = "Design/BulkStream.dat";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        for (dimension, operation) in [
            (
                ResourceDimension::CollectionItems,
                "f3d construction recipe counters",
            ),
            (
                ResourceDimension::MaterializedBytes,
                "f3d construction recipe counters",
            ),
            (
                ResourceDimension::CollectionItems,
                "f3d construction recipes",
            ),
            (ResourceDimension::RetainedBytes, "f3d native stream key"),
            (
                ResourceDimension::RetainedBytes,
                "f3d body record identifier",
            ),
            (ResourceDimension::WorkUnits, "find F3D construction recipe"),
        ] {
            assert_refuses_at(dimension, operation, |ctx| {
                let mut recipes = Vec::new();
                super::decode_stream(ctx, &bytes, STREAM, &mut recipes)
            });
        }
    }

    #[test]
    fn construction_recipes_count_per_kind_and_design_id() {
        let mut bytes = Vec::new();
        for (design_id, name) in [
            (&b"2265"[..], &b"body_recipe_data"[..]),
            (b"2265", b"edge_recipe_data"),
            (b"2265", b"body_recipe_data"),
            (b"77", b"body_recipe_data"),
        ] {
            bytes.extend_from_slice(&u32::try_from(design_id.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(design_id);
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 12]);
            bytes.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(name);
        }
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &mut recipes,
        )
        .unwrap();
        let mut indexes = recipes
            .iter()
            .map(|recipe| {
                (
                    recipe.byte_offset,
                    recipe.kind,
                    recipe.design.as_ref().map(|design| design.id.value.clone()),
                    recipe.recipe_index,
                )
            })
            .collect::<Vec<_>>();
        indexes.sort_by_key(|(offset, ..)| *offset);
        assert_eq!(
            indexes
                .into_iter()
                .map(|(_, kind, design, index)| (kind, design, index))
                .collect::<Vec<_>>(),
            [
                (
                    crate::records::recipes::ConstructionRecipeKind::Body,
                    Some("2265".to_owned()),
                    0
                ),
                (
                    crate::records::recipes::ConstructionRecipeKind::Edge,
                    Some("2265".to_owned()),
                    0
                ),
                (
                    crate::records::recipes::ConstructionRecipeKind::Body,
                    Some("2265".to_owned()),
                    1
                ),
                (
                    crate::records::recipes::ConstructionRecipeKind::Body,
                    Some("77".to_owned()),
                    0
                ),
            ]
        );
    }

    #[test]
    fn bounded_face_record_identity_is_not_a_second_design_id() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            let mut prefix = [0u8; 27];
            prefix[11..15].copy_from_slice(&309i32.to_le_bytes());
            prefix[23..27].copy_from_slice(&24u32.to_le_bytes());
            bytes.extend_from_slice(&prefix);
            bytes.extend_from_slice(b"bounded_face_recipe_data");
            bytes.extend_from_slice(&(-1i64).to_le_bytes());
        }
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &mut recipes,
        )
        .unwrap();
        assert_eq!(recipes.len(), 2);
        assert!(recipes
            .iter()
            .all(|recipe| recipe.record_index.map(|index| index.value) == Some(309)));
        assert!(recipes.iter().all(|recipe| recipe.design.is_none()));
        assert_eq!(recipes[0].recipe_index, 0);
        assert_eq!(recipes[1].recipe_index, 1);

        let mut body = Vec::new();
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"2265");
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(
            &cadmpeg_test_support::service_decode_context(),
            &body,
            "Design/BulkStream.dat",
            &mut recipes,
        )
        .unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .map(|design| design.id.value.as_str()),
            Some("2265")
        );
        assert_eq!(
            recipes[0].design.as_ref().map(|design| design.id.offset),
            Some(4)
        );
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .and_then(|design| design.selector),
            Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 3,
                byte_offset: 8,
            })
        );
    }

    #[test]
    fn construction_recipe_design_id_refuses_retained_limit() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"2265");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        assert_refuses_at(
            ResourceDimension::RetainedBytes,
            "f3d construction recipe design ID",
            |ctx| {
                let mut recipes = Vec::new();
                super::decode_stream(ctx, &bytes, "Design/BulkStream.dat", &mut recipes)
            },
        );
    }
}
