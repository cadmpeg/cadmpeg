// SPDX-License-Identifier: Apache-2.0
//! Historical record revision binding, archive materialization and history framing.

use super::historical_topology_with_tags;
use super::topology::topology_entity_slots;
use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistoryRecord,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::u64_from_index;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(super) fn bind_snapshot_revision_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut old_references_storage = ctx.reserve_scoped(0, "collect F3D ASM old references")?;
    let mut old_references = Vec::new();
    for state in ctx.admit_iter(&*states, "scan F3D snapshot reference states")? {
        for board in ctx.admit_iter(&state.bulletin_boards, "scan F3D snapshot reference boards")? {
            for change in ctx.admit_iter(&board.changes, "scan F3D snapshot reference changes")? {
                let Some(old_reference) = change.old_ref() else {
                    continue;
                };
                ctx.push_scoped_vec(
                    &mut old_references_storage,
                    &mut old_references,
                    old_reference,
                    "collect F3D ASM old references",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut old_references,
        |value| value,
        Ord::cmp,
        "sort F3D ASM old references",
    )?;
    let Some(&first) = old_references.first() else {
        return Ok(());
    };
    if !is_consecutive_from(ctx, &old_references, first, "check F3D ASM old references")? {
        return Ok(());
    }
    let mut snapshot_record_count = 0usize;
    for state in ctx.admit_iter(&*states, "scan F3D snapshot record states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D snapshot records")? {
            if record.name() != "End-of-ASM-data" {
                snapshot_record_count = snapshot_record_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count F3D snapshot records",
                        u64_from_index(usize::MAX - 1),
                        u64_from_index(snapshot_record_count),
                    )
                })?;
            }
        }
    }
    if snapshot_record_count != old_references.len() {
        return Ok(());
    }
    let mut revision_ids = ctx.admit_iter(old_references, "bind F3D snapshot revision IDs")?;
    for state in ctx.admit_iter(&mut *states, "bind F3D snapshot revision states")? {
        for record in ctx.admit_iter(&mut state.records, "bind F3D snapshot revision records")? {
            if record.name() != "End-of-ASM-data" {
                record.revision_id = revision_ids.next();
            }
        }
    }
    Ok(())
}

/// Tests that `values` are exactly `first..first + values.len()`, an end that
/// must be representable, charging only the values compared before the first
/// gap.
fn is_consecutive_from(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[i64],
    first: i64,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let end = i64::try_from(values.len())
        .ok()
        .and_then(|count| first.checked_add(count));
    if end.is_none() {
        return Ok(false);
    }
    ctx.all_by(
        values.iter().zip(0_i64..),
        |(&value, offset)| Ok(first.checked_add(offset) == Some(value)),
        operation,
    )
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

/// Collects every archived record revision ID in ascending order, held for
/// as long as the caller keeps the reservation.
fn archived_revision_ids<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    operation: &'static str,
) -> Result<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut archived = Vec::new();
    for state in ctx.admit_iter(states, "scan F3D archived history states")? {
        for record in ctx.admit_iter(&state.records, "scan F3D archived state records")? {
            let Some(revision_id) = record.revision_id else {
                continue;
            };
            ctx.push_scoped_vec(&mut storage, &mut archived, revision_id, operation)?;
        }
    }
    ctx.sort_unstable_by(
        &mut archived,
        |value| value,
        Ord::cmp,
        "sort F3D archived revisions",
    )?;
    Ok((archived, storage))
}

/// Returns the active `RecordTable` length named by sorted archived revision
/// IDs, which must be exactly the slots after the active records.
fn archived_active_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    archived: &[i64],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    if active_count <= 0
        || !is_consecutive_from(ctx, archived, active_count, "check F3D archived revisions")?
    {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

fn archived_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let (archived, _storage) =
        archived_revision_ids(ctx, states, "collect F3D archived revisions")?;
    archived_active_count(ctx, &archived)
}

/// Return the active `RecordTable` length for a history that has no archived
/// snapshot. Insert-only chains use the active records themselves as every
/// revision, so their bulletin-board references must cover every non-header
/// slot exactly once.
fn insert_only_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut has_boundary_record = false;
    let only_boundary_records = ctx.all_by(
        states,
        |state| {
            ctx.all_by(
                &state.records,
                |record| {
                    has_boundary_record = true;
                    Ok(record.revision_id.is_none() && is_history_boundary_record(record))
                },
                "scan F3D insert-only state records",
            )
        },
        "scan F3D insert-only history states",
    )?;
    if !only_boundary_records || !has_boundary_record {
        return Ok(None);
    }
    let mut inserted_storage = ctx.reserve_scoped(0, "index F3D insert-only revisions")?;
    let mut inserted = BTreeSet::new();
    let mut last = 0;
    let mut history_source = IntoIterator::into_iter(states);
    while let Some(state) =
        ctx.next_charged(&mut history_source, "scan F3D insert-only history states")?
    {
        let mut history_source = IntoIterator::into_iter(&state.bulletin_boards);
        while let Some(board) =
            ctx.next_charged(&mut history_source, "scan F3D insert-only bulletin boards")?
        {
            let mut history_source = IntoIterator::into_iter(&board.changes);
            while let Some(change) =
                ctx.next_charged(&mut history_source, "scan F3D insert-only board changes")?
            {
                let Some(new_ref) = change.new_ref().filter(|_| change.old_ref().is_none()) else {
                    return Ok(None);
                };
                if new_ref <= 0
                    || !ctx.insert_scoped_btree_set(
                        &mut inserted_storage,
                        &mut inserted,
                        new_ref,
                        "find F3D insert-only revision",
                        "index F3D insert-only revisions",
                    )?
                {
                    return Ok(None);
                }
                last = last.max(new_ref);
            }
        }
    }
    // Distinct positive references cover `1..=last` exactly when there are
    // `last` of them.
    if inserted.is_empty() {
        return Ok(None);
    }
    if usize::try_from(last).ok() != Some(inserted.len()) {
        return Ok(None);
    }
    Ok(last
        .checked_add(1)
        .and_then(|count| usize::try_from(count).ok()))
}

