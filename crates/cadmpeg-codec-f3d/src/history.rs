// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(clippy::cloned_ref_to_slice_refs, clippy::default_trait_access)
)]
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use cadmpeg_core::decode::u64_from_index;

mod archive;
pub(crate) mod selection;
mod topology;

use crate::history_records::{
    AsmDeltaState, AsmHistoricalCylinder, AsmHistoricalTopology, AsmHistoricalTransition,
    AsmHistory, AsmPreamble,
};
use crate::records::feature::base_feature::DesignBaseFeatureBodyReferenceSource;
use crate::records::topology::{
    body_recipe::AsmHistoricalEntityKind, extrude_selection::DesignOperandRole,
};
use cadmpeg_asm::kernel_header::RefWidth;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7: f64 = 1.0e-7;
const EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7: f64 = 1.0e-7;
const EPS_HISTORY_PARALLEL_DIRECTIONS_E7: f64 = 1.0e-7;
const EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9: f64 = 1.0e-9;
const EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12: f64 = 1.0e-12;
const EPS_HISTORY_SAME_AXIS_LINE_E9: f64 = 1.0e-9;
const EPS_HISTORY_SAME_AXIS_LINE_E8: f64 = 1.0e-8;
const EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8: f64 = 1.0e-8;
const EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9: f64 = 1.0e-9;

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for admitting an oriented planar Hole support face.
const HOLE_SUPPORT_NORMAL_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for the Hole point lying on its support plane.
const HOLE_SUPPORT_POINT_TOLERANCE: f64 = 1.0e-8;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let Ok((decode_ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    ) else {
        return false;
    };
    let decode_ctx = &decode_ctx;

    graph_is_coherent_inner(decode_ctx, history).unwrap_or(false)
}

pub(crate) fn graph_is_coherent_charged(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    graph_is_coherent_inner(decode, history)
}

fn graph_is_coherent_inner(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    if history.states.is_empty() {
        return Ok(false);
    }
    let mut by_index_storage = decode.reserve_scoped(0, "index F3D ASM history states")?;
    let mut by_index = HashMap::new();
    for state in decode.admit_iter(&history.states, "scan F3D ASM history states")? {
        by_index_storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut by_index,
                    state.node_index,
                    state,
                    "index F3D ASM history states",
                )
                .map(|_| ())
        })?;
    }
    if by_index.len() != history.states.len()
        || decode.any_by(
            &history.states,
            |state| {
                if state.node_index < 0 {
                    return Ok(true);
                }
                Ok(!decode.equal(
                    &state.parent,
                    &history.id,
                    "compare F3D ASM history state parent",
                )?)
            },
            "scan F3D ASM history state parents",
        )?
    {
        return Ok(false);
    }
    let mut head = None;
    let mut heads = 0_usize;
    let mut tails = 0_usize;
    for state in decode.admit_iter(&history.states, "scan F3D ASM history chain ends")? {
        if state.previous_ref.is_none() {
            head = head.or(Some(state));
            heads += 1;
        }
        if state.next_ref.is_none() {
            tails += 1;
        }
    }
    let (Some(head), 1, 1) = (head, heads, tails) else {
        return Ok(false);
    };
    if let Some(preamble) = history.preamble {
        if head.state_id != preamble.stream_size || preamble.history_entry_count < 0 {
            return Ok(false);
        }
    }
    // Every visited state must name the state visited before it, and only the
    // head names none, so the walk cannot revisit a state: the first repeat
    // would need its predecessor to repeat earlier. Counting steps therefore
    // counts distinct states.
    let mut visited = 0_usize;
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        decode.charge_work(1, "visit F3D ASM history chain link")?;
        let Some(state) = decode.get_hash_map(&by_index, &index, "find F3D history by index")?
        else {
            return Ok(false);
        };
        if state.previous_ref != previous {
            return Ok(false);
        }
        visited += 1;
        if state.version_flag != 1 || state.state_flag != 0 {
            return Ok(false);
        }
        let mut history_source = IntoIterator::into_iter(&state.bulletin_boards);
        while let Some(board) =
            decode.next_charged(&mut history_source, "scan F3D ASM history bulletin boards")?
        {
            if !decode.equal(
                &board.parent,
                &state.id,
                "compare F3D ASM history board parent",
            )? || decode.any_by(
                &board.changes,
                |change| {
                    decode
                        .equal(
                            &change.parent,
                            &board.id,
                            "compare F3D ASM history change parent",
                        )
                        .map(|matches| !matches)
                },
                "scan F3D ASM history board changes",
            )? {
                return Ok(false);
            }
        }
        if decode.any_by(
            &state.records,
            |record| {
                let parent_matches = decode.equal(
                    &record.parent,
                    &state.id,
                    "compare F3D ASM history record parent",
                )?;
                Ok(!parent_matches || record.raw_bytes.is_empty())
            },
            "scan F3D ASM history records",
        )? {
            return Ok(false);
        }
        previous = Some(index);
        current = state.next_ref;
    }
    Ok(visited == history.states.len())
}

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#32-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body. `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    _limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let (output, storage) = ctx.with_scoped_storage(
        "retain F3D ASM history output",
        || -> Result<_, cadmpeg_core::CodecError> {
            let preamble_offset = ctx.find_bytes(bytes, PREAMBLE, "find F3D ASM preamble")?;
            let history_offset = preamble_offset.unwrap_or(0);
            let mut history_id = None;
            let mut delta_offsets_storage =
                ctx.reserve_scoped(0, "collect F3D ASM delta offsets")?;
            let mut delta_offsets = Vec::new();
            for offset in ctx.find_bytes_iter(bytes, DELTA, "find F3D ASM delta markers")? {
                ctx.push_scoped_vec(
                    &mut delta_offsets_storage,
                    &mut delta_offsets,
                    offset,
                    "collect F3D ASM delta offsets",
                )?;
            }
            let mut states = Vec::new();
            let mut history_source = IntoIterator::into_iter(&delta_offsets).enumerate();
            while let Some((ordinal, &offset)) =
                ctx.next_charged(&mut history_source, "scan F3D ASM delta offsets")?
            {
                let mut position = offset + DELTA.len();
                let Some((
                    state_id,
                    version_flag,
                    state_flag,
                    previous,
                    next,
                    node_index,
                    partner,
                    owner_ref,
                )) = (|| {
                    Some((
                        archive::take_int(bytes, &mut position, 0x04, width)?,
                        archive::take_int(bytes, &mut position, 0x04, width)?,
                        archive::take_int(bytes, &mut position, 0x04, width)?,
                        archive::take_int(bytes, &mut position, 0x0c, width)?,
                        archive::take_int(bytes, &mut position, 0x0c, width)?,
                        archive::take_int(bytes, &mut position, 0x0c, width)?,
                        archive::take_int(bytes, &mut position, 0x0c, width)?,
                        archive::take_int(bytes, &mut position, 0x0c, width)?,
                    ))
                })()
                else {
                    return Ok(None);
                };
                if bytes.get(position) != Some(&0x0b) {
                    continue;
                }
                let state_record_id = crate::ids::native_scoped_id(
                    ctx,
                    stream,
                    "asm-delta-state",
                    format_args!("{offset:010}"),
                )?;
                let Some((bulletin_boards, body_end)) = archive::decode_bulletin_boards(
                    ctx,
                    bytes,
                    position + 1,
                    stream,
                    offset,
                    &state_record_id,
                    width,
                )?
                else {
                    return Ok(None);
                };
                let records = archive::decode_history_records(
                    ctx,
                    bytes,
                    body_end,
                    delta_offsets.get(ordinal + 1).copied(),
                    stream,
                    &state_record_id,
                    width,
                )?;

                ctx.reserve_vec(&mut states, 1, "admit F3D ASM delta state")?;
                if history_id.is_none() {
                    history_id = Some(crate::ids::native_scoped_id(
                        ctx,
                        stream,
                        "asm-history",
                        format_args!("{history_offset:010}"),
                    )?);
                }
                let Some(history_id) = &history_id else {
                    return Ok(None);
                };
                let parent = ctx.copy_retained_text(history_id, "copy F3D ASM history parent")?;
                states.push(AsmDeltaState {
                    id: state_record_id,
                    parent,
                    byte_offset: u64_from_index(offset),
                    state_id,
                    version_flag,
                    state_flag,
                    previous_ref: (previous >= 0).then_some(previous),
                    next_ref: (next >= 0).then_some(next),
                    node_index,
                    partner_ref: (partner >= 0).then_some(partner),
                    owner_ref,
                    bulletin_boards,
                    records,
                    entity_versions: Vec::new(),
                    topology_cache: crate::history_records::AsmTopologyCache::Absent,
                    transition: None,
                });
            }
            let Some(history_id) = history_id else {
                return Ok(None);
            };
            archive::bind_snapshot_revision_ids(ctx, &mut states)?;
            archive::bind_historical_entity_versions(ctx, &mut states)?;
            archive::bind_complete_record_tables(ctx, &mut states, bytes, width)?;

            let preamble = preamble_offset
                .and_then(|offset| archive::decode_preamble(bytes, offset + PREAMBLE.len(), width))
                .map(|(stream_size, history_entry_count)| AsmPreamble {
                    stream_size,
                    history_entry_count,
                });
            let offset = history_offset;
            Ok(Some(AsmHistory {
                id: history_id,
                byte_offset: u64_from_index(offset),
                preamble,
                record_table_binding_budget_exceeded: false,
                states,
            }))
        },
    )?;
    if output.is_some() {
        storage.commit()?;
    }
    Ok(output)
}

fn retain_mirror_plane_topology(
    mut topology: AsmHistoricalTopology,
) -> Option<AsmHistoricalTopology> {
    let retained = AsmHistoricalTopology {
        bodies: std::mem::take(&mut topology.bodies),
        regions: std::mem::take(&mut topology.regions),
        shells: std::mem::take(&mut topology.shells),
        faces: std::mem::take(&mut topology.faces),
        loops: std::mem::take(&mut topology.loops),
        coedges: std::mem::take(&mut topology.coedges),
        edges: std::mem::take(&mut topology.edges),
        vertices: std::mem::take(&mut topology.vertices),
        points: std::mem::take(&mut topology.points),
        surfaces: std::mem::take(&mut topology.surfaces),
        curves: std::mem::take(&mut topology.curves),
        pcurves: std::mem::take(&mut topology.pcurves),
        face_surfaces: std::mem::take(&mut topology.face_surfaces),
        surface_planes: std::mem::take(&mut topology.surface_planes),
        loop_coedges: std::mem::take(&mut topology.loop_coedges),
        coedge_topology: std::mem::take(&mut topology.coedge_topology),
        face_loops: std::mem::take(&mut topology.face_loops),
        edge_curves: std::mem::take(&mut topology.edge_curves),
        coedge_pcurves: std::mem::take(&mut topology.coedge_pcurves),
        curve_axes: std::mem::take(&mut topology.curve_axes),
        ..Default::default()
    };
    (!retained.face_surfaces.is_empty()
        || !retained.surface_planes.is_empty()
        || !retained.loop_coedges.is_empty()
        || !retained.coedge_topology.is_empty()
        || !retained.face_loops.is_empty()
        || !retained.edge_curves.is_empty()
        || !retained.coedge_pcurves.is_empty()
        || !retained.curve_axes.is_empty())
    .then_some(retained)
}

/// Release complete historical snapshots after every projection consumer has
/// finished. Raw history records, sparse transitions, and the compact
/// plane-selection topology remain retained.
pub(crate) fn discard_projection_caches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for history in ctx.admit_iter(histories, "scan F3D projection cache histories")? {
        for state in ctx.admit_iter(&mut history.states, "scan F3D projection cache states")? {
            let topology = match std::mem::take(&mut state.topology_cache) {
                crate::history_records::AsmTopologyCache::Absent
                | crate::history_records::AsmTopologyCache::Released => None,
                crate::history_records::AsmTopologyCache::Complete(topology)
                | crate::history_records::AsmTopologyCache::Retained(topology) => {
                    retain_mirror_plane_topology(topology)
                }
            };
            if let Some(topology) = topology {
                let slots = topology::topology_entity_slots(ctx, &topology)?;
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
                state.topology_cache = crate::history_records::AsmTopologyCache::Retained(topology);
            } else {
                ctx.clear_vec(
                    &mut state.entity_versions,
                    "clear F3D historical entity versions",
                )?;
                state.topology_cache = crate::history_records::AsmTopologyCache::Released;
            }
        }
    }
    Ok(())
}

pub(crate) fn projection_was_finalized(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[AsmHistory],
) -> Result<bool, cadmpeg_core::CodecError> {
    if histories.is_empty() {
        return Ok(false);
    }
    decode.all_by(
        histories,
        |history| history.projection_finalized(decode),
        "check F3D history projection finalization",
    )
}

pub(crate) fn bind_feature_outputs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    // Each history's states by node, absent when node indices repeat, and
    // every state by ID across histories, absent when the ID repeats.
    let mut state_index_storage = ctx.reserve_scoped(0, "index F3D feature output states")?;
    let mut history_nodes = Vec::new();
    let mut states_by_id = HashMap::<i64, Option<(usize, &AsmDeltaState)>>::new();
    for (history_index, history) in ctx
        .admit_iter(histories, "scan F3D feature output histories")?
        .enumerate()
    {
        let nodes = state_index_storage.with_storage(|| {
            ctx.collect_hash_map(
                history.states.iter().map(|state| (state.node_index, state)),
                "index F3D feature output history nodes",
            )
        })?;
        let nodes = (nodes.len() == history.states.len()).then_some(nodes);
        state_index_storage.with_storage(|| {
            ctx.push_vec(
                &mut history_nodes,
                nodes,
                "index F3D feature output history nodes",
            )
        })?;
        for state in ctx.admit_iter(&history.states, "scan F3D history states")? {
            if let Some(indexed) = ctx.get_mut_hash_map(
                &mut states_by_id,
                &state.state_id,
                "index F3D feature output states",
            )? {
                *indexed = None;
                continue;
            }
            state_index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut states_by_id,
                    state.state_id,
                    Some((history_index, state)),
                    "index F3D feature output states",
                )
                .map(|_| ())
            })?;
        }
    }
    let mut active_storage = ctx.reserve_scoped(0, "index F3D active feature output bodies")?;
    let mut active = HashMap::new();
    for body in ctx.admit_iter(active_bodies, "scan F3D active bodies")? {
        let Some(slot) = stable_ref(ctx, body.id.as_str())? else {
            continue;
        };
        active_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut active,
                slot,
                &body.id,
                "index F3D active feature output bodies",
            )
            .map(|_| ())
        })?;
    }
    let mut scope_storage = ctx.reserve_scoped(0, "index F3D feature scopes")?;
    let mut first_scopes = HashMap::new();
    for scope in ctx.admit_iter(scopes, "index F3D feature scopes")? {
        scope_storage.with_storage(|| {
            ctx.entry_hash_map(&mut first_scopes, scope.id.as_str(), "index F3D feature scopes")?
                .or_insert(scope);
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    // Affected bodies per state, computed only for states a feature names.
    let mut outputs_storage = ctx.reserve_scoped(0, "collect F3D feature output states")?;
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    for feature in ctx.admit_iter(features, "scan F3D feature outputs")? {
        let Some(id) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&scope) = ctx.get_hash_map(&first_scopes, id, "index F3D feature scopes")? else {
            continue;
        };
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id(), scope.previous_history_state_id())
        else {
            continue;
        };
        let Some(&Some((history_index, state))) =
            ctx.get_hash_map(&states_by_id, &state_id, "find F3D history states by id")?
        else {
            continue;
        };
        if state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
        {
            continue;
        }
        if !ctx.contains_key_hash_map(
            &state_outputs,
            &state_id,
            "find F3D history state outputs",
        )? {
            let outputs = outputs_storage.with_storage(|| {
                state_output_bodies(ctx, history_nodes.get(history_index), state)
            })?;
            outputs_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut state_outputs,
                    state_id,
                    outputs,
                    "collect F3D feature output states",
                )
                .map(|_| ())
            })?;
        }
        let Some(Some(outputs)) =
            ctx.get_hash_map(&state_outputs, &state_id, "find F3D history state outputs")?
        else {
            continue;
        };
        let mut resolved = Vec::new();
        for slot in ctx.admit_iter(outputs, "scan F3D feature output slots")? {
            let Some(id) = ctx.get_hash_map(&active, slot, "find F3D history active")? else {
                continue;
            };
            let id = id.try_clone_for_decode(ctx, "copy F3D feature output body identity")?;

            ctx.reserve_vec(&mut resolved, 1, "collect F3D feature output bodies")?;
            resolved.push(id);
        }
        feature.evaluation.set_outputs(
            cadmpeg_ir::features::DistinctMembers::try_from(resolved, ctx)
                .map_err(cadmpeg_core::CodecError::from)?,
        );
        bind_base_feature_output_selection(ctx, feature)?;
    }
    Ok(())
}

/// Bodies changed by one state's transition from the state it follows in its
/// history, when that history links its states by unique node indices.
fn state_output_bodies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nodes: Option<&Option<HashMap<i64, &AsmDeltaState>>>,
    state: &AsmDeltaState,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let Some(Some(nodes)) = nodes else {
        return Ok(None);
    };
    let previous = match state.next_ref {
        Some(node) => match ctx.get_hash_map(nodes, &node, "find F3D history nodes")? {
            Some(previous) => Some(*previous),
            None => return Ok(None),
        },
        None => None,
    };
    topology::affected_body_refs(ctx, state, previous)
}

fn bind_base_feature_output_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
) -> Result<(), cadmpeg_core::CodecError> {
    if feature.evaluation.outputs().is_empty() {
        return Ok(());
    }
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Native(native),
        },
    ) = feature.evaluation.definition()
    else {
        return Ok(());
    };
    let mut selected = Vec::new();
    for body in ctx.admit_iter(
        feature.evaluation.outputs(),
        "scan F3D BaseFeature output bodies",
    )? {
        let id = body.try_clone_for_decode(ctx, "copy F3D BaseFeature body identity")?;

        ctx.reserve_vec(&mut selected, 1, "collect F3D BaseFeature output bodies")?;
        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: ctx.copy_retained_text(native, "copy F3D BaseFeature native selection")?,
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode, SweepShape};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kind_storage = ctx.reserve_scoped(0, "index F3D sweep body kinds")?;
    let mut body_kinds = HashMap::new();
    for body in ctx.admit_iter(bodies, "scan F3D sweep bodies")? {
        body_kind_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut body_kinds,
                &body.id,
                body.kind,
                "index F3D sweep body kinds",
            )
            .map(|_| ())
        })?;
    }
    let mut history_source = IntoIterator::into_iter(features);
    while let Some(feature) = ctx.next_charged(&mut history_source, "scan F3D sweep features")? {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { sections, .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let section_count = sections.len();
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in
            ctx.admit_iter(feature.evaluation.outputs(), "scan F3D sweep output bodies")?
        {
            match ctx.get_hash_map(&body_kinds, output, "resolve F3D sweep output kind")? {
                Some(BodyKind::Sheet) => all_solid = false,
                Some(BodyKind::Solid) => all_sheet = false,
                _ => {
                    all_sheet = false;
                    all_solid = false;
                }
            }
        }
        let mode = if all_sheet {
            SweepMode::Surface {}
        } else if all_solid {
            SweepMode::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
            }
        } else {
            SweepMode::Unresolved {}
        };
        let mut solid_sections = Vec::new();
        if matches!(mode, SweepMode::Solid { .. }) {
            ctx.reserve_vec(
                &mut solid_sections,
                section_count,
                "convert F3D solid sweep sections",
            )?;
        }
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            let SweepShape::Unresolved { section, sections } =
                std::mem::replace(shape, SweepShape::unresolved(None))
            else {
                return;
            };
            *shape = match mode {
                SweepMode::Solid { op } => {
                    match ctx.admit_iter(sections, "convert F3D solid sweep sections") {
                        Ok(sections) => solid_sections.extend(sections.map(Into::into)),
                        Err(error) => {
                            edit_result = Err(cadmpeg_core::CodecError::ResourceLimit(error));
                        }
                    }
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
        edit_result?;
    }
    Ok(())
}

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub(crate) construction_recipes: &'a [crate::records::recipes::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub(crate) persistent_design_links: &'a [crate::records::sketch_links::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub(crate) histories: &'a [AsmHistory],
    /// Neutral top-level bodies.
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub(crate) shells: &'a [cadmpeg_ir::topology::Shell],
}

/// The one value matching `matches`, or `None` when none or several do.
/// Charges each visited value, stopping at a second match.
fn unique_by<'v, T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &'v [T],
    mut matches: impl FnMut(&T) -> Result<bool, cadmpeg_core::CodecError>,
    operation: &'static str,
) -> Result<Option<&'v T>, cadmpeg_core::CodecError> {
    let Some(index) = ctx.position_by(values, &mut matches, operation)? else {
        return Ok(None);
    };
    let repeated = ctx.any_by(&values[index + 1..], matches, operation)?;
    Ok((!repeated).then(|| &values[index]))
}

/// The native stream prefix before the last identity delimiter.
pub(super) fn native_stream_of<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &'a str,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    Ok(ctx
        .rsplit_once(id, ":", "split F3D native stream")?
        .map(|(stream, _)| stream))
}

type IndexKey<'a, K, T> = fn(
    &cadmpeg_core::decode::DecodeContext<'_>,
    &'a T,
) -> Result<Option<K>, cadmpeg_core::CodecError>;

#[derive(Debug)]
enum UniqueLookup<'a, T> {
    Absent,
    Repeated,
    Value(&'a T),
}

/// Values of a slice keyed for lookup, built on the first lookup and held as
/// scratch while the index lives. A key that names one value finds it; a key
/// that several values share finds none, as `DecodeContext::unique_index`
/// keeps it.
pub(crate) struct UniqueIndex<'a, 'ctx, K, T> {
    values: &'a [T],
    key: IndexKey<'a, K, T>,
    operation: &'static str,
    table: std::cell::OnceCell<(
        HashMap<K, Option<&'a T>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
}

impl<'a, 'ctx, K, T> UniqueIndex<'a, 'ctx, K, T>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    pub(crate) fn new(values: &'a [T], key: IndexKey<'a, K, T>, operation: &'static str) -> Self {
        Self {
            values,
            key,
            operation,
            table: std::cell::OnceCell::new(),
        }
    }

    pub(crate) fn get<Q>(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        key: &Q,
    ) -> Result<Option<&'a T>, cadmpeg_core::CodecError>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost + ?Sized,
    {
        Ok(match self.get_entry(ctx, key)? {
            UniqueLookup::Value(value) => Some(value),
            UniqueLookup::Absent | UniqueLookup::Repeated => None,
        })
    }

    /// Distinguishes an absent key from a repeated key's tombstone.
    fn get_entry<Q>(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        key: &Q,
    ) -> Result<UniqueLookup<'a, T>, cadmpeg_core::CodecError>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost + ?Sized,
    {
        let (table, _) = match self.table.get() {
            Some(table) => table,
            None => {
                let built = self.build(ctx)?;
                self.table.get_or_init(|| built)
            }
        };
        Ok(
            match ctx.get_hash_map(table, key, self.operation)?.copied() {
                Some(Some(value)) => UniqueLookup::Value(value),
                Some(None) => UniqueLookup::Repeated,
                None => UniqueLookup::Absent,
            },
        )
    }

    fn build(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<
        (
            HashMap<K, Option<&'a T>>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
        cadmpeg_core::CodecError,
    > {
        let mut storage = ctx.reserve_scoped(0, self.operation)?;
        let mut table = HashMap::new();
        for value in ctx.admit_iter(self.values, self.operation)? {
            let Some(key) = (self.key)(ctx, value)? else {
                continue;
            };
            if let Some(indexed) = ctx.get_mut_hash_map(&mut table, &key, self.operation)? {
                *indexed = None;
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut table, key, Some(value), self.operation)
                    .map(|_| ())
            })?;
        }
        Ok((table, storage))
    }
}

type DesignScope = crate::records::feature::scope::DesignParameterScope;
type OperandGroup = crate::records::topology::construction::DesignConstructionOperandGroup;
type BodyRecipeOperand = crate::records::topology::body_recipe::DesignBodyRecipeOperand;

/// Design scopes by native ID. Each scope ID names one native record.
pub(crate) type ScopeIndex<'a, 'ctx> = UniqueIndex<'a, 'ctx, &'a str, DesignScope>;

pub(crate) fn scope_index<'ctx>(scopes: &[DesignScope]) -> ScopeIndex<'_, 'ctx> {
    UniqueIndex::new(
        scopes,
        |_, scope| Ok(Some(scope.id.as_str())),
        "index F3D feature scopes",
    )
}

/// Operand groups by native ID and owning scope record. Eligibility is
/// applied within a bucket before requiring a unique group.
type GroupsById<'a> = HashMap<(&'a str, u32), Vec<&'a OperandGroup>>;

pub(crate) struct GroupIndex<'a, 'ctx> {
    values: &'a [OperandGroup],
    table: std::cell::OnceCell<(
        GroupsById<'a>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
}

pub(crate) fn group_index<'ctx>(groups: &[OperandGroup]) -> GroupIndex<'_, 'ctx> {
    GroupIndex { values: groups, table: std::cell::OnceCell::new() }
}

/// Returns the only group named `id` that belongs to `scope`'s stream and
/// record and passes `accept`.
fn scope_group<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    groups: &GroupIndex<'a, 'ctx>,
    scope: &DesignScope,
    id: &str,
    mut accept: impl FnMut(&OperandGroup) -> bool,
) -> Result<Option<&'a OperandGroup>, cadmpeg_core::CodecError> {
    let operation = "index F3D operand groups";
    let (table, _) = match groups.table.get() {
        Some(table) => table,
        None => {
            let mut storage = ctx.reserve_scoped(0, operation)?;
            let mut table = HashMap::new();
            for group in ctx.admit_iter(groups.values, operation)? {
                storage.with_storage(|| ctx.push_hash_group(
                    &mut table, (group.id.as_str(), group.scope_record_index), group,
                    operation, operation,
                ))?;
            }
            groups.table.get_or_init(|| (table, storage))
        }
    };
    let Some(bucket) = ctx.get_hash_map(table, &(id, scope.record_index), operation)? else {
        return Ok(None);
    };
    let stream = native_stream_of(ctx, &scope.id)?;
    let mut eligible = |group: &&OperandGroup| {
        if !accept(group) { return Ok(false); }
        ctx.equal(
            &native_stream_of(ctx, &group.id)?, &stream,
            "compare F3D operand group stream",
        )
    };
    let Some(position) = ctx.position_by(bucket, &mut eligible, "find F3D eligible operand group")? else {
        return Ok(None);
    };
    if ctx.any_by(&bucket[position + 1..], eligible, "find F3D eligible operand group")? {
        return Ok(None);
    }
    Ok(Some(bucket[position]))
}

/// Stream, owning group record, member ordinal and record of a group-owned
/// body recipe operand.
type MemberOperandKey<'a> = (Option<&'a str>, u32, u32, u32);
/// Stream, owning scope record and record of a scope-reference body recipe
/// operand.
type ScopeOperandKey<'a> = (Option<&'a str>, u32, u32);

fn member_operand_key<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &'a BodyRecipeOperand,
) -> Result<Option<MemberOperandKey<'a>>, cadmpeg_core::CodecError> {
    let Some((group_record_index, ordinal)) = operand.owner.group() else {
        return Ok(None);
    };
    Ok(Some((
        native_stream_of(ctx, &operand.id)?,
        group_record_index,
        ordinal,
        operand.record_index(),
    )))
}

fn scope_operand_key<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &'a BodyRecipeOperand,
) -> Result<Option<ScopeOperandKey<'a>>, cadmpeg_core::CodecError> {
    if !matches!(
        operand.owner,
        crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
    ) {
        return Ok(None);
    }
    Ok(Some((
        native_stream_of(ctx, &operand.id)?,
        operand.scope_record_index,
        operand.record_index(),
    )))
}

/// Scope-reference body recipe operands by stream, scope record and record.
pub(crate) type ScopeOperandIndex<'a, 'ctx> =
    UniqueIndex<'a, 'ctx, ScopeOperandKey<'a>, BodyRecipeOperand>;

pub(crate) fn scope_operand_index<'ctx>(
    operands: &[BodyRecipeOperand],
) -> ScopeOperandIndex<'_, 'ctx> {
    UniqueIndex::new(
        operands,
        scope_operand_key,
        "index F3D scope-reference body recipe operands",
    )
}

/// Body construction recipes by native ID.
pub(crate) type BodyRecipeIndex<'a, 'ctx> =
    UniqueIndex<'a, 'ctx, &'a str, crate::records::recipes::ConstructionRecipe>;

pub(crate) fn body_recipe_index<'ctx>(
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> BodyRecipeIndex<'_, 'ctx> {
    UniqueIndex::new(
        recipes,
        |_, recipe| {
            Ok(
                (recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body)
                    .then_some(recipe.id.as_str()),
            )
        },
        "index F3D body construction recipes",
    )
}

/// Active bodies by identity and the body each shell face belongs to. A face
/// in several shells maps to the last shell whose region names a body, and a
/// repeated identity maps to its last record.
struct ExternalBodies<'a, 'ctx> {
    body_by_face: HashMap<&'a cadmpeg_ir::ids::FaceId, &'a cadmpeg_ir::ids::BodyId>,
    bodies: HashMap<&'a cadmpeg_ir::ids::BodyId, &'a cadmpeg_ir::topology::Body>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn external_bodies<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    bodies: &'a [cadmpeg_ir::topology::Body],
    regions: &'a [cadmpeg_ir::topology::Region],
    shells: &'a [cadmpeg_ir::topology::Shell],
) -> Result<ExternalBodies<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D external bodies")?;
    let (body_by_region, _region_storage) =
        ctx.with_scoped_storage("index F3D external body regions", || {
            ctx.collect_hash_map(
                regions.iter().map(|region| (&region.id, &region.body)),
                "index F3D external body regions",
            )
        })?;
    let mut body_by_face = HashMap::new();
    for shell in ctx.admit_iter(shells, "scan F3D external body shells")? {
        let Some(body) = ctx.get_hash_map(
            &body_by_region,
            &shell.region,
            "find F3D external shell body",
        )?
        else {
            continue;
        };
        for face in ctx.admit_iter(shell.faces(), "scan F3D external shell faces")? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut body_by_face,
                    face,
                    *body,
                    "index F3D external body faces",
                )
                .map(|_| ())
            })?;
        }
    }
    let bodies = storage.with_storage(|| {
        ctx.collect_hash_map(
            bodies.iter().map(|body| (&body.id, body)),
            "index F3D external body metadata",
        )
    })?;
    Ok(ExternalBodies {
        body_by_face,
        bodies,
        _storage: storage,
    })
}

