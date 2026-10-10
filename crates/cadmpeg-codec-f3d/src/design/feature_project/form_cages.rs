// SPDX-License-Identifier: Apache-2.0
//! Parse Form cage references and bind committed neutral cages.

use crate::container::ContainerScan;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::{self, native_stream_charged};
use crate::layout::{
    form_class_325_cage_entry, form_class_325_cage_table, form_class_328_cage_group,
    form_class_328_metadata_group, form_class_328_reference_entry, form_class_328_scope,
    form_class_350_member_owner_tail, form_compact_one_cage_list, form_legacy_one_cage_owner,
    form_serializer_frame_132,
};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{bounded_len, DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

fn distinct_form_cage_ids<T: Eq + Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    ids: &[T],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "F3D form cage uniqueness")?;
    let mut distinct = HashSet::new();
    let mut source = ids.iter();
    while let Some(id) = ctx.next_charged(&mut source, "f3d form cage uniqueness source")? {
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut distinct, id, "f3d form cage uniqueness index")
        })? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn push_form_cage_id(
    ctx: &DecodeContext<'_>,
    ids: &mut Vec<cadmpeg_ir::ids::SubdId>,
    id: &cadmpeg_ir::ids::SubdId,
) -> Result<(), CodecError> {
    let copy = (id).try_clone_for_decode(ctx, "f3d form cage id")?;
    ctx.push_vec(ids, copy, "f3d form resolved cage")
}