pub(super) fn bind_historical_entity_versions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let (archived_ids, _archived_ids_storage) =
        archived_revision_ids(ctx, states, "index F3D archived revision IDs")?;
    let active_count = match archived_active_count(ctx, &archived_ids)? {
        Some(count) => Some(count),
        None => insert_only_active_record_count(ctx, states)?,
    }
    .and_then(|count| i64::try_from(count).ok());
    let Some(active_count) = active_count else {
        return Ok(());
    };
    let mut by_node_storage = ctx.reserve_scoped(0, "index F3D history node ordinals")?;
    let mut by_node = HashMap::new();
    for (ordinal, state) in ctx
        .admit_iter(&*states, "scan F3D history node ordinals")?
        .enumerate()
    {
        by_node_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_node,
                state.node_index,
                ordinal,
                "index F3D history node ordinals",
            )
            .map(|_| ())
        })?;
    }
    if by_node.len() != states.len() {
        return Ok(());
    }
    let is_head =
        |state: &AsmDeltaState| Ok::<_, cadmpeg_core::CodecError>(state.previous_ref.is_none());
    let Some(mut ordinal) = ctx.position_by(&*states, is_head, "find F3D history version head")?
    else {
        return Ok(());
    };
    if ctx.any_by(
        states.get(ordinal + 1..).unwrap_or_default(),
        is_head,
        "find F3D history version head",
    )? {
        return Ok(());
    }
    let mut versions_storage = ctx.reserve_scoped(0, "seed F3D history versions")?;
    let mut versions = BTreeMap::new();
    versions_storage.with_storage(|| {
        for id in ctx.admit_iter(0..active_count.cast_unsigned(), "seed F3D history versions")? {
            let id = id.cast_signed();
            ctx.insert_btree_map(&mut versions, id, id, "seed F3D history versions")?;
        }
        Ok::<(), cadmpeg_core::CodecError>(())
    })?;
    // The projected vectors become the states' retained versions when the
    // whole chain binds; the index over them is scratch.
    let mut state_versions_storage = ctx.reserve_scoped(0, "materialize F3D state versions")?;
    let mut projected_storage = ctx.reserve_scoped(0, "index F3D state version projections")?;
    let mut projected = HashMap::new();
    loop {
        ctx.charge_work(1, "bind F3D historical entity versions")?;
        let state = &states[ordinal];
        // Every visited state is projected before the walk moves on, so a
        // projected state is a revisited one.
        if ctx.contains_key_hash_map(&projected, &state.node_index, "find F3D history projected")? {
            return Ok(());
        }
        let state_versions = state_versions_storage.with_storage(|| {
            let mut state_versions = Vec::new();
            for (&entity_ref, &record_ref) in ctx.admit_iter(&versions, "scan F3D versions")? {
                ctx.push_vec(
                    &mut state_versions,
                    AsmEntityVersion {
                        entity_ref,
                        record_ref,
                    },
                    "materialize F3D state versions",
                )?;
            }
            Ok::<_, cadmpeg_core::CodecError>(state_versions)
        })?;
        projected_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut projected,
                state.node_index,
                state_versions,
                "index F3D state version projections",
            )
            .map(|_| ())
        })?;
        let mut history_source = IntoIterator::into_iter(&state.bulletin_boards);
        while let Some(board) = ctx.next_charged(
            &mut history_source,
            "scan F3D historical version bulletin boards",
        )? {
            let mut history_source = IntoIterator::into_iter(&board.changes);
            while let Some(change) =
                ctx.next_charged(&mut history_source, "scan F3D historical version changes")?
            {
                match change.kind {
                    AsmEntityChangeKind::Update { old, new } => {
                        if !ctx.contains_key_btree_map(
                            &versions,
                            &new,
                            "find F3D updated historical version",
                        )? || ctx
                            .binary_search(
                                archived_ids.as_slice(),
                                &old,
                                "find archived F3D revision for update",
                            )?
                            .is_err()
                        {
                            return Ok(());
                        }
                        versions_storage.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut versions,
                                new,
                                old,
                                "update F3D historical version",
                            )
                        })?;
                    }
                    AsmEntityChangeKind::Insert { new } => {
                        if ctx
                            .remove_btree_map(
                                &mut versions,
                                &new,
                                "remove F3D inserted historical version",
                            )?
                            .is_none()
                        {
                            return Ok(());
                        }
                    }
                    AsmEntityChangeKind::Delete { old } => {
                        if ctx.contains_key_btree_map(
                            &versions,
                            &old,
                            "find F3D deleted historical version",
                        )? || ctx
                            .binary_search(
                                archived_ids.as_slice(),
                                &old,
                                "find archived F3D revision for delete",
                            )?
                            .is_err()
                        {
                            return Ok(());
                        }
                        versions_storage.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut versions,
                                old,
                                old,
                                "restore F3D historical version",
                            )
                        })?;
                    }
                }
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = ctx.get_hash_map(&by_node, &next, "find F3D history by node")?
        else {
            return Ok(());
        };
        ordinal = next_ordinal;
    }
    if projected.len() != states.len()
        || versions.len() != 1
        || ctx.get_btree_map(&versions, &0, "find F3D root historical version")? != Some(&0)
    {
        return Ok(());
    }
    state_versions_storage.commit()?;
    for state in ctx.admit_iter(&mut *states, "bind F3D historical version states")? {
        state.entity_versions = ctx
            .remove_hash_map(
                &mut projected,
                &state.node_index,
                "find F3D history projected",
            )?
            .unwrap_or_default();
    }
    Ok(())
}