/// Persistent design links by design identifier and reference, and whether
/// each link is its target's latest: the highest ordinal, the earliest link
/// among equal ordinals.
struct PersistentBodyLinks<'a, 'ctx> {
    by_design: HashMap<(&'a str, i64), Vec<usize>>,
    current: Vec<bool>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn link_target_key(target: &cadmpeg_ir::attributes::AttributeTarget) -> (u8, &str) {
    use cadmpeg_ir::attributes::AttributeTarget;
    match target {
        AttributeTarget::Document => (0, ""),
        AttributeTarget::Body(id) => (1, id.as_str()),
        AttributeTarget::Face(id) => (2, id.as_str()),
        AttributeTarget::Shell(id) => (3, id.as_str()),
        AttributeTarget::Loop(id) => (4, id.as_str()),
        AttributeTarget::Coedge(id) => (5, id.as_str()),
        AttributeTarget::Edge(id) => (6, id.as_str()),
        AttributeTarget::Vertex(id) => (7, id.as_str()),
    }
}

fn persistent_body_links<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    links: &'a [crate::records::sketch_links::PersistentDesignLink],
) -> Result<PersistentBodyLinks<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D persistent body links")?;
    let mut latest_storage = ctx.reserve_scoped(0, "index F3D latest persistent links")?;
    let mut latest = HashMap::<(u8, &str), usize>::new();
    let mut by_design = HashMap::new();
    for (index, link) in ctx
        .admit_iter(links, "scan F3D persistent body links")?
        .enumerate()
    {
        let target = link_target_key(&link.target);
        match ctx.get_mut_hash_map(&mut latest, &target, "index F3D latest persistent links")? {
            Some(latest) => {
                if link.ordinal > links[*latest].ordinal {
                    *latest = index;
                }
            }
            None => latest_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut latest,
                    target,
                    index,
                    "index F3D latest persistent links",
                )
                .map(|_| ())
            })?,
        }
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut by_design,
                (link.design_id.as_str(), link.design_reference),
                index,
                "index F3D persistent body links",
                "group F3D persistent body links",
            )
        })?;
    }
    let mut current = storage
        .with_storage(|| ctx.collection_vec(links.len(), "mark F3D latest persistent links"))?;
    for (index, link) in ctx
        .admit_iter(links, "mark F3D latest persistent links")?
        .enumerate()
    {
        let target = link_target_key(&link.target);
        current.push(
            ctx.get_hash_map(&latest, &target, "find F3D latest persistent link")? == Some(&index),
        );
    }
    Ok(PersistentBodyLinks {
        by_design,
        current,
        _storage: storage,
    })
}

/// Body-selection inputs keyed for the per-feature lookups of one binding
/// pass. Each index is built on its first lookup and held as scratch while
/// this value lives.
pub(crate) struct BodySelectionIndex<'a, 'ctx> {
    inputs: &'a FeatureBodySelectionInputs<'a>,
    scopes: ScopeIndex<'a, 'ctx>,
    groups: GroupIndex<'a, 'ctx>,
    member_operands: UniqueIndex<'a, 'ctx, MemberOperandKey<'a>, BodyRecipeOperand>,
    scope_operands: ScopeOperandIndex<'a, 'ctx>,
    recipes: BodyRecipeIndex<'a, 'ctx>,
    external: std::cell::OnceCell<ExternalBodies<'a, 'ctx>>,
    links: std::cell::OnceCell<PersistentBodyLinks<'a, 'ctx>>,
}

impl<'a, 'ctx> BodySelectionIndex<'a, 'ctx> {
    pub(crate) fn new(inputs: &'a FeatureBodySelectionInputs<'a>) -> Self {
        Self {
            inputs,
            scopes: scope_index(inputs.scopes),
            groups: group_index(inputs.groups),
            member_operands: UniqueIndex::new(
                inputs.body_recipe_operands,
                member_operand_key,
                "index F3D group body recipe operands",
            ),
            scope_operands: scope_operand_index(inputs.body_recipe_operands),
            recipes: body_recipe_index(inputs.construction_recipes),
            external: std::cell::OnceCell::new(),
            links: std::cell::OnceCell::new(),
        }
    }

    fn external(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<&ExternalBodies<'a, 'ctx>, cadmpeg_core::CodecError> {
        if let Some(external) = self.external.get() {
            return Ok(external);
        }
        let built = external_bodies(
            ctx,
            self.inputs.bodies,
            self.inputs.regions,
            self.inputs.shells,
        )?;
        Ok(self.external.get_or_init(|| built))
    }

    fn links(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<&PersistentBodyLinks<'a, 'ctx>, cadmpeg_core::CodecError> {
        if let Some(links) = self.links.get() {
            return Ok(links);
        }
        let built = persistent_body_links(ctx, self.inputs.persistent_design_links)?;
        Ok(self.links.get_or_init(|| built))
    }
}

pub(crate) fn bind_feature_body_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let histories = inputs.histories;
    let index = BodySelectionIndex::new(inputs);
    let mut feature_states = FeatureHistoryStates::new(ctx)?;

    bind_pattern_body_selections(ctx, features, &index)?;
    let mut pattern_body_slots_storage =
        ctx.reserve_scoped(0, "index F3D pattern body features")?;
    let mut pattern_body_slots = HashMap::new();
    let mut history_source = IntoIterator::into_iter(&*features);
    while let Some(feature) =
        ctx.next_charged(&mut history_source, "scan F3D pattern body features")?
    {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::patterns::PatternTransform::Circular { count, .. } =
            pattern.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Bodies(BodySelection::Historical {
            bodies: seed_bodies,
            ..
        })] = seeds.as_slice()
        else {
            continue;
        };
        let [seed_body] = seed_bodies.as_slice() else {
            continue;
        };
        let Ok(expected_count) = usize::try_from(*count) else {
            continue;
        };
        if feature.evaluation.outputs().len().checked_add(1) != Some(expected_count) {
            continue;
        }
        let Some(seed_slot) = historical_body_slot(ctx, seed_body.as_str())? else {
            continue;
        };
        let (candidate, candidate_storage) = ctx.with_scoped_storage(
            "index F3D pattern body slots",
            || -> Result<_, cadmpeg_core::CodecError> {
                let mut slots = BTreeSet::new();
                ctx.insert_btree_set(&mut slots, seed_slot, "index F3D pattern body slots")
                    .map(|_| ())?;
                let mut history_source = IntoIterator::into_iter(feature.evaluation.outputs());
                while let Some(body) =
                    ctx.next_charged(&mut history_source, "scan F3D pattern body outputs")?
                {
                    let Some(slot) = stable_ref(ctx, body.as_str())? else {
                        return Ok(None);
                    };
                    ctx.insert_btree_set(&mut slots, slot, "index F3D pattern body slots")
                        .map(|_| ())?;
                }
                if slots.len() != expected_count {
                    return Ok(None);
                }
                let feature_id = feature
                    .id
                    .try_clone_for_decode(ctx, "copy F3D pattern body feature ID")?;
                Ok(Some((feature_id, slots)))
            },
        )?;
        if let Some((feature_id, slots)) = candidate {
            pattern_body_slots_storage.with_storage(|| {
                candidate_storage.commit()?;
                ctx.insert_hash_map(
                    &mut pattern_body_slots,
                    feature_id,
                    slots,
                    "index F3D pattern body features",
                )
                .map(|_| ())
            })?;
        }
    }

    let mut history_source = IntoIterator::into_iter(features);
    while let Some(feature) =
        ctx.next_charged(&mut history_source, "scan F3D body selection features")?
    {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let scope = match index.scopes.get(ctx, native_ref) {
                Ok(Some(scope)) => scope,
                Ok(None) => break 'feature_edit,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            if matches!(
                definition,
                FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
            ) {
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill { tools, cells }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        tools,
                        feature_id,
                        previous_state_id,
                        scope,
                        &index,
                    );
                    let mut cells = cells.iter_mut();
                    while edit_result.is_ok() {
                        let cell = match ctx
                            .next_charged(&mut cells, "scan F3D BoundaryFill cell selections")
                        {
                            Ok(Some(cell)) => cell,
                            Ok(None) => break,
                            Err(error) => {
                                edit_result = Err(error);
                                break;
                            }
                        };
                        edit_result = bind_body_recipe_body_selection(
                            ctx,
                            cell,
                            feature_id,
                            previous_state_id,
                            scope,
                            &index,
                        );
                    }
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, tools, scope, &index);
                    let mut cells = cells.iter_mut();
                    while edit_result.is_ok() {
                        let cell = match ctx
                            .next_charged(&mut cells, "scan F3D BoundaryFill cell selections")
                        {
                            Ok(Some(cell)) => cell,
                            Ok(None) => break,
                            Err(error) => {
                                edit_result = Err(error);
                                break;
                            }
                        };
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, cell, scope, &index);
                    }
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                let edit = operands.try_edit(|target, tools| {
                    macro_rules! admitted {
                        ($value:expr) => {
                            match $value {
                                Ok(value) => value,
                                Err(error) => {
                                    edit_result = Err(error);
                                    return;
                                }
                            }
                        };
                    }
                    let (Some(state_id), Some(previous_state_id)) =
                        (scope.history_state_id(), scope.previous_history_state_id())
                    else {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, target, scope, &index);
                        if edit_result.is_err() {
                            return;
                        }
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, tools, scope, &index);
                        if edit_result.is_err() {
                            return;
                        }
                        if matches!(
                            tools,
                            BodySelection::Native(_) | BodySelection::NativeSet(_)
                        ) {
                            if let Some(local) = admitted!(combine_external_local_tools(ctx, scope))
                            {
                                *tools = local;
                            }
                        }
                        return;
                    };
                    if let Some(local) = admitted!(combine_external_local_tools(ctx, scope)) {
                        *tools = local;
                    }
                    let BodySelection::Native(native) = target else {
                        return;
                    };
                    let Some((history, state, _)) = admitted!(unique_history_state_pair(
                        ctx,
                        histories,
                        state_id,
                        previous_state_id,
                    )) else {
                        return;
                    };
                    let history_states = match feature_states.states_of(ctx, history) {
                        Ok(Some(states)) => states,
                        Ok(None) => return,
                        Err(error) => {
                            edit_result = Err(error);
                            return;
                        }
                    };
                    let body = match singleton_revised_input_body_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        history_states,
                    ) {
                        Ok(Some(body)) => body,
                        Ok(None) => return,
                        Err(error) => {
                            edit_result = Err(error);
                            return;
                        }
                    };
                    let (historical, target_storage) = admitted!(ctx.with_scoped_storage(
                        "retain F3D Combine target selection",
                        || -> Result<_, cadmpeg_core::CodecError> {
                            let body_id = crate::ids::history_input_body_id_charged(
                                ctx,
                                feature_id,
                                previous_state_id,
                                body,
                            )?;
                            let native_id =
                                ctx.copy_retained_text(native, "copy F3D Combine target identity")?;
                            let mut target_bodies =
                                ctx.collection_vec(1, "validate F3D Combine target body")?;
                            target_bodies.push(body_id);
                            Ok(BodySelection::historical(
                                crate::ids::history_input_state_id_charged(
                                    ctx,
                                    feature_id,
                                    previous_state_id,
                                )?,
                                target_bodies,
                                native_id,
                                ctx,
                            )?
                            .ok())
                        }
                    ));
                    if let Some(historical) = historical {
                        admitted!(target_storage.commit());
                        *target = historical;
                    }
                    if let Some(selection) = admitted!(combine_history_tools(
                        ctx,
                        CombineHistoryTools {
                            feature_id,
                            scope,
                            state,
                            previous_state_id,
                            target_body: body,
                            dependencies,
                            pattern_bodies: &pattern_body_slots
                        },
                        &index
                    )) {
                        *tools = selection;
                    }
                });
                if let Err(error) = edit {
                    edit_result = Err(cadmpeg_core::CodecError::malformed(error));
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) = definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        targets,
                        feature_id,
                        previous_state_id,
                        scope,
                        &index,
                    );
                } else {
                    edit_result =
                        bind_direct_body_recipe_body_selection(ctx, targets, scope, &index);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        &index,
                    );
                } else {
                    edit_result =
                        bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) = definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        &index,
                    );
                    if edit_result.is_ok() && matches!(bodies, BodySelection::Native(_)) {
                        edit_result =
                            bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                    }
                } else {
                    edit_result =
                        bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                }
                break 'feature_edit;
            }
            let (bodies, proof) = match definition {
                FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                    (bodies, BodySelectionProof::TopologyStableRevision)
                }
                FeatureDefinition::Operation(FeatureOperation::Shell {
                    bodies: Some(bodies),
                    ..
                }) => (bodies, BodySelectionProof::RevisedInput),
                FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) => {
                    (targets, BodySelectionProof::RevisedInput)
                }
                _ => break 'feature_edit,
            };
            let BodySelection::Native(group_id) = bodies else {
                break 'feature_edit;
            };
            let group = match scope_group(ctx, &index.groups, scope, group_id, |group| {
                group.role() == DesignOperandRole::BODIES_A
            }) {
                Ok(Some(group)) => group,
                Ok(None) => break 'feature_edit,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            if group.members().len() != 1 {
                break 'feature_edit;
            }
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id(), scope.previous_history_state_id())
            else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                break 'feature_edit;
            };
            let Some((history, state, _previous)) =
                (match unique_history_state_pair(ctx, histories, state_id, previous_state_id) {
                    Ok(pair) => pair,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                })
            else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, &index);
                break 'feature_edit;
            };
            let history_states = match feature_states.states_of(ctx, history) {
                Ok(Some(states)) => states,
                Ok(None) => break 'feature_edit,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let body = match proof {
                BodySelectionProof::TopologyStableRevision => {
                    singleton_body_revision_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        history_states,
                    )
                }
                BodySelectionProof::RevisedInput => {
                    singleton_revised_input_body_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        history_states,
                    )
                }
            };
            let body = match body {
                Ok(body) => body,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let Some(body) = body else {
                break 'feature_edit;
            };
            let state_id =
                crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id);
            let body_id =
                crate::ids::history_input_body_id_charged(ctx, feature_id, previous_state_id, body);
            let native = ctx.copy_retained_text(group_id, "copy F3D pattern body group identity");
            let (state_id, body_id, native) = match (state_id, body_id, native) {
                (Ok(state_id), Ok(body_id), Ok(native)) => (state_id, body_id, native),
                (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let mut selected = match ctx.collection_vec(1, "validate F3D pattern body selection") {
                Ok(selected) => selected,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            selected.push(body_id);
            match cadmpeg_ir::features::BodySelection::historical(state_id, selected, native, ctx) {
                Ok(Ok(historical)) => *bodies = historical,
                Ok(Err(_)) => {}
                Err(limit) => {
                    edit_result = Err(limit.into());
                    break 'feature_edit;
                }
            }
        });
        edit_result?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct CombineHistoryTools<'a> {
    feature_id: &'a cadmpeg_ir::features::FeatureId,
    scope: &'a DesignScope,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    target_body: i64,
    dependencies: &'a cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    pattern_bodies: &'a HashMap<cadmpeg_ir::features::FeatureId, BTreeSet<i64>>,
}

/// Prove one complete tool lane before encoding the identities it keeps.
fn combine_history_tools<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    input: CombineHistoryTools<'a>,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<Option<cadmpeg_ir::features::BodySelection>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodyMembers, BodySelection, DistinctMembers};
    let Some(stream) = native_stream_of(ctx, &input.scope.id)? else {
        return Ok(None);
    };
    let Some(operation) = input.scope.combine_operation() else {
        return Ok(None);
    };
    let count = operation
        .tools
        .additional
        .len()
        .checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit("count F3D Combine tools", u64::MAX - 1, u64::MAX))?;
    let mut cached_source = None;
    // Proof rows borrow direct identities and store scalar historical slots.
    let mut proof_storage = ctx.reserve_scoped(0, "collect F3D Combine tool proof")?;
    let mut historical = Vec::new();
    let mut direct = Vec::new();
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D Combine tool bodies")?;
    let mut historical_slots = HashSet::new();
    let mut direct_bodies = HashSet::new();
    let mut first = Some(&operation.tools.first);
    let mut additional = operation.tools.additional.iter();
    let mut complete = true;
    loop {
        let tool = match first.take() {
            Some(first) => first,
            None => match ctx.next_charged(&mut additional, "scan F3D Combine tool identities")? {
                Some(tool) => tool,
                None => break,
            },
        };
        let record = tool.record_index;
        let Some(operand) = index
            .scope_operands
            .get(ctx, &(Some(stream), input.scope.record_index, record))?
        else {
            complete = false;
            break;
        };
        if let Some(slot) = operand.resolved_body_slot {
            if !seen_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut historical_slots,
                    slot,
                    "index F3D Combine historical tool slots",
                )
            })? {
                complete = false;
                break;
            }
            proof_storage.with_storage(|| {
                ctx.push_vec(
                    &mut historical,
                    slot,
                    "collect F3D Combine historical tool rows",
                )
            })?;
        } else {
            let source = match cached_source {
                Some(source) => source,
                None => {
                    let source = historical_brep_source(ctx, &input.state.id)?;
                    cached_source = Some(source);
                    source
                }
            };
            let Some(body) =
                unique_external_body_candidate(ctx, operand, source, index.external(ctx)?)?
            else {
                complete = false;
                break;
            };
            if !seen_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut direct_bodies,
                    body,
                    "index F3D Combine direct tool bodies",
                )
            })? {
                complete = false;
                break;
            }
            proof_storage.with_storage(|| {
                ctx.push_vec(
                    &mut direct,
                    (record, body),
                    "collect F3D Combine direct tool rows",
                )
            })?;
        }
    }
    drop(historical_slots);
    drop(direct_bodies);
    drop(seen_storage);
    let (selection, output_storage) = ctx.with_scoped_storage(
        "retain F3D Combine tool selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            if complete && direct.len() == count {
                if let [(record, body)] = direct.as_slice() {
                    let body =
                        body.try_clone_for_decode(ctx, "copy F3D Combine direct tool body")?;
                    let native = ctx.format_retained(
                        format_args!("{stream}:design-record#{record}"),
                        "retain F3D Combine tool identity",
                    )?;
                    let mut selected =
                        ctx.collection_vec(1, "validate F3D Combine resolved body")?;
                    selected.push(body);
                    let bodies = match DistinctMembers::try_from(selected, ctx) {
                        Ok(value) => value,
                        Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => {
                            return Ok(None)
                        }
                        Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                            return Err(limit.into())
                        }
                    };
                    return Ok(Some(BodySelection::Resolved { bodies, native }));
                }
                let mut rows = ctx.collection_vec(count, "collect F3D Combine direct tool rows")?;
                for &(record, body) in
                    ctx.admit_iter(&direct, "scan F3D Combine direct tool proof")?
                {
                    let body =
                        body.try_clone_for_decode(ctx, "copy F3D Combine direct tool body")?;
                    let native = ctx.format_retained(
                        format_args!("{stream}:design-record#{record}"),
                        "retain F3D Combine tool identity",
                    )?;
                    let Some(row) = body_member(ctx, body, native)? else {
                        return Ok(None);
                    };
                    rows.push(row);
                }
                return Ok(BodyMembers::try_from_rows(rows, ctx)?
                    .ok()
                    .map(|members| BodySelection::ResolvedSet { members }));
            }
            let (slots, temporary) = if complete && historical.len() == count {
                (historical, proof_storage)
            } else {
                // Neither partial lane contributes identities to a fallback.
                drop(historical);
                drop(direct);
                drop(proof_storage);
                let mut record_storage =
                    ctx.reserve_scoped(0, "collect F3D Combine tool record indices")?;
                let records = record_storage.with_storage(|| {
                    ctx.collect_vec(
                        operation.tools.iter().map(|tool| tool.record_index),
                        "collect F3D Combine tool record indices",
                    )
                })?;
                let (slots, slots_storage) =
                    ctx.with_scoped_storage("collect F3D Combine fallback slots", || {
                        if let Some(slots) = combine_recipe_family_tool_slots(
                            ctx,
                            (stream, input.scope.record_index),
                            &records,
                            input.previous_state_id,
                            input.target_body,
                            &index.scope_operands,
                            &index.recipes,
                        )? {
                            return Ok::<_, cadmpeg_core::CodecError>(Some(slots));
                        }
                        let mut pattern = None;
                        let mut dependencies = input.dependencies.iter();
                        while let Some(dependency) = ctx
                            .next_charged(&mut dependencies, "scan F3D pattern body dependencies")?
                        {
                            if let Some(bodies) = ctx.get_hash_map(
                                input.pattern_bodies,
                                dependency,
                                "find F3D pattern body dependency",
                            )? {
                                if pattern.is_some() {
                                    return Ok(None);
                                }
                                pattern = Some(bodies);
                            }
                        }
                        match pattern {
                            Some(pattern) => {
                                pattern_combine_tool_slots(ctx, pattern, input.target_body, count)
                            }
                            None => Ok(None),
                        }
                    })?;
                let Some(slots) = slots else {
                    return Ok(None);
                };
                if slots.len() != count {
                    return Ok(None);
                }
                (slots, slots_storage)
            };
            historical_combine_selection(ctx, &input, stream, slots, temporary)
        },
    )?;
    if selection.is_some() {
        output_storage.commit()?;
    }
    Ok(selection)
}

/// Encode a proved historical lane directly into its final member rows.
fn historical_combine_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    input: &CombineHistoryTools<'_>,
    stream: &str,
    slots: Vec<i64>,
    _temporary: cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<cadmpeg_ir::features::BodySelection>, cadmpeg_core::CodecError> {
    let Some(operation) = input.scope.combine_operation() else {
        return Ok(None);
    };
    let native = std::iter::once(&operation.tools.first)
        .chain(operation.tools.additional.iter())
        .map(|tool| {
            ctx.format_retained(
                format_args!("{stream}:design-record#{}", tool.record_index),
                "retain F3D Combine tool identity",
            )
        });
    let Some(rows) = combine_historical_rows(
        ctx,
        input.feature_id,
        input.previous_state_id,
        slots,
        native,
    )?
    else {
        return Ok(None);
    };
    let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)? else {
        return Ok(None);
    };
    let state =
        crate::ids::history_input_state_id_charged(ctx, input.feature_id, input.previous_state_id)?;
    Ok(Some(cadmpeg_ir::features::BodySelection::HistoricalSet {
        state,
        members,
    }))
}

fn combine_historical_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    slots: Vec<i64>,
    native_tools: impl IntoIterator<Item = Result<String, cadmpeg_core::CodecError>>,
) -> Result<
    Option<Vec<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>>,
    cadmpeg_core::CodecError,
> {
    let (output, storage) = ctx.with_scoped_storage(
        "retain F3D Combine fallback selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            ctx.collect_fallible_options(
                slots.into_iter().zip(native_tools).map(|(slot, native)| {
                    let native = native?;
                    let body = crate::ids::history_input_body_id_charged(
                        ctx,
                        feature_id,
                        previous_state_id,
                        slot,
                    )?;
                    body_member(ctx, body, native).map_err(cadmpeg_core::CodecError::ResourceLimit)
                }),
                "collect F3D Combine fallback rows",
            )
        },
    )?;
    if output.is_some() {
        storage.commit()?;
    }
    Ok(output)
}

fn pattern_combine_tool_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    if pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
        || !ctx.contains_btree_set(
            pattern_bodies,
            &target_body,
            "find F3D pattern Combine target",
        )?
    {
        return Ok(None);
    }
    let mut tools = ctx.collection_vec(native_tool_count, "collect F3D pattern Combine tools")?;
    for &body in ctx.admit_iter(pattern_bodies, "scan F3D pattern Combine bodies")? {
        if body != target_body {
            tools.push(body);
        }
    }
    Ok(Some(tools))
}

fn combine_recipe_family_tool_slots<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scope_key: (&'a str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &ScopeOperandIndex<'a, 'ctx>,
    recipes: &BodyRecipeIndex<'a, 'ctx>,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    type FamilyKey<'a> = (
        &'a crate::records::mesh::DesignRelaxedGuidText,
        &'a crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u32,
        &'a str,
    );
    type FamilyMember<'a> = (u32, Option<i64>, &'a [i64]);
    let (stream, scope_record_index) = scope_key;

    if tool_record_indices.is_empty() {
        return Ok(None);
    }
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D Combine tool record indices")?;
    let mut seen = HashSet::new();
    let mut history_source = IntoIterator::into_iter(tool_record_indices);
    while let Some(&index) =
        ctx.next_charged(&mut history_source, "scan F3D Combine tool record indices")?
    {
        if !seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, index, "index F3D Combine tool record indices")
        })? {
            return Ok(None);
        }
    }
    let mut families_storage = ctx.reserve_scoped(0, "index F3D Combine tool families")?;
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    let mut history_source = IntoIterator::into_iter(tool_record_indices);
    while let Some(&record_index) =
        ctx.next_charged(&mut history_source, "scan F3D Combine tool record indices")?
    {
        let operand =
            required!(operands.get(ctx, &(Some(stream), scope_record_index, record_index))?);
        let [reference] = operand.references().as_slice() else {
            return Ok(None);
        };
        let recipe = required!(recipes.get(ctx, operand.recipe_id.as_str())?);
        if !ctx.equal(
            &native_stream_of(ctx, &recipe.id)?,
            &Some(stream),
            "compare F3D Combine recipe streams",
        )? {
            return Ok(None);
        }
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return Ok(None);
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return Ok(None),
        };
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        families_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut families,
                key,
                (
                    selector,
                    resolved,
                    reference.preceding_body_slots.as_slice(),
                ),
                "index F3D Combine tool families",
                "collect F3D Combine family members",
            )
        })?;
    }

    let mut selected_storage = ctx.reserve_scoped(0, "index F3D Combine selected tools")?;
    let mut selected = BTreeSet::new();
    let mut history_source = IntoIterator::into_iter(&families);
    while let Some((_, family)) =
        ctx.next_charged(&mut history_source, "scan F3D Combine tool families")?
    {
        let mut family_storage = ctx.reserve_scoped(0, "index F3D Combine family bodies")?;
        let mut exact = BTreeSet::new();
        let mut all_exact = true;
        for (_, body, _) in ctx.admit_iter(family, "scan F3D Combine exact tool bodies")? {
            let Some(body) = *body else {
                all_exact = false;
                continue;
            };
            ctx.insert_scoped_btree_set(
                &mut family_storage,
                &mut exact,
                body,
                "find F3D Combine exact tool body",
                "index F3D Combine exact tool bodies",
            )?;
        }
        if all_exact {
            if exact.len() != family.len() {
                return Ok(None);
            }
            for body in ctx.admit_iter(exact, "scan F3D Combine exact tool bodies")? {
                ctx.insert_scoped_btree_set(
                    &mut selected_storage,
                    &mut selected,
                    body,
                    "find F3D Combine selected tool",
                    "index F3D Combine selected tools",
                )?;
            }
            continue;
        }
        // Distinct selectors covering 1..=n are exactly n distinct values in
        // that range.
        let count = required!(u32::try_from(family.len()).ok());
        let mut selectors = BTreeSet::new();
        let mut history_source = IntoIterator::into_iter(family);
        while let Some(&(selector, _, _)) =
            ctx.next_charged(&mut history_source, "scan F3D family")?
        {
            if selector == 0
                || selector > count
                || !ctx.insert_scoped_btree_set(
                    &mut family_storage,
                    &mut selectors,
                    selector,
                    "find F3D Combine selector",
                    "index F3D Combine selectors",
                )?
            {
                return Ok(None);
            }
        }
        let Some(first) = ctx.position_by(
            family,
            |(_, _, candidates)| Ok(!candidates.is_empty()),
            "find F3D Combine candidate tools",
        )?
        else {
            return Ok(None);
        };
        let mut candidates = BTreeSet::new();
        for &candidate in ctx.admit_iter(family[first].2, "scan F3D Combine candidate tools")? {
            ctx.insert_scoped_btree_set(
                &mut family_storage,
                &mut candidates,
                candidate,
                "find F3D Combine candidate tool",
                "index F3D Combine candidate tools",
            )?;
        }
        if candidates.len() != family.len()
            || ctx.contains_btree_set(&candidates, &target_body, "find F3D Combine target tool")?
            || !ctx.is_subset_btree_set(&exact, &candidates, "check F3D Combine exact tools")?
        {
            return Ok(None);
        }
        // Every other non-empty candidate list must name exactly this set.
        let mismatched = ctx.any_by(
            &family[first + 1..],
            |(_, _, other)| {
                if other.is_empty() {
                    return Ok(false);
                }
                let (other, _other_storage) =
                    ctx.with_scoped_storage("index F3D Combine other candidate tools", || {
                        ctx.collect_btree_set(
                            other.iter().copied(),
                            "index F3D Combine other candidate tools",
                        )
                    })?;
                Ok(other.len() != candidates.len()
                    || !ctx.is_subset_btree_set(
                        &other,
                        &candidates,
                        "compare F3D Combine candidate tools",
                    )?)
            },
            "scan F3D Combine candidate tool lists",
        )?;
        if mismatched {
            return Ok(None);
        }
        for body in ctx.admit_iter(candidates, "scan F3D Combine candidate tools")? {
            ctx.insert_scoped_btree_set(
                &mut selected_storage,
                &mut selected,
                body,
                "find F3D Combine selected tool",
                "index F3D Combine selected tools",
            )?;
        }
    }
    if selected.len() != tool_record_indices.len()
        || ctx.contains_btree_set(&selected, &target_body, "find F3D Combine target tool")?
    {
        return Ok(None);
    }
    let mut slots = ctx.collection_vec(selected.len(), "collect F3D Combine recipe tool slots")?;
    for body in ctx.admit_iter(selected, "collect F3D Combine recipe tool slots")? {
        slots.push(body);
    }
    Ok(Some(slots))
}