/// Replace a resolved Form scope's native definition with its committed cages.
///
/// The Form's cage-list record owns ordered cage-object references. Each object
/// reaches a surface record, and the serializer naming that surface supplies
/// the archive entry identity of the neutral cage.
pub(crate) fn bind_form_cages(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    features: &mut [cadmpeg_ir::features::Feature],
    cages: &[cadmpeg_ir::subd::SubdSurface],
) -> Result<(), CodecError> {
    let mut streams_storage = ctx.reserve_scoped(0, "F3D form stream cache")?;
    let mut streams = HashMap::new();
    let mut cages_storage = ctx.reserve_scoped(0, "F3D form cage entry index")?;
    let mut cages_by_entry = HashMap::new();
    let mut remaining_cages = Some(cages);
    let mut cage_for_entry =
        |entry_name: &str| -> Result<Option<Option<&cadmpeg_ir::subd::SubdSurface>>, CodecError> {
            if let Some(cages) = remaining_cages.take() {
                for cage in ctx.admit_iter(cages, "f3d form cage entry source")? {
                    let Some(source) = &cage.source_object else {
                        continue;
                    };
                    let object = source.object_id.as_str();
                    let name = ctx
                        .rsplit_once(object, "/", "f3d form cage entry suffix")?
                        .map_or(object, |(_, name)| name);
                    if let Some(previous) = ctx.get_mut_hash_map(
                        &mut cages_by_entry,
                        name,
                        "f3d form cage entry duplicate",
                    )? {
                        *previous = None;
                    } else {
                        cages_storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut cages_by_entry,
                                name,
                                Some(cage),
                                "f3d form cage entry index",
                            )
                        })?;
                    }
                }
            }
            Ok(ctx
                .get_hash_map(&cages_by_entry, entry_name, "f3d form cage entry lookup")?
                .copied())
        };
    let mut features_storage = ctx.reserve_scoped(0, "F3D form feature index")?;
    let mut features_by_id = HashMap::new();
    let mut remaining_features = Some(features);
    let mut bind_resolved = |scope: &DesignParameterScope,
                             resolved: &[&cadmpeg_ir::ids::SubdId]|
     -> Result<(), CodecError> {
        if let Some(features) = remaining_features.take() {
            for feature in ctx.admit_iter(features, "f3d form feature source")? {
                if !ctx.contains_key_hash_map(
                    &features_by_id,
                    &feature.id,
                    "f3d form feature duplicate",
                )? {
                    features_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut features_by_id,
                            &feature.id,
                            &mut feature.evaluation,
                            "f3d form feature index",
                        )
                    })?;
                }
            }
        }
        let mut identity_storage = ctx.reserve_scoped(0, "F3D form feature identity")?;
        let feature_id = identity_storage
            .with_storage(|| crate::design::identity::neutral_feature_id(ctx, scope))?;
        let Some(evaluation) =
            ctx.get_mut_hash_map(&mut features_by_id, &feature_id, "f3d form feature lookup")?
        else {
            return Ok(());
        };
        if matches!(
            evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { .. }
            )
        ) {
            let mut ids = Vec::new();
            for id in ctx.admit_iter(resolved, "f3d form cage output source")? {
                push_form_cage_id(ctx, &mut ids, id)?;
            }
            evaluation.set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Form { cages: ids },
            ));
        }
        Ok(())
    };
    let mut one_form_scope = None;
    let mut has_one_form_scope = || -> Result<bool, CodecError> {
        if let Some(unique) = one_form_scope {
            return Ok(unique);
        }
        let mut candidates = scopes.iter();
        let first = ctx.find_by(
            candidates.by_ref(),
            |scope| Ok(scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form),
            "f3d form scope uniqueness",
        )?;
        let unique = first.is_some()
            && !ctx.any_by(
                candidates,
                |scope| Ok(scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form),
                "f3d form scope uniqueness",
            )?;
        one_form_scope = Some(unique);
        Ok(unique)
    };
    'scope: for scope in ctx.admit_iter(scopes, "f3d form scope scan")? {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Form {
            continue;
        }
        let Some(stream) = native_stream_charged(ctx, &scope.id, "f3d form cage native stream")? else {
            continue;
        };
        let Some(stream) = ctx.strip_prefix(stream, ids::SCHEME_PREFIX, "f3d form cage stream prefix")? else {
            continue;
        };
        if !ctx.contains_key_hash_map(&streams, stream, "f3d form stream cache query")? {
            let bytes = scan.entry_bytes(ctx, stream)?;
            let scoped_records = IndexedRecordOffsets::build_scoped(ctx, bytes)?;
            streams_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut streams,
                    stream,
                    FormStream {
                        bytes,
                        records: scoped_records.0,
                        serializers: None,
                        _storage: scoped_records.1,
                    },
                    "f3d form stream cache entry",
                )
            })?;
        }
        let stream_data = ctx
            .get_mut_hash_map(&mut streams, stream, "f3d form stream cache lookup")?
            .ok_or_else(|| CodecError::malformed("Form stream cache entry is absent"))?;
        let bytes = stream_data.bytes;
        let records = &stream_data.records;
        let mut parsed_storage = ctx.reserve_scoped(0, "F3D form scope parsing")?;
        let (cage_lists, cage_counts) = parsed_storage.with_storage(|| {
            form_cage_lists(
                ctx,
                bytes,
                records,
                scope.reference_members().values().copied(),
                scope.record_index,
            )
        })?;
        if scope.class_tag.as_str() == "325" {
            if let Some(cage_objects) = parsed_storage.with_storage(|| {
                form_class_325_cage_objects(
                    ctx,
                    bytes,
                    records,
                    scope.record_index,
                    scope.reference_members().values().copied(),
                )
            })? {
                if stream_data.serializers.is_none() {
                    stream_data.serializers = Some(form_cage_serializers(ctx, bytes, records)?);
                }
                let serializers = stream_data
                    .serializers
                    .as_ref()
                    .ok_or_else(|| CodecError::malformed("Form serializers are absent"))?;
                let mut resolved = Vec::new();
                let mut valid = true;
                for &object in &cage_objects {
                    let Some(surface) = form_class_325_cage_surface(ctx, bytes, records, object)?
                    else {
                        valid = false;
                        break;
                    };
                    let Some(entry_name) = serializers.entry_name(ctx, surface)? else {
                        valid = false;
                        break;
                    };
                    let cage = match cage_for_entry(entry_name)? {
                        Some(Some(cage)) => cage,
                        None => continue,
                        Some(None) => {
                            valid = false;
                            break;
                        }
                    };
                    parsed_storage.with_storage(|| {
                        ctx.push_vec(&mut resolved, &cage.id, "f3d form candidate cage")
                    })?;
                }
                if valid && !resolved.is_empty() && distinct_form_cage_ids(ctx, &resolved)? {
                    bind_resolved(scope, &resolved)?;
                }
                continue;
            }
        }
        if scope.class_tag.as_str() == "328"
            && has_one_form_scope()?
            && form_class_328_envelope(ctx, bytes, records, scope)?
        {
            if stream_data.serializers.is_none() {
                stream_data.serializers = Some(form_cage_serializers(ctx, bytes, records)?);
            }
            let serializers = stream_data
                .serializers
                .as_ref()
                .ok_or_else(|| CodecError::malformed("Form serializers are absent"))?;
            let mut resolved = Vec::new();
            let mut valid = true;
            let mut surface_source = serializers.ordered.iter();
            while let Some(surface) =
                ctx.next_charged(&mut surface_source, "f3d form serializer binding source")?
            {
                let Some(entry_name) = serializers.entry_name(ctx, *surface)? else {
                    valid = false;
                    break;
                };
                let cage = match cage_for_entry(entry_name)? {
                    Some(Some(cage)) => cage,
                    None => continue,
                    Some(None) => {
                        valid = false;
                        break;
                    }
                };
                parsed_storage.with_storage(|| {
                    ctx.push_vec(&mut resolved, &cage.id, "f3d form candidate cage")
                })?;
            }
            if valid
                && !resolved.is_empty()
                && resolved.len() == cages.len()
                && distinct_form_cage_ids(ctx, &resolved)?
            {
                bind_resolved(scope, &resolved)?;
            }
            continue;
        }
        if has_one_form_scope()? && cages.len() == 1 && cage_counts.as_slice() == [1] {
            bind_resolved(scope, &[&cages[0].id])?;
            continue;
        }
        let [cage_objects] = cage_lists.as_slice() else {
            continue;
        };
        let Some(surfaces) = parsed_storage.with_storage(|| {
            form_cage_surfaces(ctx, bytes, records, cage_objects, scope.record_index)
        })?
        else {
            continue;
        };
        if stream_data.serializers.is_none() {
            stream_data.serializers = Some(form_cage_serializers(ctx, bytes, records)?);
        }
        let serializers = stream_data
            .serializers
            .as_ref()
            .ok_or_else(|| CodecError::malformed("Form serializers are absent"))?;
        let mut resolved = Vec::new();
        let mut surface_source = surfaces.iter();
        while let Some(surface) =
            ctx.next_charged(&mut surface_source, "f3d form surface binding source")?
        {
            let Some(entry_name) = serializers.entry_name(ctx, *surface)? else {
                continue 'scope;
            };
            let Some(Some(cage)) = cage_for_entry(entry_name)? else {
                continue 'scope;
            };
            parsed_storage.with_storage(|| {
                ctx.push_vec(&mut resolved, &cage.id, "f3d form candidate cage")
            })?;
        }
        if !distinct_form_cage_ids(ctx, &resolved)? {
            continue;
        }
        bind_resolved(scope, &resolved)?;
    }
    Ok(())
}