pub(super) fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return Ok(());
    };
    // An archive and an insert-only chain state their active count before
    // the active stream needs framing.
    let (active_count, insert_only) = match archived_active_record_count(ctx, states)? {
        Some(count) => (count, false),
        None => match insert_only_active_record_count(ctx, states)? {
            Some(count) => (count, true),
            None => return Ok(()),
        },
    };
    let active_limit =
        cadmpeg_asm::asm_header::solved_record_limit(ctx, bytes)?.unwrap_or(bytes.len());
    let (framed, _framed_storage) =
        ctx.with_scoped_storage("frame F3D active history records", || {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_asm::sab::frame(
                ctx,
                bytes,
                start,
                active_limit,
                width,
                None,
            ))
        })?;
    let framed = match framed {
        Ok(records) => records,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
            return Err(cadmpeg_core::CodecError::ResourceLimit(error))
        }
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        Err(_) => return Ok(()),
    };
    if insert_only && framed.len() != active_count {
        return Ok(());
    }
    let Some(active_records) = framed.get(..active_count) else {
        return Ok(());
    };
    let mut archived_frames_storage = ctx.reserve_scoped(0, "retain F3D archived record frame")?;
    let mut archived_frames = BTreeMap::new();
    let mut history_source = IntoIterator::into_iter(&*states);
    while let Some(state) =
        ctx.next_charged(&mut history_source, "scan F3D complete record states")?
    {
        let mut history_source = IntoIterator::into_iter(&state.records);
        while let Some(record) =
            ctx.next_charged(&mut history_source, "scan F3D complete state records")?
        {
            if record.revision_id.is_none() {
                continue;
            }
            let Some(revision_id) = record.revision_id else {
                return Ok(());
            };
            let Some(offset) = usize::try_from(record.byte_offset).ok() else {
                return Ok(());
            };
            let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
                return Ok(());
            };
            let Some(record_bytes) = bytes.get(offset..limit) else {
                return Ok(());
            };
            if !ctx.equal(
                record_bytes,
                record.raw_bytes.as_slice(),
                "compare F3D archived record bytes",
            )? {
                return Ok(());
            }
            let framed = archived_frames_storage.with_storage(|| {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_asm::sab::frame(
                    ctx, bytes, offset, limit, width, None,
                ))
            })?;
            let mut framed = match framed {
                Ok(records) => records,
                Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                    return Err(cadmpeg_core::CodecError::ResourceLimit(error))
                }
                Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                    return Err(error.into_codec_error())
                }
                Err(_) => return Ok(()),
            };
            if framed.len() != 1 {
                return Ok(());
            }
            let Some(framed) = framed.pop() else {
                return Ok(());
            };
            if !ctx.equal(
                framed.name.as_str(),
                record.name(),
                "compare F3D archived record names",
            )? {
                return Ok(());
            }

            if !ctx.insert_scoped_btree_map_if_vacant(
                &mut archived_frames_storage,
                &mut archived_frames,
                revision_id,
                framed,
                "find F3D archived record frame",
                "retain F3D archived record frame",
            )? {
                return Ok(());
            }
        }
    }
    let Some(archive) = historical_record_archive(ctx, states, active_records, archived_frames)?
    else {
        return Ok(());
    };
    let mut cache_storage =
        ctx.reserve_scoped(0, "collect F3D complete historical topology caches")?;
    let mut complete = true;
    let mut complete_states = states.iter_mut();
    while let Some(state) =
        ctx.next_charged(&mut complete_states, "bind F3D complete record tables")?
    {
        let Some(records) = materialize_record_table(ctx, state, &archive)? else {
            complete = false;
            break;
        };
        let (decoded, _decoded_storage) =
            ctx.with_scoped_storage("decode F3D historical topology source", || {
                Ok::<_, cadmpeg_core::CodecError>(crate::brep::decode_history_topology(
                    ctx,
                    &records.records,
                    bytes,
                    crate::ids::ID_FORMAT,
                ))
            })?;
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => {
                complete = false;
                break;
            }
        };
        let Some(topology) =
            cache_storage.with_storage(|| historical_topology_with_tags(ctx, &decoded))?
        else {
            complete = false;
            break;
        };
        state.topology_cache = crate::history_records::AsmTopologyCache::Complete(topology);
    }
    if complete {
        cache_storage.commit()?;
        bind_historical_transitions(ctx, states)?;
        // Keep only record revisions that can be named by a late persistent
        // selection. Non-topological ASM attributes do not participate in
        // feature selection and need not survive projection finalization.
        for state in ctx.admit_iter(states, "retain F3D historical entity versions")? {
            if let Some(topology) = state.topology() {
                let slots = topology_entity_slots(ctx, topology)?;
                ctx.retain_vec(
                    &mut state.entity_versions,
                    |version| {
                        ctx.contains_hash_set(
                            &slots.slots,
                            &version.entity_ref,
                            "find F3D historical entity version slot",
                        )
                    },
                    "retain F3D historical entity versions",
                )?;
            } else {
                ctx.clear_vec(
                    &mut state.entity_versions,
                    "clear F3D historical entity versions",
                )?;
            }
        }
    } else {
        for state in ctx.admit_iter(states, "release F3D incomplete topology caches")? {
            state.topology_cache = crate::history_records::AsmTopologyCache::Absent;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct HistoricalRecordArchive<'ctx> {
    records: BTreeMap<i64, cadmpeg_asm::sab::Record>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    _token_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn historical_record_archive<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: BTreeMap<i64, cadmpeg_asm::sab::Record>,
) -> Result<Option<HistoricalRecordArchive<'ctx>>, cadmpeg_core::CodecError> {
    if ctx.any_by(
        active_records.iter().enumerate(),
        |(index, record)| Ok(record.index != index),
        "validate F3D active record ordinals",
    )? {
        return Ok(None);
    }
    let Some(active_count) = i64::try_from(active_records.len()).ok() else {
        return Ok(None);
    };

    let mut revision_entities_storage =
        ctx.reserve_scoped(0, "index F3D active record revisions")?;
    let mut revision_entities = HashMap::new();
    for entity_ref in ctx.admit_iter(
        0..active_count.cast_unsigned(),
        "scan F3D active record revisions",
    )? {
        let entity_ref = entity_ref.cast_signed();
        revision_entities_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut revision_entities,
                entity_ref,
                entity_ref,
                "index F3D active record revisions",
            )
            .map(|_| ())
        })?;
    }
    let mut history_source = IntoIterator::into_iter(states);
    while let Some(state) =
        ctx.next_charged(&mut history_source, "scan F3D archived history states")?
    {
        let mut history_source = IntoIterator::into_iter(&state.bulletin_boards);
        while let Some(board) =
            ctx.next_charged(&mut history_source, "scan F3D archived bulletin boards")?
        {
            let mut history_source = IntoIterator::into_iter(&board.changes);
            while let Some(change) =
                ctx.next_charged(&mut history_source, "scan F3D archived board changes")?
            {
                let Some(old_ref) = change.old_ref() else {
                    continue;
                };
                let entity_ref = change.new_ref().unwrap_or(old_ref);

                if revision_entities_storage
                    .with_storage(|| {
                        ctx.insert_hash_map(
                            &mut revision_entities,
                            old_ref,
                            entity_ref,
                            "index F3D archived record revisions",
                        )
                    })?
                    .is_some()
                {
                    return Ok(None);
                }
            }
        }
    }

    let mut records_storage = ctx.reserve_scoped(0, "retain F3D active record archive")?;
    let mut token_storage = ctx.reserve_scoped(0, "copy F3D archived record tokens")?;
    let mut records = BTreeMap::new();
    let mut history_source = IntoIterator::into_iter(active_records).enumerate();
    while let Some((revision, record)) =
        ctx.next_charged(&mut history_source, "scan F3D active record archive")?
    {
        let Some(revision) = i64::try_from(revision).ok() else {
            return Ok(None);
        };
        records_storage.with_storage(|| {
            let cloned = clone_historical_record(ctx, record)?;
            ctx.insert_btree_map(
                &mut records,
                revision,
                cloned,
                "retain F3D active record archive",
            )
            .map(|_| ())
        })?;
    }
    let mut history_source = IntoIterator::into_iter(archived_frames);
    while let Some((revision_id, framed)) =
        ctx.next_charged(&mut history_source, "scan F3D archived record frames")?
    {
        if records_storage
            .with_storage(|| {
                ctx.insert_btree_map(
                    &mut records,
                    revision_id,
                    framed,
                    "retain F3D archived record archive",
                )
            })?
            .is_some()
        {
            return Ok(None);
        }
    }
    if records.len() != revision_entities.len() {
        return Ok(None);
    }
    let mut history_source = IntoIterator::into_iter(&mut records);
    while let Some((&revision_ref, record)) =
        ctx.next_charged(&mut history_source, "scan F3D record archive revisions")?
    {
        let Some(&entity_ref) = ctx.get_hash_map(
            &revision_entities,
            &revision_ref,
            "find F3D history revision entities",
        )?
        else {
            return Ok(None);
        };
        let Some(index) = usize::try_from(entity_ref).ok() else {
            return Ok(None);
        };
        record.index = index;
        if std::sync::Arc::strong_count(&record.tokens) > 1 {
            let token_count = record.tokens.len();
            let token_width = std::mem::size_of::<cadmpeg_asm::sab::Token>();
            let maximum_token_count = usize::MAX / token_width;
            let token_bytes = token_count.checked_mul(token_width).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "copy F3D archived record tokens",
                    u64_from_index(maximum_token_count),
                    u64_from_index(token_count),
                )
            })?;
            let token_bytes = u64_from_index(token_bytes);
            let mut token_vector_storage =
                ctx.reserve_scoped(0, "copy F3D archived record tokens")?;
            let mut tokens = Vec::new();
            for token in ctx.admit_iter(record.tokens.as_ref(), "scan F3D history record tokens")? {
                let cloned = token_storage.with_storage(|| clone_historical_token(ctx, token))?;
                token_vector_storage.with_storage(|| {
                    ctx.push_vec(&mut tokens, cloned, "copy F3D archived record tokens")
                })?;
            }
            ctx.charge_work(token_bytes, "copy F3D archived record tokens")?;
            token_storage.grow(token_bytes)?;
            record.tokens = tokens.into();
        }
        let mut history_source =
            IntoIterator::into_iter(std::sync::Arc::make_mut(&mut record.tokens));
        while let Some(token) =
            ctx.next_charged(&mut history_source, "rebind F3D archived record references")?
        {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                let Some(&entity_ref) = ctx.get_hash_map(
                    &revision_entities,
                    reference,
                    "find F3D history revision entities",
                )?
                else {
                    return Ok(None);
                };
                *reference = entity_ref;
            }
        }
    }
    Ok(Some(HistoricalRecordArchive {
        records,
        _storage: records_storage,
        _token_storage: token_storage,
    }))
}