fn combine_external_local_tools(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<Option<cadmpeg_ir::features::BodySelection>, cadmpeg_core::CodecError> {
    let (output, storage) = ctx.with_scoped_storage(
        "retain F3D Combine external selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            let Some(operation) = scope.combine_operation() else {
                return Ok(None);
            };
            let mut bodies = Vec::new();
            let mut first = Some(&operation.tools.first);
            let mut additional = operation.tools.additional.iter();
            loop {
                let tool = match first.take() {
                    Some(first) => first,
                    None => match ctx.next_charged(&mut additional, "scan F3D operation tools")? {
                        Some(tool) => tool,
                        None => break,
                    },
                };
                let Some(identity) = tool.external_identity.as_ref() else {
                    return Ok(None);
                };
                let id = crate::ids::neutral_combine_external_body_id_charged(ctx, identity)?;

                if ctx.any_by(
                    &bodies,
                    |body| ctx.equal(body, &id, "compare F3D Combine external tool identities"),
                    "scan F3D Combine external tool identities",
                )? {
                    return Ok(None);
                }
                ctx.push_vec(&mut bodies, id, "collect F3D Combine external tools")?;
            }
            let native = ctx.copy_retained_text(&scope.id, "copy F3D Combine scope identity")?;
            Ok(cadmpeg_ir::features::BodySelection::local(bodies, native, ctx)?.ok())
        },
    )?;
    if output.is_some() {
        storage.commit()?;
    }
    Ok(output)
}

fn historical_body_slot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(id) = ctx.strip_prefix(
        id,
        "f3d:history-input:body#",
        "strip F3D historical body identity prefix",
    )?
    else {
        return Ok(None);
    };
    let Some((_, slot)) = ctx.rsplit_once(id, ":", "split F3D historical body slot")? else {
        return Ok(None);
    };
    Ok(ctx
        .parse_text::<i64>(slot, "parse F3D historical body slot")?
        .ok())
}

fn bind_pattern_body_selections<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    index: &BodySelectionIndex<'_, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };

    // The non-empty second-body group of each scope, by stream and scope record.
    let seed_groups = UniqueIndex::new(
        index.inputs.groups,
        |ctx, group| {
            if group.role() != DesignOperandRole::BODIES_B || group.members().is_empty() {
                return Ok(None);
            }
            Ok(Some((
                native_stream_of(ctx, &group.id)?,
                group.scope_record_index,
            )))
        },
        "index F3D pattern seed groups",
    );
    let mut history_source = IntoIterator::into_iter(features);
    while let Some(feature) =
        ctx.next_charged(&mut history_source, "scan F3D pattern body features")?
    {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = index.scopes.get(ctx, native_ref)? else {
            continue;
        };
        let stream = native_stream_of(ctx, &scope.id)?;
        let Some(group) = seed_groups.get(ctx, &(stream, scope.record_index))? else {
            continue;
        };
        let mut seed = if seeds.is_empty() {
            Some(PatternSeed::Bodies(BodySelection::Native(
                ctx.copy_retained_text(&group.id, "copy F3D pattern seed native identity")?,
            )))
        } else {
            None
        };
        let feature_id = &feature.id;
        let mut reserve_error = None;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                if let Err(error) = ctx.push_vec(seeds, seed, "collect F3D pattern body seeds") {
                    reserve_error = Some(error);
                    return;
                }
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                reserve_error = bind_body_recipe_body_selection(
                    ctx,
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    index,
                )
                .err();
            } else {
                reserve_error =
                    bind_direct_body_recipe_body_selection(ctx, selection, scope, index).err();
            }
        });
        if let Some(error) = reserve_error {
            return Err(error);
        }
    }
    Ok(())
}

/// The one active body that every operand reference's candidate faces share,
/// preferring displayed bodies when any candidate is displayed. Bodies under
/// the current history's B-rep prefix are not external candidates.
fn unique_external_body_candidate<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    current_history_source: Option<&str>,
    external: &ExternalBodies<'a, '_>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let (current_prefix, _prefix_storage) = match current_history_source {
        Some(source) => {
            let (prefix, storage) =
                ctx.with_scoped_storage("retain F3D current history prefix", || {
                    ctx.format_retained(
                        format_args!("f3d:brep/{source}/"),
                        "retain F3D current history prefix",
                    )
                })?;
            (Some(prefix), Some(storage))
        }
        None => (None, None),
    };
    let mut candidates_storage = ctx.reserve_scoped(0, "collect F3D external body candidates")?;
    let mut candidates: Option<BTreeSet<&'a cadmpeg_ir::ids::BodyId>> = None;
    let mut history_source = IntoIterator::into_iter(operand.references());
    while let Some(reference) =
        ctx.next_charged(&mut history_source, "scan F3D external body references")?
    {
        let mut reference_storage =
            ctx.reserve_scoped(0, "collect F3D external body candidates")?;
        let mut reference_candidates = BTreeSet::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D reference candidate faces",
        )? {
            let Some(&body) =
                ctx.get_hash_map(&external.body_by_face, face, "find F3D external face body")?
            else {
                continue;
            };
            if let Some(prefix) = current_prefix.as_ref() {
                if ctx.starts_with(
                    body.as_str(),
                    prefix,
                    "check F3D external body history prefix",
                )? {
                    continue;
                }
            }
            ctx.insert_scoped_btree_set(
                &mut reference_storage,
                &mut reference_candidates,
                body,
                "check F3D external body candidate",
                "collect F3D external body candidates",
            )?;
        }
        if let Some(candidates) = &mut candidates {
            if reference_candidates.is_empty() {
                return Ok(None);
            }
            ctx.retain_btree_set(
                candidates,
                |body| {
                    ctx.contains_btree_set(
                        &reference_candidates,
                        body,
                        "intersect F3D external body candidates",
                    )
                },
                "intersect F3D external body candidates",
            )?;
        } else {
            candidates_storage.with_storage(|| reference_storage.commit())?;
            candidates = Some(reference_candidates);
        }
    }
    let Some(candidates) = candidates else {
        return Ok(None);
    };
    let mut displayed = None;
    let mut displayed_count = 0_usize;
    for &body in ctx.admit_iter(&candidates, "scan F3D displayed external body candidates")? {
        let Some(metadata) =
            ctx.get_hash_map(&external.bodies, body, "find F3D external body metadata")?
        else {
            continue;
        };
        if metadata.visible == Some(true) {
            displayed = displayed.or(Some(body));
            displayed_count += 1;
        }
    }
    Ok(match displayed_count {
        0 if candidates.len() == 1 => candidates.first().copied(),
        1 => displayed,
        _ => None,
    })
}

fn bind_body_recipe_body_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &'a DesignScope,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (bound, storage) = ctx.with_scoped_storage(
        "retain F3D historical body selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            use cadmpeg_ir::features::BodySelection;

            let BodySelection::Native(group_id) = selection else {
                return Ok(false);
            };
            let Some(group) = scope_group(ctx, &index.groups, scope, group_id, |group| {
                matches!(
                    group.role(),
                    DesignOperandRole::BODIES_A
                        | DesignOperandRole::ROLE_0X5
                        | DesignOperandRole::BODIES_B
                )
            })?
            else {
                return Ok(false);
            };
            if group.members().is_empty() {
                return Ok(false);
            }
            let stream = native_stream_of(ctx, &scope.id)?;
            let mut slots_storage = ctx.reserve_scoped(0, "collect F3D body recipe slots")?;
            let mut seen = HashSet::new();
            let mut body_slots = Vec::new();
            let mut history_source = IntoIterator::into_iter(group.members())
                .map(|member| member.value)
                .enumerate();
            while let Some((ordinal, record_index)) =
                ctx.next_charged(&mut history_source, "scan F3D body recipe group members")?
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Ok(false);
                };
                let Some(operand) = index
                    .member_operands
                    .get(ctx, &(stream, group.record_index, ordinal, record_index))?
                else {
                    return Ok(false);
                };
                let Some(body_slot) = operand.resolved_body_slot else {
                    return Ok(false);
                };
                slots_storage.with_storage(|| {
                    if ctx.insert_hash_set(&mut seen, body_slot, "collect F3D body recipe slots")? {
                        ctx.push_vec(&mut body_slots, body_slot, "collect F3D body recipe slots")?;
                    }
                    Ok::<(), cadmpeg_core::CodecError>(())
                })?;
            }

            let mut body_ids =
                ctx.collection_vec(body_slots.len(), "collect F3D body recipe identities")?;
            for &slot in ctx.admit_iter(&body_slots, "scan F3D body recipe slots")? {
                body_ids.push(crate::ids::history_input_body_id_charged(
                    ctx,
                    feature_id,
                    previous_state_id,
                    slot,
                )?);
            }
            let state =
                crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
            let native =
                ctx.copy_retained_text(&group.id, "copy F3D body recipe group identity")?;
            if let Ok(historical) =
                cadmpeg_ir::features::BodySelection::historical(state, body_ids, native, ctx)?
            {
                *selection = historical;
                return Ok(true);
            }
            Ok(false)
        },
    )?;
    if bound {
        storage.commit()?;
    }
    Ok(())
}

fn bind_direct_body_recipe_body_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &'a DesignScope,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (bound, storage) = ctx.with_scoped_storage(
        "retain F3D direct body selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            use cadmpeg_ir::features::BodySelection;

            let stream = native_stream_of(ctx, &scope.id)?;
            let mut seen_storage =
                ctx.reserve_scoped(0, "index F3D direct body recipe selections")?;
            let mut seen_bodies = HashSet::new();
            let native_members = match selection {
                BodySelection::Native(group_id) => {
                    let Some(group) = scope_group(ctx, &index.groups, scope, group_id, |group| {
                        matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::BODIES_B
                        )
                    })?
                    else {
                        return Ok(false);
                    };
                    if group.members().is_empty() {
                        return Ok(false);
                    }
                    let mut selected = ctx.collection_vec(
                        group.members().len(),
                        "collect F3D direct body recipe selections",
                    )?;
                    let mut history_source = IntoIterator::into_iter(group.members())
                        .map(|member| member.value)
                        .enumerate();
                    while let Some((ordinal, record_index)) = ctx.next_charged(
                        &mut history_source,
                        "scan F3D direct body recipe group members",
                    )? {
                        let Ok(ordinal) = u32::try_from(ordinal) else {
                            return Ok(false);
                        };
                        let Some(operand) = index
                            .member_operands
                            .get(ctx, &(stream, group.record_index, ordinal, record_index))?
                        else {
                            return Ok(false);
                        };
                        let Some(body) = direct_body_recipe_candidate(ctx, operand, index)? else {
                            return Ok(false);
                        };
                        if !seen_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut seen_bodies,
                                body,
                                "index F3D direct body recipe selections",
                            )
                        })? {
                            return Ok(false);
                        }
                        selected.push(
                            body.try_clone_for_decode(
                                ctx,
                                "copy F3D direct body recipe selection",
                            )?,
                        );
                    }
                    let bodies =
                        match cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx) {
                            Ok(value) => value,
                            Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => {
                                return Ok(false)
                            }
                            Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                                return Err(limit.into())
                            }
                        };
                    let native = ctx.copy_retained_text(
                        &group.id,
                        "copy F3D direct body recipe group identity",
                    )?;
                    *selection = BodySelection::Resolved { bodies, native };
                    return Ok(true);
                }
                BodySelection::NativeSet(native) => native.as_slice(),
                _ => return Ok(false),
            };
            if native_members.is_empty() {
                return Ok(false);
            }
            let mut members_storage =
                ctx.reserve_scoped(0, "index F3D direct body recipe native members")?;
            let mut members = HashSet::new();
            let mut history_source = IntoIterator::into_iter(native_members);
            while let Some(native) = ctx.next_charged(
                &mut history_source,
                "scan F3D direct body recipe native members",
            )? {
                if !members_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut members,
                        native.as_str(),
                        "index F3D direct body recipe native members",
                    )
                })? {
                    return Ok(false);
                }
            }
            let Some(stream) = stream else {
                return Ok(false);
            };
            let mut rows =
                ctx.collection_vec(native_members.len(), "collect F3D direct body recipe rows")?;
            let mut history_source = IntoIterator::into_iter(native_members);
            while let Some(native) =
                ctx.next_charged(&mut history_source, "scan F3D native members")?
            {
                let Some((native_stream_name, record_index)) = ctx.rsplit_once(
                    native,
                    ":design-record#",
                    "split F3D direct body recipe native identity",
                )?
                else {
                    return Ok(false);
                };
                let Ok(record_index) = ctx
                    .parse_text::<u32>(record_index, "parse F3D direct body recipe record index")?
                else {
                    return Ok(false);
                };
                if !ctx.equal(
                    native_stream_name,
                    stream,
                    "compare F3D direct body recipe stream identity",
                )? {
                    return Ok(false);
                }
                let Some(operand) = index
                    .scope_operands
                    .get(ctx, &(Some(stream), scope.record_index, record_index))?
                else {
                    return Ok(false);
                };
                let Some(body) = direct_body_recipe_candidate(ctx, operand, index)? else {
                    return Ok(false);
                };
                if !seen_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut seen_bodies,
                        body,
                        "index F3D direct body recipe selections",
                    )
                })? {
                    return Ok(false);
                }
                let body =
                    body.try_clone_for_decode(ctx, "copy F3D direct body recipe member body")?;
                let native =
                    ctx.copy_retained_text(native, "copy F3D direct body recipe member identity")?;
                let Some(row) = body_member(ctx, body, native)? else {
                    return Ok(false);
                };
                rows.push(row);
            }
            if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)? {
                *selection = BodySelection::ResolvedSet { members };
                return Ok(true);
            }
            Ok(false)
        },
    )?;
    if bound {
        storage.commit()?;
    }
    Ok(())
}

fn direct_body_recipe_candidate<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let external = index.external(ctx)?;
    if let Some(body) = body_recipe_link_candidate(ctx, operand, index)? {
        let (any_candidate, contains_body) =
            body_recipe_face_body_candidates(ctx, operand, body, external)?;
        if !any_candidate || contains_body {
            return Ok(Some(body));
        }
        return Ok(None);
    }
    unique_external_body_candidate(ctx, operand, None, external)
}

/// The active body that the operand's body recipe names through its latest
/// persistent design links, when every such link names the same body.
fn body_recipe_link_candidate<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    index: &BodySelectionIndex<'a, 'ctx>,
) -> Result<Option<&'a cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let Some(stream) = native_stream_of(ctx, &operand.id)? else {
        return Ok(None);
    };
    let Some(recipe) = index.recipes.get(ctx, operand.recipe_id.as_str())? else {
        return Ok(None);
    };
    if !ctx.equal(
        &native_stream_of(ctx, &recipe.id)?,
        &Some(stream),
        "compare F3D body recipe stream",
    )? {
        return Ok(None);
    }
    let Some(design) = recipe.design.as_ref() else {
        return Ok(None);
    };
    let Some(selector) = design.selector else {
        return Ok(None);
    };
    let links = index.links(ctx)?;
    let Some(candidates) = ctx.get_hash_map(
        &links.by_design,
        &(design.id.value.as_str(), i64::from(selector.value)),
        "find F3D persistent body links",
    )?
    else {
        return Ok(None);
    };
    let external = index.external(ctx)?;
    let mut matching_body: Option<&'a cadmpeg_ir::ids::BodyId> = None;
    let mut history_source = IntoIterator::into_iter(candidates);
    while let Some(&link) =
        ctx.next_charged(&mut history_source, "resolve F3D persistent body link")?
    {
        if !links.current[link] {
            continue;
        }
        let AttributeTarget::Body(body) = &index.inputs.persistent_design_links[link].target else {
            continue;
        };
        if !ctx.contains_key_hash_map(
            &external.bodies,
            body,
            "find F3D persistent body candidate",
        )? {
            continue;
        }
        if let Some(existing) = matching_body {
            if !ctx.equal(existing, body, "compare F3D selected persistent body")? {
                return Ok(None);
            }
        }
        matching_body = Some(body);
    }
    Ok(matching_body)
}

/// Whether any candidate face of the operand belongs to an active body, and
/// whether one of those bodies is `selected`.
fn body_recipe_face_body_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &BodyRecipeOperand,
    selected: &cadmpeg_ir::ids::BodyId,
    external: &ExternalBodies<'_, '_>,
) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    let mut any_candidate = false;
    let mut contains_selected = false;
    for reference in ctx.admit_iter(operand.references(), "scan F3D body recipe references")? {
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D body recipe candidate faces",
        )? {
            let Some(&body) = ctx.get_hash_map(
                &external.body_by_face,
                face,
                "resolve F3D body recipe face carrier",
            )?
            else {
                continue;
            };
            if ctx.contains_key_hash_map(
                &external.bodies,
                body,
                "resolve F3D body recipe face carrier",
            )? {
                any_candidate = true;
                contains_selected |=
                    ctx.equal(body, selected, "compare F3D selected body recipe candidate")?;
            }
        }
    }
    Ok((any_candidate, contains_selected))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

type HistoryStates<'a> = HashMap<i64, Option<&'a AsmDeltaState>>;

/// A history's states by ID; a repeated ID names no state.
fn unique_feature_history_states<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    history: &'a AsmHistory,
) -> Result<
    (
        HistoryStates<'a>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    ctx.unique_index(
        history.states.iter().map(|state| (state.state_id, state)),
        "index F3D feature history states",
    )
}

/// Each history's states by ID, indexed for the first feature that walks
/// that history and kept for the rest of one binding pass.
struct FeatureHistoryStates<'a, 'ctx> {
    by_history: HashMap<
        usize,
        (
            HistoryStates<'a>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
    >,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> FeatureHistoryStates<'a, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            by_history: HashMap::new(),
            storage: ctx.reserve_scoped(0, "index F3D feature histories")?,
        })
    }

    fn states_of(
        &mut self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        history: &'a AsmHistory,
    ) -> Result<Option<&HistoryStates<'a>>, cadmpeg_core::CodecError> {
        // A history is keyed by its address: the histories outlive the pass.
        let key = std::ptr::from_ref(history).addr();
        if !ctx.contains_key_hash_map(&self.by_history, &key, "find F3D history by history")? {
            let states = unique_feature_history_states(ctx, history)?;
            self.storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut self.by_history,
                    key,
                    states,
                    "index F3D feature histories",
                )
                .map(|_| ())
            })?;
        }
        Ok(ctx
            .get_hash_map(&self.by_history, &key, "find F3D history by history")?
            .map(|(states, _)| states))
    }
}

fn singleton_revised_input_body_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HistoryStates<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "index F3D revised input bodies")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut visited,
                current.state_id,
                "visit F3D revised input body states",
            )
        })? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        let bodies = &transition.topology.bodies;
        for &body in ctx
            .admit_iter(&bodies.updated, "scan F3D revised input bodies")?
            .chain(ctx.admit_iter(&bodies.deleted, "scan F3D revised input bodies")?)
        {
            ctx.insert_scoped_btree_set(
                &mut storage,
                &mut revised,
                body,
                "find F3D revised input body",
                "index F3D revised input bodies",
            )?;
        }
        let Some(previous_id) = transition.previous_state_id else {
            return Ok(None);
        };
        let Some(Some(previous)) =
            ctx.get_hash_map(states, &previous_id, "find F3D history states")?
        else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(input) = current.topology() else {
        return Ok(None);
    };
    let is_revised =
        |body: &i64| ctx.contains_btree_set(&revised, body, "find F3D revised input body");
    let Some(first) = ctx.position_by(&input.bodies, is_revised, "find F3D revised input body")?
    else {
        return Ok(None);
    };
    let repeated = ctx.any_by(
        &input.bodies[first + 1..],
        is_revised,
        "find F3D revised input body",
    )?;
    Ok((!repeated).then_some(input.bodies[first]))
}

fn singleton_body_revision_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HistoryStates<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(result_topology) = state.topology() else {
        return Ok(None);
    };
    let mut visited_storage = ctx.reserve_scoped(0, "visit F3D stable body revision states")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut visited,
                current.state_id,
                "visit F3D stable body revision states",
            )
        })? {
            return Ok(None);
        }
        let Some(revision) = body_revision_without_topology_change(current) else {
            return Ok(None);
        };
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return Ok(None),
            }
        }
        let Some(previous) = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
        else {
            return Ok(None);
        };
        let Some(Some(previous)) =
            ctx.get_hash_map(states, &previous, "find F3D history states")?
        else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(body) = selected else {
        return Ok(None);
    };
    let Some(input_topology) = current.topology() else {
        return Ok(None);
    };
    let operation = "find F3D stable revised body";
    Ok((ctx.contains(&result_topology.bodies, &body, operation)?
        && ctx.contains(&input_topology.bodies, &body, operation)?)
    .then_some(body))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopologyStableBodyRevision {
    Unchanged,
    Revised(i64),
}

fn body_revision_without_topology_change(
    current: &AsmDeltaState,
) -> Option<TopologyStableBodyRevision> {
    let transition = current.transition.as_ref()?;
    let delta = &transition.topology;
    let body = match delta.bodies.updated.as_slice() {
        [] => TopologyStableBodyRevision::Unchanged,
        [body] => TopologyStableBodyRevision::Revised(*body),
        _ => return None,
    };
    if !delta.bodies.inserted.is_empty()
        || !delta.bodies.deleted.is_empty()
        || [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
        ]
        .into_iter()
        .any(|family| {
            !family.inserted.is_empty() || !family.deleted.is_empty() || !family.updated.is_empty()
        })
    {
        return None;
    }
    Some(body)
}

#[derive(Clone, Copy)]
pub(crate) struct FeatureFaceSelectionInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) operands: &'a [crate::records::topology::face::DesignFaceOperand],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    pub(crate) histories: &'a [AsmHistory],
}

type EntityOperand = crate::records::topology::entity_selection::DesignEntitySelectionOperand;
type FaceOperand = crate::records::topology::face::DesignFaceOperand;
/// Stream, scope record, owning group record, member ordinal and record of an
/// entity-selection operand.
type EntityOperandKey<'a> = (Option<&'a str>, u32, u32, u32, u32);
/// Stream, scope record and record of a face operand.
type FaceOperandKey<'a> = (Option<&'a str>, u32, u32);

type StitchGroups<'a, 'ctx> = (
    HashMap<(Option<&'a str>, u32), Vec<&'a OperandGroup>>,
    cadmpeg_core::decode::ScopedReservation<'ctx>,
);

/// Face-selection inputs keyed for the per-feature lookups of one binding
/// pass, each index built on its first lookup.
pub(crate) struct FaceSelectionIndex<'a, 'ctx> {
    pub(super) inputs: FeatureFaceSelectionInputs<'a>,
    pub(super) groups: GroupIndex<'a, 'ctx>,
    pub(super) face_operands: UniqueIndex<'a, 'ctx, FaceOperandKey<'a>, FaceOperand>,
    pub(super) body_recipe_operands: UniqueIndex<'a, 'ctx, MemberOperandKey<'a>, BodyRecipeOperand>,
    entity_operands: UniqueIndex<'a, 'ctx, EntityOperandKey<'a>, EntityOperand>,
    /// Plain role-0x5 groups of each scope, by stream and scope record.
    stitch_groups: std::cell::OnceCell<StitchGroups<'a, 'ctx>>,
}

impl<'a, 'ctx> FaceSelectionIndex<'a, 'ctx> {
    pub(crate) fn new(inputs: FeatureFaceSelectionInputs<'a>) -> Self {
        Self {
            inputs,
            groups: group_index(inputs.groups),
            face_operands: UniqueIndex::new(
                inputs.operands,
                |ctx, operand| {
                    Ok(Some((
                        native_stream_of(ctx, &operand.id)?,
                        operand.scope_record_index,
                        operand.record_index(),
                    )))
                },
                "index F3D face selection operands",
            ),
            body_recipe_operands: UniqueIndex::new(
                inputs.body_recipe_operands,
                member_operand_key,
                "index F3D group body recipe operands",
            ),
            entity_operands: UniqueIndex::new(
                inputs.entity_operands,
                |ctx, operand| {
                    Ok(Some((
                        native_stream_of(ctx, &operand.id)?,
                        operand.scope_record_index,
                        operand.group_record_index,
                        operand.group_member_ordinal,
                        operand.record_index(),
                    )))
                },
                "index F3D entity selection operands",
            ),
            stitch_groups: std::cell::OnceCell::new(),
        }
    }

    fn stitch_groups(
        &self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        stream: Option<&'a str>,
        scope_record_index: u32,
    ) -> Result<&[&'a OperandGroup], cadmpeg_core::CodecError> {
        let operation = "index F3D Stitch face groups";
        let (groups, _) = match self.stitch_groups.get() {
            Some(groups) => groups,
            None => {
                let mut storage = ctx.reserve_scoped(0, operation)?;
                let mut groups = HashMap::new();
                for group in ctx.admit_iter(self.inputs.groups, operation)? {
                    if group.role() != DesignOperandRole::ROLE_0X5
                        || group.extrude_role().is_some()
                        || group.extrude_face_role().is_some()
                    {
                        continue;
                    }
                    let key = (native_stream_of(ctx, &group.id)?, group.scope_record_index);
                    storage.with_storage(|| {
                        ctx.push_hash_group(&mut groups, key, group, operation, operation)
                    })?;
                }
                self.stitch_groups.get_or_init(|| (groups, storage))
            }
        };
        Ok(ctx
            .get_hash_map(groups, &(stream, scope_record_index), operation)?
            .map_or(&[][..], Vec::as_slice))
    }
}

/// The faces of each feature input topology, keyed by topology and feature
/// identity, so selections can add the faces they name. A key that several
/// topologies share names none.
struct InputTopologyFaces<'t, 'ctx> {
    positions: HashMap<(&'t str, &'t str), Option<usize>>,
    faces: Vec<&'t mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::HistoricalFaceId>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'t, 'ctx> InputTopologyFaces<'t, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        topologies: &'t mut [cadmpeg_ir::features::FeatureInputTopology],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D feature input topologies";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut faces = storage.with_storage(|| ctx.collection_vec(topologies.len(), operation))?;
        let mut positions = HashMap::new();
        for (position, topology) in ctx.admit_iter(topologies, operation)?.enumerate() {
            let cadmpeg_ir::features::FeatureInputTopology {
                id,
                input_of,
                faces: topology_faces,
                ..
            } = topology;
            let (id, input_of): (&'t _, &'t _) = (id, input_of);
            let key = (id.as_str(), input_of.as_str());
            if let Some(indexed) = ctx.get_mut_hash_map(&mut positions, &key, operation)? {
                *indexed = None;
            } else {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut positions, key, Some(position), operation)
                        .map(|_| ())
                })?;
            }
            faces.push(topology_faces);
        }
        Ok(Self {
            positions,
            faces,
            _storage: storage,
        })
    }

    fn faces_of(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        state_id: &cadmpeg_ir::ids::FeatureInputTopologyId,
        feature_id: &cadmpeg_ir::features::FeatureId,
    ) -> Result<
        Option<&mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::HistoricalFaceId>>,
        cadmpeg_core::CodecError,
    > {
        let Some(&Some(position)) = ctx.get_hash_map(
            &self.positions,
            &(state_id.as_str(), feature_id.as_str()),
            "find F3D feature input topology",
        )?
        else {
            return Ok(None);
        };
        Ok(self.faces.get_mut(position).map(|faces| &mut **faces))
    }

    /// Adds `face` to a topology's faces unless it is already present.
    fn add_face(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        state_id: &cadmpeg_ir::ids::FeatureInputTopologyId,
        feature_id: &cadmpeg_ir::features::FeatureId,
        face: &cadmpeg_ir::ids::HistoricalFaceId,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(faces) = self.faces_of(ctx, state_id, feature_id)? else {
            return Ok(());
        };
        if !ctx.contains(faces, face, operation)? {
            let retained = face.try_clone_for_decode(ctx, operation)?;
            faces.insert(ctx, retained, operation)?;
        }
        Ok(())
    }
}

pub(crate) fn bind_feature_face_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let histories = input.histories;
    let scopes = scope_index(input.scopes);
    let index = FaceSelectionIndex::new(input);
    let mut input_faces = InputTopologyFaces::new(ctx, input_topologies)?;
    let mut history_source = IntoIterator::into_iter(features);
    while let Some(feature) =
        ctx.next_charged(&mut history_source, "scan F3D face selection features")?
    {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            macro_rules! admitted {
                ($value:expr) => {
                    if let Err(error) = $value {
                        edit_result = Err(error);
                        return;
                    }
                };
            }
            'feature_edit: {
                let Some(native_ref) = native_ref else {
                    break 'feature_edit;
                };
                let scope = match scopes.get(ctx, native_ref) {
                    Ok(Some(scope)) => scope,
                    Ok(None) => break 'feature_edit,
                    Err(error) => {
                        edit_result = Err(error);
                        break 'feature_edit;
                    }
                };
                let Some(state_id) = scope.history_state_id() else {
                    break 'feature_edit;
                };
                let previous_state_id =
                    match effective_scope_previous_history_state_id(ctx, scope, histories) {
                        Ok(previous_state_id) => previous_state_id,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    };
                let Some(previous_state_id) = previous_state_id else {
                    break 'feature_edit;
                };
                let history_state_pair =
                    match unique_history_state_pair(ctx, histories, state_id, previous_state_id) {
                        Ok(history_state_pair) => history_state_pair,
                        Err(error) => {
                            edit_result = Err(error);
                            break 'feature_edit;
                        }
                    };
                let Some((history, state, previous)) = history_state_pair else {
                    break 'feature_edit;
                };
                let Some(transition) = &state.transition else {
                    break 'feature_edit;
                };
                if transition.previous_state_id != Some(previous_state_id) {
                    break 'feature_edit;
                }
                let Some(_topology) = previous.topology() else {
                    break 'feature_edit;
                };
                let updated_faces = &transition.topology.faces.updated;
                let mut binding = FaceSelectionBinding {
                    feature_id,
                    previous_state_id,
                    operation_history_id: &history.id,
                    scope,
                    input_faces: &mut input_faces,
                };
                match definition {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { start, extent, .. },
                    ) => {
                        if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                            admitted!(selection::bind_face_selection(
                                ctx,
                                face,
                                scope,
                                &index,
                                updated_faces,
                            ));
                            admitted!(bind_entity_face_selection(ctx, face, &mut binding, &index,));
                        }
                        let sides = match extent {
                            cadmpeg_ir::features::ExtrudeExtent::OneSided { side }
                            | cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => {
                                [Some(side), None]
                            }
                            cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
                                [Some(first), Some(second)]
                            }
                        };
                        for side in sides.into_iter().flatten() {
                            if let cadmpeg_ir::features::LinearTermination::ToFace {
                                face, ..
                            } = &mut side.termination
                            {
                                admitted!(selection::bind_face_selection(
                                    ctx,
                                    face,
                                    scope,
                                    &index,
                                    updated_faces,
                                ));
                            }
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                    ) => {
                        let mut seeds = seeds.iter_mut();
                        loop {
                            let seed =
                                match ctx.next_charged(&mut seeds, "scan F3D pattern face seeds") {
                                    Ok(Some(seed)) => seed,
                                    Ok(None) => break,
                                    Err(error) => {
                                        edit_result = Err(error);
                                        return;
                                    }
                                };
                            let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed
                            else {
                                continue;
                            };
                            admitted!(selection::bind_face_selection(
                                ctx,
                                faces,
                                scope,
                                &index,
                                updated_faces,
                            ));
                            admitted!(
                                bind_entity_face_selection(ctx, faces, &mut binding, &index,)
                            );
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::MoveFace { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            &index,
                            updated_faces,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            &index,
                            updated_faces,
                        ));
                        admitted!(selection::bind_body_recipe_face_selection(
                            ctx,
                            faces,
                            feature_id,
                            previous_state_id,
                            scope,
                            &index,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                    ) => {
                        admitted!(bind_surface_stitch_face_selection(
                            ctx,
                            faces,
                            &mut binding,
                            &index,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            targets,
                            scope,
                            &index,
                            updated_faces,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Hole {
                            face: Some(face), ..
                        },
                    ) => {
                        admitted!(bind_hole_face_selection(ctx, face, &mut binding));
                    }
                    _ => {}
                }
            }
        });
        edit_result?;
    }
    Ok(())
}