struct FormStream<'bytes, 'ctx> {
    bytes: &'bytes [u8],
    records: IndexedRecordOffsets,
    serializers: Option<FormCageSerializers<'ctx>>,
    _storage: ScopedReservation<'ctx>,
}

/// Read the one-cage envelopes used by legacy Form record generations.
///
/// The compact envelope identifies the sole cage object directly. The older
/// owner envelope identifies one nested cage-object wrapper; its companion
/// record carries generation-specific scalar data and is not a cage count.
fn legacy_form_cage_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<usize>, CodecError> {
    (|| {
        if let Some(count) =
            legacy_form_owner_count(bytes, records, record_index, scope_record_index)
        {
            return Some(Ok(count));
        }
        let mut frames = records.offsets(record_index).windows(2);
        while let Some(frame) = match ctx.next_charged(&mut frames, "f3d legacy form frame source")
        {
            Ok(frame) => frame,
            Err(error) => return Some(Err(error)),
        } {
            let [start, paired] = [frame[0], frame[1]];
            if paired.checked_sub(start)? != form_compact_one_cage_list::LEN
                || bytes.get(
                    start + form_compact_one_cage_list::ZERO_RUN_10
                        ..start + form_compact_one_cage_list::OWNER_MARKER,
                )? != [0; 10]
                || bytes.get(start + form_compact_one_cage_list::OWNER_MARKER) != Some(&1)
                || View::u64_le_at(
                    bytes,
                    start + form_compact_one_cage_list::OWNER_SCOPE_RECORD_INDEX,
                )? != u64::from(scope_record_index)
                || bytes.get(
                    start + form_compact_one_cage_list::ZERO_RUN_2
                        ..start + form_compact_one_cage_list::CAGE_COUNT,
                )? != [0; 2]
            {
                continue;
            }
            let count = usize::try_from(View::u32_le_at(
                bytes,
                start + form_compact_one_cage_list::CAGE_COUNT,
            )?)
            .ok()?;
            if count != 1 {
                continue;
            }
            let member = start + form_compact_one_cage_list::MEMBER_MARKER;
            if bytes.get(member) != Some(&1)
                || bytes.get(
                    start + form_compact_one_cage_list::MEMBER_ZERO
                        ..start + form_compact_one_cage_list::MEMBER_FLAGS + 2,
                )? != [0, 0, 0xfc, 0]
            {
                continue;
            }
            let object = u32::try_from(View::u64_le_at(
                bytes,
                start + form_compact_one_cage_list::CAGE_OBJECT_RECORD_INDEX,
            )?)
            .ok()?;
            if records.offsets(object).is_empty() {
                continue;
            }
            return Some(Ok(count));
        }
        None
    })()
    .transpose()
}