fn clone_historical_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &cadmpeg_asm::sab::Record,
) -> Result<cadmpeg_asm::sab::Record, cadmpeg_core::CodecError> {
    let name = ctx.copy_retained_text(&record.name, "copy F3D historical record text")?;
    Ok(cadmpeg_asm::sab::Record {
        index: record.index,
        name,
        tokens: record.tokens.clone(),
        offset: record.offset,
        len: record.len,
    })
}

fn clone_historical_token(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    token: &cadmpeg_asm::sab::Token,
) -> Result<cadmpeg_asm::sab::Token, cadmpeg_core::CodecError> {
    use cadmpeg_asm::sab::Token;
    Ok(match token {
        Token::Str(value) => {
            Token::Str(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::Ident(value) => {
            Token::Ident(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::SubIdent(value) => {
            Token::SubIdent(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::Char(value) => Token::Char(*value),
        Token::Short(value) => Token::Short(*value),
        Token::Long(value) => Token::Long(*value),
        Token::Float(value) => Token::Float(*value),
        Token::Double(value) => Token::Double(*value),
        Token::True => Token::True,
        Token::False => Token::False,
        Token::Ref(value) => Token::Ref(*value),
        Token::SubtypeOpen => Token::SubtypeOpen,
        Token::SubtypeClose => Token::SubtypeClose,
        Token::Enum(value) => Token::Enum(*value),
        Token::Position(value) => Token::Position(*value),
        Token::Vector3(value) => Token::Vector3(*value),
        Token::Vector2(value) => Token::Vector2(*value),
        Token::Int64(value) => Token::Int64(*value),
    })
}

fn bind_historical_transitions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut by_node_storage = ctx.reserve_scoped(0, "index F3D transition nodes")?;
    let mut by_node = HashMap::new();
    for (ordinal, state) in ctx
        .admit_iter(&*states, "scan F3D transition nodes")?
        .enumerate()
    {
        by_node_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_node,
                state.node_index,
                ordinal,
                "index F3D transition nodes",
            )
            .map(|_| ())
        })?;
    }
    if by_node.len() != states.len() {
        return Ok(());
    }

    let mut transitions_storage = ctx.reserve_scoped(0, "collect F3D historical transitions")?;
    let mut output_storage = ctx.reserve_scoped(0, "retain F3D historical transitions")?;
    let mut transitions = Vec::new();
    let mut history_source = IntoIterator::into_iter(&*states);
    while let Some(state) = ctx.next_charged(&mut history_source, "scan F3D states")? {
        let previous = match state.next_ref {
            Some(node) => {
                let Some(&ordinal) =
                    ctx.get_hash_map(&by_node, &node, "find F3D history by node")?
                else {
                    return Ok(());
                };
                states.get(ordinal)
            }
            None => None,
        };
        let Some(transition) =
            output_storage.with_storage(|| historical_transition(ctx, state, previous))?
        else {
            return Ok(());
        };
        transitions_storage.with_storage(|| {
            ctx.push_vec(
                &mut transitions,
                transition,
                "collect F3D historical transitions",
            )
        })?;
    }
    output_storage.commit()?;
    for (state, transition) in ctx
        .admit_iter(&mut *states, "bind F3D historical transitions")?
        .zip(transitions)
    {
        state.transition = Some(transition);
    }
    Ok(())
}

fn historical_transition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Result<Option<AsmHistoricalTransition>, cadmpeg_core::CodecError> {
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous
        .and_then(|state| state.topology())
        .unwrap_or(&empty);
    let (current_versions, _current_versions_storage) =
        historical_version_map(ctx, &current.entity_versions)?;
    let (previous_versions, _previous_versions_storage) = historical_version_map(
        ctx,
        previous.map_or(&[][..], |state| state.entity_versions.as_slice()),
    )?;
    let versions = (&current_versions, &previous_versions);
    let mut record_key_storage = ctx.reserve_scoped(0, "collect F3D transition version keys")?;
    let current_record_keys = record_key_storage.with_storage(|| {
        historical_version_keys(
            ctx,
            &current_versions,
            "scan F3D current transition version keys",
        )
    })?;
    let previous_record_keys = record_key_storage.with_storage(|| {
        historical_version_keys(
            ctx,
            &previous_versions,
            "scan F3D previous transition version keys",
        )
    })?;
    let delta = |current: &[i64], previous: &[i64]| {
        let (current, _current_storage) = sorted_transition_entities(ctx, current)?;
        let (previous, _previous_storage) = sorted_transition_entities(ctx, previous)?;
        entity_delta(ctx, &current, &previous, versions)
    };
    Ok(Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(ctx, &current_record_keys, &previous_record_keys, versions)?,
        topology: AsmHistoricalTopologyDelta {
            bodies: delta(&current_topology.bodies, &previous_topology.bodies)?,
            regions: delta(&current_topology.regions, &previous_topology.regions)?,
            shells: delta(&current_topology.shells, &previous_topology.shells)?,
            faces: delta(&current_topology.faces, &previous_topology.faces)?,
            loops: delta(&current_topology.loops, &previous_topology.loops)?,
            coedges: delta(&current_topology.coedges, &previous_topology.coedges)?,
            edges: delta(&current_topology.edges, &previous_topology.edges)?,
            vertices: delta(&current_topology.vertices, &previous_topology.vertices)?,
            points: delta(&current_topology.points, &previous_topology.points)?,
            surfaces: delta(&current_topology.surfaces, &previous_topology.surfaces)?,
            curves: delta(&current_topology.curves, &previous_topology.curves)?,
            pcurves: delta(&current_topology.pcurves, &previous_topology.pcurves)?,
        },
    }))
}