struct FaceSelectionBinding<'b, 'a, 't, 'ctx> {
    feature_id: &'b cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &'b str,
    scope: &'a DesignScope,
    input_faces: &'b mut InputTopologyFaces<'t, 'ctx>,
}

fn bind_entity_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return Ok(()),
    };
    let Some(group) = scope_group(ctx, &index.groups, binding.scope, group_id, |_| true)? else {
        return Ok(());
    };
    if group.members().is_empty() {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &group.id,
        binding,
        std::slice::from_ref(&group),
        index,
    )
}

fn bind_surface_stitch_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let scope = binding.scope;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return Ok(()),
    };
    if !ctx.equal(
        native_id,
        scope.id.as_str(),
        "compare F3D Stitch selection scope",
    )? {
        return Ok(());
    }
    let references = scope.reference_members();
    let Some(input_end) = references.len().checked_sub(2) else {
        return Ok(());
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let candidates = index.stitch_groups(ctx, stream, scope.record_index)?;
    if candidates.len().checked_mul(2) != Some(input_end) {
        return Ok(());
    }
    let (mut matching_groups, _matching_storage) = ctx
        .copy_temporary_slice(candidates, "collect F3D Stitch face groups")
        .map_err(cadmpeg_core::CodecError::ResourceLimit)?;
    ctx.stable_sort_by(
        &mut matching_groups,
        |value| &value.scope_reference_ordinal,
        Ord::cmp,
        "sort F3D Stitch face groups",
    )?;
    // Group k must sit at reference ordinal 2k, name reference 2k as its record
    // and hold exactly reference 2k + 1 as its one member.
    let mismatched = ctx.any_by(
        matching_groups.iter().enumerate().zip(
            references
                .values()
                .step_by(2)
                .zip(references.values().skip(1).step_by(2)),
        ),
        |((ordinal, group), (group_reference, member_reference))| {
            let [member] = group.members() else {
                return Ok(true);
            };
            Ok(
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || member.value != *member_reference,
            )
        },
        "check F3D Stitch face groups",
    )?;
    if mismatched {
        return Ok(());
    }
    bind_entity_face_groups(ctx, selection, &scope.id, binding, &matching_groups, index)
}

fn bind_entity_face_groups<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    binding: &mut FaceSelectionBinding<'_, 'a, '_, '_>,
    groups: &[&'a OperandGroup],
    index: &FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let feature_id = binding.feature_id;
    let previous_state_id = binding.previous_state_id;
    let scope = binding.scope;

    if groups.is_empty() {
        return Ok(());
    }
    let stream = native_stream_of(ctx, &scope.id)?;
    let mut selected_storage = ctx.reserve_scoped(0, "collect F3D entity face candidates")?;
    let mut seen = HashSet::new();
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let mut history_source = IntoIterator::into_iter(groups);
    while let Some(group) = ctx.next_charged(&mut history_source, "scan F3D entity face groups")? {
        if group.members().is_empty() {
            return Ok(());
        }
        let mut history_source = IntoIterator::into_iter(group.members())
            .map(|member| member.value)
            .enumerate();
        while let Some((ordinal, record_index)) =
            ctx.next_charged(&mut history_source, "scan F3D entity face group members")?
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(());
            };
            let Some(operand) = index.entity_operands.get(
                ctx,
                &(
                    stream,
                    scope.record_index,
                    group.record_index,
                    ordinal,
                    record_index,
                ),
            )?
            else {
                return Ok(());
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return Ok(());
            };
            let local = ctx.equal(
                candidate.history_id.as_str(),
                binding.operation_history_id,
                "compare F3D entity face candidate history",
            )? && ctx.contains(
                &candidate.historical.state_ids,
                &previous_state_id,
                "find F3D entity face candidate state",
            )?;
            let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
                return Ok(());
            };
            let entry = (source, candidate.face_slot, local);
            selected_storage.with_storage(|| {
                if ctx.insert_hash_set(&mut seen, entry, "index F3D entity face candidates")? {
                    ctx.push_vec(&mut selected, entry, "collect F3D entity face candidates")?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
        }
    }
    let (candidate, mut output_storage) = ctx.with_scoped_storage(
        "retain F3D historical entity face selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            let state_id =
                crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
            if binding
                .input_faces
                .faces_of(ctx, &state_id, feature_id)?
                .is_none()
            {
                return Ok(None);
            }
            let mut faces =
                ctx.collection_vec(selected.len(), "collect F3D historical entity faces")?;
            let mut history_source = IntoIterator::into_iter(&selected);
            while let Some(&(source, face, local)) =
                ctx.next_charged(&mut history_source, "scan F3D entity face candidates")?
            {
                let Some(face) = historical_input_face_id(
                    ctx,
                    feature_id,
                    previous_state_id,
                    source,
                    face,
                    local,
                )?
                else {
                    return Ok(None);
                };
                faces.push(face);
            }
            Ok(Some((state_id, faces)))
        },
    )?;
    let Some((state_id, faces)) = candidate else {
        return Ok(());
    };
    for face in ctx.admit_iter(&faces, "scan F3D faces")? {
        binding.input_faces.add_face(
            ctx,
            &state_id,
            feature_id,
            face,
            "index F3D historical entity faces",
        )?;
    }
    let historical = output_storage.with_storage(|| {
        let native =
            ctx.copy_retained_text(native_id, "copy F3D historical face selection identity")?;
        Ok::<_, cadmpeg_core::CodecError>(
            cadmpeg_ir::features::FaceSelection::historical(state_id, faces, native, ctx)?.ok(),
        )
    })?;
    if let Some(historical) = historical {
        output_storage.commit()?;
        *selection = historical;
    }
    Ok(())
}

/// The history-input identity of a candidate face: its slot alone for a face
/// of the operation's own input state, otherwise qualified by its B-rep source,
/// which must then hold no `#` and no whitespace.
fn historical_input_face_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    source: &str,
    face: i64,
    local: bool,
) -> Result<Option<cadmpeg_ir::ids::HistoricalFaceId>, cadmpeg_core::CodecError> {
    if local {
        return crate::ids::history_input_face_id_charged(ctx, feature_id, previous_state_id, face)
            .map(Some);
    }
    let operation = "check F3D historical face source";
    if ctx.contains_text(source, "#", operation)?
        || ctx.any_by(
            source.chars(),
            |character| Ok(character.is_whitespace()),
            operation,
        )?
    {
        return Ok(None);
    }
    crate::ids::history_input_face_id_charged(
        ctx,
        feature_id,
        previous_state_id,
        format_args!("{}:{source}:{face}", source.len()),
    )
    .map(Some)
}

fn bind_hole_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    binding: &mut FaceSelectionBinding<'_, '_, '_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let feature_id = binding.feature_id;
    let previous_state_id = binding.previous_state_id;

    let FaceSelection::Native(native_id) = selection else {
        return Ok(());
    };
    let Some(construction) = binding.scope.hole_construction() else {
        return Ok(());
    };
    let Some(face_selection) = &construction.face_selection else {
        return Ok(());
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return Ok(());
    };
    let local = ctx.equal(
        candidate.history_id.as_str(),
        binding.operation_history_id,
        "compare F3D hole face candidate history",
    )? && ctx.contains(
        &candidate.historical.state_ids,
        &previous_state_id,
        "find F3D hole face candidate state",
    )?;
    let Some(source) = historical_brep_source(ctx, &candidate.history_id)? else {
        return Ok(());
    };
    let (candidate, mut output_storage) = ctx.with_scoped_storage(
        "retain F3D historical hole face selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            let state_id =
                crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
            if binding
                .input_faces
                .faces_of(ctx, &state_id, feature_id)?
                .is_none()
            {
                return Ok(None);
            }
            let Some(face) = historical_input_face_id(
                ctx,
                feature_id,
                previous_state_id,
                source,
                candidate.face_slot,
                local,
            )?
            else {
                return Ok(None);
            };
            Ok(Some((state_id, face)))
        },
    )?;
    let Some((state_id, face)) = candidate else {
        return Ok(());
    };
    binding.input_faces.add_face(
        ctx,
        &state_id,
        feature_id,
        &face,
        "index F3D historical hole face",
    )?;
    let historical = output_storage.with_storage(|| {
        let native = ctx.copy_retained_text(native_id, "copy F3D historical hole identity")?;
        let mut selected = ctx.collection_vec(1, "validate F3D historical hole face")?;
        selected.push(face);
        Ok::<_, cadmpeg_core::CodecError>(
            cadmpeg_ir::features::FaceSelection::historical(state_id, selected, native, ctx)?.ok(),
        )
    })?;
    if let Some(historical) = historical {
        output_storage.commit()?;
        *selection = historical;
    }
    Ok(())
}

/// Stream, owning group record, member ordinal and record of an
/// entity-selection operand.
type PathOperandKey<'a> = (Option<&'a str>, u32, u32, u32);

pub(crate) fn bind_feature_path_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    let scopes = scope_index(scopes);
    let groups = group_index(groups);
    let operands = path_operand_index(operands);
    let mut history_source = IntoIterator::into_iter(features);
    while let Some(feature) =
        ctx.next_charged(&mut history_source, "scan F3D path selection features")?
    {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let scope = match scopes.get(ctx, native_ref) {
                Ok(Some(scope)) => scope,
                Ok(None) => break 'feature_edit,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            let bind = |path: &mut cadmpeg_ir::features::PathRef| {
                bind_entity_selection_path(
                    ctx,
                    path,
                    feature_id,
                    previous_state_id,
                    scope,
                    &groups,
                    &operands,
                )
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary: SurfaceBoundary::Path(path),
                    ..
                }) => {
                    edit_result = bind(path);
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths.as_mut_slice(),
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    let mut paths = paths.iter_mut();
                    loop {
                        let path = match ctx.next_charged(&mut paths, "scan F3D loft guide paths") {
                            Ok(Some(path)) => path,
                            Ok(None) => break,
                            Err(error) => {
                                edit_result = Err(error);
                                break 'feature_edit;
                            }
                        };
                        edit_result = bind(path);
                        if edit_result.is_err() {
                            break;
                        }
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        edit_result = bind(path);
                        if edit_result.is_err() {
                            break 'feature_edit;
                        }
                    }
                    if let Some(guide_rail) = guide_rail {
                        edit_result = bind(&mut guide_rail.path);
                    }
                }
                _ => {}
            }
        });
        edit_result?;
    }
    Ok(())
}

/// Entity-selection operands by stream, owning group record, member ordinal
/// and record.
fn path_operand_index<'ctx>(
    operands: &[EntityOperand],
) -> UniqueIndex<'_, 'ctx, PathOperandKey<'_>, EntityOperand> {
    UniqueIndex::new(
        operands,
        |ctx, operand| {
            Ok(Some((
                native_stream_of(ctx, &operand.id)?,
                operand.group_record_index,
                operand.group_member_ordinal,
                operand.record_index(),
            )))
        },
        "index F3D path selection operands",
    )
}

fn bind_entity_selection_path<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &'a DesignScope,
    groups: &GroupIndex<'a, 'ctx>,
    operands: &UniqueIndex<'a, 'ctx, PathOperandKey<'a>, EntityOperand>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (bound, storage) = ctx.with_scoped_storage(
        "retain F3D historical path selection",
        || -> Result<_, cadmpeg_core::CodecError> {
            use cadmpeg_ir::features::PathRef;

            let PathRef::Native(group_id) = path else {
                return Ok(false);
            };
            let Some(group) = scope_group(ctx, groups, scope, group_id, |_| true)? else {
                return Ok(false);
            };
            if group.members().is_empty() {
                return Ok(false);
            }
            let stream = native_stream_of(ctx, &scope.id)?;
            let (mut edge_slots, _edge_slots_storage) = ctx
                .with_scoped_storage("collect F3D path edge slots", || {
                    ctx.collection_vec(group.members().len(), "collect F3D path edge slots")
                })?;
            let mut history_source = IntoIterator::into_iter(group.members())
                .map(|member| member.value)
                .enumerate();
            while let Some((ordinal, record_index)) =
                ctx.next_charged(&mut history_source, "scan F3D path group members")?
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Ok(false);
                };
                let Some(operand) =
                    operands.get(ctx, &(stream, group.record_index, ordinal, record_index))?
                else {
                    return Ok(false);
                };
                let Some(edge_slot) = operand.resolved_edge_slot else {
                    return Ok(false);
                };
                edge_slots.push(edge_slot);
            }

            let mut edge_ids =
                ctx.collection_vec(edge_slots.len(), "collect F3D path edge identities")?;
            for &slot in ctx.admit_iter(&edge_slots, "scan F3D path edge slots")? {
                edge_ids.push(crate::ids::history_input_edge_id_charged(
                    ctx,
                    feature_id,
                    previous_state_id,
                    slot,
                )?);
            }
            let state =
                crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
            let native = ctx.copy_retained_text(&group.id, "copy F3D path group identity")?;
            if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native, ctx)? {
                *path = historical;
                return Ok(true);
            }
            Ok(false)
        },
    )?;
    if bound {
        storage.commit()?;
    }
    Ok(())
}

pub(crate) fn project_feature_input_topologies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
) -> Result<Vec<cadmpeg_ir::features::FeatureInputTopology>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FeatureInputTopology;

    let scopes = scope_index(scopes);
    let mut projected = Vec::new();
    for feature in ctx.admit_iter(features, "scan F3D input topology features")? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.get(ctx, native_ref)? else {
            continue;
        };
        let previous_state_id = match scope.previous_history_state_id() {
            Some(previous_state_id) => Some(previous_state_id),
            None => {
                crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
                    .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            }
        };
        let previous_state_id = match previous_state_id {
            Some(previous_state_id) => Some(previous_state_id),
            None => effective_scope_previous_history_state_id(ctx, scope, histories)?,
        };
        let Some(previous_state_id) = previous_state_id else {
            continue;
        };
        let state_from_pair = if let Some(state_id) = scope.history_state_id() {
            unique_history_state_pair(ctx, histories, state_id, previous_state_id)?
                .map(|(_, _, previous)| previous)
        } else {
            None
        };
        let state = match state_from_pair {
            Some(state) => Some(state),
            None => {
                unique_history_state(ctx, histories, previous_state_id)?.map(|(_, state)| state)
            }
        };
        let Some(state) = state else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let (projection, storage) = ctx.with_scoped_storage(
            "retain F3D input topology",
            || -> Result<_, cadmpeg_core::CodecError> {
                let Some(bodies) = project_input_members(
                    ctx,
                    &topology.bodies,
                    "collect F3D input bodies",
                    |slot| {
                        crate::ids::history_input_body_id_charged(
                            ctx,
                            &feature.id,
                            previous_state_id,
                            slot,
                        )
                    },
                )?
                else {
                    return Ok(None);
                };
                let Some(faces) = project_input_members(
                    ctx,
                    &topology.faces,
                    "collect F3D input faces",
                    |slot| {
                        crate::ids::history_input_face_id_charged(
                            ctx,
                            &feature.id,
                            previous_state_id,
                            slot,
                        )
                    },
                )?
                else {
                    return Ok(None);
                };
                let Some(edges) = project_input_members(
                    ctx,
                    &topology.edges,
                    "collect F3D input edges",
                    |slot| {
                        crate::ids::history_input_edge_id_charged(
                            ctx,
                            &feature.id,
                            previous_state_id,
                            slot,
                        )
                    },
                )?
                else {
                    return Ok(None);
                };
                let Some(vertices) = project_input_members(
                    ctx,
                    &topology.vertices,
                    "collect F3D input vertices",
                    |slot| {
                        crate::ids::history_input_vertex_id_charged(
                            ctx,
                            &feature.id,
                            previous_state_id,
                            slot,
                        )
                    },
                )?
                else {
                    return Ok(None);
                };
                let id = crate::ids::history_input_state_id_charged(
                    ctx,
                    &feature.id,
                    previous_state_id,
                )?;
                let input_of = feature
                    .id
                    .try_clone_for_decode(ctx, "copy F3D input feature identity")?;
                let native_ref =
                    ctx.copy_retained_text(&state.id, "copy F3D input state reference")?;

                Ok(Some(FeatureInputTopology {
                    id,
                    input_of,
                    bodies,
                    faces,
                    edges,
                    vertices,
                    native_ref: Some(native_ref),
                }))
            },
        )?;
        if let Some(projection) = projection {
            storage.commit()?;
            ctx.push_vec(&mut projected, projection, "collect F3D input topologies")?;
        }
    }
    Ok(projected)
}

fn project_input_members<T: Eq + std::hash::Hash>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    slots: &[i64],
    operation: &'static str,
    mut id: impl FnMut(i64) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<Option<cadmpeg_ir::features::DistinctMembers<T>>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    ctx.reserve_vec(&mut members, slots.len(), operation)?;
    for &slot in ctx.admit_iter(slots, "scan F3D input topology members")? {
        members.push(id(slot)?);
    }
    Ok(Some(
        match cadmpeg_ir::features::DistinctMembers::try_from(members, ctx) {
            Ok(value) => value,
            Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(None),
            Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                return Err(limit.into())
            }
        },
    ))
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let (input_states, _input_state_storage) = ctx
        .with_scoped_storage("index F3D vertex recipe input states", || {
            vertex_recipe_input_states(ctx, scopes, timelines)
        })?;
    for (scope, &state_id) in ctx
        .admit_iter(&mut *scopes, "scan F3D work point recipe scopes")?
        .zip(&input_states)
    {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::WorkPoint {
            continue;
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        // The input topology and its index are found for the first recipe.
        let mut topology = None;
        let mut index = None;
        // A work point has a fixed number of inputs.
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let found = match topology {
                Some(found) => found,
                None => *topology.insert(
                    unique_history_state(ctx, histories, state_id)?
                        .and_then(|(_, state)| state.topology()),
                ),
            };
            let Some(topology) = found else {
                continue;
            };
            let index = match &mut index {
                Some(index) => index,
                None => index.insert(topology::boundary_vertex_index(ctx, topology)?),
            };
            let Some((vertex, position)) = vertex_recipe_candidate(ctx, recipe, topology, index)?
            else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.resolution = crate::records::feature::work_geometry::DesignVertexResolution::new(
                state_id, vertex,
            );
        }
    }

    for (scope, &state_id) in ctx
        .admit_iter(&mut *scopes, "scan F3D work plane recipe scopes")?
        .zip(&input_states)
    {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::WorkPlane {
            continue;
        }
        let transform = scope.work_plane_transform();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((_, state)) = unique_history_state(ctx, histories, state_id)? else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let index = topology::boundary_vertex_index(ctx, topology)?;
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(ctx, first, topology, &index)?,
            vertex_recipe_candidate(ctx, second, topology, &index)?,
            vertex_recipe_candidate(ctx, third, topology, &index)?,
        ) else {
            continue;
        };
        let candidates = [first, second, third];
        if !three_point_plane_matches(
            transform.map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            [first.1, second.1, third.1],
        ) {
            continue;
        }
        let [Some(first), Some(second), Some(third)] = candidates.map(|(vertex, _)| {
            crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, vertex)
        }) else {
            continue;
        };
        construction
            .try_set_resolution([first, second, third])
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

/// The input state of each work plane and work point scope: the state of the
/// latest earlier history-bearing scope of its stream in authored order. Among
/// equally ordered predecessors the first scope wins. Other scopes have none.
fn vertex_recipe_input_states<'s>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &'s [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
) -> Result<Vec<Option<i64>>, cadmpeg_core::CodecError> {
    use crate::records::feature::scope::DesignFeatureKind;

    let (source_ordinals, _ordinal_storage) =
        ctx.with_scoped_storage("index F3D authored scope ordinals", || {
            crate::design::feature_project::authored_scope_ordinals_per_stream(
                ctx, scopes, timelines,
            )
        })?;
    let ordinal_of = |scope: &'s crate::records::feature::scope::DesignParameterScope| {
        let stream = native_stream_of(ctx, &scope.id)?.unwrap_or(crate::ids::DEFAULT_STREAM);
        Ok::<_, cadmpeg_core::CodecError>(
            ctx.get_hash_map(
                &source_ordinals,
                &(stream, scope.record_index),
                "find F3D vertex recipe scope ordinal",
            )?
            .map(|&ordinal| (stream, ordinal)),
        )
    };
    // History-bearing scopes of each stream in ascending authored order; the
    // stable sort keeps scope order among equal ordinals.
    let operation = "index F3D vertex recipe predecessors";
    let mut predecessors_storage = ctx.reserve_scoped(0, operation)?;
    let mut predecessors = HashMap::<&str, Vec<(u64, i64)>>::new();
    for scope in ctx.admit_iter(scopes, "scan F3D vertex recipe predecessors")? {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some((stream, ordinal)) = ordinal_of(scope)? else {
            continue;
        };
        predecessors_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut predecessors,
                stream,
                (ordinal, state_id),
                operation,
                operation,
            )
        })?;
    }
    // Each stream's list is sorted on its first lookup.
    let mut sorted_streams = HashSet::new();
    let mut input_states =
        ctx.collection_vec(scopes.len(), "index F3D vertex recipe input states")?;
    for scope in ctx.admit_iter(scopes, "scan F3D vertex recipe scopes")? {
        input_states.push(None);
        if !matches!(
            scope.kind(),
            DesignFeatureKind::WorkPlane | DesignFeatureKind::WorkPoint
        ) {
            continue;
        }
        let Some((stream, ordinal)) = ordinal_of(scope)? else {
            continue;
        };
        let Some(candidates) = ctx.get_mut_hash_map(&mut predecessors, stream, operation)? else {
            continue;
        };
        if predecessors_storage
            .with_storage(|| ctx.insert_hash_set(&mut sorted_streams, stream, operation))?
        {
            ctx.stable_sort_by_key(
                candidates,
                |&(ordinal, _)| ordinal,
                Ord::cmp,
                "sort F3D vertex recipe predecessors",
            )?;
        }
        let earlier = ctx.partition_point(
            candidates,
            |&(candidate, _)| Ok(candidate < ordinal),
            "find F3D vertex recipe predecessor",
        )?;
        let Some(&(latest, _)) = earlier.checked_sub(1).and_then(|last| candidates.get(last))
        else {
            continue;
        };
        let first_latest = ctx.partition_point(
            candidates,
            |&(candidate, _)| Ok(candidate < latest),
            "find F3D vertex recipe predecessor",
        )?;
        if let (Some(slot), Some(&(_, state_id))) =
            (input_states.last_mut(), candidates.get(first_latest))
        {
            *slot = Some(state_id);
        }
    }
    Ok(input_states)
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let scopes = ScopesByRecord::new(decode, scopes)?;
    let mut recipe_index = RecentTopologyValue::default();
    for operand in decode.admit_iter(operands, "scan F3D edge treatment vertex operands")? {
        operand.recipe.resolution = None;
        let Some(scope) = scopes.find(decode, &operand.id, operand.scope_record_index)? else {
            continue;
        };
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, histories)?
        else {
            continue;
        };
        let Some((_, _, previous)) = bound_history_state_pair(
            decode,
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )?
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let index = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe.recipe_references,
            "scan F3D edge treatment vertex recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, index)?;
        }
        let Some(vertex) = recipe_reference_common_vertex(decode, &operand.recipe, topology)?
        else {
            continue;
        };
        operand.recipe.resolution =
            crate::records::feature::work_geometry::DesignVertexResolution::new(
                previous_state_id,
                vertex,
            );
    }
    Ok(())
}

fn vertex_recipe_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &AsmHistoricalTopology,
    index: &topology::BoundaryVertexIndex<'_>,
) -> Result<Option<(i64, cadmpeg_ir::math::Point3)>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "collect F3D vertex recipe face slots")?;
    let mut face_slots = Vec::new();
    let mut history_source = IntoIterator::into_iter(&recipe.recipe_references);
    while let Some(reference) =
        ctx.next_charged(&mut history_source, "scan F3D recipe recipe references")?
    {
        let mut slots_storage =
            ctx.reserve_scoped(0, "collect F3D vertex recipe candidate faces")?;
        let mut slots = Vec::new();
        for face in ctx.admit_iter(
            &reference.candidate_faces,
            "scan F3D vertex recipe candidate faces",
        )? {
            let Some(slot) = stable_ref(ctx, face.as_str())? else {
                continue;
            };
            if !ctx.contains_hash_set(&index.faces, &slot, "find F3D history faces")? {
                continue;
            }
            ctx.push_scoped_vec(
                &mut slots_storage,
                &mut slots,
                slot,
                "collect F3D vertex recipe candidate faces",
            )?;
        }
        ctx.sort_unstable_by(
            &mut slots,
            |value| value,
            Ord::cmp,
            "sort F3D vertex recipe candidate faces",
        )?;
        ctx.dedup_vec(&mut slots, "deduplicate F3D vertex recipe candidate faces")?;
        let [slot] = slots.as_slice() else {
            return Ok(None);
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut face_slots,
            *slot,
            "collect F3D vertex recipe face slots",
        )?;
    }
    if face_slots.is_empty() {
        return Ok(None);
    }
    let Some(vertex) = common_face_vertex(ctx, &face_slots, index)? else {
        return Ok(None);
    };
    Ok(
        unique_historical_vertex_position(ctx, vertex, topology)?
            .map(|position| (vertex, position)),
    )
}