fn legacy_form_owner_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<usize> {
    let (start, paired) = records.only_frame(record_index)?;
    let owner_class = bytes.get(start + 4..start + 7)?;
    let paired_class = bytes.get(paired + 4..paired + 7)?;
    let nested_class: &[u8] = if owner_class == b"335" && paired_class == b"262" {
        b"328"
    } else if owner_class == b"395" && paired_class == b"264" {
        b"329"
    } else if owner_class == b"448" && paired_class == b"258" {
        b"276"
    } else if owner_class == b"295" && paired_class == b"258" {
        b"274"
    } else {
        return None;
    };
    if paired.checked_sub(start)? != form_legacy_one_cage_owner::LEN
        || View::u64_le_at(bytes, start + 7)? != u64::from(record_index)
        || bytes.get(
            start + form_legacy_one_cage_owner::ZERO_RUN_14
                ..start + form_legacy_one_cage_owner::OWNER_MARKER,
        )? != [0; 14]
        || bytes.get(start + form_legacy_one_cage_owner::OWNER_MARKER) != Some(&1)
        || View::u64_le_at(
            bytes,
            start + form_legacy_one_cage_owner::OWNER_SCOPE_RECORD_INDEX,
        )? != u64::from(scope_record_index)
        || bytes.get(
            start + form_legacy_one_cage_owner::ZERO_RUN_24
                ..start + form_legacy_one_cage_owner::NESTED_MARKER,
        )? != [0; 24]
        || bytes.get(start + form_legacy_one_cage_owner::NESTED_MARKER) != Some(&1)
        || bytes.get(
            start + form_legacy_one_cage_owner::NESTED_ZERO_RUN
                ..start + form_legacy_one_cage_owner::OWNER_REPEAT_MARKER,
        )? != [0; 3]
        || bytes.get(start + form_legacy_one_cage_owner::OWNER_REPEAT_MARKER) != Some(&1)
        || View::u64_le_at(
            bytes,
            start + form_legacy_one_cage_owner::OWNER_REPEAT_SCOPE,
        )? != u64::from(scope_record_index)
        || bytes.get(
            start + form_legacy_one_cage_owner::TAIL_ZERO_RUN
                ..start + form_legacy_one_cage_owner::LEN,
        )? != [0; 2]
    {
        return None;
    }
    let nested_record = u32::try_from(View::u64_le_at(
        bytes,
        start + form_legacy_one_cage_owner::NESTED_RECORD_INDEX,
    )?)
    .ok()?;
    let [nested_at] = records.offsets(nested_record) else {
        return None;
    };
    (bytes.get(nested_at + 4..nested_at + 7) == Some(nested_class)).then_some(1)
}

fn form_class_328_envelope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<bool, CodecError> {
    let Some((scope_start, scope_paired)) = records.only_frame(scope.record_index) else {
        return Ok(false);
    };
    if scope_paired.checked_sub(scope_start) != Some(form_class_328_scope::LEN)
        || bytes.get(scope_start + 4..scope_start + 7) != Some(b"328")
        || bytes.get(scope_paired + 4..scope_paired + 7) != Some(b"267")
    {
        return Ok(false);
    }
    if scope.reference_members().len() != 2 {
        return Ok(false);
    }
    let mut group_record = None;
    for record_index in scope.reference_members().values().copied() {
        if first_frame_with_classes(ctx, bytes, records, record_index, (b"417", b"267"))?.is_some()
        {
            group_record = Some(record_index);
            break;
        }
    }
    let Some(group_record) = group_record else {
        return Ok(false);
    };
    let mut metadata_record = None;
    for record_index in scope.reference_members().values().copied() {
        if record_index != group_record
            && first_frame_with_classes(ctx, bytes, records, record_index, (b"341", b"267"))?
                .is_some()
        {
            metadata_record = Some(record_index);
            break;
        }
    }
    let Some(metadata_record) = metadata_record else {
        return Ok(false);
    };
    let Some((group_start, group_paired)) =
        first_frame_with_classes(ctx, bytes, records, group_record, (b"417", b"267"))?
    else {
        return Ok(false);
    };
    if group_paired.checked_sub(group_start) != Some(form_class_328_cage_group::LEN)
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14
                ..group_start + form_class_328_cage_group::OWNER_MARKER,
        ) != Some(&[0; 14])
        || bytes.get(group_start + form_class_328_cage_group::OWNER_MARKER)
            != Some(&form_class_328_cage_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14_AFTER_OWNER
                ..group_start + form_class_328_cage_group::MEMBER_COUNT,
        ) != Some(&[0; 14])
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::MEMBER_COUNT)
            != Some(form_class_328_cage_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::TERMINAL_U32)
            != Some(form_class_328_cage_group::TERMINAL_U32_VALUE)
        || bytes.get(group_start + form_class_328_cage_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_cage_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::FIRST_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(group_start + form_class_328_cage_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_cage_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::SECOND_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_cage_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::PAIRED_ZERO_TAIL
                ..group_start + form_class_328_cage_group::LEN,
        ) != Some(&[0; 2])
    {
        return Ok(false);
    }
    let Some(group_class_272) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return Ok(false);
    };
    let Some(group_class_404) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return Ok(false);
    };
    if !unique_record_has_class(bytes, records, group_class_272, b"272")
        || !unique_record_has_class(bytes, records, group_class_404, b"404")
    {
        return Ok(false);
    }
    let mut members = [None; 4];
    for ordinal in 0..members.len() {
        let at = group_start
            + form_class_328_cage_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return Ok(false);
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return Ok(false);
        };
        if members[..ordinal].contains(&Some(member)) {
            return Ok(false);
        }
        members[ordinal] = Some(member);
        let Some((member_start, member_paired)) = records.only_frame(member) else {
            return Ok(false);
        };
        if bytes.get(member_start + 4..member_start + 7) != Some(b"350")
            || bytes.get(member_paired + 4..member_paired + 7) != Some(b"351")
            || member_paired < member_start + form_class_350_member_owner_tail::LEN
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_MARKER,
            ) != Some(&form_class_350_member_owner_tail::OWNER_MARKER_VALUE)
            || View::u64_le_at(
                bytes,
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_GROUP_RECORD_INDEX,
            ) != Some(u64::from(group_record))
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::ZERO_RUN_3
                    ..member_paired - form_class_350_member_owner_tail::LEN
                        + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&[0; 3])
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&form_class_350_member_owner_tail::PAIRED_MARKER_VALUE)
        {
            return Ok(false);
        }
    }
    let Some((metadata_start, metadata_paired)) =
        first_frame_with_classes(ctx, bytes, records, metadata_record, (b"341", b"267"))?
    else {
        return Ok(false);
    };
    if metadata_paired.checked_sub(metadata_start) != Some(form_class_328_metadata_group::LEN)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_10
                ..metadata_start + form_class_328_metadata_group::OWNER_MARKER,
        ) != Some(&[0; 10])
        || bytes.get(metadata_start + form_class_328_metadata_group::OWNER_MARKER)
            != Some(&form_class_328_metadata_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_2
                ..metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(&[0; 2])
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(form_class_328_metadata_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_U32,
        ) != Some(form_class_328_metadata_group::TAIL_U32_VALUE)
        || View::f64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_SCALAR,
        )
        .is_none_or(|value| !value.is_finite())
        || bytes.get(metadata_start + form_class_328_metadata_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::FIRST_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::SECOND_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_metadata_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::PAIRED_ZERO_TAIL
                ..metadata_start + form_class_328_metadata_group::LEN,
        ) != Some(&[0; 2])
    {
        return Ok(false);
    }
    let Some(metadata_class_259) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return Ok(false);
    };
    let Some(metadata_class_404) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return Ok(false);
    };
    if !unique_record_has_class(bytes, records, metadata_class_259, b"259")
        || !unique_record_has_class(bytes, records, metadata_class_404, b"404")
    {
        return Ok(false);
    }
    let mut metadata_members = [None; 19];
    for ordinal in 0..metadata_members.len() {
        let at = metadata_start
            + form_class_328_metadata_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return Ok(false);
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return Ok(false);
        };
        if metadata_members[..ordinal].contains(&Some(member))
            || records.offsets(member).len() != 1
            || records
                .offsets(member)
                .first()
                .and_then(|offset| bytes.get(*offset + 4..*offset + 7))
                != Some(b"320")
        {
            return Ok(false);
        }
        metadata_members[ordinal] = Some(member);
    }
    Ok(true)
}