/// Indexes a state's record version per entity, keeping the last version of a
/// repeated entity, for the duration of one transition.
fn historical_version_map<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    versions: &[AsmEntityVersion],
) -> Result<
    (
        BTreeMap<i64, i64>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    ctx.collect_scoped_btree_map(
        versions
            .iter()
            .map(|version| (version.entity_ref, version.record_ref)),
        "index F3D transition versions",
    )
}

/// Copies the ascending, distinct entity keys of a version map.
fn historical_version_keys(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    versions: &BTreeMap<i64, i64>,
    operation: &'static str,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut keys = ctx.collection_vec(versions.len(), "collect F3D transition version keys")?;
    for &entity_ref in ctx
        .admit_iter(versions, operation)?
        .map(|(entity_ref, _)| entity_ref)
    {
        keys.push(entity_ref);
    }
    Ok(keys)
}

/// Copies topology entity references into ascending, distinct scratch order.
fn sorted_transition_entities<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    entities: &[i64],
) -> Result<(Vec<i64>, cadmpeg_core::decode::ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let (mut sorted, storage) = ctx
        .copy_temporary_slice(entities, "copy F3D transition entities")
        .map_err(cadmpeg_core::CodecError::ResourceLimit)?;
    ctx.sort_unstable_by(
        &mut sorted,
        |entity| entity,
        Ord::cmp,
        "sort F3D transition entities",
    )?;
    ctx.dedup_vec(&mut sorted, "deduplicate F3D transition entities")?;
    Ok((sorted, storage))
}