fn three_point_plane_matches(
    transform: Option<[[f64; 4]; 4]>,
    points: [cadmpeg_ir::math::Point3; 3],
) -> bool {
    use cadmpeg_ir::math::{Point3, Vector3};

    let Some(transform) = transform else {
        return false;
    };
    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let Some(normal) = Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()
    else {
        return false;
    };
    let Some(point_normal) = points[1]
        .vector_from(points[0])
        .cross(points[2].vector_from(points[0]))
        .unit()
    else {
        return false;
    };
    let scale = points
        .iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    (normal.dot(point_normal).abs() - 1.0).abs() <= WORK_POINT_POSITION_TOLERANCE
        && points.iter().all(|point| {
            point.vector_from(origin).dot(normal).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn common_face_vertex(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    index: &topology::BoundaryVertexIndex<'_>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D boundary vertices")?;
    let mut common = None::<BTreeSet<i64>>;
    let mut history_source = IntoIterator::into_iter(face_slots);
    while let Some(face) = decode.next_charged(&mut history_source, "scan F3D common face slots")? {
        let (vertices, reference_storage) =
            decode.with_scoped_storage("collect F3D boundary vertices", || {
                topology::boundary_vertices_for_faces(
                    decode,
                    std::iter::once(Ok::<_, cadmpeg_core::CodecError>(Some(*face))),
                    index,
                )
            })?;
        let Some(vertices) = vertices else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            decode.retain_btree_set(
                common,
                |vertex| {
                    decode.contains_btree_set(&vertices, vertex, "find F3D common face vertex")
                },
                "intersect F3D common face vertices",
            )?;
        } else {
            vertices_storage.with_storage(|| reference_storage.commit())?;
            common = Some(vertices);
        }
    }
    Ok(single_btree_value(common))
}

fn recipe_reference_common_vertex(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let index = topology::boundary_vertex_index(decode, topology)?;
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D boundary vertices")?;
    let mut common = None::<BTreeSet<i64>>;
    let mut history_source = IntoIterator::into_iter(&recipe.recipe_references);
    while let Some(reference) =
        decode.next_charged(&mut history_source, "scan F3D recipe recipe references")?
    {
        let faces = reference.candidate_faces.iter().map(|face| {
            let Some(slot) = stable_ref(decode, face.as_str())? else {
                return Ok(None);
            };
            Ok(decode
                .contains_hash_set(&index.faces, &slot, "find F3D history faces")?
                .then_some(slot))
        });
        let (vertices, reference_storage) = decode
            .with_scoped_storage("collect F3D boundary vertices", || {
                topology::boundary_vertices_for_faces(decode, faces, &index)
            })?;
        let Some(vertices) = vertices else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            decode.retain_btree_set(
                common,
                |vertex| {
                    decode.contains_btree_set(&vertices, vertex, "find F3D common face vertex")
                },
                "intersect F3D common face vertices",
            )?;
        } else {
            vertices_storage.with_storage(|| reference_storage.commit())?;
            common = Some(vertices);
        }
    }
    Ok(single_btree_value(common))
}

/// Returns the only value of a present set; an absent, empty or multi-valued
/// set yields `None`. Only a singleton is visited.
fn single_btree_value<T>(values: Option<BTreeSet<T>>) -> Option<T> {
    let values = values.filter(|values| values.len() == 1)?;
    values.into_iter().next()
}

fn unique_historical_vertex_position(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<cadmpeg_ir::math::Point3>, cadmpeg_core::CodecError> {
    let Some(binding) = unique_by(
        ctx,
        &topology.vertex_points,
        |binding| Ok(binding.entity == vertex),
        "find F3D historical vertex point",
    )?
    else {
        return Ok(None);
    };
    let Some(position) = unique_by(
        ctx,
        &topology.point_positions,
        |position| Ok(position.point == binding.carrier),
        "find F3D historical point position",
    )?
    else {
        return Ok(None);
    };
    Ok(position.position.is_finite().then_some(position.position))
}

fn point_matches(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

#[cfg(test)]
fn feature_input_prefix(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::IdentityKey {
    let feature_key = feature.key();
    crate::ids::history_input_prefix(&feature_key, previous_state_id)
}

fn unique_history_state<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
) -> Result<Option<(&'a AsmHistory, &'a AsmDeltaState)>, cadmpeg_core::CodecError> {
    let operation = "find F3D history state";
    let mut found_state_index = None;
    let Some(index) = decode.position_by(
        histories,
        |history| {
            found_state_index = unique_history_state_in(decode, history, state_id)?;
            Ok(found_state_index.is_some())
        },
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| Ok(unique_history_state_in(decode, history, state_id)?.is_some()),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    let (Some(history), Some(state_index)) = (histories.get(index), found_state_index) else {
        return Ok(None);
    };
    Ok(history
        .states
        .get(state_index)
        .map(|state| (history, state)))
}

fn unique_history_state_in(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state_id: i64,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let operation = "find F3D ASM history state";
    let Some(index) = decode.position_by(
        &history.states,
        |state| Ok(state.state_id == state_id),
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &history.states[index + 1..],
            |state| Ok(state.state_id == state_id),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(index))
}

/// Return the effective input state for a scope. Some Design scope envelopes
/// omit the preceding state identity even though the current ASM delta state
/// carries the direct transition predecessor.
pub(crate) fn effective_scope_previous_history_state_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if let Some(previous_state_id) = scope.previous_history_state_id() {
        return Ok(Some(previous_state_id));
    }
    let Some(state_id) = scope.history_state_id() else {
        return Ok(None);
    };
    let Some((history, state)) = unique_history_state(decode, histories, state_id)? else {
        return Ok(None);
    };
    linked_previous_state_id(decode, history, state)
}

pub(crate) fn unique_history_state_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
    // A directly linked pair decides the answer, ambiguous or not; only an
    // absent direct link falls back to reachability.
    match unique_history_state_pair_for_link(decode, histories, state_id, previous_state_id, true)?
    {
        StatePairSearch::Unique(pair) => return Ok(Some(pair)),
        StatePairSearch::Ambiguous => return Ok(None),
        StatePairSearch::Absent => {}
    }
    Ok(
        match unique_history_state_pair_for_link(
            decode,
            histories,
            state_id,
            previous_state_id,
            false,
        )? {
            StatePairSearch::Unique(pair) => Some(pair),
            StatePairSearch::Ambiguous | StatePairSearch::Absent => None,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HemGapLengthForm {
    Flat,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HemGeometrySemantics {
    pub(crate) direction: Option<cadmpeg_ir::features::SheetMetalHemDirection>,
    pub(crate) gap_length_form: Option<HemGapLengthForm>,
}

/// Resolve Hem semantics from the bend carriers in the owning history
/// transition. The source operation's fixed fields do not carry these
/// meanings; the selected edge and the inserted coaxial cylinders do.
pub(crate) fn hem_geometry_semantics(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> Result<HemGeometrySemantics, cadmpeg_core::CodecError> {
    let unresolved = HemGeometrySemantics {
        direction: None,
        gap_length_form: None,
    };
    let Some(state_id) = scope.history_state_id() else {
        return Ok(unresolved);
    };
    let Some(previous_state_id) =
        effective_scope_previous_history_state_id(decode, scope, histories)?
    else {
        return Ok(unresolved);
    };
    let Some((_, state, previous)) =
        unique_history_state_pair(decode, histories, state_id, previous_state_id)?
    else {
        return Ok(unresolved);
    };
    let (Some(previous_topology), Some(transition)) =
        (previous.topology(), state.transition.as_ref())
    else {
        return Ok(unresolved);
    };
    let Some(current_topology) = state.topology() else {
        return Ok(unresolved);
    };
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !decode.any_by(
        &current_topology.surface_cylinders,
        |cylinder| {
            decode.contains(
                inserted_surfaces,
                &cylinder.surface,
                "find F3D inserted Hem cylinder surface",
            )
        },
        "scan F3D inserted Hem cylinders",
    )? {
        return Ok(unresolved);
    }
    let edge_direction = topology::historical_edge_axis(decode, edge_slot, previous_topology)?
        .map(|(_, direction)| direction);
    let Some(_) = decode.find_by(
        &current_topology.surface_cylinders,
        |cylinder| {
            hem_cylinder_matches(
                decode,
                cylinder,
                inserted_surfaces,
                edge_direction,
                "match F3D inserted Hem cylinder surface",
            )
        },
        "find F3D Hem direction cylinder",
    )?
    else {
        return Ok(unresolved);
    };
    Ok(HemGeometrySemantics {
        direction: hem_direction_from_transition(
            decode,
            edge_slot,
            &current_topology.surface_cylinders,
            inserted_surfaces,
            edge_direction,
            previous_topology,
            transition,
        )?,
        gap_length_form: hem_gap_length_form(
            decode,
            &current_topology.surface_cylinders,
            inserted_surfaces,
            edge_direction,
        )?,
    })
}

fn hem_cylinder_matches(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    cylinder: &AsmHistoricalCylinder,
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
    membership_operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if !decode.contains(inserted_surfaces, &cylinder.surface, membership_operation)? {
        return Ok(false);
    }
    Ok(edge_direction.is_none_or(|direction| parallel_directions(direction, cylinder.axis)))
}

fn hem_gap_length_form(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    cylinders: &[AsmHistoricalCylinder],
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
) -> Result<Option<HemGapLengthForm>, cadmpeg_core::CodecError> {
    let mut first = None;
    let mut second = None;
    let mut history_source = IntoIterator::into_iter(cylinders);
    while let Some(cylinder) =
        decode.next_charged(&mut history_source, "scan F3D Hem gap-length carriers")?
    {
        if !hem_cylinder_matches(
            decode,
            cylinder,
            inserted_surfaces,
            edge_direction,
            "match F3D inserted Hem cylinder surface",
        )? {
            continue;
        }
        if first.is_none() {
            first = Some(cylinder);
        } else if second.is_none() {
            second = Some(cylinder);
        } else {
            return Ok(None);
        }
    }
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return Ok(None);
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return Ok(None);
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Ok(Some(HemGapLengthForm::Open))
    } else if 2.0 * inner < thickness - tolerance {
        Ok(Some(HemGapLengthForm::Flat))
    } else {
        Ok(None)
    }
}

fn hem_direction_from_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_slot: i64,
    cylinders: &[AsmHistoricalCylinder],
    inserted_surfaces: &[i64],
    edge_direction: Option<cadmpeg_ir::math::Vector3>,
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Result<Option<cadmpeg_ir::features::SheetMetalHemDirection>, cadmpeg_core::CodecError> {
    if decode
        .find_by(
            cylinders,
            |cylinder| {
                hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )
            },
            "find F3D Hem direction cylinder",
        )?
        .is_none()
    {
        return Ok(None);
    }
    let edge_context = selection::historical_edge_context(decode, edge_slot, previous)?;
    let mut candidate = None;
    let mut history_source = IntoIterator::into_iter(&edge_context.incident_loops).enumerate();
    while let Some((ordinal, incident)) =
        decode.next_charged(&mut history_source, "scan F3D Hem incident loops")?
    {
        let face = incident.face_slot;
        let duplicate = decode.any_by(
            &edge_context.incident_loops[..ordinal],
            |earlier| Ok(earlier.face_slot == face),
            "find duplicate F3D Hem incident face",
        )?;
        if duplicate {
            continue;
        }
        if decode.contains(
            &transition.topology.faces.deleted,
            &face,
            "find deleted F3D Hem face",
        )? {
            continue;
        }
        let Some(binding) = unique_by(
            decode,
            &previous.face_surfaces,
            |binding| Ok(binding.entity == face),
            "find F3D Hem face surface",
        )?
        else {
            continue;
        };
        let Some(plane) = unique_by(
            decode,
            &previous.surface_planes,
            |plane| Ok(plane.surface == binding.carrier),
            "find F3D Hem face plane",
        )?
        else {
            continue;
        };
        let length = plane.normal.norm();
        if !(length.is_finite() && length > 0.0) {
            continue;
        }
        let normal = plane.normal.scale(1.0 / length);
        let Some(first_cylinder) = decode.find_by(
            cylinders,
            |cylinder| {
                hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )
            },
            "find F3D Hem direction cylinder",
        )?
        else {
            continue;
        };
        let first = normal.dot(first_cylinder.origin.vector_from(plane.origin));
        if !first.is_finite() {
            continue;
        }
        let scale = decode.fold(
            cylinders,
            0.0_f64,
            |scale, cylinder| {
                if !hem_cylinder_matches(
                    decode,
                    cylinder,
                    inserted_surfaces,
                    edge_direction,
                    "match F3D inserted Hem cylinder surface",
                )? {
                    return Ok(scale);
                }
                Ok(scale
                    .max(cylinder.origin.x.abs())
                    .max(cylinder.origin.y.abs())
                    .max(cylinder.origin.z.abs()))
            },
            "scan F3D Hem cylinder direction scale",
        )?;
        let sign_tolerance = EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7
            * (1.0 + plane.origin.x.abs() + plane.origin.y.abs() + plane.origin.z.abs() + scale);
        if first.abs() <= sign_tolerance
            || decode.any_by(
                cylinders,
                |cylinder| {
                    if !hem_cylinder_matches(
                        decode,
                        cylinder,
                        inserted_surfaces,
                        edge_direction,
                        "match F3D inserted Hem cylinder surface",
                    )? {
                        return Ok(false);
                    }
                    let offset = normal.dot(cylinder.origin.vector_from(plane.origin));
                    Ok(!offset.is_finite()
                        || offset.abs() <= sign_tolerance
                        || offset.is_sign_positive() != first.is_sign_positive())
                },
                "check F3D Hem cylinder direction offsets",
            )?
        {
            continue;
        }
        if candidate.replace(first.is_sign_positive()).is_some() {
            return Ok(None);
        }
    }
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    Ok(Some(if candidate {
        cadmpeg_ir::features::SheetMetalHemDirection::Forward
    } else {
        cadmpeg_ir::features::SheetMetalHemDirection::Reverse
    }))
}

fn parallel_directions(left: cadmpeg_ir::math::Vector3, right: cadmpeg_ir::math::Vector3) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    if !(left_length.is_finite()
        && left_length > 0.0
        && right_length.is_finite()
        && right_length > 0.0)
    {
        return false;
    }
    let dot = left
        .scale(1.0 / left_length)
        .dot(right.scale(1.0 / right_length));
    dot.is_finite() && (dot.abs() - 1.0).abs() <= EPS_HISTORY_PARALLEL_DIRECTIONS_E7
}

fn history_state_pair(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Result<Option<(usize, usize)>, cadmpeg_core::CodecError> {
    let Some(state_index) = unique_history_state_in(decode, history, state_id)? else {
        return Ok(None);
    };
    let Some(state) = history.states.get(state_index) else {
        return Ok(None);
    };
    if require_direct {
        let Some(linked_previous) = linked_previous_state_id(decode, history, state)? else {
            return Ok(None);
        };
        if linked_previous != previous_state_id {
            return Ok(None);
        }
    }
    let Some(previous_index) = unique_history_state_in(decode, history, previous_state_id)? else {
        return Ok(None);
    };
    if !require_direct && !history_state_reaches(decode, history, state, previous_state_id)? {
        return Ok(None);
    }
    Ok(Some((state_index, previous_index)))
}

/// A unique state pair: its history, the state and the previous state.
type HistoryStatePair<'a> = (&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState);

/// The outcome of searching every history for one state pair.
enum StatePairSearch<'a> {
    /// No history holds the pair.
    Absent,
    /// More than one history holds it, or its states cannot be read back.
    Ambiguous,
    /// Exactly one history holds it.
    Unique(HistoryStatePair<'a>),
}

fn unique_history_state_pair_for_link<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Result<StatePairSearch<'a>, cadmpeg_core::CodecError> {
    let operation = "find F3D history state pair";
    let mut pair_indices = None;
    let Some(index) = decode.position_by(
        histories,
        |history| {
            pair_indices =
                history_state_pair(decode, history, state_id, previous_state_id, require_direct)?;
            Ok(pair_indices.is_some())
        },
        operation,
    )?
    else {
        return Ok(StatePairSearch::Absent);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| {
                Ok(history_state_pair(
                    decode,
                    history,
                    state_id,
                    previous_state_id,
                    require_direct,
                )?
                .is_some())
            },
            operation,
        )?
        .is_some()
    {
        return Ok(StatePairSearch::Ambiguous);
    }
    let (Some(history), Some((state_index, previous_index))) = (histories.get(index), pair_indices)
    else {
        return Ok(StatePairSearch::Ambiguous);
    };
    let (Some(state), Some(previous)) = (
        history.states.get(state_index),
        history.states.get(previous_index),
    ) else {
        return Ok(StatePairSearch::Ambiguous);
    };
    Ok(StatePairSearch::Unique((history, state, previous)))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Result<Option<&'a AsmHistory>, cadmpeg_core::CodecError> {
    let Some(history_id) =
        decode.get_hash_map(scope_histories, scope_id, "find F3D scope history binding")?
    else {
        return Ok(None);
    };
    let operation = "find F3D bound history";
    let Some(index) = decode.position_by(
        histories,
        |history| decode.equal(history.id.as_str(), history_id.as_str(), operation),
        operation,
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[index + 1..],
            |history| decode.equal(history.id.as_str(), history_id.as_str(), operation),
            operation,
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(&histories[index]))
}

fn bound_history_state_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
    let Some(history) = bound_scope_history(decode, scope_id, scope_histories, histories)? else {
        return Ok(None);
    };
    let Some((state_index, previous_index)) =
        history_state_pair(decode, history, state_id, previous_state_id, false)?
    else {
        return Ok(None);
    };
    let (Some(state), Some(previous)) = (
        history.states.get(state_index),
        history.states.get(previous_index),
    ) else {
        return Ok(None);
    };
    Ok(Some((history, state, previous)))
}

fn insert_scope_history_binding(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    let scope = {
        let ctx = decode;
        ctx.copy_retained_text(scope_id, "copy F3D bound scope identity")?
    };
    let history = {
        let ctx = decode;
        ctx.copy_retained_text(history_id, "copy F3D bound history identity")?
    };
    decode
        .insert_hash_map(resolved, scope, history, "index F3D scope history bindings")
        .map(|_| ())?;
    Ok(())
}

pub(crate) fn bind_scope_histories(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> Result<HashMap<String, String>, cadmpeg_core::CodecError> {
    let mut candidate_storage = decode.reserve_scoped(0, "collect F3D scope history candidates")?;
    let mut candidates = Vec::new();
    for scope in decode.admit_iter(scopes, "scan F3D scope history candidates")? {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let mut scope_candidates = Vec::new();
        if let Some(previous_state_id) = scope.previous_history_state_id() {
            for history in decode.admit_iter(histories, "scan F3D direct scope histories")? {
                if history_state_pair(decode, history, state_id, previous_state_id, true)?.is_some()
                {
                    decode.push_scoped_vec(
                        &mut candidate_storage,
                        &mut scope_candidates,
                        history,
                        "collect F3D direct scope histories",
                    )?;
                }
            }
            if scope_candidates.is_empty() {
                for history in decode.admit_iter(histories, "scan F3D reachable scope histories")? {
                    if history_state_pair(decode, history, state_id, previous_state_id, false)?
                        .is_some()
                    {
                        decode.push_scoped_vec(
                            &mut candidate_storage,
                            &mut scope_candidates,
                            history,
                            "collect F3D reachable scope histories",
                        )?;
                    }
                }
            }
        } else {
            for history in decode.admit_iter(histories, "scan F3D matching scope histories")? {
                if unique_history_state_in(decode, history, state_id)?.is_some() {
                    decode.push_scoped_vec(
                        &mut candidate_storage,
                        &mut scope_candidates,
                        history,
                        "collect F3D matching scope histories",
                    )?;
                }
            }
        }
        if !scope_candidates.is_empty() {
            decode.push_scoped_vec(
                &mut candidate_storage,
                &mut candidates,
                (scope, scope_candidates),
                "collect F3D scopes with histories",
            )?;
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    let mut history_source = IntoIterator::into_iter(&candidates);
    while let Some((scope, candidates)) =
        decode.next_charged(&mut history_source, "scan F3D candidates")?
    {
        if candidates.len() == 1 {
            insert_scope_history_binding(decode, &mut resolved, &scope.id, &candidates[0].id)?;
            continue;
        }
        let next_scope_record_index = decode.fold(
            scopes,
            None::<u32>,
            |next, candidate| {
                if crate::ids::same_native_occurrence(decode, &candidate.id, &scope.id)?
                    && candidate.record_index > scope.record_index
                {
                    Ok(Some(next.map_or(candidate.record_index, |record_index| {
                        record_index.min(candidate.record_index)
                    })))
                } else {
                    Ok(next)
                }
            },
            "find F3D next scope record index",
        )?;
        let mut is_output_binding = |binding: &crate::records::bodies::DesignBodyBinding| {
            Ok(
                crate::ids::same_native_occurrence(decode, binding.id(), &scope.id)?
                    && binding.entity_suffix > u64::from(scope.record_index)
                    && next_scope_record_index
                        .is_none_or(|next| binding.entity_suffix < u64::from(next)),
            )
        };
        if let Some(binding_index) = decode.position_by(
            body_bindings,
            &mut is_output_binding,
            "find F3D output body binding",
        )? {
            if decode
                .position_by(
                    &body_bindings[binding_index + 1..],
                    &mut is_output_binding,
                    "find F3D output body binding",
                )?
                .is_none()
            {
                let binding = &body_bindings[binding_index];
                let mut matches_binding_source = |history: &&AsmHistory| {
                    let Some(source) = historical_brep_source(decode, &history.id)? else {
                        return Ok(false);
                    };
                    let Some(blob_source) = decode.strip_prefix(
                        binding.blob_name(),
                        "BREP.",
                        "strip F3D output BREP prefix",
                    )?
                    else {
                        return Ok(false);
                    };
                    decode.equal(blob_source, source, "compare F3D output BREP source")
                };
                if let Some(history_index) = decode.position_by(
                    candidates.as_slice(),
                    &mut matches_binding_source,
                    "find F3D output BREP history",
                )? {
                    if decode
                        .position_by(
                            &candidates.as_slice()[history_index + 1..],
                            &mut matches_binding_source,
                            "find F3D output BREP history",
                        )?
                        .is_none()
                    {
                        let history = candidates[history_index];
                        insert_scope_history_binding(
                            decode,
                            &mut resolved,
                            &scope.id,
                            &history.id,
                        )?;
                        continue;
                    }
                }
            }
        }
        let candidate_has_face = |source: Option<&str>| -> Result<bool, cadmpeg_core::CodecError> {
            let mut history_source = IntoIterator::into_iter(body_recipe_operands);
            while let Some(operand) = decode.next_charged(
                &mut history_source,
                "scan F3D body recipe candidate operands",
            )? {
                if !crate::ids::same_native_occurrence(decode, &operand.id, &scope.id)?
                    || operand.scope_record_index != scope.record_index
                {
                    continue;
                }
                let mut history_source = IntoIterator::into_iter(operand.references());
                while let Some(reference) = decode.next_charged(
                    &mut history_source,
                    "scan F3D body recipe candidate references",
                )? {
                    let mut history_source = IntoIterator::into_iter(&reference.candidate_faces);
                    while let Some(face) = decode
                        .next_charged(&mut history_source, "scan F3D body recipe candidate faces")?
                    {
                        let matches_source = match source {
                            Some(source) => active_brep_face_matches_source(decode, face, source)?,
                            None => true,
                        };
                        if matches_source {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        };
        if candidate_has_face(None)? {
            let mut matching_history = None;
            let mut ambiguous_history = false;
            let mut history_source = IntoIterator::into_iter(candidates.as_slice());
            while let Some(history) =
                decode.next_charged(&mut history_source, "scan F3D candidate face histories")?
            {
                let Some(source) = historical_brep_source(decode, &history.id)? else {
                    continue;
                };
                if candidate_has_face(Some(source))? {
                    if matching_history.is_some() {
                        ambiguous_history = true;
                        break;
                    }
                    matching_history = Some(*history);
                }
            }
            if !ambiguous_history {
                if let Some(history) = matching_history {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    continue;
                }
            }
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let mut referenced_history_id = None;
        let mut ambiguous_referenced_history = false;
        {
            let mut resolve_body_reference =
                |suffix: u32| -> Result<bool, cadmpeg_core::CodecError> {
                    if ambiguous_referenced_history {
                        return Ok(true);
                    }
                    let mut binding_matches_reference =
                        |binding: &crate::records::bodies::DesignBodyBinding| {
                            Ok(
                                crate::ids::same_native_occurrence(
                                    decode,
                                    binding.id(),
                                    &scope.id,
                                )? && binding.entity_suffix == u64::from(suffix),
                            )
                        };
                    let Some(binding_index) = decode.position_by(
                        body_bindings,
                        &mut binding_matches_reference,
                        "find F3D referenced body binding",
                    )?
                    else {
                        return Ok(false);
                    };
                    if decode
                        .position_by(
                            &body_bindings[binding_index + 1..],
                            &mut binding_matches_reference,
                            "find F3D referenced body binding",
                        )?
                        .is_some()
                    {
                        return Ok(false);
                    }
                    let binding = &body_bindings[binding_index];
                    let mut history_matches_binding = |history: &&AsmHistory| {
                        let Some(source) = historical_brep_source(decode, &history.id)? else {
                            return Ok(false);
                        };
                        let Some(blob_source) = decode.strip_prefix(
                            binding.blob_name(),
                            "BREP.",
                            "strip F3D referenced BREP prefix",
                        )?
                        else {
                            return Ok(false);
                        };
                        decode.equal(blob_source, source, "compare F3D referenced BREP source")
                    };
                    let Some(history_index) = decode.position_by(
                        candidates.as_slice(),
                        &mut history_matches_binding,
                        "find F3D referenced BREP history",
                    )?
                    else {
                        return Ok(false);
                    };
                    if decode
                        .position_by(
                            &candidates.as_slice()[history_index + 1..],
                            &mut history_matches_binding,
                            "find F3D referenced BREP history",
                        )?
                        .is_some()
                    {
                        return Ok(false);
                    }
                    let history_id = candidates[history_index].id.as_str();
                    if let Some(previous_history_id) = referenced_history_id {
                        if !decode.equal(
                            previous_history_id,
                            history_id,
                            "compare F3D referenced history IDs",
                        )? {
                            ambiguous_referenced_history = true;
                            return Ok(true);
                        }
                    } else {
                        referenced_history_id = Some(history_id);
                    }
                    Ok(false)
                };
            match construction.body_reference_records() {
                DesignBaseFeatureBodyReferenceSource::ResultRows(rows) => {
                    let mut history_source = IntoIterator::into_iter(rows);
                    while let Some(row) =
                        decode.next_charged(&mut history_source, "scan F3D body reference rows")?
                    {
                        if resolve_body_reference(row.reference.value)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::RepeatedResultRows { first, rest } => {
                    if !resolve_body_reference(first.reference.value)? {
                        decode.any_by(
                            rest,
                            |(row, _)| resolve_body_reference(row.reference.value),
                            "scan F3D repeated body reference rows",
                        )?;
                    }
                }
                DesignBaseFeatureBodyReferenceSource::LegacyRows(rows) => {
                    let mut history_source = IntoIterator::into_iter(rows);
                    while let Some(row) = decode
                        .next_charged(&mut history_source, "scan F3D legacy body reference rows")?
                    {
                        if resolve_body_reference(row.entity.value)? {
                            break;
                        }
                    }
                }
                DesignBaseFeatureBodyReferenceSource::SingleBody(suffix) => {
                    resolve_body_reference(*suffix)?;
                }
                DesignBaseFeatureBodyReferenceSource::Empty => {}
            }
        }
        if !ambiguous_referenced_history {
            if let Some(history_id) = referenced_history_id {
                insert_scope_history_binding(decode, &mut resolved, &scope.id, history_id)?;
            }
        }
    }
    let mut groups_storage = decode.reserve_scoped(0, "index F3D scope history groups")?;
    let mut groups = BTreeMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in decode
        .admit_iter(&candidates, "scan F3D scope history group candidates")?
        .enumerate()
    {
        let (Some(stream), Some(state_id)) = (
            native_stream_of(decode, &scope.id)?,
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        decode.push_scoped_btree_group(
            &mut groups_storage,
            &mut groups,
            key,
            || index,
            0,
            "index F3D scope history groups",
        )?;
    }
    for members in decode
        .admit_iter(&groups, "scan F3D groups")?
        .map(|(_, value)| value)
    {
        let mut candidate_histories_storage =
            decode.reserve_scoped(0, "index F3D scope candidate histories")?;
        let mut candidate_histories = HashSet::new();
        for index in decode.admit_iter(members, "scan F3D scope history group members")? {
            for history in decode.admit_iter(
                &candidates[*index].1,
                "scan F3D scope history group histories",
            )? {
                candidate_histories_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut candidate_histories,
                        history.id.as_str(),
                        "index F3D scope candidate histories",
                    )
                })?;
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned_storage =
                decode.reserve_scoped(0, "index F3D assigned scope histories")?;
            let mut assigned = HashSet::new();
            for index in decode.admit_iter(members, "scan F3D assigned history members")? {
                if let Some(history_id) = decode.get_hash_map(
                    &resolved,
                    &candidates[*index].0.id,
                    "find F3D assigned history identity",
                )? {
                    assigned_storage.with_storage(|| {
                        decode.insert_string_set(
                            &mut assigned,
                            history_id,
                            "index F3D assigned scope histories",
                        )
                    })?;
                }
            }
            let mut resolved_member_count: usize = 0;
            for index in decode.admit_iter(members, "count F3D resolved history members")? {
                if decode.contains_key_hash_map(
                    &resolved,
                    &candidates[*index].0.id,
                    "check F3D resolved history member",
                )? {
                    resolved_member_count =
                        resolved_member_count.checked_add(1).ok_or_else(|| {
                            decode.refuse_codec_limit(
                                "count F3D resolved history members",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?;
                }
            }
            if assigned.len() != resolved_member_count {
                break;
            }
            let mut progress = false;
            for index in decode.admit_iter(members, "scan F3D unresolved history members")? {
                let (scope, scope_candidates) = &candidates[*index];
                if decode.contains_key_hash_map(
                    &resolved,
                    &scope.id,
                    "check F3D scope history resolution",
                )? {
                    continue;
                }
                if let Some(history) = unique_by(
                    decode,
                    scope_candidates,
                    |history| {
                        decode
                            .contains_hash_set(
                                &assigned,
                                history.id.as_str(),
                                "find F3D history assigned",
                            )
                            .map(|present| !present)
                    },
                    "scan F3D unassigned histories",
                )? {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    Ok(resolved)
}

fn history_state_reaches(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut current = state;
    let state_chain_steps = 0..=history.states.len();
    let mut history_source = IntoIterator::into_iter(state_chain_steps);
    while decode
        .next_charged(&mut history_source, "walk F3D history state chain")?
        .is_some()
    {
        if current.state_id == previous_state_id {
            return Ok(true);
        }
        let Some(previous_state_id) = linked_previous_state_id(decode, history, current)? else {
            return Ok(false);
        };
        let Some(previous_index) = unique_history_state_in(decode, history, previous_state_id)?
        else {
            return Ok(false);
        };
        let Some(previous) = history.states.get(previous_index) else {
            return Ok(false);
        };
        current = previous;
    }
    Ok(false)
}

/// Return the state ID reached by a delta state's `next` link.
///
/// Reject disagreement between the derived transition predecessor and the raw
/// `next` chain.
fn linked_previous_state_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let linked = if let Some(node_index) = state.next_ref {
        let operation = "find F3D ASM predecessor node";
        if let Some(index) = decode.position_by(
            &history.states,
            |candidate| Ok(candidate.node_index == node_index),
            operation,
        )? {
            if decode
                .position_by(
                    &history.states[index + 1..],
                    |candidate| Ok(candidate.node_index == node_index),
                    operation,
                )?
                .is_some()
            {
                None
            } else {
                Some(history.states[index].state_id)
            }
        } else {
            None
        }
    } else {
        None
    };
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) => {
            if derived == linked {
                Ok(Some(derived))
            } else {
                Ok(None)
            }
        }
        (Some(derived), _) => Ok(Some(derived)),
        (None, Some(linked)) => Ok(Some(linked)),
        (None, None) => Ok(None),
    }
}

/// Design scopes by record index, for operands that name their scope by
/// stream and record.
struct ScopesByRecord<'s, 'ctx> {
    scopes: HashMap<u32, Vec<&'s DesignScope>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'s, 'ctx> ScopesByRecord<'s, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        scopes: &'s [DesignScope],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D scopes by record";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut by_record = HashMap::new();
        for scope in ctx.admit_iter(scopes, operation)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut by_record,
                    scope.record_index,
                    scope,
                    operation,
                    operation,
                )
            })?;
        }
        Ok(Self {
            scopes: by_record,
            _storage: storage,
        })
    }

    /// The one scope with `record_index` in the stream of `operand_id`.
    fn find(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operand_id: &str,
        record_index: u32,
    ) -> Result<Option<&'s DesignScope>, cadmpeg_core::CodecError> {
        let Some(candidates) =
            ctx.get_hash_map(&self.scopes, &record_index, "find F3D history scopes")?
        else {
            return Ok(None);
        };
        let stream = native_stream_of(ctx, operand_id)?;
        let same_stream = |scope: &&DesignScope| {
            ctx.equal(
                &native_stream_of(ctx, &scope.id)?,
                &stream,
                "compare F3D operand scope stream",
            )
        };
        let operation = "find F3D operand scope";
        let Some(index) = ctx.position_by(candidates, same_stream, operation)? else {
            return Ok(None);
        };
        if ctx.any_by(&candidates[index + 1..], same_stream, operation)? {
            return Ok(None);
        }
        Ok(Some(candidates[index]))
    }
}

/// Operand groups by owning scope record and group record.
struct GroupsByRecord<'g, 'ctx> {
    groups: HashMap<(u32, u32), Vec<&'g OperandGroup>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'g, 'ctx> GroupsByRecord<'g, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        groups: &'g [OperandGroup],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let operation = "index F3D operand groups by record";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut by_record = HashMap::new();
        for group in ctx.admit_iter(groups, operation)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut by_record,
                    (group.scope_record_index, group.record_index),
                    group,
                    operation,
                    operation,
                )
            })?;
        }
        Ok(Self {
            groups: by_record,
            _storage: storage,
        })
    }

    /// The one group of `scope` with `record_index` in `stream` that passes
    /// `accept`.
    fn find(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        stream: Option<&str>,
        scope: &DesignScope,
        record_index: u32,
        mut accept: impl FnMut(&OperandGroup) -> bool,
    ) -> Result<Option<&'g OperandGroup>, cadmpeg_core::CodecError> {
        let Some(candidates) = ctx.get_hash_map(
            &self.groups,
            &(scope.record_index, record_index),
            "find F3D history groups",
        )?
        else {
            return Ok(None);
        };
        let mut matches = |group: &&OperandGroup| {
            Ok(accept(group)
                && ctx.equal(
                    &native_stream_of(ctx, &group.id)?,
                    &stream,
                    "compare F3D operand group stream",
                )?)
        };
        let operation = "find F3D operand group";
        let Some(index) = ctx.position_by(candidates, &mut matches, operation)? else {
            return Ok(None);
        };
        if ctx.any_by(&candidates[index + 1..], &mut matches, operation)? {
            return Ok(None);
        }
        Ok(Some(candidates[index]))
    }
}

/// The `ROLE_0X10` group that lists `operand` at its member ordinal, in the
/// operand's own scope and stream.
fn exact_face_selection_group<'g>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &DesignScope,
    groups: &GroupsByRecord<'g, '_>,
) -> Result<Option<&'g OperandGroup>, cadmpeg_core::CodecError> {
    let Some(stream) = native_stream_of(ctx, &operand.id)? else {
        return Ok(None);
    };
    if !ctx.equal(
        &native_stream_of(ctx, &scope.id)?,
        &Some(stream),
        "compare F3D face selection group scope stream",
    )? {
        return Ok(None);
    }
    let (Some(group_record_index), Some(ordinal)) =
        (operand.group_record_index(), operand.group_member_ordinal())
    else {
        return Ok(None);
    };
    let Ok(ordinal) = usize::try_from(ordinal) else {
        return Ok(None);
    };
    groups.find(ctx, Some(stream), scope, group_record_index, |group| {
        group.role() == DesignOperandRole::ROLE_0X10
            && group.members().get(ordinal).map(|member| member.value)
                == Some(operand.record_index())
    })
}

/// A topology's live persistent tags, keyed for recipe reference binding.
struct RecipeTopologyIndex<'t, 'ctx> {
    /// Tags of live faces and edges by token and design reference, in tag order.
    tags: HashMap<
        (&'t str, i64),
        Vec<&'t crate::history_records::AsmHistoricalPersistentSubentityTag>,
    >,
    /// Live face slots by tagged design reference, in tag order.
    face_tags: HashMap<i64, Vec<i64>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn recipe_topology_index<'t, 'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &'t AsmHistoricalTopology,
) -> Result<RecipeTopologyIndex<'t, 'ctx>, cadmpeg_core::CodecError> {
    let (live_faces, _live_faces_storage) =
        decode.with_scoped_storage("index F3D live recipe faces", || {
            decode.collect_hash_set(
                topology.faces.iter().copied(),
                "index F3D live recipe faces",
            )
        })?;
    let (live_edges, _live_edges_storage) =
        decode.with_scoped_storage("index F3D live recipe edges", || {
            decode.collect_hash_set(
                topology.edges.iter().copied(),
                "index F3D live recipe edges",
            )
        })?;
    let operation = "index F3D recipe reference tags";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let mut tags = HashMap::<_, Vec<&_>>::new();
    let mut face_tags = HashMap::new();
    for tag in decode.admit_iter(
        &topology.persistent_subentity_tags,
        "scan F3D topology persistent subentity tags",
    )? {
        let face = match tag.entity_kind {
            AsmHistoricalEntityKind::Face
                if decode.contains_hash_set(
                    &live_faces,
                    &tag.entity_ref,
                    "find F3D history live faces",
                )? =>
            {
                true
            }
            AsmHistoricalEntityKind::Edge
                if decode.contains_hash_set(
                    &live_edges,
                    &tag.entity_ref,
                    "find F3D history live edges",
                )? =>
            {
                false
            }
            _ => continue,
        };
        for &design_reference in decode.admit_iter(&tag.design_references, operation)? {
            let key = (tag.token.as_str(), design_reference);
            // A tag that repeats a design reference matches it once.
            if decode
                .get_hash_map(&tags, &key, operation)?
                .and_then(|group| group.last())
                .is_some_and(|last| std::ptr::eq(*last, tag))
            {
                continue;
            }
            storage.with_storage(|| {
                decode.push_hash_group(&mut tags, key, tag, operation, operation)?;
                if face {
                    decode.push_hash_group(
                        &mut face_tags,
                        design_reference,
                        tag.entity_ref,
                        operation,
                        operation,
                    )?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
        }
    }
    Ok(RecipeTopologyIndex {
        tags,
        face_tags,
        _storage: storage,
    })
}

/// A value derived from the most recently used topology. Consecutive operands
/// usually share their input state, so one cached value serves a run of them;
/// switching topology releases the old value before building the new one.
struct RecentTopologyValue<T> {
    topology: usize,
    value: Option<T>,
}

impl<T> Default for RecentTopologyValue<T> {
    fn default() -> Self {
        Self {
            topology: 0,
            value: None,
        }
    }
}

impl<T> RecentTopologyValue<T> {
    fn of(
        &mut self,
        topology: &AsmHistoricalTopology,
        build: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
    ) -> Result<&T, cadmpeg_core::CodecError> {
        let address = std::ptr::from_ref(topology).addr();
        let value = match self.value.take() {
            Some(value) if self.topology == address => value,
            _ => {
                let value = build()?;
                self.topology = address;
                value
            }
        };
        Ok(self.value.insert(value))
    }
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
fn bind_historical_recipe_reference_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    index: &RecipeTopologyIndex<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    decode.clear_vec(
        &mut reference.candidate_faces,
        "clear F3D recipe reference candidate faces",
    )?;
    decode.clear_vec(
        &mut reference.candidate_edges,
        "clear F3D recipe reference candidate edges",
    )?;
    decode.clear_vec(
        &mut reference.alternate_selector_faces,
        "clear F3D recipe reference alternate faces",
    )?;
    decode.clear_vec(
        &mut reference.alternate_selector_edges,
        "clear F3D recipe reference alternate edges",
    )?;
    let Some(tags) = decode.get_hash_map(
        &index.tags,
        &(reference.token.as_str(), reference.design_reference),
        "find F3D recipe reference tags",
    )?
    else {
        return Ok(());
    };
    for tag in decode.admit_iter(tags, "scan F3D recipe reference tags")? {
        if tag.entity_kind == AsmHistoricalEntityKind::Face {
            decode.reserve_vec(
                &mut reference.candidate_faces,
                1,
                "collect F3D recipe reference faces",
            )?;
            reference
                .candidate_faces
                .push(historical_face_id(decode, tag.entity_ref)?);
        } else {
            decode.reserve_vec(
                &mut reference.candidate_edges,
                1,
                "collect F3D recipe reference edges",
            )?;
            reference
                .candidate_edges
                .push(historical_edge_id(decode, tag.entity_ref)?);
        }
    }
    decode.stable_sort_by(
        &mut reference.candidate_faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference faces",
    )?;
    decode.dedup_vec(
        &mut reference.candidate_faces,
        "deduplicate F3D recipe reference faces",
    )?;
    decode.stable_sort_by(
        &mut reference.candidate_edges,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D recipe reference edges",
    )?;
    decode.dedup_vec(
        &mut reference.candidate_edges,
        "deduplicate F3D recipe reference edges",
    )?;
    Ok(())
}

fn historical_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    design_reference: i64,
    index: &RecipeTopologyIndex<'_, '_>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let Some(slots) = decode.get_hash_map(
        &index.face_tags,
        &design_reference,
        "find F3D historical recipe faces",
    )?
    else {
        return Ok(Vec::new());
    };
    let mut faces = decode.collection_vec(slots.len(), "collect F3D historical recipe faces")?;
    for &slot in decode.admit_iter(slots, "scan F3D historical recipe faces")? {
        faces.push(historical_face_id(decode, slot)?);
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D historical recipe faces",
    )?;
    decode.dedup_vec(&mut faces, "deduplicate F3D historical recipe faces")?;
    Ok(faces)
}

fn direct_face_recipe_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for reference in decode.admit_iter(references, "scan F3D direct face recipe references")? {
        if reference.design_reference != i64::from(recipe_record_index) {
            continue;
        }
        for face in decode.admit_iter(
            &reference.candidate_faces,
            "scan F3D direct face recipe candidates",
        )? {
            decode.reserve_vec(&mut faces, 1, "collect F3D direct face recipe candidates")?;
            faces.push(face.try_clone_for_decode(decode, "copy F3D historical face identity")?);
        }
    }
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D direct face recipe candidates",
    )?;
    decode.dedup_vec(&mut faces, "deduplicate F3D direct face recipe candidates")?;
    Ok((!faces.is_empty()).then_some(faces))
}

fn historical_face_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical face identity",
    )?;
    ctx.charge_work(
        u64_from_index(text.len()),
        "validate F3D historical face identity",
    )?;
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn historical_edge_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical edge identity",
    )?;
    ctx.charge_work(
        u64_from_index(text.len()),
        "validate F3D historical edge identity",
    )?;
    cadmpeg_ir::ids::EdgeId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