fn unique_record_has_class(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class: &[u8],
) -> bool {
    let [offset] = records.offsets(record_index) else {
        return false;
    };
    bytes.get(*offset + 4..*offset + 7) == Some(class)
}

fn first_frame_with_classes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    classes: (&[u8; 3], &[u8; 3]),
) -> Result<Option<(usize, usize)>, CodecError> {
    ctx.find_map(
        records.offsets(record_index).windows(2),
        |frame| {
            Ok((bytes.get(frame[0] + 4..frame[0] + 7) == Some(classes.0)
                && bytes.get(frame[1] + 4..frame[1] + 7) == Some(classes.1))
            .then_some((frame[0], frame[1])))
        },
        "f3d form frame class source",
    )
}

fn unique_indexed_frame_by(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    record_index: u32,
    matches: impl Fn(usize, usize) -> bool,
) -> Result<Option<(usize, usize)>, CodecError> {
    let offsets = records.offsets(record_index);
    let Some(first) = ctx.position_by(
        offsets.windows(2),
        |frame| Ok(matches(frame[0], frame[1])),
        "f3d form frame first source",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        offsets[first + 1..].windows(2),
        |frame| Ok(matches(frame[0], frame[1])),
        "f3d form frame unique suffix",
    )? {
        return Ok(None);
    }
    Ok(Some((offsets[first], offsets[first + 1])))
}