/// Splits two ascending, distinct entity lists into inserted, deleted and
/// re-versioned entities in one merge pass. Each list keeps ascending order.
fn entity_delta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &[i64],
    previous: &[i64],
    (current_versions, previous_versions): (&BTreeMap<i64, i64>, &BTreeMap<i64, i64>),
) -> Result<AsmHistoricalEntityDelta, cadmpeg_core::CodecError> {
    let mut inserted = Vec::new();
    let mut deleted = Vec::new();
    let mut updated = Vec::new();
    let (mut current_index, mut previous_index) = (0, 0);
    loop {
        let endpoints = (current.get(current_index), previous.get(previous_index));
        if matches!(endpoints, (None, None)) {
            break;
        }
        ctx.charge_work(1, "merge F3D transition entities")?;
        let (target, entity) = match endpoints {
            (None, None) => break,
            (Some(&entity), None) => {
                current_index += 1;
                (&mut inserted, entity)
            }
            (None, Some(&entity)) => {
                previous_index += 1;
                (&mut deleted, entity)
            }
            (Some(&current_entity), Some(&previous_entity)) => {
                match current_entity.cmp(&previous_entity) {
                    std::cmp::Ordering::Less => {
                        current_index += 1;
                        (&mut inserted, current_entity)
                    }
                    std::cmp::Ordering::Greater => {
                        previous_index += 1;
                        (&mut deleted, previous_entity)
                    }
                    std::cmp::Ordering::Equal => {
                        current_index += 1;
                        previous_index += 1;
                        let current_record = ctx.get_btree_map(
                            current_versions,
                            &current_entity,
                            "find F3D current transition record version",
                        )?;
                        let previous_record = ctx.get_btree_map(
                            previous_versions,
                            &current_entity,
                            "find F3D previous transition record version",
                        )?;
                        if current_record == previous_record {
                            continue;
                        }
                        (&mut updated, current_entity)
                    }
                }
            }
        };
        ctx.push_vec(target, entity, "collect F3D transition delta")?;
    }
    Ok(AsmHistoricalEntityDelta {
        inserted,
        deleted,
        updated,
    })
}