/// Copies the candidate faces that no recipe reference names.
fn unreferenced_candidate_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    references: &[crate::records::dimensions::DesignRecipeReference],
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let operation = "index F3D referenced candidate faces";
    let mut referenced_storage = decode.reserve_scoped(0, operation)?;
    let mut referenced = HashSet::new();
    for reference in decode.admit_iter(references, operation)? {
        for face in decode.admit_iter(&reference.candidate_faces, operation)? {
            referenced_storage
                .with_storage(|| decode.insert_hash_set(&mut referenced, face, operation))?;
        }
    }
    let mut unreferenced = Vec::new();
    for face in decode.admit_iter(candidates, "scan F3D unreferenced candidate faces")? {
        if !decode.contains_hash_set(&referenced, face, operation)? {
            let face = face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
            decode.push_vec(
                &mut unreferenced,
                face,
                "collect F3D unreferenced candidate faces",
            )?;
        }
    }
    Ok(unreferenced)
}

pub(crate) fn bind_face_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let mut recipe_record_indices_storage =
        decode.reserve_scoped(0, "index F3D face operand recipe records")?;
    let mut recipe_record_indices = HashMap::new();
    for recipe in decode.admit_iter(recipes, "scan F3D face operand recipe records")? {
        let Some(record_index) = recipe.record_index else {
            continue;
        };
        recipe_record_indices_storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut recipe_record_indices,
                    recipe.id.as_str(),
                    record_index.value,
                    "index F3D face operand recipe records",
                )
                .map(|_| ())
        })?;
    }
    let scope_records = ScopesByRecord::new(decode, scopes)?;
    let groups = GroupsByRecord::new(decode, operand_groups)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut recipe_index = RecentTopologyValue::default();
    let mut history_source = IntoIterator::into_iter(&mut *operands);
    while let Some(operand) = decode.next_charged(&mut history_source, "scan F3D face operands")? {
        decode.clear_vec(
            &mut operand.preceding_candidate_faces,
            "clear F3D preceding face candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_candidate_faces,
            "clear F3D changed face candidates",
        )?;
        decode.clear_vec(
            &mut operand.historical_support_contexts,
            "clear F3D historical face support contexts",
        )?;
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
        let stream = native_stream_of(decode, &operand.id)?;
        let Some(scope) = scope_records.find(decode, &operand.id, operand.scope_record_index)?
        else {
            continue;
        };
        let scoped_history = if decode.contains_key_hash_map(
            scope_histories,
            &scope.id,
            "check F3D profile scope history binding",
        )? {
            let Some(history) = bound_scope_history(decode, &scope.id, scope_histories, histories)?
            else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, scoped_histories)?
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                decode,
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )?
        } else {
            unique_history_state_pair(decode, histories, state_id, previous_state_id)?
        }) else {
            continue;
        };
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let recipe_tags = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe_references,
            "scan F3D face operand recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, recipe_tags)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D face operand recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), recipe_tags)?;
            operand.unreferenced_candidate_faces = unreferenced_candidate_faces(
                decode,
                &operand.candidate_faces,
                &operand.recipe_references,
            )?;
            decode.clear_vec(
                &mut operand.alternate_selector_candidate_faces,
                "clear F3D alternate selector face candidates",
            )?;
        }
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(changed_faces) = changed_faces else {
            continue;
        };
        let (direct_face_candidates, _direct_storage) =
            decode.with_scoped_storage("collect F3D direct face candidate lane", || {
                Ok::<_, cadmpeg_core::CodecError>(
                    if let Some(record_index) = decode.get_hash_map(
                        &recipe_record_indices,
                        operand.recipe_id.as_str(),
                        "find F3D direct face recipe record",
                    )? {
                        direct_face_recipe_candidates(
                            decode,
                            operand.recipe_kind,
                            &operand.recipe_references,
                            *record_index,
                        )?
                    } else {
                        None
                    },
                )
            })?;
        let feature_family = crate::design::design_feature_family(&scope.kind());
        let (thread_face_candidates, _thread_face_candidates_storage) = decode
            .with_scoped_storage("collect F3D thread face candidates", || {
                Ok::<_, cadmpeg_core::CodecError>(
                    if feature_family == Some(crate::design::DesignFeatureFamily::Thread)
                        && exact_face_selection_group(decode, operand, scope, &groups)?.is_some()
                    {
                        operand
                            .recipe_references
                            .first()
                            .and_then(|reference| {
                                let candidates = effective_faces(reference);
                                (!candidates.is_empty()).then_some(candidates)
                            })
                            .map(|candidates| {
                                decode.try_collect_vec(
                                    (candidates).iter().map(|face| {
                                        face.try_clone_for_decode(
                                            decode,
                                            "copy F3D historical face identity",
                                        )
                                    }),
                                    "copy F3D thread face candidates",
                                )
                            })
                            .transpose()?
                    } else {
                        None
                    },
                )
            })?;
        let (nested_split_face_candidates, _nested_split_face_candidates_storage) = decode
            .with_scoped_storage("collect F3D nested split face candidates", || {
                Ok::<_, cadmpeg_core::CodecError>(
                    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
                        && exact_face_selection_group(decode, operand, scope, &groups)?.is_some()
                    {
                        crate::design::face_resolve::nested_bounded_face_history_candidates(
                            decode, operand,
                        )?
                    } else {
                        None
                    },
                )
            })?;
        let (grouped_reference_face_candidates, _grouped_reference_face_candidates_storage) =
            decode.with_scoped_storage("collect F3D grouped reference face candidates", || {
                (!matches!(
                    feature_family,
                    Some(
                        crate::design::DesignFeatureFamily::Thread
                            | crate::design::DesignFeatureFamily::Split
                    )
                ))
                .then(|| {
                    grouped_reference_face_candidate(decode, operand, topology, &changed_faces)
                })
                .transpose()?
                .flatten()
                .map(|face| {
                    let mut faces =
                        decode.collection_vec(1, "collect F3D grouped reference face candidate")?;
                    faces.push(face);
                    Ok::<_, cadmpeg_core::CodecError>(faces)
                })
                .transpose()
            })?;
        let (legacy_face_candidates, _legacy_face_candidates_storage) = decode.with_scoped_storage("collect F3D legacy face candidates", || -> Result<Option<_>, cadmpeg_core::CodecError> {
            if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
                return Ok(None);
            }
            let Some(group_record_index) = operand.group_record_index() else {
                return Ok(None);
            };
            if groups
                .find(decode, stream, scope, group_record_index, |group| {
                    group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    }) && group.extrude_face_role().is_some()
                })?
                .is_none()
            {
                return Ok(None);
            }
            let Some(recipe_record_index) = decode.get_hash_map(
                &recipe_record_indices,
                operand.recipe_id.as_str(),
                "find F3D edge operand recipe record",
            )?
            else {
                return Ok(None);
            };
            crate::design::face_resolve::legacy_face_recipe_reference_candidates(
                decode,
                operand,
                *recipe_record_index,
            )
        })?;
        let fallback_candidates;
        let mut fallback_storage =
            decode.reserve_scoped(0, "collect F3D fallback face candidate lane")?;
        let history_candidates = if let Some(candidates) = direct_face_candidates
            .as_deref()
            .or(thread_face_candidates.as_deref())
            .or(nested_split_face_candidates.as_deref())
            .or(grouped_reference_face_candidates.as_deref())
        {
            candidates
        } else {
            fallback_candidates = fallback_storage.with_storage(|| {
                crate::design::face_resolve::historical_face_operand_candidates(decode, operand)
            })?;
            &fallback_candidates
        };
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, history_candidates, topology)?;
        let mut changed_candidate_faces = Vec::new();
        for face in decode.admit_iter(
            operand.preceding_candidate_faces.as_slice(),
            "scan F3D changed candidate faces",
        )? {
            if stable_ref(decode, face.as_str())?
                .map(|slot| {
                    decode.contains_hash_set(
                        &changed_faces,
                        &slot,
                        "find F3D history changed faces",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                let face =
                    face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                decode.push_vec(
                    &mut changed_candidate_faces,
                    face,
                    "collect F3D changed candidate faces",
                )?;
            }
        }
        operand.changed_candidate_faces = changed_candidate_faces;
        operand.historical_support_contexts = topology::historical_face_support_contexts(
            decode,
            history_candidates,
            history,
            topology,
            &changed_faces,
        )?;
        if direct_face_candidates.is_some() {
            let mut resolved_face_slots = Vec::new();
            for face in decode.admit_iter(
                operand.preceding_candidate_faces.as_slice(),
                "scan F3D direct resolved face slots",
            )? {
                if let Some(slot) = stable_ref(decode, face.as_str())? {
                    decode.push_vec(
                        &mut resolved_face_slots,
                        slot,
                        "collect F3D direct resolved face slots",
                    )?;
                }
            }
            operand.resolved_face_slots = resolved_face_slots;
            continue;
        }
        let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || match operand.group_record_index() {
                Some(group_record_index) => groups
                    .find(decode, stream, scope, group_record_index, |_| true)?
                    .is_some_and(|group| {
                        group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                    }),
                None => false,
            };
        operand.resolved_face_slots = match &scope.payload() {
            crate::records::feature::scope::DesignScopePayload::OffsetFaces(Some(_))
            | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(Some(_)) => {
                let direct = resolve_direct_face_recipe_clauses(
                    decode,
                    &operand.recipe_references,
                    topology,
                    &changed_faces,
                )?;
                if direct.is_empty() {
                    {
                        let selected =
                            crate::design::face_resolve::resolve_face_operand_history_candidates(
                                operand,
                            );
                        let mut slots = decode.collection_vec(
                            usize::from(selected.is_some()),
                            "collect F3D resolved face slots",
                        )?;
                        if let Some(face) = selected {
                            slots.push(face);
                        }
                        slots
                    }
                } else {
                    direct
                }
            }
            _ => {
                let direct =
                    crate::design::face_resolve::resolve_face_operand_history_candidates(operand);
                if let Some(direct) = direct {
                    let mut slots = decode.collection_vec(1, "collect F3D resolved face slots")?;
                    slots.push(direct);
                    slots
                } else if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                {
                    crate::design::face_resolve::resolve_surface_delete_face_history_set(
                        decode, operand,
                    )?
                    .unwrap_or_default()
                } else if preserves_stable_face_set {
                    if let Some(stable) =
                        crate::design::face_resolve::resolve_stable_bounded_face_history_set(
                            decode, operand,
                        )?
                    {
                        stable
                    } else {
                        crate::design::face_resolve::resolve_bounded_face_history_candidates(
                            decode, operand,
                        )?
                        .unwrap_or_default()
                    }
                } else if let Some(bounded) =
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(
                        decode, operand,
                    )?
                {
                    bounded
                } else {
                    let pattern = if feature_family
                        == Some(crate::design::DesignFeatureFamily::CircularPattern)
                    {
                        if let Some(result) = state.topology() {
                            resolve_pattern_face_by_surface_radius(
                                decode,
                                crate::design::face_resolve::face_operand_candidates(operand),
                                topology,
                                result,
                                &changed_faces,
                            )?
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    {
                        let selected = pattern;
                        let mut slots = decode.collection_vec(
                            usize::from(selected.is_some()),
                            "collect F3D resolved face slots",
                        )?;
                        if let Some(face) = selected {
                            slots.push(face);
                        }
                        slots
                    }
                }
            }
        };
        if let Some(candidates) = &thread_face_candidates {
            // Thread's first prefix reference is its exclusive selection lane.
            // An unresolved lane must not fall back to construction context.
            let resolved =
                match crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                ) {
                    Some(face) => Some(face),
                    None => resolve_thread_face_by_transition(
                        decode,
                        scope,
                        candidates,
                        history,
                        topology,
                        &changed_faces,
                    )?,
                };
            operand.resolved_face_slots = {
                let selected = resolved;
                let mut slots = decode.collection_vec(
                    usize::from(selected.is_some()),
                    "collect F3D resolved face slots",
                )?;
                if let Some(face) = selected {
                    slots.push(face);
                }
                slots
            };
        }
        if let Some(candidates) = &grouped_reference_face_candidates {
            // A grouped frame's unique changed topology-face reference is its
            // exact historical selection lane.
            operand.resolved_face_slots = {
                let selected =
                    crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                        operand, candidates,
                    );
                let mut slots = decode.collection_vec(
                    usize::from(selected.is_some()),
                    "collect F3D resolved face slots",
                )?;
                if let Some(face) = selected {
                    slots.push(face);
                }
                slots
            };
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Split) {
            operand.resolved_face_slots = {
                let selected = resolve_split_tool_face(decode, operand, topology)?;
                let mut slots = decode.collection_vec(
                    usize::from(selected.is_some()),
                    "collect F3D resolved face slots",
                )?;
                if let Some(face) = selected {
                    slots.push(face);
                }
                slots
            };
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Loft)
            && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            && state
                .transition
                .as_ref()
                .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
        {
            if let Some((result, transition)) = state.topology().zip(state.transition.as_ref()) {
                if let Some(face) = resolve_bounded_face_recipe_target(
                    decode,
                    operand,
                    topology,
                    result,
                    &transition.topology.bodies.inserted,
                )? {
                    operand.resolved_face_slots =
                        decode.collection_vec(1, "collect F3D resolved face slots")?;
                    operand.resolved_face_slots.push(face);
                }
            }
        }
        if operand.resolved_face_slots.is_empty()
            && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Draft
        {
            if let Some(result) = state.topology() {
                if let Some(face) =
                    resolve_draft_face_by_surface_transition(decode, operand, topology, result)?
                {
                    operand.resolved_face_slots =
                        decode.collection_vec(1, "collect F3D resolved face slots")?;
                    operand.resolved_face_slots.push(face);
                }
            }
        }
        if operand.resolved_face_slots.is_empty() {
            if let Some(candidates) = legacy_face_candidates {
                let history_source = historical_brep_source(decode, &history.id)?;
                match select_legacy_extrude_face_candidate(
                    decode,
                    candidates,
                    topology,
                    &changed_faces,
                    history_source,
                )? {
                    Some(LegacyFaceResolution::Historical(slot)) => {
                        operand.resolved_face_slots =
                            decode.collection_vec(1, "collect F3D resolved face slots")?;
                        operand.resolved_face_slots.push(slot);
                    }
                    Some(LegacyFaceResolution::Active(face)) => {
                        operand.resolved_active_face = Some(
                            face.try_clone_for_decode(decode, "copy F3D resolved active face")?,
                        );
                    }
                    None => {}
                }
            }
        }
    }
    bind_profile_face_group_cardinality(
        decode,
        operands,
        scopes,
        operand_groups,
        histories,
        scope_histories,
    )?;
    Ok(())
}

/// Resolve a Draft face whose persistent selector lane is ambiguous in the
/// active B-rep by following the surface carrier through the exact Draft
/// transition.
///
/// Draft changes the selected face's actual surface geometry. The same
/// transition may update tangent or context faces, so the transition is
/// admissible only when exactly one face from the operand's alternate lane (or,
/// when no alternate lane exists, its exact candidate lane) changes surface
/// geometry. A missing, duplicate, or unchanged surface is not a selection
/// proof. Carrier record identities are not compared: a history update may
/// assign a new carrier record without changing the surface.
#[derive(Clone, Copy, PartialEq)]
enum DraftSurfaceGeometry {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
    },
    Cylinder {
        origin: cadmpeg_ir::math::Point3,
        axis: cadmpeg_ir::math::Vector3,
        radius: f64,
    },
    Axis {
        origin: cadmpeg_ir::math::Point3,
        direction: cadmpeg_ir::math::Vector3,
        radius: Option<f64>,
    },
}

fn draft_surface_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Result<Option<DraftSurfaceGeometry>, cadmpeg_core::CodecError> {
    let operation = "find F3D draft surface geometry";
    let Some(binding) = unique_by(
        ctx,
        &topology.face_surfaces,
        |binding| Ok(binding.entity == face),
        operation,
    )?
    else {
        return Ok(None);
    };
    let carrier = binding.carrier;
    if let Some(plane) = ctx.find_by(
        &topology.surface_planes,
        |surface| Ok(surface.surface == carrier),
        operation,
    )? {
        return Ok(Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        }));
    }
    if let Some(cylinder) = ctx.find_by(
        &topology.surface_cylinders,
        |surface| Ok(surface.surface == carrier),
        operation,
    )? {
        return Ok(Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        }));
    }
    let Some(axis) = ctx.find_by(
        &topology.surface_axes,
        |surface| Ok(surface.surface == carrier),
        operation,
    )?
    else {
        return Ok(None);
    };
    let radius = ctx
        .find_by(
            &topology.surface_radii,
            |radius| Ok(radius.surface == carrier),
            operation,
        )?
        .map(|radius| radius.radius);
    Ok(Some(DraftSurfaceGeometry::Axis {
        origin: axis.origin,
        direction: axis.direction,
        radius,
    }))
}

fn resolve_draft_face_by_surface_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return Ok(None);
    }
    let mut storage = decode.reserve_scoped(0, "index F3D draft face candidates")?;
    let mut candidate_slots = HashSet::new();
    for face in decode.admit_iter(&operand.candidate_faces, "scan F3D draft face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            storage.with_storage(|| {
                decode.insert_hash_set(
                    &mut candidate_slots,
                    face_slot,
                    "index F3D draft face candidates",
                )
            })?;
        }
    }
    if candidate_slots.is_empty() {
        return Ok(None);
    }
    // Candidate faces named by each reference's alternate lane, else by its
    // exact lane.
    let mut lane_slots = |alternate: bool,
                          operation: &'static str|
     -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
        let mut slots = BTreeSet::new();
        for reference in decode.admit_iter(&operand.recipe_references, operation)? {
            let faces = if alternate {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            for face in decode.admit_iter(faces, operation)? {
                let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                    continue;
                };
                if decode.contains_hash_set(
                    &candidate_slots,
                    &face_slot,
                    "find F3D history candidate slots",
                )? {
                    decode.insert_scoped_btree_set(
                        &mut storage,
                        &mut slots,
                        face_slot,
                        operation,
                        operation,
                    )?;
                }
            }
        }
        Ok(slots)
    };
    let alternate_slots = lane_slots(true, "collect F3D draft alternate faces")?;
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        lane_slots(false, "collect F3D draft exact faces")?
    };
    let operation = "find F3D draft face in both states";
    if !has_alternates {
        // A one-entry set is a single leaf.
        let (1, Some(&face)) = (candidates.len(), candidates.first()) else {
            return Ok(None);
        };
        return Ok((decode.contains(&preceding.faces, &face, operation)?
            && decode.contains(&result.faces, &face, operation)?)
        .then_some(face));
    }
    let mut changed = None;
    let mut history_source = IntoIterator::into_iter(&candidates);
    while let Some(&face) =
        decode.next_charged(&mut history_source, "scan F3D draft surface candidates")?
    {
        if !decode.contains(&preceding.faces, &face, operation)?
            || !decode.contains(&result.faces, &face, operation)?
        {
            continue;
        }
        let (Some(before), Some(after)) = (
            draft_surface_geometry(decode, preceding, face)?,
            draft_surface_geometry(decode, result, face)?,
        ) else {
            continue;
        };
        if before != after && changed.replace(face).is_some() {
            return Ok(None);
        }
    }
    Ok(changed)
}

/// One surface radius per surface; a surface with several radii has none.
fn unique_surface_radii<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<
    (
        HashMap<i64, Option<f64>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    decode.unique_index(
        topology
            .surface_radii
            .iter()
            .map(|radius| (radius.surface, radius.radius)),
        "index F3D surface radii",
    )
}

fn resolve_pattern_face_by_surface_radius(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut storage = decode.reserve_scoped(0, "index F3D pattern face candidates")?;
    let mut candidate_faces = HashSet::new();
    for face in decode.admit_iter(candidates, "scan F3D pattern face candidates")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            storage.with_storage(|| {
                decode.insert_hash_set(
                    &mut candidate_faces,
                    face_slot,
                    "index F3D pattern face candidates",
                )
            })?;
        }
    }
    if candidate_faces.is_empty() {
        return Ok(None);
    }
    let (result_radii, _result_radii_storage) = unique_surface_radii(decode, result)?;
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    let mut history_source = IntoIterator::into_iter(&result.face_surfaces);
    while let Some(binding) =
        decode.next_charged(&mut history_source, "scan F3D pattern result face surfaces")?
    {
        if !decode.contains_hash_set(
            &candidate_faces,
            &binding.entity,
            "find F3D history candidate faces",
        )? {
            continue;
        }
        if !storage.with_storage(|| {
            decode.insert_hash_set(
                &mut bound_candidates,
                binding.entity,
                "index F3D pattern bound faces",
            )
        })? {
            return Ok(None);
        }
        let Some(&Some(radius)) = decode.get_hash_map(
            &result_radii,
            &binding.carrier,
            "find F3D history result radii",
        )?
        else {
            return Ok(None);
        };
        if !radius.is_finite() || radius <= 0.0 {
            return Ok(None);
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return Ok(None),
        }
    }
    // Every bound face passed the candidate filter, so equal sizes mean that
    // each candidate face is bound.
    if bound_candidates.len() != candidate_faces.len() {
        return Ok(None);
    }
    let Some(result_radius) = result_radius else {
        return Ok(None);
    };
    let (preceding_radii, _preceding_radii_storage) = unique_surface_radii(decode, preceding)?;
    let mut matched = None;
    let mut history_source = IntoIterator::into_iter(&preceding.face_surfaces);
    while let Some(binding) = decode.next_charged(
        &mut history_source,
        "scan F3D pattern preceding face surfaces",
    )? {
        if !decode.contains_hash_set(
            changed_faces,
            &binding.entity,
            "find F3D history changed faces",
        )? {
            continue;
        }
        let Some(&Some(radius)) = decode.get_hash_map(
            &preceding_radii,
            &binding.carrier,
            "find F3D history preceding radii",
        )?
        else {
            continue;
        };
        if radius.to_bits() == result_radius && matched.replace(binding.entity).is_some() {
            return Ok(None);
        }
    }
    Ok(matched)
}