fn form_class_325_cage_objects(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner_record_indices: impl Iterator<Item = u32>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let parsed = (|| {
        const CAGE_COUNT: usize = 32;
        const TYPE_DISCRIMINATOR_FIRST: u32 = 307;

        let (start, paired) = records.only_frame(scope_record_index)?;
        if bytes.get(start + 4..start + 7) != Some(b"325")
            || bytes.get(paired + 4..paired + 7) != Some(b"258")
            || paired.checked_sub(start)? != form_class_325_cage_table::LEN
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_9
                    ..start + form_class_325_cage_table::LIST_MARKER,
            )? != [0; 9]
            || bytes.get(start + form_class_325_cage_table::LIST_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_5
                    ..start + form_class_325_cage_table::OWNER_MARKER,
            )? != [0; 5]
            || bytes.get(start + form_class_325_cage_table::OWNER_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_2
                    ..start + form_class_325_cage_table::CAGE_COUNT,
            )? != [0; 2]
        {
            return None;
        }
        let owner_record = u32::try_from(View::u64_le_at(
            bytes,
            start + form_class_325_cage_table::OWNER_RESULT_RECORD_INDEX,
        )?)
        .ok()?;
        let [owner_at, ..] = records.offsets(owner_record) else {
            return None;
        };
        if !(match ctx.any_by(
            owner_record_indices,
            |index| Ok(index == owner_record),
            "f3d form cage owner source",
        ) {
            Ok(found) => found,
            Err(error) => return Some(Err(error)),
        }) || bytes.get(*owner_at + 4..*owner_at + 7) != Some(b"407")
        {
            return None;
        }
        let count = bounded_len(
            u64::from(View::u32_le_at(
                bytes,
                start + form_class_325_cage_table::CAGE_COUNT,
            )?),
            form_class_325_cage_entry::LEN,
            paired.checked_sub(start + form_class_325_cage_table::CAGE_ENTRIES)?,
        )?;
        if count != CAGE_COUNT {
            return None;
        }
        let mut objects = Vec::new();
        let mut seen_type_discriminators = [false; CAGE_COUNT];
        for ordinal in 0..count {
            let entry = start
                .checked_add(form_class_325_cage_table::CAGE_ENTRIES)?
                .checked_add(form_class_325_cage_entry::LEN.checked_mul(ordinal)?)?;
            if bytes.get(entry + form_class_325_cage_entry::CAGE_OBJECT_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::CAGE_OBJECT_ZERO
                        ..entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
                )? != [0, 0]
                || bytes.get(entry + form_class_325_cage_entry::COMPANION_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::COMPANION_ZERO
                        ..entry + form_class_325_cage_entry::LEN,
                )? != [0, 0]
            {
                return None;
            }
            let type_discriminator = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
            )?)
            .ok()?;
            let type_slot =
                usize::try_from(type_discriminator.checked_sub(TYPE_DISCRIMINATOR_FIRST)?).ok()?;
            if type_slot >= CAGE_COUNT || seen_type_discriminators[type_slot] {
                return None;
            }
            seen_type_discriminators[type_slot] = true;
            let object = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::CAGE_OBJECT_RECORD_INDEX,
            )?)
            .ok()?;
            let companion = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::COMPANION_RECORD_INDEX,
            )?)
            .ok()?;
            let (object_at, _) = match unique_indexed_frame_by(ctx, records, object, |_, paired| {
                bytes.get(paired + 4..paired + 7) == Some(b"258")
            }) {
                Ok(value) => value?,
                Err(error) => return Some(Err(error)),
            };
            let [companion_at, ..] = records.offsets(companion) else {
                return None;
            };
            if bytes.get(object_at + 4..object_at + 7) != Some(b"289")
                || bytes.get(*companion_at + 4..*companion_at + 7) != Some(b"273")
            {
                return None;
            }
            if let Err(error) =
                ctx.push_vec(&mut objects, object, "collect F3D class 325 cage objects")
            {
                return Some(Err(error));
            }
        }
        Some(Ok(objects))
    })();
    parsed.transpose()
}

fn form_class_325_cage_surface(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
) -> Result<Option<u32>, CodecError> {
    (|| {
        let (start, paired) =
            match unique_indexed_frame_by(ctx, records, object_record, |_, paired| {
                bytes.get(paired + 4..paired + 7) == Some(b"258")
            }) {
                Ok(value) => value?,
                Err(error) => return Some(Err(error)),
            };
        if bytes.get(start + 4..start + 7) != Some(b"289") {
            return None;
        }
        let mut surface = None;
        let mut source = start.checked_add(11)?..paired;
        while let Some(at) =
            match ctx.next_charged(&mut source, "f3d form cage surface reference source") {
                Ok(at) => at,
                Err(error) => return Some(Err(error)),
            }
        {
            if bytes.get(at) != Some(&1) {
                continue;
            }
            let target = View::u32_le_at(bytes, at + 1)?;
            let [target_at] = records.offsets(target) else {
                continue;
            };
            if bytes.get(target_at + 4..target_at + 7) == Some(b"310")
                && surface.replace(target).is_some()
            {
                return None;
            }
        }
        (surface).map(Ok)
    })()
    .transpose()
}