#[derive(Debug)]
struct MaterializedRecordTable<'ctx> {
    records: Vec<cadmpeg_asm::sab::Record>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn materialize_record_table<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    state: &AsmDeltaState,
    archive: &HistoricalRecordArchive<'_>,
) -> Result<Option<MaterializedRecordTable<'ctx>>, cadmpeg_core::CodecError> {
    if state.entity_versions.is_empty() {
        return Ok(None);
    }

    let mut present_storage = ctx.reserve_scoped(0, "index F3D historical record presence")?;
    let mut present = HashSet::new();
    for version in ctx.admit_iter(&state.entity_versions, "scan F3D state entity versions")? {
        present_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut present,
                version.entity_ref,
                "index F3D historical record presence",
            )
        })?;
    }
    if present.len() != state.entity_versions.len() {
        return Ok(None);
    }

    let mut records_storage = ctx.reserve_scoped(0, "materialize F3D historical record table")?;
    let mut records = Vec::new();
    let mut history_source = IntoIterator::into_iter(&state.entity_versions);
    while let Some(version) =
        ctx.next_charged(&mut history_source, "scan F3D state entity versions")?
    {
        let Some(record) = ctx.get_btree_map(
            &archive.records,
            &version.record_ref,
            "find F3D archived record revision",
        )?
        else {
            return Ok(None);
        };
        if i64::try_from(record.index).ok() != Some(version.entity_ref) {
            return Ok(None);
        }
        let mut history_source = IntoIterator::into_iter(record.tokens.as_ref());
        while let Some(token) =
            ctx.next_charged(&mut history_source, "scan F3D history record tokens")?
        {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0
                && !ctx.contains_hash_set(&present, reference, "find F3D history present")?
            {
                return Ok(None);
            }
        }
        records_storage.with_storage(|| {
            let cloned = clone_historical_record(ctx, record)?;
            ctx.push_vec(
                &mut records,
                cloned,
                "materialize F3D historical record table",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut records,
        |value| &value.index,
        Ord::cmp,
        "sort F3D historical records",
    )?;
    Ok(Some(MaterializedRecordTable {
        records,
        _storage: records_storage,
    }))
}

pub(super) fn decode_bulletin_boards(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Result<Option<(Vec<AsmBulletinBoard>, usize)>, cadmpeg_core::CodecError> {
    let (output, storage) = ctx.with_scoped_storage(
        "retain F3D ASM bulletin boards",
        || -> Result<_, cadmpeg_core::CodecError> {
            if bytes.get(position) == Some(&0x11) {
                return Ok(Some((Vec::new(), position)));
            }
            let mut boards = Vec::new();
            loop {
                ctx.charge_work(1, "read F3D ASM bulletin board entry")?;
                let board_offset = position;
                let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
                    return Ok(None);
                };
                if present == 0 {
                    break;
                }
                let Some(owner_ref) = take_int(bytes, &mut position, 0x0c, width) else {
                    return Ok(None);
                };
                let Some(number) = take_int(bytes, &mut position, 0x04, width) else {
                    return Ok(None);
                };
                let board_id = crate::ids::native_scoped_id(
                    ctx,
                    stream,
                    "asm-bulletin-board",
                    format_args!("{state_offset:010}:{:06}", boards.len()),
                )?;
                let mut changes = Vec::new();
                loop {
                    ctx.charge_work(1, "read F3D ASM entity change entry")?;
                    let change_offset = position;
                    let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
                        return Ok(None);
                    };
                    if present == 0 {
                        break;
                    }
                    let Some(old) = take_int(bytes, &mut position, 0x0c, width) else {
                        return Ok(None);
                    };
                    let Some(new) = take_int(bytes, &mut position, 0x0c, width) else {
                        return Ok(None);
                    };
                    let kind = match (old >= 0, new >= 0) {
                        (false, true) => AsmEntityChangeKind::Insert { new },
                        (true, false) => AsmEntityChangeKind::Delete { old },
                        (true, true) => AsmEntityChangeKind::Update { old, new },
                        (false, false) => return Ok(None),
                    };

                    ctx.reserve_vec(&mut changes, 1, "admit F3D ASM entity change")?;
                    let change_id = crate::ids::native_scoped_id(
                        ctx,
                        stream,
                        "asm-entity-change",
                        format_args!(
                            "{state_offset:010}:{:06}:{:06}",
                            boards.len(),
                            changes.len()
                        ),
                    )?;
                    let parent = ctx.copy_retained_text(&board_id, "copy F3D ASM change parent")?;
                    changes.push(AsmEntityChange {
                        id: change_id,
                        parent,
                        byte_offset: u64_from_index(change_offset),
                        kind,
                    });
                }

                ctx.reserve_vec(&mut boards, 1, "admit F3D ASM bulletin board")?;
                let parent = ctx.copy_retained_text(state_id, "copy F3D ASM board parent")?;
                boards.push(AsmBulletinBoard {
                    id: board_id,
                    parent,
                    byte_offset: u64_from_index(board_offset),
                    owner_ref,
                    number,
                    changes,
                });
            }
            Ok(Some((boards, position)))
        },
    )?;
    if output.is_some() {
        storage.commit()?;
    }
    Ok(output)
}

pub(super) fn decode_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
    state_id: &str,
    width: RefWidth,
) -> Result<Vec<AsmHistoryRecord>, cadmpeg_core::CodecError> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width.bytes()) == Some(&0x11)
    {
        start += 2 + width.bytes();
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Ok(Vec::new());
    }
    let (framed, framing_storage) = ctx.with_scoped_storage("frame F3D history scratch", || {
        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_asm::sab::frame_history(
            ctx, bytes, start, limit, width, None,
        ))
    })?;
    match framed {
        Ok(records) => ctx.try_collect_vec(
            records.into_iter().map(|record| {
                let mut entity_references = Vec::new();
                for token in
                    ctx.admit_iter(record.tokens.as_ref(), "scan F3D history record tokens")?
                {
                    if let cadmpeg_asm::sab::Token::Ref(value) = token {
                        ctx.push_vec(
                            &mut entity_references,
                            *value,
                            "frame F3D history references",
                        )?;
                    }
                }
                let end = record.offset.checked_add(record.len).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("F3D history record byte range overflows")
                })?;
                let source = bytes.get(record.offset..end).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "F3D history record byte range exceeds stream",
                    )
                })?;
                let raw_bytes = ctx.copy_retained(source, "retain F3D history record")?;

                let id = crate::ids::native_scoped_id(
                    ctx,
                    stream,
                    "asm-history-record",
                    format_args!("{:010}", record.offset),
                )?;
                let parent = ctx.copy_retained_text(state_id, "copy F3D history record parent")?;
                Ok(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record.offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record.index),
                        name: ctx
                            .copy_retained_text(&record.name, "copy F3D history record name")?,
                        entity_references,
                    },
                    raw_bytes,
                })
            }),
            "frame F3D history record",
        ),
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            Err(error.into_codec_error())
        }
        Err(error) => {
            drop(framing_storage);
            let raw_bytes =
                ctx.copy_retained(&bytes[start..limit], "retain opaque F3D history record")?;
            let mut decoded = ctx.collection_vec(1, "frame opaque F3D history record")?;
            let id = crate::ids::native_scoped_id(
                ctx,
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            )?;
            let parent =
                ctx.copy_retained_text(state_id, "copy opaque F3D history record parent")?;
            let error =
                ctx.format_retained(format_args!("{error}"), "retain opaque F3D history error")?;
            decoded.push(AsmHistoryRecord {
                id,
                parent,
                revision_id: None,
                byte_offset: u64_from_index(start),
                framing: crate::history_records::AsmHistoryRecordFraming::Opaque { error },
                raw_bytes,
            });
            Ok(decoded)
        }
    }
}

pub(super) fn decode_preamble(
    bytes: &[u8],
    mut position: usize,
    width: RefWidth,
) -> Option<(i64, i64)> {
    let size = take_int(bytes, &mut position, 0x04, width)?;
    let duplicate = take_int(bytes, &mut position, 0x04, width)?;
    let zero = take_int(bytes, &mut position, 0x04, width)?;
    let entry_count = take_int(bytes, &mut position, 0x04, width)?;
    (size == duplicate && zero == 0).then_some((size, entry_count))
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
pub(super) fn take_int(
    bytes: &[u8],
    position: &mut usize,
    tag: u8,
    width: RefWidth,
) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = int_at(bytes, *position + 1, width)?;
    *position += 1 + width.bytes();
    Some(value)
}

#[cfg(test)]
mod tests;