fn resolve_split_tool_face(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return Ok(None);
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return Ok(None);
    };
    let (candidates, _candidates_storage) = decode
        .with_scoped_storage("collect F3D split tool face candidates", || {
            selection::faces_in_topology(decode, &reference.candidate_faces, topology)
        })?;
    let [face] = candidates.as_slice() else {
        return Ok(None);
    };
    stable_ref(decode, face.as_str())
}

fn effective_faces(
    reference: &crate::records::dimensions::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

fn resolve_thread_face_by_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(construction) = scope.thread_construction() else {
        return Ok(None);
    };
    let Some(source) = historical_brep_source(decode, &history.id)? else {
        return Ok(None);
    };
    let mut is_source_face =
        |face: &cadmpeg_ir::ids::FaceId| active_brep_face_matches_source(decode, face, source);
    let Some(source_face_index) = decode.position_by(
        candidates,
        &mut is_source_face,
        "find F3D Thread source face candidate",
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &candidates[source_face_index + 1..],
            &mut is_source_face,
            "find additional F3D Thread source face candidate",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return Ok(None);
    }
    let tolerance = EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9 * (1.0 + maximum_radius.abs());
    let (face_carriers, _face_carriers_storage) = decode.unique_index(
        topology
            .face_surfaces
            .iter()
            .map(|binding| (binding.entity, binding.carrier)),
        "index F3D Thread face carriers",
    )?;
    let (cylinders, _cylinders_storage) = decode.unique_index(
        topology
            .surface_cylinders
            .iter()
            .map(|cylinder| (cylinder.surface, cylinder.radius)),
        "index F3D Thread cylinders",
    )?;
    // Every changed face on one cylinder within the thread diameters must be
    // the same face.
    let mut matched = None;
    let mut history_source = IntoIterator::into_iter(&topology.faces);
    while let Some(&face) =
        decode.next_charged(&mut history_source, "scan F3D Thread transition faces")?
    {
        if !decode.contains_hash_set(changed_faces, &face, "find F3D history changed faces")? {
            continue;
        }
        let Some(&Some(carrier)) =
            decode.get_hash_map(&face_carriers, &face, "find F3D history face carriers")?
        else {
            continue;
        };
        let Some(&Some(radius)) =
            decode.get_hash_map(&cylinders, &carrier, "find F3D history cylinders")?
        else {
            continue;
        };
        if radius + tolerance < minimum_radius || radius > maximum_radius + tolerance {
            continue;
        }
        if matched.is_some_and(|matched| matched != face) {
            return Ok(None);
        }
        matched = Some(face);
    }
    Ok(matched)
}

fn grouped_reference_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            decode,
            &operand.recipe_prefix_bytes,
        )?
    {
        return Ok(None);
    }
    let mut face = None;
    let mut history_source = IntoIterator::into_iter(&operand.recipe_references);
    while let Some(reference) = decode.next_charged(
        &mut history_source,
        "scan F3D grouped recipe face references",
    )? {
        let candidate = reference.design_reference;
        if !decode.contains_hash_set(changed_faces, &candidate, "find F3D history changed faces")?
            || !decode.contains(
                &topology.faces,
                &candidate,
                "check F3D grouped topology face membership",
            )?
        {
            continue;
        }
        if face.is_some_and(|expected| expected != candidate) {
            return Ok(None);
        }
        face = Some(candidate);
    }
    face.map(|face| historical_face_id(decode, face))
        .transpose()
}

fn resolve_bounded_face_recipe_target<'r>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &'r crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || decode.any_by(
            &operand.recipe_nodes,
            |node| Ok(node.recipe_structure.is_none()),
            "check F3D bounded recipe nodes",
        )?
    {
        return Ok(None);
    }
    let Some(first) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let first_clause_len = decode
        .position_by(
            &operand.recipe_references,
            |reference| {
                Ok(reference.selector_offset != first.selector_offset
                    || reference.token_offset != first.token_offset)
            },
            "measure F3D bounded first clause",
        )?
        .unwrap_or(operand.recipe_references.len());
    let first_clause = &operand.recipe_references[..first_clause_len];
    let mut storage = decode.reserve_scoped(0, "index F3D bounded topology faces")?;
    let topology_faces = storage.with_storage(|| {
        decode.collect_hash_set(
            preceding.faces.iter().copied(),
            "index F3D bounded topology faces",
        )
    })?;
    let mut target_candidates = BTreeSet::new();
    if let Some(reference) = first_clause.first() {
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded target faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if decode.contains_hash_set(
                &topology_faces,
                &face_slot,
                "find F3D history topology faces",
            )? {
                decode.insert_scoped_btree_set(
                    &mut storage,
                    &mut target_candidates,
                    face_slot,
                    "find F3D bounded target face",
                    "collect F3D bounded target faces",
                )?;
            }
        }
    }
    for reference in decode.admit_iter(&first_clause[1..], "scan F3D bounded face clauses")? {
        let mut clause_storage = decode.reserve_scoped(0, "collect F3D bounded clause faces")?;
        let mut candidates = HashSet::new();
        for face in
            decode.admit_iter(effective_faces(reference), "scan F3D bounded clause faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            if decode.contains_hash_set(
                &topology_faces,
                &face_slot,
                "find F3D history topology faces",
            )? {
                clause_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut candidates,
                        face_slot,
                        "collect F3D bounded clause faces",
                    )
                })?;
            }
        }
        decode.retain_btree_set(
            &mut target_candidates,
            |face| decode.contains_hash_set(&candidates, face, "find F3D history candidates"),
            "intersect F3D bounded clause faces",
        )?;
    }
    let operation = "index F3D bounded result relations";
    let relations = |relations: &'r [crate::history_records::AsmHistoricalRelation]| {
        decode.unique_index(
            relations
                .iter()
                .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
            operation,
        )
    };
    let (body_regions, _body_regions_storage) = relations(&result.body_regions)?;
    let (region_shells, _region_shells_storage) = relations(&result.region_shells)?;
    let (shell_faces, _shell_faces_storage) = relations(&result.shell_faces)?;
    let mut construction_faces = Vec::new();
    let mut history_source = IntoIterator::into_iter(inserted_bodies);
    'bodies: while let Some(body) =
        decode.next_charged(&mut history_source, "scan F3D inserted bodies")?
    {
        let Some(&Some(regions)) =
            decode.get_hash_map(&body_regions, body, "find F3D history body regions")?
        else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        let mut history_source = IntoIterator::into_iter(regions);
        while let Some(region) =
            decode.next_charged(&mut history_source, "scan F3D inserted body regions")?
        {
            let Some(&Some(shells)) =
                decode.get_hash_map(&region_shells, region, "find F3D history region shells")?
            else {
                continue 'bodies;
            };
            let mut history_source = IntoIterator::into_iter(shells);
            while let Some(shell) =
                decode.next_charged(&mut history_source, "scan F3D inserted body shells")?
            {
                let Some(&Some(faces)) =
                    decode.get_hash_map(&shell_faces, shell, "find F3D history shell faces")?
                else {
                    continue 'bodies;
                };
                for face in decode.admit_iter(faces, "scan F3D inserted body faces")? {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                decode.push_scoped_vec(
                    &mut storage,
                    &mut construction_faces,
                    face,
                    "collect F3D construction faces",
                )?;
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let face_loop_positions = |face,
                               topology,
                               storage: &mut cadmpeg_core::decode::ScopedReservation<'_>|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let (contexts, _contexts_storage) = decode
            .with_scoped_storage("collect F3D bounded face loop contexts", || {
                topology::face_boundary_contexts_for_slots(decode, &[face], topology)
            })?;
        let [context] = contexts.as_slice() else {
            return Ok(None);
        };
        let [loop_] = context.loops.as_slice() else {
            return Ok(None);
        };
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
            rows,
        ) = &loop_.boundary
        else {
            return Ok(None);
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let positions = storage.with_storage(|| {
            decode.collect_vec(
                rows.iter().map(|row| row.position),
                "collect F3D bounded loop positions",
            )
        })?;
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    let mut history_source = IntoIterator::into_iter(target_candidates);
    while let Some(candidate) =
        decode.next_charged(&mut history_source, "scan F3D bounded target candidates")?
    {
        let mut positions_storage =
            decode.reserve_scoped(0, "collect F3D bounded loop positions")?;
        let Some((edge_count, candidate_points)) =
            face_loop_positions(candidate, preceding, &mut positions_storage)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let matched = decode.any_by(
            &construction_faces,
            |face| {
                let mut construction_storage =
                    decode.reserve_scoped(0, "collect F3D bounded loop positions")?;
                let Some((construction_edge_count, construction_points)) =
                    face_loop_positions(*face, result, &mut construction_storage)?
                else {
                    return Ok(false);
                };
                Ok(construction_edge_count >= edge_count
                    && cyclic_point_subsequence(decode, &candidate_points, &construction_points)?)
            },
            "scan F3D construction faces",
        )?;
        if matched {
            decode.push_scoped_vec(
                &mut storage,
                &mut matches,
                candidate,
                "collect F3D bounded face matches",
            )?;
        }
    }
    // Candidates come from an ordered set, so the matches are distinct and in order.
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

/// Whether `candidate` occurs, in order or reversed, as a cyclic subsequence
/// of `construction` under the coincidence tolerance. Each start position and
/// each cursor step is charged as it is examined.
fn cyclic_point_subsequence(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> Result<bool, cadmpeg_core::CodecError> {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let (Some(first), Some(last)) = (candidate.first(), candidate.last()) else {
        return Ok(false);
    };
    if candidate.len() > construction.len() {
        return Ok(false);
    }
    let operation = "match F3D bounded loop positions";
    let length = construction.len();
    for reversed in [false, true] {
        let start_point = if reversed { last } else { first };
        let mut starts = 0..length;
        while let Some(start) = ctx.next_charged(&mut starts, operation)? {
            if !coincident(start_point, &construction[start]) {
                continue;
            }
            // Each remaining candidate point must appear after the previous
            // one within one turn of the loop.
            let mut cursor = start;
            let limit = start + length;
            let mut targets = 1..candidate.len();
            let mut matched = true;
            while let Some(offset) = ctx.next_charged(&mut targets, operation)? {
                let target = if reversed {
                    &candidate[candidate.len() - 1 - offset]
                } else {
                    &candidate[offset]
                };
                let mut found = false;
                while cursor < limit {
                    ctx.charge_work(1, operation)?;
                    cursor += 1;
                    if coincident(target, &construction[cursor % length]) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(crate) fn bind_body_recipe_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let scopes = ScopesByRecord::new(decode, scopes)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut topology_faces = RecentTopologyValue::default();
    let mut closures = RecentTopologyValue::default();
    for operand in decode.admit_iter(&mut *operands, "scan F3D body recipe operands")? {
        {
            let mut bindings = operand.reference_bindings_mut();
            while let Some(reference) =
                decode.next_charged(&mut bindings, "clear F3D body recipe reference bindings")?
            {
                decode.clear_vec(
                    reference.preceding_candidate_faces,
                    "clear F3D preceding body recipe face candidates",
                )?;
                reference.preceding_body_slots.clear();
            }
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        let Some((history, state, previous)) =
            body_recipe_operand_history_pair(decode, operand, &scopes, histories)?
        else {
            continue;
        };
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous.state_id, states)
            })?;
        if changed_faces.is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(decode, &previous.id)? else {
            continue;
        };
        let (faces, _) = topology_faces.of(topology, || {
            decode.with_scoped_storage("index F3D topology faces", || {
                decode.collect_hash_set(topology.faces.iter().copied(), "index F3D topology faces")
            })
        })?;
        let closures = closures.of(topology, || topology::body_closures(decode, topology))?;
        {
            let mut bindings = operand.reference_bindings_mut();
            while let Some(reference) =
                decode.next_charged(&mut bindings, "scan F3D body recipe reference bindings")?
            {
                let mut face_slots_storage =
                    decode.reserve_scoped(0, "index F3D body recipe face slots")?;
                let mut face_slots = BTreeSet::new();
                let mut preceding_candidate_faces = Vec::new();
                for face in decode.admit_iter(
                    reference.candidate_faces.as_slice(),
                    "scan F3D body recipe candidate faces",
                )? {
                    if !active_brep_face_matches_source(decode, face, source)? {
                        continue;
                    }
                    let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                        continue;
                    };
                    if decode.contains_hash_set(faces, &face_slot, "find F3D history faces")? {
                        let face =
                            face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                        decode.push_vec(
                            &mut preceding_candidate_faces,
                            face,
                            "collect F3D faces in topology",
                        )?;
                        decode.insert_scoped_btree_set(
                            &mut face_slots_storage,
                            &mut face_slots,
                            face_slot,
                            "index F3D body recipe face slots",
                            "index F3D body recipe face slots",
                        )?;
                    }
                }
                *reference.preceding_candidate_faces = preceding_candidate_faces;
                let Some(body_slots) = face_slots_storage.with_storage(|| {
                    topology::closures_intersecting(decode, closures, &face_slots)
                })?
                else {
                    continue;
                };
                let mut slots = decode
                    .collection_vec(body_slots.len(), "collect F3D body recipe preceding bodies")?;
                for body in
                    decode.admit_iter(body_slots, "scan F3D body recipe preceding bodies")?
                {
                    slots.push(body);
                }
                *reference.preceding_body_slots = slots;
            }
        }
        if let [reference] = operand.references().as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(decode, face.as_str())?;
            }
        }
        let Some((first, rest)) = operand.references().split_first() else {
            continue;
        };
        if first.preceding_body_slots.is_empty() {
            continue;
        }
        if decode.any_by(
            operand.references(),
            |reference| Ok(reference.preceding_body_slots.is_empty()),
            "scan F3D body recipe references with empty body slots",
        )? {
            continue;
        }
        let mut intersection_storage =
            decode.reserve_scoped(0, "index F3D body recipe intersection")?;
        let mut intersection = BTreeSet::new();
        for &body in decode.admit_iter(
            &first.preceding_body_slots,
            "scan F3D first preceding body slots",
        )? {
            decode.insert_scoped_btree_set(
                &mut intersection_storage,
                &mut intersection,
                body,
                "index F3D body recipe intersection",
                "index F3D body recipe intersection",
            )?;
        }
        for reference in decode.admit_iter(rest, "scan F3D body recipe intersection references")? {
            decode.retain_btree_set(
                &mut intersection,
                |body| {
                    decode.contains(
                        &reference.preceding_body_slots,
                        body,
                        "check F3D body recipe intersection membership",
                    )
                },
                "intersect F3D body recipe references",
            )?;
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.pop_first();
        }
    }
    let recipes_by_id = body_recipe_identity_index(recipes);
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = native_stream_of(decode, &operand.id)? else { return Ok(None) };
        let Some(recipe) = recipes_by_id.get(decode, operand.recipe_id.as_str())? else {
            return Ok(None);
        };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            decode.copy_retained_text(stream, "copy F3D body recipe identity stream")?,
            operand
                .asset_id
                .try_clone_for_decode(decode, "copy F3D body recipe asset ID")?,
            operand
                .context_id
                .try_clone_for_decode(decode, "copy F3D body recipe context ID")?,
            decode.collect_vec(operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)), "collect F3D body recipe identity references")?,
            decode.copy_retained_text(&design.id.value, "copy F3D body recipe design id")?,
            selector.value,
        )))
    };
    let mut resolved_by_identity_storage =
        decode.reserve_scoped(0, "index F3D resolved body recipe identities")?;
    let mut resolved_by_identity = HashMap::new();
    let mut history_source = IntoIterator::into_iter(&*operands);
    while let Some(operand) = decode.next_charged(&mut history_source, "scan F3D operands")? {
        let Some(body) = operand.resolved_body_slot else {
            continue;
        };
        let (identity, key_storage) = decode
            .with_scoped_storage("retain F3D body recipe identity key", || identity(operand))?;
        let Some(identity) = identity else {
            continue;
        };
        resolved_by_identity_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            match decode.entry_hash_map(
                &mut resolved_by_identity,
                identity,
                "index F3D resolved body recipe identities",
            )? {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if *entry.get() != Some(body) {
                        *entry.get_mut() = None;
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    key_storage.commit()?;
                    entry.insert(Some(body));
                }
            }
            Ok(())
        })?;
    }

    let mut body_faces = RecentTopologyValue::default();
    let mut history_source = IntoIterator::into_iter(operands);
    while let Some(operand) =
        decode.next_charged(&mut history_source, "bind F3D body recipe operand faces")?
    {
        if operand.resolved_body_slot.is_none() {
            let resolved_body_slot = {
                let mut lookup_identity_storage =
                    decode.reserve_scoped(0, "lookup F3D resolved body recipe identity")?;
                lookup_identity_storage.with_storage(
                    || -> Result<Option<i64>, cadmpeg_core::CodecError> {
                        let Some(identity) = identity(operand)? else {
                            return Ok(None);
                        };
                        Ok(decode
                            .get_hash_map(
                                &resolved_by_identity,
                                &identity,
                                "find F3D resolved body recipe identity",
                            )?
                            .copied()
                            .flatten())
                    },
                )?
            };
            operand.resolved_body_slot = resolved_body_slot;
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        let Some((_, _, previous)) =
            body_recipe_operand_history_pair(decode, operand, &scopes, histories)?
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let index = body_faces.of(topology, || topology::body_face_index(decode, topology))?;
        let Some(faces) = topology::complete_body_face_slots(decode, index, body_slot)? else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
    Ok(())
}

/// Construction recipes by native ID; a repeated ID names no recipe.
fn body_recipe_identity_index<'ctx>(
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> UniqueIndex<'_, 'ctx, &str, crate::records::recipes::ConstructionRecipe> {
    UniqueIndex::new(
        recipes,
        |_, recipe| Ok(Some(recipe.id.as_str())),
        "index F3D body recipes by id",
    )
}

fn body_recipe_operand_history_pair<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &ScopesByRecord<'_, '_>,
    histories: &'a [AsmHistory],
) -> Result<Option<HistoryStatePair<'a>>, cadmpeg_core::CodecError> {
    if native_stream_of(decode, &operand.id)?.is_none() {
        return Ok(None);
    }
    let Some(scope) = scopes.find(decode, &operand.id, operand.scope_record_index)? else {
        return Ok(None);
    };
    let Some(state_id) = scope.history_state_id() else {
        return Ok(None);
    };
    let Some(previous_state_id) =
        effective_scope_previous_history_state_id(decode, scope, histories)?
    else {
        return Ok(None);
    };
    unique_history_state_pair(decode, histories, state_id, previous_state_id)
}

fn active_brep_face_matches_source(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face: &cadmpeg_ir::ids::FaceId,
    source: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if decode.starts_with(
        face.as_str(),
        "f3d:brep:entity#",
        "match F3D default BREP face prefix",
    )? {
        Ok(true)
    } else {
        face_in_brep_source(decode, face.as_str(), source)
    }
}

fn face_in_brep_source(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
    source: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(tail) = decode.strip_prefix(id, "f3d:brep/", "strip F3D BREP face prefix")? else {
        return Ok(false);
    };
    let Some(tail) = decode.strip_prefix(tail, source, "strip F3D BREP source")? else {
        return Ok(false);
    };
    decode.starts_with(tail, "/", "match F3D BREP source separator")
}

#[derive(Debug, PartialEq)]
enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

fn select_legacy_extrude_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Result<Option<LegacyFaceResolution>, cadmpeg_core::CodecError> {
    for changed_only in [true, false] {
        let mut matches_historical_face = |face: &cadmpeg_ir::ids::FaceId| {
            let Some(slot) = stable_ref(decode, face.as_str())? else {
                return Ok(false);
            };
            if !decode.contains(
                &topology.faces,
                &slot,
                "find F3D legacy Extrude topology face",
            )? {
                return Ok(false);
            }
            if changed_only
                && !decode.contains_hash_set(
                    changed_faces,
                    &slot,
                    "find F3D legacy Extrude changed face",
                )?
            {
                return Ok(false);
            }
            Ok(true)
        };
        let Some(face_index) = decode.position_by(
            &candidates,
            &mut matches_historical_face,
            "find F3D legacy Extrude history face",
        )?
        else {
            continue;
        };
        if decode
            .position_by(
                &candidates[face_index + 1..],
                &mut matches_historical_face,
                "find additional F3D legacy Extrude history face",
            )?
            .is_none()
        {
            if let Some(slot) = stable_ref(decode, candidates[face_index].as_str())? {
                return Ok(Some(LegacyFaceResolution::Historical(slot)));
            }
        }
    }
    if let Some(source) = history_source {
        let mut matches_source_face =
            |face: &cadmpeg_ir::ids::FaceId| face_in_brep_source(decode, face.as_str(), source);
        if let Some(index) = decode.position_by(
            &candidates,
            &mut matches_source_face,
            "find F3D legacy Extrude source face",
        )? {
            if decode
                .position_by(
                    &candidates[index + 1..],
                    &mut matches_source_face,
                    "find additional F3D legacy Extrude source face",
                )?
                .is_none()
            {
                return Ok(Some(LegacyFaceResolution::Active(
                    candidates.swap_remove(index),
                )));
            }
        }
    }
    if candidates.len() == 1 {
        Ok(candidates.pop().map(LegacyFaceResolution::Active))
    } else {
        Ok(None)
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
fn body_member<B>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: B,
    native: String,
) -> Result<Option<cadmpeg_ir::features::BodyMember<B>>, cadmpeg_core::decode::ResourceLimit> {
    Ok(
        cadmpeg_core::text::NonBlankString::for_decode(ctx, native, "validate body native member")?
            .map(|native| cadmpeg_ir::features::BodyMember::new(body, native)),
    )
}

fn historical_brep_source<'text>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state_id: &'text str,
) -> Result<Option<&'text str>, cadmpeg_core::CodecError> {
    let brep_suffix =
        match decode.rsplit_once(state_id, "/BREP.", "find F3D historical BREP source")? {
            Some((_, suffix)) => suffix,
            None => {
                let Some((_, suffix)) =
                    decode.rsplit_once(state_id, "BREP.", "find F3D historical BREP source")?
                else {
                    return Ok(None);
                };
                suffix
            }
        };
    Ok(decode
        .split_once(brep_suffix, ":asm-", "find F3D BREP assembly delimiter")?
        .map(|(source, _)| source))
}

fn resolve_direct_face_recipe_clauses(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let (resolved, storage) = decode.with_scoped_storage(
        "retain F3D direct face clause result",
        || -> Result<_, cadmpeg_core::CodecError> {
            let mut grouping_storage =
                decode.reserve_scoped(0, "group F3D direct face references")?;
            let mut clause_positions = HashMap::new();
            let mut clauses = Vec::<Vec<&crate::records::dimensions::DesignRecipeReference>>::new();
            for reference in decode.admit_iter(references, "scan F3D direct face references")? {
                let key = (reference.selector_offset, reference.token_offset);
                let position = match decode.get_hash_map(
                    &clause_positions,
                    &key,
                    "find F3D direct face clause",
                )? {
                    Some(&position) => position,
                    None => {
                        let position = clauses.len();
                        grouping_storage.with_storage(|| {
                            decode.push_vec(
                                &mut clauses,
                                Vec::new(),
                                "collect F3D direct face clauses",
                            )?;
                            decode
                                .insert_hash_map(
                                    &mut clause_positions,
                                    key,
                                    position,
                                    "index F3D direct face clauses",
                                )
                                .map(|_| ())
                        })?;
                        position
                    }
                };
                grouping_storage.with_storage(|| {
                    decode.push_vec(
                        &mut clauses[position],
                        reference,
                        "group F3D direct face references",
                    )
                })?;
            }
            let mut topology_faces = HashSet::new();
            for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
                grouping_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut topology_faces,
                        *face,
                        "index F3D direct topology faces",
                    )
                })?;
            }
            let mut resolved = Vec::new();
            let mut history_source = IntoIterator::into_iter(&clauses);
            while let Some(references) =
                decode.next_charged(&mut history_source, "scan F3D direct face clauses")?
            {
                let mut clause_storage =
                    decode.reserve_scoped(0, "collect F3D direct face clause candidates")?;
                let mut intersection = None::<BTreeSet<i64>>;
                let mut history_source = IntoIterator::into_iter(references);
                while let Some(reference) = decode.next_charged(
                    &mut history_source,
                    "scan F3D direct face clause references",
                )? {
                    let candidates = if reference.candidate_faces.is_empty() {
                        &reference.alternate_selector_faces
                    } else {
                        &reference.candidate_faces
                    };
                    let mut reference_storage =
                        decode.reserve_scoped(0, "collect F3D direct face clause candidates")?;
                    let mut eligible = BTreeSet::new();
                    for face in
                        decode.admit_iter(candidates, "scan F3D direct face clause candidates")?
                    {
                        let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                            continue;
                        };
                        if decode.contains_hash_set(
                            &topology_faces,
                            &face_slot,
                            "find F3D history topology faces",
                        )? && decode.contains_hash_set(
                            changed_faces,
                            &face_slot,
                            "find F3D history changed faces",
                        )? {
                            decode.insert_scoped_btree_set(
                                &mut reference_storage,
                                &mut eligible,
                                face_slot,
                                "collect F3D direct face clause candidates",
                                "collect F3D direct face clause candidates",
                            )?;
                        }
                    }
                    let candidates = eligible;
                    if candidates.is_empty() {
                        return Ok(Vec::new());
                    }
                    intersection = Some(match intersection {
                        None => {
                            clause_storage.with_storage(|| reference_storage.commit())?;
                            candidates
                        }
                        Some(mut intersection) => {
                            decode.retain_btree_set(
                                &mut intersection,
                                |face| {
                                    decode.contains_btree_set(
                                        &candidates,
                                        face,
                                        "find F3D direct face clause candidate",
                                    )
                                },
                                "intersect F3D direct face clauses",
                            )?;
                            intersection
                        }
                    });
                }
                let Some(face) = single_btree_value(intersection) else {
                    return Ok(Vec::new());
                };
                if !decode.contains(&resolved, &face, "find F3D history resolved")? {
                    decode.reserve_vec(&mut resolved, 1, "collect F3D resolved direct faces")?;
                    resolved.push(face);
                }
            }
            Ok(resolved)
        },
    )?;
    if !resolved.is_empty() {
        storage.commit()?;
    }
    Ok(resolved)
}

fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut history_states = FeatureHistoryStates::new(decode)?;
    for scope in decode.admit_iter(scopes, "scan F3D profile face scopes")? {
        let scoped_history = if decode.contains_key_hash_map(
            scope_histories,
            &scope.id,
            "check F3D edge scope history binding",
        )? {
            let Some(history) = bound_scope_history(decode, &scope.id, scope_histories, histories)?
            else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let (profile_groups, _groups_storage) =
            decode.with_scoped_storage("collect F3D profile roots", || {
                crate::design::face_resolve::extrude_profile_group_roots(
                    decode,
                    scope,
                    operand_groups,
                )
            })?;
        let Some(profile_groups) = profile_groups else {
            continue;
        };
        for group in decode.admit_iter(profile_groups, "scan F3D profile face groups")? {
            let (indices, _indices_storage) =
                decode.with_scoped_storage("collect F3D profile operand indices", || {
                    crate::design::face_resolve::extrude_profile_group_operand_indices(
                        decode,
                        group,
                        operand_groups,
                        operands,
                    )
                })?;
            let Some(indices) = indices else {
                continue;
            };
            if group.members().len() != indices.len()
                || decode.any_by(
                    &indices,
                    |index| {
                        let operand = &operands[*index];
                        Ok(!operand.resolved_face_slots.is_empty()
                            || operand.resolved_active_face.is_some())
                    },
                    "scan F3D resolved profile operands",
                )?
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) = (
                scope.history_state_id(),
                effective_scope_previous_history_state_id(decode, scope, scoped_histories)?,
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    decode,
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )?
            } else {
                unique_history_state_pair(decode, histories, state_id, previous_state_id)?
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let Some(states) = history_states.states_of(decode, history)? else {
                continue;
            };
            let (changed_faces, _changed_faces_storage) = decode
                .with_scoped_storage("collect F3D changed faces", || {
                    face_changes_across_state_chain(decode, state, previous_state_id, states)
                })?;
            let (Some(topology), Some(changed_faces)) = (previous.topology(), changed_faces) else {
                continue;
            };
            let paired_aggregate =
                crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                    decode,
                    group,
                    operand_groups,
                    operands,
                )?;
            let mut carrier_faces_storage =
                decode.reserve_scoped(0, "index F3D profile face carriers")?;
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_storage =
                            decode.reserve_scoped(0, "index F3D profile preceding faces")?;
                        let mut preceding_faces = HashSet::new();
                        for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
                            preceding_storage.with_storage(|| {
                                decode.insert_hash_set(
                                    &mut preceding_faces,
                                    *face,
                                    "index F3D profile preceding faces",
                                )
                            })?;
                        }
                        let bindings = UniqueIndex::new(
                            &topology.face_surfaces,
                            |_, binding| Ok(Some(binding.entity)),
                            "index F3D profile face bindings",
                        );
                        let mut deleted = carrier_faces_storage.with_storage(|| {
                            decode.collect_vec(
                                transition.topology.faces.deleted.iter().copied(),
                                "copy F3D profile deleted faces",
                            )
                        })?;
                        decode.sort_unstable_by(
                            &mut deleted,
                            |value| value,
                            Ord::cmp,
                            "sort F3D profile deleted faces",
                        )?;
                        decode.dedup_vec(&mut deleted, "deduplicate F3D profile deleted faces")?;
                        (deleted.len() == group.members().len()
                            && deleted.len() == transition.topology.faces.deleted.len()
                            && decode.all_by(
                                &deleted,
                                |face| {
                                    Ok(decode.contains_hash_set(
                                        &preceding_faces,
                                        face,
                                        "find F3D history preceding faces",
                                    )? && bindings.get(decode, face)?.is_some())
                                },
                                "scan F3D profile deleted face bindings",
                            )?)
                        .then_some(deleted)
                    } else {
                        None
                    }
                } else {
                    carrier_faces_storage.with_storage(|| {
                        profile_face_group_cardinality_candidates(
                            decode,
                            topology,
                            &changed_faces,
                            group.members().len(),
                        )
                    })?
                };
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in decode
                .admit_iter(indices, "bind F3D profile group faces")?
                .zip(faces)
            {
                let face_id = historical_face_id(decode, face)?;
                let preceding_id =
                    face_id.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                operands[index].preceding_candidate_faces = decode.collect_vec(
                    std::iter::once(preceding_id),
                    "bind F3D profile preceding face",
                )?;
                operands[index].changed_candidate_faces = decode
                    .collect_vec(std::iter::once(face_id), "bind F3D profile changed face")?;
                operands[index].resolved_face_slots =
                    decode.collection_vec(1, "collect F3D profile resolved face slot")?;
                operands[index].resolved_face_slots.push(face);
            }
        }
    }
    Ok(())
}