fn form_cage_objects(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let parsed = (|| {
        let (offset, paired) =
            match unique_indexed_frame_by(ctx, records, record_index, |_, paired| {
                matches!(bytes.get(paired + 4..paired + 7), Some(b"258" | b"264"))
            }) {
                Ok(value) => value?,
                Err(error) => return Some(Err(error)),
            };
        if View::u64_le_at(bytes, offset + 7)? != u64::from(record_index)
            || bytes.get(offset + 15..offset + 21)? != [0; 6]
            || bytes.get(offset + 21) != Some(&1)
            || View::u64_le_at(bytes, offset + 22)? != u64::from(scope_record_index)
            || bytes.get(offset + 30..offset + 32)? != [0, 0]
        {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(bytes, offset + 32)?).ok()?;
        if paired.checked_sub(offset)? != 88usize.checked_add(11usize.checked_mul(count)?)? {
            return None;
        }
        Some(Ok((count, offset.checked_add(36)?)))
    })()
    .transpose()?;
    let Some((count, mut cursor)) = parsed else {
        return Ok(None);
    };

    let mut objects = Vec::new();
    let mut source = 0..count;
    while ctx
        .next_charged(&mut source, "f3d form cage object source")?
        .is_some()
    {
        if bytes.get(cursor) != Some(&1) {
            return Ok(None);
        }
        let Some(object) =
            View::u64_le_at(bytes, cursor + 1).and_then(|value| u32::try_from(value).ok())
        else {
            return Ok(None);
        };
        if bytes.get(cursor + 9..cursor + 11) != Some(&[0, 0][..]) {
            return Ok(None);
        }
        let Some(next) = cursor.checked_add(11) else {
            return Ok(None);
        };
        ctx.push_vec(&mut objects, object, "f3d form cage object")?;
        cursor = next;
    }
    Ok(Some(objects))
}

fn form_cage_lists(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    mut references: impl Iterator<Item = u32>,
    scope_record_index: u32,
) -> Result<(Vec<Vec<u32>>, Vec<usize>), CodecError> {
    let mut lists = Vec::new();
    let mut counts = Vec::new();
    while let Some(record_index) =
        ctx.next_charged(&mut references, "f3d form cage list reference source")?
    {
        let objects = form_cage_objects(ctx, bytes, records, record_index, scope_record_index)?;
        let count = match objects.as_ref() {
            Some(objects) => Some(objects.len()),
            None => legacy_form_cage_count(ctx, bytes, records, record_index, scope_record_index)?,
        };
        if let Some(objects) = objects {
            ctx.push_vec(&mut lists, objects, "f3d form cage list")?;
        }
        if let Some(count) = count {
            ctx.push_vec(&mut counts, count, "f3d form cage count")?;
        }
    }
    Ok((lists, counts))
}

fn next_form_header(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    bytes: &[u8],
    start: usize,
) -> Result<Option<usize>, CodecError> {
    Ok(records
        .headers_in(ctx, start, bytes.len())?
        .first()
        .copied())
}

fn form_cage_surface(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
    scope_record: u32,
) -> Result<Option<u32>, CodecError> {
    (|| {
        let [object_at] = records.offsets(object_record) else {
            return None;
        };
        if bytes.get(object_at + 4..object_at + 7) != Some(b"301")
            || (match next_form_header(ctx, records, bytes, object_at + 1) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            })? != object_at + 200
            || bytes.get(object_at + 189) != Some(&1)
        {
            return None;
        }
        let first_wrapper = u32::try_from(View::u64_le_at(bytes, object_at + 190)?).ok()?;
        let [first_at] = records.offsets(first_wrapper) else {
            return None;
        };
        if bytes.get(first_at + 4..first_at + 7) != Some(b"373")
            || (match next_form_header(ctx, records, bytes, first_at + 1) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            })? != first_at + 33
            || bytes.get(first_at + 11..first_at + 21)? != [0; 10]
            || bytes.get(first_at + 21) != Some(&1)
            || bytes.get(first_at + 30..first_at + 33)? != [0; 3]
        {
            return None;
        }
        let second_wrapper = u32::try_from(View::u64_le_at(bytes, first_at + 22)?).ok()?;
        let [second_at] = records.offsets(second_wrapper) else {
            return None;
        };
        if bytes.get(second_at + 4..second_at + 7) != Some(b"362")
            || (match next_form_header(ctx, records, bytes, second_at + 1) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            })? != second_at + 29
            || bytes.get(second_at + 11..second_at + 21)? != [0; 10]
        {
            return None;
        }
        let carrier = u32::try_from(View::u64_le_at(bytes, second_at + 21)?).ok()?;
        let (carrier_at, carrier_paired) = records.only_frame(carrier)?;
        if carrier_paired.checked_sub(carrier_at)? != 665
            || bytes.get(carrier_at + 4..carrier_at + 7) != Some(b"457")
            || bytes.get(carrier_paired + 4..carrier_paired + 7) != Some(b"264")
            || bytes.get(carrier_at + 317) != Some(&1)
            || View::u64_le_at(bytes, carrier_at + 318)? != u64::from(scope_record)
            || bytes.get(carrier_at + 339) != Some(&1)
        {
            return None;
        }
        (u32::try_from(View::u64_le_at(bytes, carrier_at + 340)?).ok()).map(Ok)
    })()
    .transpose()
}

fn form_cage_surfaces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    objects: &[u32],
    scope_record_index: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut surfaces = Vec::new();
    let mut source = objects.iter();
    while let Some(object) = ctx.next_charged(&mut source, "f3d form cage surface object source")? {
        let Some(surface) = form_cage_surface(ctx, bytes, records, *object, scope_record_index)?
        else {
            return Ok(None);
        };
        ctx.push_vec(&mut surfaces, surface, "f3d form cage surface")?;
    }
    Ok(Some(surfaces))
}

struct FormCageSerializers<'ctx> {
    ordered: Vec<u32>,
    entries: HashMap<u32, FormCageEntry<'ctx>>,
    _storage: ScopedReservation<'ctx>,
}

enum FormCageEntry<'ctx> {
    Unique {
        name: String,
        _storage: ScopedReservation<'ctx>,
    },
    Duplicate,
}

impl FormCageSerializers<'_> {
    fn entry_name(
        &self,
        ctx: &DecodeContext<'_>,
        surface: u32,
    ) -> Result<Option<&str>, CodecError> {
        Ok(
            match ctx.get_hash_map(&self.entries, &surface, "f3d form serializer entry lookup")? {
                Some(FormCageEntry::Unique { name, .. }) => Some(name),
                Some(FormCageEntry::Duplicate) | None => None,
            },
        )
    }
}

fn form_cage_serializers<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Result<FormCageSerializers<'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "F3D form serializers")?;
    let mut ordered = Vec::new();
    let mut entries = HashMap::new();
    for &offset in ctx.admit_iter(
        records.headers_in(ctx, 0, bytes.len())?,
        "f3d form serializer source",
    )? {
        let is_class_335 = bytes.get(offset + 4..offset + 7) == Some(b"335");
        if !matches!(
            bytes.get(offset + 4..offset + 7),
            Some(b"315" | b"335" | b"349" | b"360" | b"431" | b"446")
        ) || bytes.get(
            offset + form_serializer_frame_132::ZERO_RUN_10
                ..offset + form_serializer_frame_132::ENTRY_NAME_LENGTH,
        ) != Some(&[0; 10])
        {
            continue;
        }
        let Some(next) = next_form_header(ctx, records, bytes, offset + 1)? else {
            continue;
        };
        if next != offset + form_serializer_frame_132::LEN
            || (is_class_335 && bytes.get(next + 4..next + 7) != Some(b"331"))
        {
            continue;
        }
        let name_at = offset + form_serializer_frame_132::ENTRY_NAME_LENGTH;
        let Some(count) = View::u32_le_at(bytes, name_at)
            .map(u64::from)
            .filter(|count| (1..=256).contains(count))
        else {
            continue;
        };
        let name_units = usize::try_from(count).map_err(|_| {
            ctx.refuse_codec_limit("f3d form serializer name materialization", 0, 1)
        })?;
        let Some(after_name) = name_at.checked_add(4).and_then(|start| {
            name_units
                .checked_mul(2)
                .and_then(|length| start.checked_add(length))
        }) else {
            continue;
        };
        if after_name + 11 > offset + form_serializer_frame_132::LEN
            || (!is_class_335 && after_name + 11 != offset + form_serializer_frame_132::LEN)
        {
            continue;
        }
        let Some(raw_name) = bytes.get(name_at + 4..after_name) else {
            continue;
        };
        let parsed_name = match ctx.utf16le_scoped_text(
            raw_name,
            name_units,
            false,
            "f3d form serializer name materialization",
        ) {
            Ok(text) => text,
            Err(CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        let entry_name = &parsed_name.0;
        if !entry_name.starts_with("TSpline.")
            || !std::path::Path::new(entry_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("tsm"))
            || bytes.get(after_name) != Some(&1)
            || bytes.get(after_name + 9..after_name + 11) != Some(&[0, 0])
        {
            continue;
        }
        let Some(surface) =
            View::u64_le_at(bytes, after_name + 1).and_then(|surface| u32::try_from(surface).ok())
        else {
            continue;
        };
        if is_class_335
            && (bytes
                .get(after_name + 11..offset + form_serializer_frame_132::LEN)
                .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
                || !ctx.any_by(
                    records.offsets(surface),
                    |surface_offset| {
                        Ok(bytes.get(*surface_offset + 4..*surface_offset + 7) == Some(b"358"))
                    },
                    "f3d form serializer surface source",
                )?)
        {
            continue;
        }
        if let Some(entry) = ctx.get_mut_hash_map(
            &mut entries,
            &surface,
            "f3d form serializer entry duplicate",
        )? {
            *entry = FormCageEntry::Duplicate;
        } else {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut entries,
                    surface,
                    FormCageEntry::Unique {
                        name: parsed_name.0,
                        _storage: parsed_name.1,
                    },
                    "f3d form serializer entry index",
                )
            })?;
            storage.with_storage(|| {
                ctx.push_vec(&mut ordered, surface, "f3d form serializer order")
            })?;
        }
    }
    Ok(FormCageSerializers {
        ordered,
        entries,
        _storage: storage,
    })
}

#[cfg(test)]
mod tests;