fn profile_face_group_cardinality_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    // Visit each distinct preceding face once, in topology order, and keep
    // the changed ones: the same face set as the changed faces present in
    // the preceding topology.
    let mut preceding_storage = decode.reserve_scoped(0, "index F3D profile candidate faces")?;
    let bindings = UniqueIndex::new(
        &topology.face_surfaces,
        |_, binding| Ok(Some(binding.entity)),
        "index F3D profile candidate bindings",
    );
    let mut preceding_faces = HashSet::new();
    let mut faces_by_carrier = BTreeMap::<i64, Vec<i64>>::new();
    for face in decode
        .admit_iter(&topology.faces, "scan F3D topology faces")?
        .copied()
    {
        if !preceding_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut preceding_faces,
                face,
                "index F3D profile candidate faces",
            )
        })? || !decode.contains_hash_set(
            changed_faces,
            &face,
            "scan F3D changed profile faces",
        )? {
            continue;
        }
        let Some(binding) = bindings.get(decode, &face)? else {
            continue;
        };
        decode.push_btree_group(
            &mut faces_by_carrier,
            binding.carrier,
            face,
            "index F3D profile face carriers",
            "collect F3D profile carrier faces",
        )?;
    }
    let mut groups = faces_by_carrier.into_iter();
    let Some(mut faces) = decode.find_map(
        &mut groups,
        |(_, faces)| Ok((faces.len() == member_count).then_some(faces)),
        "scan F3D profile carrier groups",
    )?
    else {
        return Ok(None);
    };
    if decode.any_by(
        groups,
        |(_, faces)| Ok(faces.len() == member_count),
        "scan F3D profile carrier groups",
    )? {
        return Ok(None);
    }
    // The faces are distinct, so the sorted group keeps `member_count` faces.
    decode.sort_unstable_by(
        &mut faces,
        |value| value,
        Ord::cmp,
        "sort F3D profile carrier faces",
    )?;
    Ok(Some(faces))
}

/// The faces deleted or updated on the transition chain from `state` back to
/// `previous_state_id`, or `None` when the chain breaks or repeats a state.
/// The returned set is charged to the caller's storage scope.
fn face_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<HashSet<i64>>, cadmpeg_core::CodecError> {
    let mut visited_storage = decode.reserve_scoped(0, "track F3D face change state chain")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut visited,
                current.state_id,
                "track F3D face change state chain",
            )
        })? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        let faces = &transition.topology.faces;
        for face in decode
            .admit_iter(&faces.deleted, "scan F3D changed transition faces")?
            .chain(decode.admit_iter(&faces.updated, "scan F3D changed transition faces")?)
        {
            decode.insert_hash_set(&mut changed, *face, "collect F3D changed faces")?;
        }
        let Some(previous_id) = transition.previous_state_id else {
            return Ok(None);
        };
        let Some(previous) = decode
            .get_hash_map(states, &previous_id, "find F3D history states")?
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(changed))
}

#[derive(Debug, PartialEq)]
struct EdgeChanges {
    deleted: HashSet<i64>,
    updated: HashSet<i64>,
}

/// The edges deleted and updated on the transition chain from `state` back to
/// `previous_state_id`, or `None` when the chain breaks or repeats a state.
/// The returned sets are charged to the caller's storage scope.
fn edge_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<EdgeChanges>, cadmpeg_core::CodecError> {
    let mut visited_storage = decode.reserve_scoped(0, "track F3D edge change state chain")?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut visited,
                current.state_id,
                "track F3D edge change state chain",
            )
        })? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for edge in decode.admit_iter(
            &transition.topology.edges.deleted,
            "scan F3D transition topology edges deleted",
        )? {
            decode.insert_hash_set(&mut deleted, *edge, "collect F3D deleted edges")?;
        }
        for edge in decode.admit_iter(
            &transition.topology.edges.updated,
            "scan F3D transition topology edges updated",
        )? {
            decode.insert_hash_set(&mut updated, *edge, "collect F3D updated edges")?;
        }
        let Some(previous_id) = transition.previous_state_id else {
            return Ok(None);
        };
        let Some(previous) = decode
            .get_hash_map(states, &previous_id, "find F3D history states")?
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(EdgeChanges { deleted, updated }))
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
fn side_one_recipe_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(side) = structure.and_then(|structure| structure.sides.first()) else {
        return Ok(None);
    };
    let mut ordinal_storage = decode.reserve_scoped(0, "collect F3D recipe side ordinals")?;
    let mut ordinals = Vec::new();
    let mut first = Some(side.header_value);
    let mut scalars = side.scalars.iter().copied();
    loop {
        let value = match first.take() {
            Some(first) => first,
            None => match decode.next_charged(&mut scalars, "scan F3D recipe side scalars")? {
                Some(value) => value,
                None => break,
            },
        };
        if value == 0 {
            continue;
        }
        let Some(ordinal) = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))
        else {
            return Ok(None);
        };

        ordinal_storage.with_storage(|| {
            decode.push_vec(&mut ordinals, ordinal, "collect F3D recipe side ordinals")
        })?;
    }
    decode.sort_unstable_by(
        &mut ordinals,
        |value| value,
        Ord::cmp,
        "sort F3D recipe side ordinals",
    )?;
    decode.dedup_vec(&mut ordinals, "deduplicate F3D recipe side ordinals")?;
    let mut edge_set_storage = decode.reserve_scoped(0, "collect F3D recipe side edge sets")?;
    let mut edge_sets = Vec::new();
    let mut history_source = IntoIterator::into_iter(&ordinals);
    while let Some(&ordinal) =
        decode.next_charged(&mut history_source, "scan F3D recipe side ordinals")?
    {
        let Some(context) = reference_contexts.get(ordinal) else {
            return Ok(None);
        };
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return Ok(None);
        }

        edge_set_storage.with_storage(|| {
            decode.push_vec(
                &mut edge_sets,
                context.shared_edge_slots.as_slice(),
                "collect F3D recipe side edge sets",
            )
        })?;
    }
    if decode.any_by(
        &edge_sets,
        |edges| Ok(edges.is_empty()),
        "scan F3D recipe side edge sets",
    )? {
        return Ok(None);
    }
    let Some(first) = edge_sets.first() else {
        return Ok(None);
    };
    let mut candidate_storage = decode.reserve_scoped(0, "copy F3D recipe side edges")?;
    let mut candidates = candidate_storage
        .with_storage(|| decode.collect_vec(first.iter().copied(), "copy F3D recipe side edges"))?;
    for edges in decode
        .admit_iter(&edge_sets, "scan F3D recipe side edge intersections")?
        .skip(1)
    {
        decode.retain_vec(
            &mut candidates,
            |candidate| decode.contains(edges, candidate, "find F3D recipe side edge candidate"),
            "retain F3D recipe side edge candidates",
        )?;
    }
    decode.retain_vec(
        &mut candidates,
        |candidate| {
            decode.contains(
                candidate_edges,
                candidate,
                "find F3D terminal edge candidate",
            )
        },
        "retain F3D terminal edge candidates",
    )?;
    decode.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "sort F3D recipe side edges",
    )?;
    decode.dedup_vec(&mut candidates, "deduplicate F3D recipe side edges")?;
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets),
        ),
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let mut recipe_record_indices_storage = decode.reserve_scoped(0, "index F3D edge recipes")?;
    let mut recipe_record_indices = HashMap::new();
    for recipe in decode.admit_iter(recipes, "scan F3D edge recipes")? {
        if let Some(index) = recipe.record_index {
            recipe_record_indices_storage.with_storage(|| {
                decode
                    .insert_hash_map(
                        &mut recipe_record_indices,
                        recipe.id.as_str(),
                        index.value,
                        "index F3D edge recipes",
                    )
                    .map(|_| ())
            })?;
        }
    }
    let scope_records = ScopesByRecord::new(decode, scopes)?;
    let mut history_states = FeatureHistoryStates::new(decode)?;
    let mut recipe_index = RecentTopologyValue::default();
    let mut terminal_storage = decode.reserve_scoped(0, "collect F3D terminal topologies")?;
    let mut terminal_topologies = Vec::new();
    for history in decode.admit_iter(histories, "scan F3D edge operand histories")? {
        let mut preceding_storage = decode.reserve_scoped(0, "index F3D terminal predecessors")?;
        let mut preceding = HashSet::new();
        for state in decode.admit_iter(&history.states, "scan F3D history states")? {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                preceding_storage.with_storage(|| {
                    decode.insert_hash_set(
                        &mut preceding,
                        previous,
                        "index F3D terminal predecessors",
                    )
                })?;
            }
        }
        let terminal = unique_by(
            decode,
            &history.states,
            |state| {
                decode
                    .contains_hash_set(&preceding, &state.state_id, "find F3D terminal predecessor")
                    .map(|present| !present)
            },
            "find F3D terminal history state",
        )?;
        if let Some(state) = terminal {
            if let Some(topology) = state.topology() {
                decode.push_scoped_vec(
                    &mut terminal_storage,
                    &mut terminal_topologies,
                    (state.state_id, topology),
                    "collect F3D terminal topologies",
                )?;
            }
        }
    }
    for operand in decode.admit_iter(operands, "scan F3D edge operands")? {
        decode.clear_vec(
            &mut operand.result_candidate_faces,
            "clear F3D edge result face candidates",
        )?;
        operand.result_boundary_edge_slots.clear();
        decode.clear_vec(
            &mut operand.preceding_candidate_faces,
            "clear F3D preceding edge face candidates",
        )?;
        decode.clear_vec(
            &mut operand.terminal_candidate_faces,
            "clear F3D terminal edge face candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_candidate_faces,
            "clear F3D changed edge face candidates",
        )?;
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        decode.clear_vec(
            &mut operand.treatment_radius_candidates,
            "clear F3D edge treatment radius candidates",
        )?;
        decode.clear_vec(
            &mut operand.changed_boundary_edge_contexts,
            "clear F3D changed edge boundary contexts",
        )?;
        decode.clear_vec(
            &mut operand.terminal_boundary_edge_contexts,
            "clear F3D terminal edge boundary contexts",
        )?;
        decode.clear_vec(
            &mut operand.recipe_reference_contexts,
            "clear F3D edge recipe reference contexts",
        )?;
        decode.clear_vec(
            &mut operand.recipe_selectors,
            "clear F3D edge recipe selectors",
        )?;
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        operand.resolved_axis = None;
        let Some(scope) = scope_records.find(decode, &operand.id, operand.scope_record_index)?
        else {
            continue;
        };
        let Some(state_id) = scope.history_state_id() else {
            bind_active_edge_operand_for_scope(decode, operand, scope, &terminal_topologies)?;
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(decode, scope, histories)?
        else {
            bind_active_edge_operand_for_scope(decode, operand, scope, &terminal_topologies)?;
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            decode,
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )?
        else {
            continue;
        };
        let (Some(result_topology), Some(topology)) = (state.topology(), previous.topology())
        else {
            continue;
        };
        let recipe_tags = recipe_index.of(topology, || recipe_topology_index(decode, topology))?;
        for reference in decode.admit_iter(
            &mut operand.recipe_references,
            "scan F3D edge operand recipe references",
        )? {
            bind_historical_recipe_reference_candidates(decode, reference, recipe_tags)?;
        }
        if let Some(recipe_record_index) = decode.get_hash_map(
            &recipe_record_indices,
            operand.recipe_id.as_str(),
            "find F3D edge recipe record",
        )? {
            operand.candidate_faces =
                historical_recipe_faces(decode, i64::from(*recipe_record_index), recipe_tags)?;
        }
        let Some(states) = history_states.states_of(decode, history)? else {
            continue;
        };
        let (changed_faces, _changed_faces_storage) = decode
            .with_scoped_storage("collect F3D changed faces", || {
                face_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(changed_faces) = changed_faces else {
            continue;
        };
        let (edge_changes, _edge_changes_storage) = decode
            .with_scoped_storage("collect F3D changed edges", || {
                edge_changes_across_state_chain(decode, state, previous_state_id, states)
            })?;
        let Some(EdgeChanges {
            deleted: chain_deleted_edges,
            updated: chain_updated_edges,
        }) = edge_changes
        else {
            continue;
        };
        let mut scratch = decode.reserve_scoped(0, "index F3D edge operand topology")?;
        let preceding_faces = scratch.with_storage(|| {
            decode.collect_hash_set(
                topology.faces.iter().copied(),
                "index F3D preceding edge faces",
            )
        })?;
        let mut inserted_faces = Vec::new();
        for &face in decode.admit_iter(&result_topology.faces, "scan F3D result topology faces")? {
            if !decode.contains_hash_set(
                &preceding_faces,
                &face,
                "find F3D history preceding faces",
            )? {
                decode.push_scoped_vec(
                    &mut scratch,
                    &mut inserted_faces,
                    face,
                    "collect F3D inserted faces",
                )?;
            }
        }
        let result_edges = scratch.with_storage(|| {
            decode.collect_hash_set(
                result_topology.edges.iter().copied(),
                "index F3D result edges",
            )
        })?;
        let mut deleted_edges = Vec::new();
        let mut updated_edges = Vec::new();
        for &edge in decode.admit_iter(&topology.edges, "scan F3D preceding topology edges")? {
            let (target, operation) = if decode.contains_hash_set(
                &result_edges,
                &edge,
                "find F3D history result edges",
            )? {
                if !decode.contains_hash_set(
                    &chain_deleted_edges,
                    &edge,
                    "find F3D history chain deleted edges",
                )? && !decode.contains_hash_set(
                    &chain_updated_edges,
                    &edge,
                    "find F3D history chain updated edges",
                )? {
                    continue;
                }
                (&mut updated_edges, "collect F3D updated edge candidates")
            } else {
                if !decode.contains_hash_set(
                    &chain_deleted_edges,
                    &edge,
                    "find F3D history chain deleted edges",
                )? {
                    continue;
                }
                (&mut deleted_edges, "collect F3D deleted edge candidates")
            };
            decode.push_scoped_vec(&mut scratch, target, edge, operation)?;
        }
        operand.recipe_state_id = Some(previous_state_id);
        operand.result_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, result_topology)?;
        operand.result_boundary_edge_slots = topology::face_boundary_edges(
            decode,
            &operand.result_candidate_faces,
            result_topology,
        )?;
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, topology)?;
        let mut changed_candidate_faces = Vec::new();
        for face in decode.admit_iter(
            operand.preceding_candidate_faces.as_slice(),
            "scan F3D changed edge faces",
        )? {
            if stable_ref(decode, face.as_str())?
                .map(|slot| {
                    decode.contains_hash_set(
                        &changed_faces,
                        &slot,
                        "find F3D history changed faces",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                let face =
                    face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                decode.push_vec(
                    &mut changed_candidate_faces,
                    face,
                    "collect F3D changed edge faces",
                )?;
            }
        }
        operand.changed_candidate_faces = changed_candidate_faces;
        operand.preceding_boundary_edge_slots =
            topology::face_boundary_edges(decode, &operand.preceding_candidate_faces, topology)?;
        let mut changed_edges = HashSet::new();
        for edge in decode
            .admit_iter(&deleted_edges, "scan F3D changed edge candidates")?
            .chain(decode.admit_iter(&updated_edges, "scan F3D changed edge candidates")?)
        {
            scratch.with_storage(|| {
                decode.insert_hash_set(
                    &mut changed_edges,
                    *edge,
                    "index F3D changed edge candidates",
                )
            })?;
        }
        let mut changed_boundary_edge_slots = Vec::new();
        for &edge in decode.admit_iter(
            &operand.preceding_boundary_edge_slots,
            "scan F3D changed boundary edges",
        )? {
            if decode.contains_hash_set(&changed_edges, &edge, "find F3D history changed edges")? {
                decode.push_vec(
                    &mut changed_boundary_edge_slots,
                    edge,
                    "collect F3D changed boundary edges",
                )?;
            }
        }
        operand.changed_boundary_edge_slots = changed_boundary_edge_slots;
        operand.deleted_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &deleted_edges,
        )?;
        operand.updated_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &updated_edges,
        )?;
        operand.treatment_radius_candidates = topology::treatment_radius_candidates(
            decode,
            Some(&operand.result_candidate_faces),
            &inserted_faces,
            result_topology,
            topology,
            &deleted_edges,
        )?;
        operand.changed_boundary_edge_contexts = decode.try_collect_vec(
            operand
                .changed_boundary_edge_slots
                .iter()
                .copied()
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D changed boundary contexts",
        )?;
        let mut reference_contexts = Vec::new();
        for (ordinal, reference) in decode
            .admit_iter(
                &operand.recipe_references,
                "scan F3D edge recipe references",
            )?
            .enumerate()
        {
            let Ok(reference_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let context = topology::edge_recipe_reference_context(
                decode,
                reference_ordinal,
                reference,
                topology::EdgeBoundaryContext {
                    topology: result_topology,
                    boundary_edges: &operand.result_boundary_edge_slots,
                },
                topology::EdgeBoundaryContext {
                    topology,
                    boundary_edges: &operand.preceding_boundary_edge_slots,
                },
                &changed_edges,
            )?;

            decode.reserve_vec(
                &mut reference_contexts,
                1,
                "collect F3D edge recipe contexts",
            )?;
            reference_contexts.push(context);
        }
        operand.recipe_reference_contexts = reference_contexts;
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            && operand.surface_patch_recipe_structure.is_some()
        {
            operand.resolved_edge_slot = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
            )?;
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Sweep)
        {
            let reference_faces = scratch.with_storage(|| {
                terminal_edge_recipe_reference_faces(
                    decode,
                    &operand.recipe_references,
                    operand.local_topology_references.as_deref(),
                )
            })?;
            let reference_edge_sets = scratch.with_storage(|| {
                topology::collect_reference_edge_sets(decode, &reference_faces, topology)
            })?;
            let mut candidate_edges = BTreeSet::new();
            for edges in
                decode.admit_iter(&reference_edge_sets, "scan F3D sweep reference edges")?
            {
                for &edge in decode.admit_iter(edges, "scan F3D sweep reference edges")? {
                    decode.insert_scoped_btree_set(
                        &mut scratch,
                        &mut candidate_edges,
                        edge,
                        "index F3D sweep candidate edges",
                        "index F3D sweep candidate edges",
                    )?;
                }
            }
            let contexts = scratch.with_storage(|| {
                decode.try_collect_vec(
                    candidate_edges
                        .into_iter()
                        .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                    "collect F3D sweep edge contexts",
                )
            })?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Revolve)
        {
            let reference_faces = scratch.with_storage(|| {
                terminal_edge_recipe_reference_faces(
                    decode,
                    &operand.recipe_references,
                    operand.local_topology_references.as_deref(),
                )
            })?;
            let reference_edge_sets = scratch.with_storage(|| {
                topology::collect_reference_edge_sets(decode, &reference_faces, topology)
            })?;
            let mut candidate_edges = BTreeSet::new();
            for edges in
                decode.admit_iter(&reference_edge_sets, "scan F3D revolve reference edges")?
            {
                for &edge in decode.admit_iter(edges, "scan F3D revolve reference edges")? {
                    decode.insert_scoped_btree_set(
                        &mut scratch,
                        &mut candidate_edges,
                        edge,
                        "index F3D revolve candidate edges",
                        "index F3D revolve candidate edges",
                    )?;
                }
            }
            let contexts = scratch.with_storage(|| {
                decode.try_collect_vec(
                    candidate_edges
                        .into_iter()
                        .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                    "collect F3D revolve edge contexts",
                )
            })?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
                topology::historical_edge_axis(decode, edge, topology)?
                    .and_then(|(origin, direction)| design_axis(origin, direction))
            } else {
                None
            };
            if let Some(axis) = resolved_axis {
                operand.resolved_axis = Some(axis);
            }
            continue;
        }
        let mut changed_edge_contexts = Vec::new();
        for &edge in decode.admit_iter(&topology.edges, "scan F3D changed topology edges")? {
            if decode.contains_hash_set(&changed_edges, &edge, "find F3D history changed edges")? {
                let context = selection::historical_edge_context(decode, edge, topology)?;
                decode.push_scoped_vec(
                    &mut scratch,
                    &mut changed_edge_contexts,
                    context,
                    "collect F3D changed edge contexts",
                )?;
            }
        }
        operand.recipe_selectors = selection::recipe_selector_candidates(
            decode,
            operand.recipe_structure.as_ref(),
            &changed_edge_contexts,
        )?;
        operand.resolved_edge_slot = side_one_recipe_edge(
            decode,
            operand.recipe_structure.as_ref(),
            &operand.recipe_reference_contexts,
            &operand.recipe_selectors,
            &operand.preceding_boundary_edge_slots,
        )?;
    }
    Ok(())
}

/// A resolved axis with finite origin and unit direction.
fn design_axis(
    origin: cadmpeg_ir::math::Point3,
    direction: cadmpeg_ir::math::Vector3,
) -> Option<crate::records::feature::patterns::DesignAxis> {
    crate::records::feature::patterns::DesignAxis::from_parts(
        cadmpeg_ir::features::FinitePoint3::new(origin)?,
        cadmpeg_ir::features::FiniteVector3::new(direction)?,
    )
}

fn bind_active_edge_operand_for_scope(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &AsmHistoricalTopology)],
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(decode, operand, terminal_topologies)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        let mut terminals = terminal_topologies.iter();
        while let Some((state_id, topology)) =
            decode.next_charged(&mut terminals, "scan F3D terminal topologies")?
        {
            if let Some(edge) = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
            )? {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some((*state_id, edge));
            }
        }
        if !ambiguous {
            if let Some((state_id, edge)) = matched {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = match operand.recipe_state_id {
            Some(state_id) => decode
                .find_by(
                    terminal_topologies,
                    |(candidate, _)| Ok(*candidate == state_id),
                    "find F3D revolve terminal topology",
                )?
                .map(|(_, topology)| *topology),
            None => None,
        };
        let resolved_axis = if let Some((edge, topology)) = operand.resolved_edge_slot.zip(topology)
        {
            topology::historical_edge_axis(decode, edge, topology)?
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
    }
    Ok(())
}

fn surface_patch_edge_operand_slot(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(structure) = structure else {
        return Ok(None);
    };
    let [first, second] = &structure.clauses;
    let Some(common_edge_reference) = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(common_face_reference) = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(edge_reference) = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    let Some(face_reference) = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    if edge_reference.candidate_edges.is_empty() {
        return Ok(None);
    }
    let face_candidates = if face_reference.candidate_faces.is_empty() {
        &face_reference.alternate_selector_faces
    } else {
        &face_reference.candidate_faces
    };
    let (faces, _faces_storage) = decode
        .with_scoped_storage("collect F3D surface patch faces", || {
            selection::faces_in_topology(decode, face_candidates, topology)
        })?;
    let (face_boundary_edges, _edges_storage) = decode
        .with_scoped_storage("collect F3D surface patch boundary edges", || {
            topology::face_boundary_edges(decode, &faces, topology)
        })?;
    let mut candidates_storage =
        decode.reserve_scoped(0, "collect F3D surface patch edge candidates")?;
    let mut candidates = Vec::new();
    for edge in decode.admit_iter(
        &edge_reference.candidate_edges,
        "scan F3D surface patch edge candidates",
    )? {
        if let Some(edge_slot) = stable_ref(decode, edge.as_str())? {
            if decode.contains(
                &face_boundary_edges,
                &edge_slot,
                "find F3D history face boundary edges",
            )? {
                candidates_storage.with_storage(|| {
                    decode.push_vec(
                        &mut candidates,
                        edge_slot,
                        "collect F3D surface patch edge candidates",
                    )
                })?;
            }
        }
    }
    decode.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "sort F3D surface patch edge candidates",
    )?;
    decode.dedup_vec(
        &mut candidates,
        "deduplicate F3D surface patch edge candidates",
    )?;
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(None),
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left
        .into_iter()
        .filter(|reference| right.contains(reference));
    let reference = common.next()?;
    common.next().is_none().then_some(reference)
}

fn bind_active_edge_operand_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &AsmHistoricalTopology)],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    let mut history_source = IntoIterator::into_iter(topologies);
    while let Some((state_id, topology)) =
        decode.next_charged(&mut history_source, "scan F3D active edge topologies")?
    {
        let mut candidate_storage =
            decode.reserve_scoped(0, "collect F3D active edge candidate")?;
        let candidate =
            candidate_storage.with_storage(|| -> Result<Option<_>, cadmpeg_core::CodecError> {
                let (all_reference_faces, _all_reference_faces_storage) = decode
                    .with_scoped_storage("collect F3D active edge reference faces", || {
                        terminal_edge_recipe_reference_faces(
                            decode,
                            &operand.recipe_references,
                            None,
                        )
                    })?;
                let (reference_faces, _reference_faces_storage) = decode.with_scoped_storage(
                    "collect F3D active edge local reference faces",
                    || {
                        terminal_edge_recipe_reference_faces(
                            decode,
                            &operand.recipe_references,
                            operand.local_topology_references.as_deref(),
                        )
                    },
                )?;
                let (terminal_faces, _terminal_faces_storage) =
                    decode.with_scoped_storage("collect F3D active edge terminal faces", || {
                        terminal_edge_recipe_faces(
                            decode,
                            &operand.candidate_faces,
                            &reference_faces,
                        )
                    })?;
                let candidate_faces =
                    selection::faces_in_topology(decode, &terminal_faces, topology)?;
                if topologies.len() != 1 && candidate_faces.is_empty() {
                    return Ok(None);
                }
                let boundary_edges =
                    topology::face_boundary_edges(decode, &candidate_faces, topology)?;
                let contexts = decode.try_collect_vec(
                    boundary_edges
                        .iter()
                        .copied()
                        .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                    "collect F3D terminal edge contexts",
                )?;
                let selectors = selection::recipe_selector_candidates(
                    decode,
                    operand.recipe_structure.as_ref(),
                    &contexts,
                )?;
                let (reference_edge_sets, _reference_edge_sets_storage) = decode
                    .with_scoped_storage("collect F3D active edge reference sets", || {
                        topology::collect_reference_edge_sets(decode, &reference_faces, topology)
                    })?;
                let all_reference_edge_sets =
                    topology::collect_reference_edge_sets(decode, &all_reference_faces, topology)?;
                let edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
                Ok(Some((
                    *state_id,
                    edge,
                    candidate_faces,
                    boundary_edges,
                    contexts,
                    all_reference_edge_sets,
                    selectors,
                )))
            })?;
        let Some(candidate) = candidate else {
            continue;
        };
        if unique.is_some() {
            return Ok(());
        }
        unique = Some((candidate, candidate_storage));
    }
    let Some((
        (
            state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ),
        candidate_storage,
    )) = unique
    else {
        return Ok(());
    };
    candidate_storage.commit()?;
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
    Ok(())
}

fn terminal_edge_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut faces = decode.try_collect_vec(
        (primary.iter().chain(
            decode
                .admit_iter(reference_faces, "scan F3D terminal reference face groups")?
                .flatten(),
        ))
        .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
        "collect F3D terminal edge recipe faces",
    )?;
    decode.stable_sort_by(
        &mut faces,
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D terminal edge recipe faces",
    )?;
    decode.dedup_vec(&mut faces, "deduplicate F3D terminal edge recipe faces")?;
    Ok(faces)
}

fn terminal_edge_recipe_reference_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Result<Vec<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    let mut selected = Vec::new();
    let mut append = |reference: &crate::records::dimensions::DesignRecipeReference| {
        let faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        let faces = decode.try_collect_vec(
            (faces)
                .iter()
                .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
            "copy F3D terminal reference faces",
        )?;

        decode.reserve_vec(&mut selected, 1, "collect F3D terminal reference groups")?;
        selected.push(faces);
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    if let Some(ordinals) = local_topology_references {
        for ordinal in decode.admit_iter(ordinals, "scan F3D local topology reference ordinals")? {
            if let Some(reference) = usize::try_from(ordinal.get())
                .ok()
                .and_then(|ordinal| ordinal.checked_sub(1))
                .and_then(|ordinal| references.get(ordinal))
            {
                append(reference)?;
            }
        }
    } else {
        for reference in
            decode.admit_iter(references, "scan F3D terminal edge recipe references")?
        {
            append(reference)?;
        }
    }
    Ok(selected)
}

fn stable_ref(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some((_, tail)) = decode.rsplit_once(id, "#", "split F3D stable entity identity")? else {
        return Ok(None);
    };
    let reference = decode
        .split_once(tail, ":", "split F3D stable entity suffix")?
        .map_or(tail, |(reference, _)| reference);
    Ok(decode
        .parse_text::<i64>(reference, "parse F3D stable entity reference")?
        .ok())
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > EPS_HISTORY_SAME_AXIS_LINE_E9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= EPS_HISTORY_SAME_AXIS_LINE_E8
}

pub(crate) fn historical_topology_with_tags(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = topology::historical_topology(ctx, &brep.asm)? else {
        return Ok(None);
    };
    for tag in ctx.admit_iter(
        &brep.persistent_subentity_tags,
        "scan F3D brep persistent subentity tags",
    )? {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => (
                AsmHistoricalEntityKind::Face,
                stable_ref(ctx, face.as_str())?,
            ),
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => (
                AsmHistoricalEntityKind::Edge,
                stable_ref(ctx, edge.as_str())?,
            ),
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = ctx.collect_vec(
            tag.design_references.iter().copied(),
            "collect F3D historical tag design references",
        )?;
        let token = ctx.copy_retained_text(tag.token.as_str(), "copy F3D historical tag token")?;

        ctx.reserve_vec(
            &mut topology.persistent_subentity_tags,
            1,
            "collect F3D historical persistent tags",
        )?;
        topology.persistent_subentity_tags.push(
            crate::history_records::AsmHistoricalPersistentSubentityTag {
                entity_kind,
                entity_ref,
                selector: tag.selector,
                token,
                design_references,
                ordinal: tag.ordinal,
            },
        );
    }
    Ok(Some(topology))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
