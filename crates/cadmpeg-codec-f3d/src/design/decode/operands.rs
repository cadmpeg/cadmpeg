// SPDX-License-Identifier: Apache-2.0
//! Parse edge, face, and body operand frames and recipe structure.

use crate::bytes::lp_utf16_bounded_charged;
use crate::records::topology::{
    construction::DesignConstructionOperandRole, extrude_selection::DesignExtrudeFaceEncoding,
};

use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::take_reference;
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::dimension_frames::{
    bind_recipe_reference_candidates_charged, contiguous_i32_program,
    decode_recipe_references_charged, recipe_record_prefix,
};
use crate::design::decode::record_streams::{in_stream, record_stream, StreamOffsets};
use crate::design::decode::reference_runs::{admit_reference_values, reference_position};
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::design::decode::scopes::parameter_scope::payload_prologue;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    cached_borrowed_record_offsets, cached_owned_record_offsets, indexed_record_header_at,
    next_indexed_record_header, next_indexed_record_offset, IndexedRecordHeader,
    IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id_charged;
use crate::design::decode::text::relaxed_guid_end;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::layout::class_338_sketch_curve_identity as class_338_curve;
use crate::layout::coil_compact_face_selection_prefix as coil_face_sel;
use crate::layout::coil_compact_persistent_selection_prefix as coil_persist_sel;
use crate::layout::coil_modern_selection_prefix as coil_modern_sel;
use crate::layout::extrude_selection_member_fixed_frame as extrude_member;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::legacy_loft_body_carrier_class_322 as legacy_loft_322;
use crate::layout::legacy_loft_body_carrier_class_322_tail as legacy_loft_322_tail;
use crate::layout::legacy_loft_body_carrier_class_411 as legacy_loft_411;
use crate::layout::sketch_profile_region_member as region_member;
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::layout::work_point_sketch_point_identity as sketch_point_identity;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::{
        extrude::{DesignExtrudeExtent, DesignExtrudePrologue, DesignExtrudeStart},
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload},
        surface_ops::DesignSurfaceOffsetSupport,
        work_geometry::{
            DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignWorkPlaneConstruction,
            DesignWorkPointInputCarrier, DesignWorkPointPlaneSelection, DesignWorkPointRule,
            DesignWorkPointSketchPointSelection,
        },
    },
    identity::ReferenceRun,
    parameters::{DesignParameter, DesignParameterOwner},
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
    references::LostEdgeReference,
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_links::PersistentSubentityTag,
    sketch_relations::SketchRelationOperand,
    topology::{
        body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
        body_recipe::DesignOperandOwner, construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame,
        construction::DesignConstructionOperandIdentity,
        construction::DesignConstructionPersistentIdentity,
        construction::DesignConstructionTrackingPath, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand, edge_recipe::DesignTopologyRecipeEntry,
        edge_recipe::DesignTopologyRecipeSide, edge_recipe::DesignTopologyRecipeTriplet,
        entity_selection::DesignEntitySelectionOperand,
        entity_selection::DesignLoftLegacyBodyCarrier, extrude_selection::DesignExtrudeFaceRole,
        extrude_selection::DesignExtrudeOperandRole,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand, face::DesignFaceSourceGroup, face::DesignFaceSourceMember,
        fillet::DesignFilletRadiusGroup, fillet::DesignFilletRadiusLaw,
        sketch_profile::DesignSketchProfileOperand, sketch_profile::DesignSketchProfileRegion,
        sketch_profile::DesignSketchProfileRegionMember,
        sketch_profile::DesignSketchProfileRegionSelection,
    },
};
use cadmpeg_core::decode::{
    index_from_u32, u64_from_index, DecodeContext, ScopedReservation, View,
};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// The kind of `scope`, except a native kind, whose name is not copied.
fn known_scope_kind(scope: &DesignParameterScope) -> Option<DesignFeatureKind> {
    (!matches!(scope.payload(), DesignScopePayload::Native(_))).then(|| scope.kind())
}

/// The feature family of `scope`. A native kind has no family.
fn scope_family(scope: &DesignParameterScope) -> Option<DesignFeatureFamily> {
    design_feature_family(&known_scope_kind(scope)?)
}

/// The value at `index` of a reference run, read with fixed work.
fn reference_at(run: &ReferenceRun<u32>, index: usize) -> Option<u32> {
    match (run.unlocated_values(), run.located_rows()) {
        (Some(values), _) => values.get(index).copied(),
        (None, Some(rows)) => rows.get(index).map(|row| row.value),
        (None, None) => None,
    }
}

/// Values keyed by native stream and record index. A later value replaces an
/// earlier one with the same key. The index is held in scoped storage for as
/// long as it lives.
pub(super) struct RecordIndex<'a, 'c, T> {
    by_identity: HashMap<(&'a str, u32), &'a T>,
    _storage: ScopedReservation<'c>,
}

impl<'a, 'c, T> RecordIndex<'a, 'c, T> {
    fn build(
        ctx: &'c DecodeContext<'_>,
        values: &'a [T],
        identity: impl Fn(&'a T) -> (&'a str, u32),
        scan_operation: &'static str,
        index_operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, index_operation)?;
        let mut by_identity = HashMap::new();
        for value in ctx.admit_iter(values, scan_operation)? {
            let (id, record_index) = identity(value);
            let Some(stream) = record_stream(ctx, id)? else {
                continue;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut by_identity,
                    (stream, record_index),
                    value,
                    index_operation,
                )
            })?;
        }
        Ok(Self {
            by_identity,
            _storage: storage,
        })
    }

    fn get(
        &self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        record_index: u32,
    ) -> Result<Option<&'a T>, CodecError> {
        let by_identity: &HashMap<(&str, u32), &'a T> = &self.by_identity;
        Ok(ctx
            .get_hash_map(
                by_identity,
                &(stream, record_index),
                "find F3D indexed record",
            )?
            .copied())
    }
}

/// Record headers keyed by native stream and record index.
pub(super) type OperandHeaders<'a, 'c> = RecordIndex<'a, 'c, DesignRecordHeader>;

/// Index record headers by native stream and record index.
pub(super) fn indexed_operand_headers<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    headers: &'a [DesignRecordHeader],
) -> Result<OperandHeaders<'a, 'c>, CodecError> {
    RecordIndex::build(
        ctx,
        headers,
        |header| (header.id.as_str(), header.record_index),
        "index F3D operand headers",
        "f3d operand header index",
    )
}

/// Values grouped by native stream and record index, each group in input
/// order. The index is held in scoped storage for as long as it lives.
struct RecordGroups<'a, 'c, T> {
    groups: HashMap<(&'a str, u32), Vec<&'a T>>,
    _storage: ScopedReservation<'c>,
}

impl<'a, 'c, T> RecordGroups<'a, 'c, T> {
    fn build(
        ctx: &'c DecodeContext<'_>,
        values: &'a [T],
        identity: impl Fn(&'a T) -> (&'a str, u32),
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut groups = HashMap::new();
        for value in ctx.admit_iter(values, operation)? {
            let (id, record_index) = identity(value);
            let Some(stream) = record_stream(ctx, id)? else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut groups,
                    (stream, record_index),
                    value,
                    operation,
                    operation,
                )
            })?;
        }
        Ok(Self {
            groups,
            _storage: storage,
        })
    }

    /// The values of `record_index` in `stream`. The lookup key borrows
    /// `stream`, so the result lives no longer than it.
    fn get<'s>(
        &'s self,
        ctx: &DecodeContext<'_>,
        stream: &'s str,
        record_index: u32,
    ) -> Result<&'s [&'a T], CodecError> {
        let groups: &HashMap<(&str, u32), Vec<&'a T>> = &self.groups;
        Ok(ctx
            .get_hash_map(groups, &(stream, record_index), "find F3D grouped records")?
            .map_or(&[], Vec::as_slice))
    }
}

/// Construction-operand groups grouped by their owning scope.
fn groups_by_scope<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    groups: &'a [DesignConstructionOperandGroup],
) -> Result<RecordGroups<'a, 'c, DesignConstructionOperandGroup>, CodecError> {
    RecordGroups::build(
        ctx,
        groups,
        |group| (group.id.as_str(), group.scope_record_index),
        "index F3D construction operand groups by scope",
    )
}

/// Header offsets of every native stream, ordered by stream and offset.
fn sorted_stream_offsets<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    headers: &'a [DesignRecordHeader],
) -> Result<StreamOffsets<'a, 'c>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d edge operand stream offset")?;
    let mut offsets = Vec::new();
    for header in ctx.admit_iter(headers, "index F3D edge operand offsets")? {
        let Some(stream) = record_stream(ctx, &header.id)? else {
            continue;
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut offsets,
                (stream, header.byte_offset),
                "f3d edge operand stream offset",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut offsets,
        |pair| pair,
        Ord::cmp,
        "sort f3d edge operand stream offsets",
    )?;
    Ok((offsets, storage))
}

/// The first header offset of `stream` after `after` in `sorted_stream_offsets`
/// output. The ordered pairs are bisected.
fn next_stream_offset(
    ctx: &DecodeContext<'_>,
    offsets: &[(&str, u64)],
    stream: &str,
    after: u64,
) -> Result<Option<u64>, CodecError> {
    const OPERATION: &str = "find next F3D edge operand stream offset";
    let at = ctx.partition_point(
        offsets,
        |(candidate, offset)| {
            Ok(match ctx.compare(*candidate, stream, OPERATION)? {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Equal => *offset <= after,
                std::cmp::Ordering::Greater => false,
            })
        },
        OPERATION,
    )?;
    let Some((candidate, offset)) = offsets.get(at) else {
        return Ok(None);
    };
    Ok(ctx
        .equal_bytes(candidate.as_bytes(), stream.as_bytes(), OPERATION)?
        .then_some(*offset))
}

/// Decode edge-recipe operand frames named by edge-selecting feature scopes.
pub(crate) fn decode_edge_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let record_headers = headers;
    let headers = indexed_operand_headers(ctx, record_headers)?;
    let mut terminal_storage = ctx.reserve_scoped(0, "f3d edge operand terminal member")?;
    let mut terminal_group_members = HashSet::new();
    for group in ctx.admit_iter(groups, "index F3D edge operand terminal groups")? {
        let Some(member) = group.members().last() else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        terminal_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut terminal_group_members,
                (stream, group.scope_record_index, member.value),
                "f3d edge operand terminal member",
            )
        })?;
    }
    let (stream_offsets, _stream_offsets_storage) = sorted_stream_offsets(ctx, record_headers)?;
    let scope_groups = groups_by_scope(ctx, groups)?;
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D edge operand scopes")? {
        if !known_scope_kind(scope).is_some_and(|kind| has_edge_recipe_operands(&kind)) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let mut member_storage = ctx.reserve_scoped(0, "f3d edge operand member index")?;
        let mut member_indices = HashSet::new();
        let mut insert_member = |index: u32| {
            member_storage.with_storage(|| {
                ctx.insert_hash_set(&mut member_indices, index, "f3d edge operand member index")
            })
        };
        for group in ctx.admit_iter(
            scope_groups.get(ctx, stream, scope.record_index)?,
            "find F3D edge operand groups",
        )? {
            for member in ctx.admit_iter(group.members(), "scan F3D edge operand group members")? {
                insert_member(member.value)?;
            }
        }
        if let Some(operation) = scope.surface_extend_operation() {
            for &index in ctx.admit_iter(
                &operation.edge_record_indices,
                "scan F3D surface-extend edge references",
            )? {
                insert_member(index)?;
            }
        }
        if let Some(operation) = scope.surface_offset_operation() {
            if let DesignSurfaceOffsetSupport::BoundaryCarrier {
                edge_record_indices,
                ..
            } = &operation.support
            {
                for &index in ctx.admit_iter(
                    edge_record_indices,
                    "scan F3D surface-offset edge references",
                )? {
                    insert_member(index)?;
                }
            }
        }
        if let Some(construction) = scope.work_point_construction() {
            for input in
                ctx.admit_iter(construction.rule.inputs(), "scan F3D WorkPoint rule inputs")?
            {
                insert_member(input.record_index())?;
            }
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let stream_end = u64::try_from(bytes.len()).map_err(|_| {
            CodecError::malformed("Fusion Design BulkStream exceeds the addressable range")
        })?;
        for (ordinal, record_index) in admit_reference_values(
            ctx,
            scope.reference_members(),
            "scan F3D edge operand scope references",
        )?
        .copied()
        .enumerate()
        {
            if !ctx.contains_hash_set(
                &member_indices,
                &record_index,
                "find F3D edge operand member index",
            )? {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            let terminal_group_limit = if ctx.contains_hash_set(
                &terminal_group_members,
                &(stream, scope.record_index, header.record_index),
                "find F3D edge operand terminal member",
            )? {
                Some(
                    next_stream_offset(ctx, &stream_offsets, stream, header.byte_offset)?
                        .unwrap_or(stream_end),
                )
            } else {
                None
            };
            let Some(operand) = parse_edge_operand(
                ctx,
                bytes,
                records,
                scope,
                (ordinal, header),
                recipes,
                terminal_group_limit,
            )?
            else {
                continue;
            };
            ctx.push_vec(&mut out, operand, "f3d edge operand output")?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 1",
    )?;
    Ok(out)
}

/// The ordinal of the only member of `group` that names `record_index`.
/// The search stops at a second match.
fn unique_member_ordinal(
    ctx: &DecodeContext<'_>,
    group: &DesignConstructionOperandGroup,
    record_index: u32,
) -> Result<Option<usize>, CodecError> {
    const OPERATION: &str = "scan F3D vertex operand group members";
    let members = group.members();
    let Some(first) = ctx.position_by(
        members,
        |member| Ok(member.value == record_index),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let later = members.get(first + 1..).unwrap_or(&[]);
    if ctx.any_by(later, |member| Ok(member.value == record_index), OPERATION)? {
        return Ok(None);
    }
    Ok(Some(first))
}

/// Decode vertex-recipe members retained inside edge-treatment groups.
pub(crate) fn decode_edge_treatment_vertex_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeTreatmentVertexOperand>, CodecError> {
    const GROUP_OPERATION: &str = "find F3D vertex operand groups";
    let headers = indexed_operand_headers(ctx, headers)?;
    let scope_groups = groups_by_scope(ctx, groups)?;
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D vertex operand scopes")? {
        if !known_scope_kind(scope).is_some_and(|kind| has_edge_recipe_operands(&kind)) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let candidates = scope_groups.get(ctx, stream, scope.record_index)?;
        for (scope_reference_ordinal, record_index) in admit_reference_values(
            ctx,
            scope.reference_members(),
            "scan F3D vertex operand scope references",
        )?
        .copied()
        .enumerate()
        {
            // The member must be unique within exactly one group of the scope.
            let mut group_member_ordinal = None;
            let Some(group_at) = ctx.position_by(
                candidates,
                |group| {
                    group_member_ordinal = unique_member_ordinal(ctx, group, record_index)?;
                    Ok(group_member_ordinal.is_some())
                },
                GROUP_OPERATION,
            )?
            else {
                continue;
            };
            let (Some(group), Some(group_member_ordinal)) =
                (candidates.get(group_at), group_member_ordinal)
            else {
                continue;
            };
            let later_groups = candidates.get(group_at + 1..).unwrap_or(&[]);
            if ctx.any_by(
                later_groups,
                |group| Ok(unique_member_ordinal(ctx, group, record_index)?.is_some()),
                GROUP_OPERATION,
            )? {
                continue;
            }
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            let Some(recipe) = parse_vertex_recipe(ctx, bytes, records, stream, header, recipes)?
            else {
                continue;
            };
            let (Ok(scope_reference_ordinal), Ok(group_member_ordinal)) = (
                u32::try_from(scope_reference_ordinal),
                u32::try_from(group_member_ordinal),
            ) else {
                continue;
            };
            let id = design_record_id_charged(
                ctx,
                stream,
                ":edge-treatment-vertex-operand#",
                header.byte_offset,
                "f3d vertex operand ID",
            )?;
            ctx.push_vec(
                &mut out,
                DesignEdgeTreatmentVertexOperand {
                    id,
                    scope_record_index: scope.record_index,
                    scope_reference_ordinal,
                    group_record_index: group.record_index,
                    group_member_ordinal,
                    recipe,
                },
                "f3d vertex operand output",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 2",
    )?;
    Ok(out)
}

/// `WorkPlane` scope record indices keyed by native stream and by the record
/// index that precedes each scope. A later scope replaces an earlier one with
/// the same key.
fn work_plane_index(
    ctx: &DecodeContext<'_>,
    scopes: &[DesignParameterScope],
) -> Result<HashMap<String, HashMap<u32, u32>>, CodecError> {
    let mut work_planes: HashMap<String, HashMap<u32, u32>> = HashMap::new();
    for scope in ctx.admit_iter(scopes, "index F3D WorkPlane scopes")? {
        if !matches!(scope.payload(), DesignScopePayload::WorkPlane(_)) {
            continue;
        }
        let (Some(stream), Some(preceding_index)) = (
            record_stream(ctx, &scope.id)?,
            scope.record_index.checked_sub(1),
        ) else {
            continue;
        };
        if !ctx.contains_key_hash_map(
            &work_planes,
            stream,
            "f3d WorkPoint work-plane stream index",
        )? {
            ctx.insert_hash_map(
                &mut work_planes,
                ctx.copy_retained_text(stream, "f3d WorkPoint work-plane stream key")?,
                HashMap::new(),
                "f3d WorkPoint work-plane stream index",
            )?;
        }
        let Some(indices) = ctx.get_mut_hash_map(
            &mut work_planes,
            stream,
            "f3d WorkPoint work-plane stream index",
        )?
        else {
            continue;
        };
        ctx.insert_hash_map(
            indices,
            preceding_index,
            scope.record_index,
            "f3d WorkPoint work-plane index",
        )?;
    }
    Ok(work_planes)
}

/// Bind each `WorkPoint` input to its exact edge, vertex, or `WorkPlane` carrier.
pub(crate) fn bind_work_point_input_carriers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    edge_operands: &[DesignEdgeOperand],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let (work_planes, _work_plane_storage) = ctx
        .with_scoped_storage("f3d WorkPoint work-plane index", || {
            work_plane_index(ctx, scopes)
        })?;
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for position in ctx.admit_iter(&(0..scopes.len()), "scan F3D WorkPoint scopes")? {
        let Some(scope) = scopes.get(position) else {
            continue;
        };
        let Some(construction) = scope.work_point_construction() else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_owned_record_offsets(ctx, cache, stream, bytes))?;
        let scope_record_index = scope.record_index;
        let mut inputs = ctx.try_collect_retained_with(
            construction.rule.inputs(),
            "f3d WorkPoint input copy",
            |input| input.try_clone_for_decode(ctx, "f3d WorkPoint input copy"),
        )?;
        for input_at in ctx.admit_iter(&(0..inputs.len()), "bind F3D WorkPoint inputs")? {
            let Some(input) = inputs.get_mut(input_at) else {
                continue;
            };
            let record_index = input.record_index();
            let is_edge_match = |operand: &DesignEdgeOperand| -> Result<bool, CodecError> {
                Ok(operand.scope_record_index == scope_record_index
                    && operand.record_index() == record_index
                    && in_stream(ctx, &operand.id, stream)?)
            };
            if let Some(edge_at) = ctx.position_by(
                edge_operands,
                is_edge_match,
                "find F3D WorkPoint edge operands",
            )? {
                let later = edge_operands.get(edge_at + 1..).unwrap_or(&[]);
                if let (Some(operand), false) = (
                    edge_operands.get(edge_at),
                    ctx.any_by(later, is_edge_match, "find F3D WorkPoint edge operands")?,
                ) {
                    input
                        .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
                            operand_id: ctx
                                .copy_retained_text(&operand.id, "f3d WorkPoint edge operand ID")?,
                        })))
                        .map_err(CodecError::Malformed)?;
                    continue;
                }
            }
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            if let Some(recipe) = parse_vertex_recipe(ctx, bytes, records, stream, header, recipes)?
            {
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                        recipe,
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            if let Some(selection) =
                parse_work_point_sketch_point_frame(ctx, bytes, record_index, header.byte_offset)?
            {
                let (Ok(asset_id), Ok(context_id)) = (
                    crate::records::mesh::DesignRelaxedGuidText::try_from(selection.asset_id),
                    crate::records::mesh::DesignRelaxedGuidText::try_from(selection.context_id),
                ) else {
                    continue;
                };
                let is_point_match = |point: &SketchPoint| -> Result<bool, CodecError> {
                    Ok(point.owner_reference == Some(selection.sketch_record_index)
                        && point.persistent_id() == Some(selection.point_persistent_id)
                        && in_stream(ctx, &point.id, stream)?)
                };
                let Some(point_at) = ctx.position_by(
                    sketch_points,
                    is_point_match,
                    "find F3D WorkPoint sketch points",
                )?
                else {
                    continue;
                };
                let later = sketch_points.get(point_at + 1..).unwrap_or(&[]);
                let (Some(point), false) = (
                    sketch_points.get(point_at),
                    ctx.any_by(later, is_point_match, "find F3D WorkPoint sketch points")?,
                ) else {
                    continue;
                };
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                        selection: DesignWorkPointSketchPointSelection::try_new(
                            record_index,
                            crate::records::feature::work_geometry::DesignWorkPointSketchPointSelectionDraft {
                                class_tag: header
                                    .class_tag
                                    .try_clone_for_decode(ctx, "f3d WorkPoint selection class tag")?,
                                asset_id,
                                asset_id_offset: selection.asset_id_offset,
                                context_id,
                                context_id_offset: selection.context_id_offset,
                                identity_record_index: selection.identity_record_index,
                                identity_record_offset: selection.identity_record_offset,
                                sketch_record_index: selection.sketch_record_index,
                                sketch_record_index_offset: selection.sketch_record_index_offset,
                                point_persistent_id: selection.point_persistent_id,
                                point_persistent_id_offset: selection.point_persistent_id_offset,
                                point_native_id: ctx
                                    .copy_retained_text(&point.id, "f3d WorkPoint sketch-point ID")?,
                                next_record_index: selection.next_record_index,
                                next_byte_offset: selection.next_byte_offset,
                            },
                        )
                        .map_err(CodecError::Malformed)?,
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            let Some(selection) = parse_entity_selection_frame(
                ctx,
                bytes,
                record_index,
                header.byte_offset,
                header.class_tag.as_str(),
            )?
            else {
                continue;
            };
            if selection.secondary.is_some() {
                continue;
            }
            let Ok(primary_identity) = u32::try_from(selection.primary_identity) else {
                continue;
            };
            let Some(indices) =
                ctx.get_hash_map(&work_planes, stream, "find F3D WorkPoint work-plane stream")?
            else {
                continue;
            };
            let Some(&work_plane_scope_record_index) =
                ctx.get_hash_map(indices, &primary_identity, "find F3D WorkPoint work plane")?
            else {
                continue;
            };
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.context_id),
            ) else {
                continue;
            };
            input
                .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
                    selection: DesignWorkPointPlaneSelection::try_new(
                        record_index,
                        crate::records::feature::work_geometry::DesignWorkPointPlaneSelectionDraft {
                            class_tag: header
                                .class_tag
                                .try_clone_for_decode(ctx, "f3d WorkPoint selection class tag")?,
                            asset_id,
                            asset_id_offset: selection.asset_id_offset,
                            context_id,
                            context_id_offset: selection.context_id_offset,
                            identity_record_index: selection.identity_record_index,
                            identity_record_offset: selection.identity_record_offset,
                            primary_identity: selection.primary_identity,
                            primary_identity_offset: selection.primary_identity_offset,
                            work_plane_scope_record_index,
                            next_record_index: selection.next_record_index,
                            next_byte_offset: selection.next_byte_offset,
                        },
                    )
                    .map_err(CodecError::Malformed)?,
                })))
                .map_err(CodecError::Malformed)?;
        }
        let rule = DesignWorkPointRule::from_serialized(construction.rule.reference_type(), inputs)
            .map_err(CodecError::Malformed)?;
        if let Some(construction) = scopes
            .get_mut(position)
            .and_then(DesignParameterScope::work_point_construction_mut)
        {
            construction.rule = rule;
        }
    }
    Ok(())
}

/// Bind the exact three-vertex construction carried by a `WorkPlane` scope.
pub(crate) fn bind_work_plane_constructions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for position in ctx.admit_iter(&(0..scopes.len()), "scan F3D WorkPlane scopes")? {
        let Some(frame) = scopes
            .get_mut(position)
            .and_then(DesignParameterScope::work_plane_frame_mut)
        else {
            continue;
        };
        frame.work_plane_construction = None;
        let Some(scope) = scopes.get(position) else {
            continue;
        };
        let Some([placement_record_index, first, second, third, extra_offset]) =
            scope.reference_members().values_array()
        else {
            continue;
        };
        if scope.work_plane_reference() != Some(*extra_offset) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_owned_record_offsets(ctx, cache, stream, bytes))?;
        let Some(owner) = ctx.find_by(
            owners,
            |owner| {
                Ok(owner.record_index() == *extra_offset
                    && owner.scope_record_index() == scope.record_index
                    && owner.evaluated_value().get() == 0.0
                    && in_stream(ctx, owner.id(), stream)?)
            },
            "find F3D WorkPlane parameter owners",
        )?
        else {
            continue;
        };
        if !ctx.any_by(
            parameters,
            |parameter| {
                Ok(parameter.record_index == owner.parameter_record_index()
                    && parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get() == 0.0
                    && ctx.equal_bytes(
                        parameter.source_kind().as_bytes(),
                        b"ExtraOffset",
                        "match F3D WorkPlane offset parameter kind",
                    )?
                    && in_stream(ctx, &parameter.id, stream)?)
            },
            "find F3D WorkPlane offset parameters",
        )? {
            continue;
        }
        let recipe_at = |record_index: u32| -> Result<Option<DesignVertexRecipe>, CodecError> {
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                return Ok(None);
            };
            parse_vertex_recipe(ctx, bytes, records, stream, header, recipes)
        };
        let (Some(first_input), Some(second_input), Some(third_input)) =
            (recipe_at(*first)?, recipe_at(*second)?, recipe_at(*third)?)
        else {
            continue;
        };
        let construction = DesignWorkPlaneConstruction::try_new(
            *placement_record_index,
            Box::new([first_input, second_input, third_input]),
        )
        .map_err(CodecError::malformed)?;
        if let Some(frame) = scopes
            .get_mut(position)
            .and_then(DesignParameterScope::work_plane_frame_mut)
        {
            frame.work_plane_construction = Some(construction);
        }
    }
    Ok(())
}

/// Bind persistent subentity candidates carried by decoded vertex recipes.
pub(crate) fn bind_vertex_recipe_candidates(
    ctx: &DecodeContext<'_>,
    scopes: &mut [DesignParameterScope],
    tags: &[PersistentSubentityTag],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind F3D vertex recipe candidates";
    for position in ctx.admit_iter(&(0..scopes.len()), OPERATION)? {
        let Some(scope) = scopes.get_mut(position) else {
            continue;
        };
        if scope.work_plane_construction().is_none() && scope.work_point_construction().is_none() {
            continue;
        }
        let (scope_id, _scope_reservation) = ctx
            .with_scoped_storage("f3d scoped stream identity", || {
                ctx.copy_retained_text(&scope.id, "f3d scoped stream identity")
            })?;
        if let Some(construction) = scope.work_plane_construction_mut() {
            let count = construction
                .inputs()
                .iter()
                .map(|input| input.recipe_references.len())
                .sum::<usize>();
            for (_, reference) in ctx
                .admit_iter(&(0..count), OPERATION)?
                .zip(construction.recipe_references_mut())
            {
                bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&scope_id))?;
            }
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let input_count = construction.rule.inputs().len();
        for (_, recipe) in ctx
            .admit_iter(&(0..input_count), OPERATION)?
            .zip(construction.rule.vertex_recipes_mut())
        {
            for reference in ctx.admit_iter(&(0..recipe.recipe_references.len()), OPERATION)? {
                let Some(reference) = recipe.recipe_references.get_mut(reference) else {
                    continue;
                };
                bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&scope_id))?;
            }
        }
    }
    Ok(())
}

/// Bind active fallback candidates for edge-treatment corner recipes.
pub(crate) fn bind_edge_treatment_vertex_candidates(
    ctx: &DecodeContext<'_>,
    operands: &mut [DesignEdgeTreatmentVertexOperand],
    tags: &[PersistentSubentityTag],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind F3D edge-treatment vertex candidates";
    for position in ctx.admit_iter(&(0..operands.len()), OPERATION)? {
        let Some(operand) = operands.get_mut(position) else {
            continue;
        };
        let references = &mut operand.recipe.recipe_references;
        for reference in ctx.admit_iter(&(0..references.len()), OPERATION)? {
            let Some(reference) = references.get_mut(reference) else {
                continue;
            };
            bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&operand.id))?;
        }
    }
    Ok(())
}

/// Whether a feature family owns edge-recipe operands directly or through a
/// counted construction-operand group.
pub(crate) fn has_edge_recipe_operands(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::Fillet
                | DesignFeatureFamily::Chamfer
                | DesignFeatureFamily::Revolve
                | DesignFeatureFamily::Loft
                | DesignFeatureFamily::Sweep
                | DesignFeatureFamily::Pipe
                | DesignFeatureFamily::SurfacePatch
                | DesignFeatureFamily::SurfaceExtend
                | DesignFeatureFamily::SurfaceOffset
                | DesignFeatureFamily::SurfaceRuled
        )
    ) || matches!(kind, Kind::EdgeFlange | Kind::Hem | Kind::WorkPoint)
}

/// Indexed-record distance from an edge-recipe primary record to its terminal
/// record for the owning consumer.
pub(crate) fn edge_recipe_terminal_delta(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> u32 {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    terminal_delta(design_feature_family(kind), matches!(kind, Kind::WorkPoint))
}

fn terminal_delta(family: Option<DesignFeatureFamily>, work_point: bool) -> u32 {
    match family {
        Some(DesignFeatureFamily::Sweep) => 7,
        _ if work_point => 5,
        _ => 4,
    }
}

/// Decode persistent selection identities named by Fillet and Chamfer groups.
pub(crate) fn decode_edge_identity_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEdgeIdentityOperand>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let scopes_by_record = RecordGroups::build(
        ctx,
        scopes,
        |scope| (scope.id.as_str(), scope.record_index),
        "index F3D edge identity scopes",
    )?;
    let mut out = Vec::new();
    for group in ctx.admit_iter(groups, "scan F3D edge identity groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(scope) = ctx.find_by(
            scopes_by_record.get(ctx, stream, group.scope_record_index)?,
            |scope| {
                Ok(matches!(
                    scope_family(scope),
                    Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
                ))
            },
            "find F3D edge identity scopes",
        )?
        else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in ctx
            .admit_iter(group.members(), "scan F3D edge identity group members")?
            .map(|member| member.value)
            .enumerate()
        {
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let Some(parsed) = parse_edge_identity_member(ctx, bytes, start)? else {
                continue;
            };
            let Ok(group_member_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.context_id),
            ) else {
                continue;
            };
            let operand = DesignEdgeIdentityOperand::try_new(
                crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
                    id: design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-edge-identity-operand#",
                        header.byte_offset,
                        "f3d edge identity ID",
                    )?,
                    scope_record_index: scope.record_index,
                    group_record_index: group.record_index,
                    group_member_ordinal,
                    record_index,
                    byte_offset: header.byte_offset,
                    class_tag: header
                        .class_tag
                        .try_clone_for_decode(ctx, "f3d edge identity class tag")?,
                    layout: parsed.layout,
                    local_id: parsed.local_id,
                    asset_id,
                    asset_id_offset: parsed.asset_id_offset,
                    context_id,
                    context_id_offset: parsed.context_id_offset,
                    historical: None,
                    treatment_radius_candidates: Vec::new(),
                    transition_edge_candidates: Vec::new(),
                    resolved_edge_slots: Vec::new(),
                    resolved_edge_slot: None,
                    resolution_identity_id: None,
                },
            )
            .map_err(CodecError::Malformed)?;
            ctx.push_vec(&mut out, operand, "f3d edge identity output")?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 3",
    )?;
    Ok(out)
}

/// Whether a construction group of `scope` with `group`'s role selects faces
/// that carry face-recipe operands.
fn group_selects_face_operands(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
) -> bool {
    use DesignFeatureFamily as Family;
    let role = group.role();
    if matches!(
        group.extrude_role(),
        Some(DesignExtrudeOperandRole::Profile | DesignExtrudeOperandRole::Faces(_))
    ) {
        return true;
    }
    let by_family = match scope_family(scope) {
        Some(Family::OffsetFaces | Family::Shell | Family::ReplaceFace) => {
            role == DesignOperandRole::ROLE_0X10
        }
        Some(Family::Loft) => matches!(
            role,
            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
        ),
        Some(Family::Sweep) => role == DesignOperandRole::FACES,
        Some(Family::Revolve) => role == DesignOperandRole::ROLE_0X21,
        Some(Family::Fillet | Family::Chamfer | Family::Draft) => true,
        Some(Family::CircularPattern) => role == DesignOperandRole::BODIES_B,
        Some(Family::Mirror) => matches!(
            role,
            DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X5
        ),
        Some(Family::SurfaceOffset) => role == DesignOperandRole::PROFILE,
        _ => false,
    };
    by_family
        || match scope.payload() {
            DesignScopePayload::SplitFace
            | DesignScopePayload::DeleteFace
            | DesignScopePayload::SurfaceDeleteFace => true,
            DesignScopePayload::Thread(_) => role == DesignOperandRole::ROLE_0X10,
            DesignScopePayload::Hole(_) => role == DesignOperandRole::BODIES_A,
            _ => false,
        }
}

/// Whether `scope` is a legacy class-421 As-built envelope.
fn is_legacy_as_built_421(scope: &DesignParameterScope) -> bool {
    matches!(scope.payload(), DesignScopePayload::AsBuilt(_))
        && crate::design::assembly::legacy_as_built_421_generation(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .is_some()
}

/// Decode face-recipe operand frames named by grouped and direct feature references.
pub(crate) fn decode_face_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignFaceOperand>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let scope_index = RecordIndex::build(
        ctx,
        scopes,
        |scope| (scope.id.as_str(), scope.record_index),
        "scan F3D face operand scopes",
        "f3d face operand scope index",
    )?;
    let mut out = Vec::new();
    let mut seen_storage = ctx.reserve_scoped(0, "f3d face operand seen key")?;
    let mut seen = HashSet::new();
    let mut first_visit = |key| {
        seen_storage
            .with_storage(|| ctx.insert_hash_set(&mut seen, key, "f3d face operand seen key"))
    };
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for group in ctx.admit_iter(groups, "scan F3D face operand groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(scope) = scope_index.get(ctx, stream, group.scope_record_index)? else {
            continue;
        };
        if !group_selects_face_operands(scope, group)
            || (group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
                && scope.extrude_profile().is_some())
        {
            continue;
        }
        let follows_scope_references = matches!(
            scope_family(scope),
            Some(DesignFeatureFamily::OffsetFaces | DesignFeatureFamily::Shell)
        ) && group.role() == DesignOperandRole::ROLE_0X10;
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let members = group.members();
        for (group_member_index, record_index) in ctx
            .admit_iter(members, "scan F3D face operand group members")?
            .map(|member| member.value)
            .enumerate()
        {
            if !first_visit((stream, scope.record_index, record_index))? {
                continue;
            }
            let Ok(group_member_ordinal) = u32::try_from(group_member_index) else {
                continue;
            };
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            let group_next_byte_offset = match members.get(group_member_index + 1) {
                Some(next) => headers
                    .get(ctx, stream, next.value)?
                    .map(|header| header.byte_offset),
                None => None,
            };
            let next_byte_offset = match group_next_byte_offset {
                Some(offset) => Some(offset),
                None if follows_scope_references => {
                    let references = scope.reference_members();
                    let next = reference_position(
                        ctx,
                        references,
                        |candidate| Ok(*candidate == record_index),
                        "find F3D face operand scope reference",
                    )?
                    .and_then(|ordinal| reference_at(references, ordinal + 1));
                    match next {
                        Some(next) => headers
                            .get(ctx, stream, next)?
                            .map(|header| header.byte_offset),
                        None => None,
                    }
                }
                None => None,
            };
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                FaceOperandFrame {
                    scope,
                    scope_reference_ordinal: group.scope_reference_ordinal,
                    group_ownership: Some((group.record_index, group_member_ordinal)),
                    next_byte_offset,
                    header,
                },
                recipes,
            )? {
                ctx.push_vec(&mut out, operand, "f3d face operand output")?;
            }
        }
    }
    for scope in ctx.admit_iter(scopes, "scan F3D indexed face operand scopes")? {
        let legacy_as_built = is_legacy_as_built_421(scope);
        if !(legacy_as_built
            || matches!(
                scope_family(scope),
                Some(
                    DesignFeatureFamily::OffsetFaces
                        | DesignFeatureFamily::Shell
                        | DesignFeatureFamily::Thicken
                        | DesignFeatureFamily::Split
                        | DesignFeatureFamily::ReplaceFace
                )
            )
            || matches!(
                scope.payload(),
                DesignScopePayload::SplitFace | DesignScopePayload::Hole(_)
            ))
        {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        // A later scope with the same identity replaces an earlier one.
        if !scope_index
            .get(ctx, stream, scope.record_index)?
            .is_some_and(|indexed| std::ptr::eq(indexed, scope))
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let references = scope.reference_members();
        for (ordinal, record_index) in admit_reference_values(
            ctx,
            references,
            "scan F3D indexed face operand reference ordinals",
        )?
        .copied()
        .enumerate()
        {
            if legacy_as_built && !matches!(ordinal, 1 | 3) {
                continue;
            }
            if !first_visit((stream, scope.record_index, record_index))? {
                continue;
            }
            let (Ok(scope_reference_ordinal), Some(header)) = (
                u32::try_from(ordinal),
                headers.get(ctx, stream, record_index)?,
            ) else {
                continue;
            };
            let next_byte_offset = match reference_at(references, ordinal + 1) {
                Some(next) if legacy_as_built => headers
                    .get(ctx, stream, next)?
                    .map(|header| header.byte_offset),
                _ => None,
            };
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                FaceOperandFrame {
                    scope,
                    scope_reference_ordinal,
                    group_ownership: None,
                    next_byte_offset,
                    header,
                },
                recipes,
            )? {
                ctx.push_vec(&mut out, operand, "f3d face operand output")?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 4",
    )?;
    Ok(out)
}

/// Decode the ordered persistent source identities carried by admitted `Face`
/// source envelopes. The source envelope is distinct from a face-regeneration
/// recipe: its members can name curves and vertices in one operation.
pub(crate) fn decode_face_source_groups(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignFaceSourceGroup>, CodecError> {
    let mut out = Vec::new();
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for scope in ctx.admit_iter(scopes, "scan F3D face source scopes")? {
        if !matches!(scope.payload(), DesignScopePayload::Face) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let Ok(scope_start) = usize::try_from(scope.byte_offset()) else {
            continue;
        };
        let (reference_headers, _reference_headers_storage) =
            ctx.with_scoped_storage("f3d face source reference headers", || {
                face_source_reference_headers(
                    ctx,
                    bytes,
                    scope_start,
                    scope.reference_members(),
                    records,
                )
            })?;
        for (carrier_ordinal, carrier) in ctx
            .admit_iter(&reference_headers, "scan F3D face source carriers")?
            .enumerate()
        {
            let Some(carrier) = carrier else {
                continue;
            };
            let Some(layout) = face_source_carrier_layout(*carrier.class_tag) else {
                continue;
            };
            let Some(paired) = reference_headers
                .get(carrier_ordinal + 1)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if paired.class_tag != layout.paired_class_tag {
                continue;
            }
            let Some((source_references, source_count)) = face_source_carrier_references(
                bytes,
                carrier.byte_offset,
                scope.record_index,
                layout,
            ) else {
                continue;
            };
            // The supported carrier layouts contain at most four source slots.
            let mut source_members = ctx.vector_storage(4, "collect F3D face source members")?;
            let mut complete = true;
            for &(offset, source_record_index) in source_references.iter().take(source_count) {
                let Some(member) = face_source_member(
                    ctx,
                    bytes,
                    records,
                    carrier.byte_offset,
                    (offset, source_record_index),
                )?
                else {
                    complete = false;
                    break;
                };
                ctx.push_vec(
                    &mut source_members,
                    member,
                    "collect F3D face source members",
                )?;
            }
            if !complete {
                continue;
            }
            let Ok(carrier_reference_ordinal) = u32::try_from(carrier_ordinal) else {
                continue;
            };
            let (Ok(carrier_start), Ok(carrier_end)) = (
                u64::try_from(carrier.byte_offset),
                u64::try_from(paired.byte_offset),
            ) else {
                continue;
            };
            let Some(carrier_span) =
                crate::records::identity::NonEmptyByteSpan::new(carrier_start, carrier_end)
            else {
                continue;
            };
            let carrier_class_tag = crate::design::decode::text::retain_class_tag(
                ctx,
                *carrier.class_tag,
                "copy F3D face source carrier class tag",
            )?;
            let paired_class_tag = crate::design::decode::text::retain_class_tag(
                ctx,
                *paired.class_tag,
                "copy F3D face source paired class tag",
            )?;
            let id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-face-source-group#",
                carrier_start,
                "f3d face source group ID",
            )?;
            ctx.push_vec(
                &mut out,
                DesignFaceSourceGroup {
                    id,
                    scope_record_index: scope.record_index,
                    carrier_reference_ordinal,
                    carrier_record_index: carrier.record_index,
                    carrier_span,
                    carrier_class_tag,
                    paired_record_index: paired.record_index,
                    paired_class_tag,
                    source_members,
                },
                "f3d face source group output",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 5",
    )?;
    Ok(out)
}

/// One source slot of a face carrier, resolved to the persistent identity of
/// the first record after the carrier header that carries its record index.
fn face_source_member(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    carrier_byte_offset: usize,
    (offset, source_record_index): (usize, u32),
) -> Result<Option<crate::records::identity::Located<DesignFaceSourceMember>>, CodecError> {
    let Some(search) = carrier_byte_offset.checked_add(indexed_header::LEN) else {
        return Ok(None);
    };
    let Some(source_byte_offset) = records.first_at_or_after(ctx, search, source_record_index)?
    else {
        return Ok(None);
    };
    let Some(source_header) = indexed_record_header_at(bytes, source_byte_offset) else {
        return Ok(None);
    };
    let Some(member) = parse_extrude_identity_member(ctx, bytes, source_byte_offset)? else {
        return Ok(None);
    };
    let (Ok(source_byte_offset), Ok(offset)) =
        (u64::try_from(source_byte_offset), u64::try_from(offset))
    else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id)) = (member.asset_id.try_into(), member.context_id.try_into())
    else {
        return Ok(None);
    };
    let Ok(persistent_identity) = DesignConstructionPersistentIdentity::try_new(
        crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id,
            asset_id_offset: member.asset_id_offset,
            context_id,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        },
    ) else {
        return Ok(None);
    };
    Ok(Some(crate::records::identity::Located {
        offset,
        value: DesignFaceSourceMember {
            record_index: source_record_index,
            byte_offset: source_byte_offset,
            class_tag: source_header.retain_class_tag(ctx, "copy F3D face source class tag")?,
            persistent_identity,
        },
    }))
}

/// The indexed header that a face-source reference names after the scope.
struct FaceSourceReferenceHeader<'a> {
    record_index: u32,
    byte_offset: usize,
    class_tag: &'a [u8; 3],
}

fn face_source_reference_headers<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    scope_start: usize,
    references: &ReferenceRun<u32>,
    records: &IndexedRecordOffsets,
) -> Result<Vec<Option<FaceSourceReferenceHeader<'a>>>, CodecError> {
    let mut headers = Vec::new();
    ctx.reserve_capacity(
        &mut headers,
        references.len(),
        "f3d face source reference headers",
    )?;
    let search = scope_start.checked_add(indexed_header::LEN);
    for record_index in admit_reference_values(ctx, references, "scan F3D face source references")?
    {
        let byte_offset = match search {
            Some(search) => records.first_at_or_after(ctx, search, *record_index)?,
            None => None,
        };
        let header = byte_offset.and_then(|byte_offset| {
            indexed_record_header_at(bytes, byte_offset).map(|header| FaceSourceReferenceHeader {
                record_index: *record_index,
                byte_offset,
                class_tag: header.class_tag,
            })
        });
        ctx.push_vec(&mut headers, header, "f3d face source reference headers")?;
    }
    Ok(headers)
}

/// Fixed source-reference and scalar layout for one face carrier class.
#[derive(Clone, Copy)]
pub(crate) struct FaceSourceCarrierLayout {
    pub(crate) source_count: usize,
    pub(crate) source_reference_offset: usize,
    scalar_offset: usize,
    scalar_discriminator: u32,
    paired_class_tag: &'static [u8; 3],
}

fn face_source_carrier_layout(class_tag: [u8; 3]) -> Option<FaceSourceCarrierLayout> {
    match &class_tag {
        b"398" => Some(FaceSourceCarrierLayout {
            source_count: 4,
            source_reference_offset: 36,
            scalar_offset: 80,
            scalar_discriminator: 100,
            paired_class_tag: b"462",
        }),
        b"394" => Some(FaceSourceCarrierLayout {
            source_count: 2,
            source_reference_offset: 36,
            scalar_offset: 58,
            scalar_discriminator: 109,
            paired_class_tag: b"311",
        }),
        b"356" => Some(FaceSourceCarrierLayout {
            source_count: 2,
            source_reference_offset: 36,
            scalar_offset: 58,
            scalar_discriminator: 109,
            paired_class_tag: b"309",
        }),
        _ => None,
    }
}

pub(crate) fn face_source_carrier_spec(
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<FaceSourceCarrierLayout> {
    let layout = face_source_carrier_layout(<[u8; 3]>::try_from(class_tag.as_bytes()).ok()?)?;
    <&[u8; 3]>::try_from(paired_class_tag.as_bytes())
        .is_ok_and(|paired| paired == layout.paired_class_tag)
        .then_some(layout)
}

/// The source references of the face carrier at `start`, with their count.
/// The carrier names its owning scope and repeats its scalar discriminator.
fn face_source_carrier_references(
    bytes: &[u8],
    start: usize,
    scope_record_index: u32,
    layout: FaceSourceCarrierLayout,
) -> Option<([(usize, u32); 4], usize)> {
    if !zeros_at::<10>(bytes, start.checked_add(indexed_header::LEN)?)
        || marked_face_source_reference(bytes, start.checked_add(21)?)? != scope_record_index
        || View::u32_le_at(bytes, start.checked_add(32)?)?
            != u32::try_from(layout.source_count).ok()?
        || View::u32_le_at(bytes, start.checked_add(layout.scalar_offset)?)?
            != layout.scalar_discriminator
        || !View::f64_le_at(bytes, start.checked_add(layout.scalar_offset + 4)?)?.is_finite()
        || View::u32_le_at(bytes, start.checked_add(layout.scalar_offset + 12)?)?
            != layout.scalar_discriminator
    {
        return None;
    }
    let mut references = [(0, 0); 4];
    for (ordinal, slot) in references.iter_mut().enumerate().take(layout.source_count) {
        let offset = start
            .checked_add(layout.source_reference_offset)?
            .checked_add(ordinal.checked_mul(11)?)?;
        *slot = (offset, marked_face_source_reference(bytes, offset)?);
    }
    Some((references, layout.source_count.min(4)))
}

fn marked_face_source_reference(bytes: &[u8], offset: usize) -> Option<u32> {
    if bytes.get(offset) != Some(&1) || !zeros_at::<6>(bytes, offset.checked_add(5)?) {
        return None;
    }
    View::u32_le_at(bytes, offset.checked_add(1)?)
}

/// Index recipes by their native identity. The index is held in the returned
/// scoped reservation.
fn indexed_operand_recipes<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    recipes: &'a [ConstructionRecipe],
) -> Result<
    (
        HashMap<&'a str, &'a ConstructionRecipe>,
        ScopedReservation<'c>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("f3d operand recipe index", || {
        let mut indexed = HashMap::new();
        for recipe in ctx.admit_iter(recipes, "index F3D operand recipes")? {
            ctx.insert_hash_map(
                &mut indexed,
                recipe.id.as_str(),
                recipe,
                "f3d operand recipe index",
            )?;
        }
        Ok::<_, CodecError>(indexed)
    })
}

/// The non-negative Design reference of the recipe named by `recipe_id`.
fn recipe_design_reference(
    ctx: &DecodeContext<'_>,
    recipes: &HashMap<&str, &ConstructionRecipe>,
    recipe_id: &str,
) -> Result<Option<i64>, CodecError> {
    Ok(ctx
        .get_hash_map(recipes, recipe_id, "find F3D operand recipe")?
        .and_then(|recipe| recipe.record_index)
        .map(|record_index| i64::from(record_index.value))
        .filter(|value| *value >= 0))
}

fn push_operand_face_candidate(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<cadmpeg_ir::ids::FaceId>,
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<(), CodecError> {
    ctx.push_vec(
        candidates,
        face.try_clone_for_decode(ctx, "f3d operand face candidate ID")?,
        "f3d operand face candidate",
    )?;
    Ok(())
}

/// Active faces whose persistent tags carry `design_reference` in the same
/// native occurrence as `owner_id`, sorted and without repeats.
fn tagged_operand_faces(
    ctx: &DecodeContext<'_>,
    tags: &[PersistentSubentityTag],
    design_reference: i64,
    owner_id: &str,
    operation: &'static str,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, CodecError> {
    let mut faces = Vec::new();
    for tag in ctx.admit_iter(tags, "scan F3D operand tags")? {
        let cadmpeg_ir::attributes::AttributeTarget::Face(face) = &tag.target else {
            continue;
        };
        if !crate::ids::same_native_occurrence(&tag.id, owner_id)
            || !ctx.contains(&tag.design_references, &design_reference, operation)?
        {
            continue;
        }
        push_operand_face_candidate(ctx, &mut faces, face)?;
    }
    sort_and_dedup_faces(ctx, &mut faces)?;
    Ok(faces)
}

fn sort_and_dedup_faces(
    ctx: &DecodeContext<'_>,
    faces: &mut Vec<cadmpeg_ir::ids::FaceId>,
) -> Result<(), CodecError> {
    ctx.stable_sort_by(
        &mut faces[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort F3D operand candidate faces",
    )?;
    ctx.dedup_vec(faces, "deduplicate F3D operand candidate faces")
}

/// Candidate faces of the recipe references that carry `design_reference`.
/// The set is held in the returned scoped reservation.
fn referenced_operand_faces<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    references: &'a [crate::records::dimensions::DesignRecipeReference],
    design_reference: i64,
) -> Result<(HashSet<&'a cadmpeg_ir::ids::FaceId>, ScopedReservation<'c>), CodecError> {
    ctx.with_scoped_storage("f3d referenced face candidate", || {
        let mut referenced = HashSet::new();
        for reference in ctx.admit_iter(references, "scan F3D face operand references")? {
            if reference.design_reference != design_reference {
                continue;
            }
            for face in ctx.admit_iter(
                &reference.candidate_faces,
                "scan F3D referenced face candidates",
            )? {
                ctx.insert_hash_set(&mut referenced, face, "f3d referenced face candidate")?;
            }
        }
        Ok::<_, CodecError>(referenced)
    })
}

/// Join each face recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_face_operand_candidates(
    ctx: &DecodeContext<'_>,
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind F3D face operand candidates";
    let (recipes, _recipe_storage) = indexed_operand_recipes(ctx, recipes)?;
    for position in ctx.admit_iter(&(0..operands.len()), OPERATION)? {
        let Some(operand) = operands.get_mut(position) else {
            continue;
        };
        operand.alternate_selector_candidate_faces = Vec::new();
        for reference in ctx.admit_iter(&(0..operand.recipe_references.len()), OPERATION)? {
            let Some(reference) = operand.recipe_references.get_mut(reference) else {
                continue;
            };
            bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&operand.id))?;
        }
        let Some(design_reference) = recipe_design_reference(ctx, &recipes, &operand.recipe_id)?
        else {
            continue;
        };
        operand.candidate_faces = tagged_operand_faces(
            ctx,
            tags,
            design_reference,
            &operand.id,
            "find F3D face operand Design reference",
        )?;
        let (referenced, _referenced_storage) =
            referenced_operand_faces(ctx, &operand.recipe_references, design_reference)?;
        let mut unreferenced = Vec::new();
        for face in ctx.admit_iter(&operand.candidate_faces, "scan F3D face operand candidates")? {
            if !ctx.contains_hash_set(&referenced, &face, "find F3D referenced face candidate")? {
                push_operand_face_candidate(ctx, &mut unreferenced, face)?;
            }
        }
        operand.unreferenced_candidate_faces = unreferenced;
        let mut alternates = Vec::new();
        for reference in ctx.admit_iter(
            &operand.recipe_references,
            "scan F3D face recipe references",
        )? {
            if reference.design_reference != design_reference {
                continue;
            }
            for face in ctx.admit_iter(
                &reference.alternate_selector_faces,
                "scan F3D alternate face candidates",
            )? {
                push_operand_face_candidate(ctx, &mut alternates, face)?;
            }
        }
        sort_and_dedup_faces(ctx, &mut alternates)?;
        operand.alternate_selector_candidate_faces = alternates;
    }
    Ok(())
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_edge_operand_candidates(
    ctx: &DecodeContext<'_>,
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind F3D edge operand candidates";
    let (recipes, _recipe_storage) = indexed_operand_recipes(ctx, recipes)?;
    for position in ctx.admit_iter(&(0..operands.len()), OPERATION)? {
        let Some(operand) = operands.get_mut(position) else {
            continue;
        };
        operand.candidate_faces = Vec::new();
        for reference in ctx.admit_iter(&(0..operand.recipe_references.len()), OPERATION)? {
            let Some(reference) = operand.recipe_references.get_mut(reference) else {
                continue;
            };
            bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&operand.id))?;
        }
        let Some(design_reference) = recipe_design_reference(ctx, &recipes, &operand.recipe_id)?
        else {
            continue;
        };
        operand.candidate_faces = tagged_operand_faces(
            ctx,
            tags,
            design_reference,
            &operand.id,
            "find F3D edge operand Design reference",
        )?;
    }
    Ok(())
}

pub(crate) fn edge_operand_candidate_faces(
    ctx: &DecodeContext<'_>,
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, CodecError> {
    let mut faces = Vec::new();
    for tag in ctx.admit_iter(tags, "scan F3D edge operand candidate tags")? {
        let cadmpeg_ir::attributes::AttributeTarget::Face(face) = &tag.target else {
            continue;
        };
        if owner_id.is_some_and(|owner_id| !crate::ids::same_native_occurrence(&tag.id, owner_id))
            || !ctx.contains(
                &tag.design_references,
                &design_reference,
                "find F3D edge operand candidate design reference",
            )?
        {
            continue;
        }
        ctx.push_vec(
            &mut faces,
            face.try_clone_for_decode(ctx, "f3d operand face candidate ID")?,
            "collect F3D edge operand candidate faces",
        )?;
    }
    sort_and_dedup_faces(ctx, &mut faces)?;
    Ok(faces)
}

/// Resolve the unique sketch-profile frame named by profile-based scopes.
pub(crate) fn bind_sketch_profiles(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    for position in ctx.admit_iter(&(0..scopes.len()), "scan F3D sketch-profile scopes")? {
        let Some(scope) = scopes.get(position) else {
            continue;
        };
        let family = scope_family(scope);
        if !matches!(
            family,
            Some(DesignFeatureFamily::Extrude | DesignFeatureFamily::Sweep)
        ) && !matches!(scope.payload(), DesignScopePayload::BaseFlange(_))
        {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut unique = None;
        let mut multiple = false;
        for (ordinal, record_index) in admit_reference_values(
            ctx,
            scope.reference_members(),
            "scan F3D sketch-profile references",
        )?
        .copied()
        .enumerate()
        {
            let (Ok(ordinal), Some(header)) = (
                u32::try_from(ordinal),
                headers.get(ctx, stream, record_index)?,
            ) else {
                continue;
            };
            let Some(profile) =
                parse_sketch_profile(ctx, bytes, stream, ordinal, header, entities)?
            else {
                continue;
            };
            if unique.is_some() {
                multiple = true;
                break;
            }
            unique = Some(profile);
        }
        let Some(profile) = unique.filter(|_| !multiple) else {
            continue;
        };
        let Some(scope) = scopes.get_mut(position) else {
            continue;
        };
        match scope.payload_mut() {
            crate::records::feature::scope::DesignScopePayloadMut::BaseFlange(slot) => {
                slot.get_or_insert_with(Default::default)
                    .base_flange_profile = Some(profile);
            }
            crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) => {
                slot.get_or_insert_with(Default::default).sweep_profile = Some(profile);
            }
            crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) => {
                slot.get_or_insert_with(Default::default).extrude_profile = Some(profile);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Decode the counted selection group named by each Extrude scope.
pub(crate) fn decode_extrude_selection_groups(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D extrude selection scopes")? {
        if scope_family(scope) != Some(DesignFeatureFamily::Extrude) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in admit_reference_values(
            ctx,
            scope.reference_members(),
            "scan F3D extrude scope references",
        )?
        .copied()
        .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            if let Some(group) = parse_extrude_selection_group(ctx, bytes, scope, ordinal, header)?
            {
                push_extrude_selection_group(
                    ctx,
                    &mut out,
                    group,
                    &entry.name,
                    header.byte_offset,
                )?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 9",
    )?;
    Ok(out)
}

fn push_extrude_selection_group(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<DesignExtrudeSelectionGroup>,
    mut group: DesignExtrudeSelectionGroup,
    stream: &str,
    offset: u64,
) -> Result<(), CodecError> {
    ctx.reserve_vec(out, 1, "f3d extrude selection group output")?;
    group.id = design_record_id_charged(
        ctx,
        stream,
        ":design-extrude-selection-group#",
        offset,
        "f3d extrude selection group ID",
    )?;
    out.push(group);
    Ok(())
}

/// Whether `scope` names counted construction-operand groups among its
/// reference members.
fn owns_construction_operand_groups(scope: &DesignParameterScope) -> bool {
    use DesignFeatureFamily as Family;
    matches!(
        scope_family(scope),
        Some(
            Family::Extrude
                | Family::Coil
                | Family::Loft
                | Family::Sweep
                | Family::Pipe
                | Family::OffsetFaces
                | Family::Revolve
                | Family::Shell
                | Family::Thicken
                | Family::Move
                | Family::SurfacePatch
                | Family::SurfaceRuled
                | Family::BoundaryFill
                | Family::Split
                | Family::Draft
                | Family::ReplaceFace
                | Family::SurfaceOffset
                | Family::SurfaceTrim
                | Family::Scale
                | Family::CircularPattern
                | Family::RectangularPattern
                | Family::Mirror
                | Family::Fillet
                | Family::Chamfer
        )
    ) || matches!(
        scope.payload(),
        DesignScopePayload::SplitFace
            | DesignScopePayload::RemoveBody
            | DesignScopePayload::SurfaceStitch(_)
            | DesignScopePayload::DeleteFace
            | DesignScopePayload::SurfaceDeleteFace
            | DesignScopePayload::Decal
            | DesignScopePayload::Thread(_)
            | DesignScopePayload::Hole(_)
            | DesignScopePayload::BaseFlange(_)
            | DesignScopePayload::EdgeFlange(_)
            | DesignScopePayload::Hem(_)
    )
}

/// Decode counted construction-operand groups named by feature scopes.
///
/// A scope reference member whose record opens the group grammar but does not
/// close it is recorded on the owning scope, so a group the grammar cannot read
/// is distinguishable from a reference member that is not a group at all.
pub(crate) fn decode_construction_operand_groups(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut out = Vec::new();
    for position in ctx.admit_iter(&(0..scopes.len()), "scan F3D construction operand scopes")? {
        let Some(scope) = scopes.get(position) else {
            continue;
        };
        if !owns_construction_operand_groups(scope) {
            continue;
        }
        let scope_group_start = out.len();
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut unclosed = Vec::new();
        for (ordinal, record_index) in admit_reference_values(
            ctx,
            scope.reference_members(),
            "scan F3D construction operand scope references",
        )?
        .copied()
        .enumerate()
        {
            let (Ok(ordinal), Some(header)) = (
                u32::try_from(ordinal),
                headers.get(ctx, stream, record_index)?,
            ) else {
                continue;
            };
            match parse_construction_operand_group_at(
                ctx,
                bytes,
                scope,
                ordinal,
                (header.record_index, &header.class_tag, header.byte_offset),
            )? {
                ConstructionOperandGroupParse::Complete(group) => {
                    push_construction_operand_group(
                        ctx,
                        &mut out,
                        group,
                        &entry.name,
                        header.byte_offset,
                    )?;
                }
                ConstructionOperandGroupParse::Unclosed => {
                    ctx.push_vec(
                        &mut unclosed,
                        record_index,
                        "f3d unclosed construction operand group",
                    )?;
                }
                ConstructionOperandGroupParse::NotAGroup => {}
            }
        }
        let is_extrude = scope_family(scope) == Some(DesignFeatureFamily::Extrude);
        if is_extrude {
            assign_extrude_face_roles(scope, &mut out[scope_group_start..]);
        }
        if let Some(scope) = scopes.get_mut(position) {
            scope.unclosed_construction_operand_groups = unclosed;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 10",
    )?;
    Ok(out)
}

fn push_construction_operand_group(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<DesignConstructionOperandGroup>,
    mut group: Box<DesignConstructionOperandGroup>,
    stream: &str,
    offset: u64,
) -> Result<(), CodecError> {
    ctx.reserve_vec(out, 1, "f3d construction operand group output")?;
    group.id = design_record_id_charged(
        ctx,
        stream,
        ":design-construction-operand-group#",
        offset,
        "f3d construction operand group ID",
    )?;
    out.push(*group);
    Ok(())
}

/// Decode the fixed role-less body carrier used by the legacy Boolean-Loft
/// envelopes. The ordinary role-`0x8` body group is admitted only when this
/// exact carrier is present at scope-reference ordinal zero.
pub(crate) fn decode_loft_legacy_body_carriers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignLoftLegacyBodyCarrier>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut seen_storage = ctx.reserve_scoped(0, "f3d legacy Loft body carrier seen key")?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D legacy loft scopes")? {
        if !matches!(
            scope.payload(),
            DesignScopePayload::Loft(Some(crate::records::feature::path_features::DesignLoftConstruction { operation, .. }))
                if *operation != crate::records::feature::extrude::DesignExtrudeOperation::NewBody
        ) {
            continue;
        }
        let Some(record_index) = reference_at(scope.reference_members(), 0) else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(header) = headers.get(ctx, stream, record_index)? else {
            continue;
        };
        // A carrier keeps the first scope that names it.
        let key = (entry.name.as_str(), header.byte_offset);
        if ctx.contains_hash_set(&seen, &key, "find F3D legacy Loft body carrier seen key")? {
            continue;
        }
        let Some(carrier) = parse_loft_legacy_body_carrier(ctx, bytes, scope, header)? else {
            continue;
        };
        seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, key, "f3d legacy Loft body carrier seen key")
        })?;
        push_loft_legacy_body_carrier(ctx, &mut out, carrier, &entry.name, header.byte_offset)?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 11",
    )?;
    Ok(out)
}

fn push_loft_legacy_body_carrier(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<DesignLoftLegacyBodyCarrier>,
    mut carrier: DesignLoftLegacyBodyCarrier,
    stream: &str,
    offset: u64,
) -> Result<(), CodecError> {
    ctx.reserve_vec(out, 1, "f3d legacy Loft body carrier output")?;
    carrier.id = design_record_id_charged(
        ctx,
        stream,
        ":design-loft-legacy-body-carrier#",
        offset,
        "f3d legacy Loft body carrier ID",
    )?;
    out.push(carrier);
    Ok(())
}

/// Fixed fields of a legacy Loft body carrier frame.
struct LegacyLoftBodyCarrierFrame<'a> {
    member: u32,
    member_offset: usize,
    opaque_index: std::num::NonZeroU8,
    opaque_scalar: cadmpeg_ir::scalar::FiniteReal,
    next_next_record_index: u32,
    next_record_index: u32,
    trailing_scope_reference_offset: Option<u64>,
    paired_byte_offset: usize,
    paired_class_tag: &'a [u8; 3],
}

/// Read the class-`322`/`262` or class-`411`/`266` legacy Loft body carrier
/// frame at `header` with fixed work.
fn legacy_loft_body_carrier_frame<'a>(
    bytes: &'a [u8],
    scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Option<LegacyLoftBodyCarrierFrame<'a>> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let (paired_class, frame_length, has_trailing_scope) = match header.class_tag.code() {
        322 => {
            let pair_matches = |at: usize| {
                indexed_record_header_at(bytes, at).is_some_and(|paired| {
                    paired.record_index == header.record_index && paired.class_tag == b"262"
                })
            };
            if pair_matches(start.checked_add(legacy_loft_322::LEN)?) {
                (b"262", legacy_loft_322::LEN, false)
            } else if pair_matches(start.checked_add(legacy_loft_322_tail::LEN)?) {
                (b"262", legacy_loft_322_tail::LEN, true)
            } else {
                return None;
            }
        }
        411 => (b"266", legacy_loft_411::LEN, true),
        _ => return None,
    };
    let parsed_header = indexed_record_header_at(bytes, start)?;
    if parsed_header.record_index != header.record_index
        || !zeros_at::<10>(bytes, start.checked_add(legacy_loft_322::ZERO_RUN_10)?)
        || bytes.get(start.checked_add(legacy_loft_322::PRESENCE)?) != Some(&1)
        || View::u32_le_at(
            bytes,
            start.checked_add(legacy_loft_322::OWNER_SCOPE_RECORD_INDEX)?,
        )? != scope_record_index
        || !zeros_at::<6>(bytes, start.checked_add(legacy_loft_322::ZERO_RUN_6)?)
        || View::u32_le_at(bytes, start.checked_add(legacy_loft_322::MEMBER_COUNT)?)? != 1
    {
        return None;
    }
    let mut cursor = start.checked_add(legacy_loft_322::MEMBER_REFERENCE)?;
    let member_offset = cursor;
    let (member, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::OPAQUE_INDEX)? {
        return None;
    }
    let opaque_index = u8::try_from(View::u32_le_at(bytes, cursor)?)
        .ok()
        .and_then(std::num::NonZeroU8::new)?;
    cursor = cursor.checked_add(4)?;
    let opaque_scalar = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(bytes, cursor)?)?;
    cursor = cursor.checked_add(8)?;
    if View::u32_le_at(bytes, cursor)? != u32::from(opaque_index.get()) {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (next_next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::FLAGS)?
        || bytes_at::<2>(bytes, cursor) != Some(&[0, 0])
    {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::LEN)? {
        return None;
    }
    let trailing_scope_reference_offset = if has_trailing_scope {
        if cursor != start.checked_add(legacy_loft_322_tail::TAIL_ZERO)?
            || bytes.get(cursor) != Some(&0)
        {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        if cursor != start.checked_add(legacy_loft_322_tail::TRAILING_SCOPE_REFERENCE)? {
            return None;
        }
        let reference_offset = cursor;
        let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
        if record_index != scope_record_index {
            return None;
        }
        Some(u64::try_from(reference_offset).ok()?)
    } else {
        None
    };
    let paired_byte_offset = start.checked_add(frame_length)?;
    if cursor != paired_byte_offset {
        return None;
    }
    let paired_header = indexed_record_header_at(bytes, paired_byte_offset)?;
    if paired_header.record_index != header.record_index || paired_header.class_tag != paired_class
    {
        return None;
    }
    Some(LegacyLoftBodyCarrierFrame {
        member,
        member_offset,
        opaque_index,
        opaque_scalar,
        next_next_record_index,
        next_record_index,
        trailing_scope_reference_offset,
        paired_byte_offset,
        paired_class_tag: paired_header.class_tag,
    })
}

/// Parse one class-`322`/`262` or class-`411`/`266` legacy Loft body carrier.
fn parse_loft_legacy_body_carrier(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &DesignRecordHeader,
) -> Result<Option<DesignLoftLegacyBodyCarrier>, CodecError> {
    let Some(frame) = legacy_loft_body_carrier_frame(bytes, scope.record_index, header) else {
        return Ok(None);
    };
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return Ok(None);
    };
    let offset = |relative: usize| {
        start
            .checked_add(relative)
            .and_then(|at| u64::try_from(at).ok())
    };
    let (
        Some(owner_scope_record_index_offset),
        Ok(member_offset),
        Some(member_count_offset),
        Some(opaque_index_offset),
        Some(opaque_scalar_offset),
        Some(repeated_opaque_index_offset),
        Some(next_next_reference_offset),
        Some(flags_offset),
        Some(next_reference_offset),
        Ok(paired_byte_offset),
    ) = (
        offset(legacy_loft_322::OWNER_SCOPE_RECORD_INDEX),
        u64::try_from(frame.member_offset),
        offset(legacy_loft_322::MEMBER_COUNT),
        offset(legacy_loft_322::OPAQUE_INDEX),
        offset(legacy_loft_322::OPAQUE_SCALAR),
        offset(legacy_loft_322::REPEATED_OPAQUE_INDEX),
        offset(legacy_loft_322::NEXT_NEXT_REFERENCE),
        offset(legacy_loft_322::FLAGS),
        offset(legacy_loft_322::NEXT_REFERENCE),
        u64::try_from(frame.paired_byte_offset),
    )
    else {
        return Ok(None);
    };
    let paired_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *frame.paired_class_tag,
        "copy F3D legacy Loft paired class tag",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D legacy Loft class tag")?;
    Ok(Some(DesignLoftLegacyBodyCarrier {
        id: String::new(),
        scope_record_index: scope.record_index,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag,
        owner_scope_record_index_offset,
        member: frame.member,
        member_offset,
        member_count_offset,
        opaque_index: frame.opaque_index,
        opaque_index_offset,
        opaque_scalar: frame.opaque_scalar,
        opaque_scalar_offset,
        repeated_opaque_index_offset,
        next_next_record_index: frame.next_next_record_index,
        next_next_reference_offset,
        flags_offset,
        next_record_index: frame.next_record_index,
        next_reference_offset,
        trailing_scope_reference_offset: frame.trailing_scope_reference_offset,
        paired_class_tag,
        paired_byte_offset,
    }))
}

pub(in crate::design) fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
) {
    let mut face_groups =
        groups
            .iter_mut()
            .filter_map(|group| match extrude_operand_role(scope, group.role()) {
                Some(PendingExtrudeRole::Faces(encoding)) => Some((group, encoding)),
                _ => None,
            });
    if scope.extrude_prologue().map(DesignExtrudePrologue::start)
        == Some(DesignExtrudeStart::FromFace)
    {
        if let Some((group, encoding)) = face_groups.next() {
            group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
                encoding,
                usage: DesignExtrudeFaceRole::Start,
            };
        }
    }
    for (group, encoding) in face_groups {
        group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
            encoding,
            usage: DesignExtrudeFaceRole::Termination,
        };
    }
}

/// Pair Fillet construction-operand groups with their radius inputs.
fn push_fillet_radius_group(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<DesignFilletRadiusGroup>,
    stream: &str,
    group_ordinal: u32,
    group: &DesignConstructionOperandGroup,
    law: DesignFilletRadiusLaw,
    tangency_weight_parameter_record_index: Option<u32>,
) -> Result<(), CodecError> {
    const SUFFIX: &str = ":design-fillet-radius-group#";

    let edge_operand_record_indices = ctx.collect_vec(
        group.members().iter().map(|member| member.value),
        "f3d Fillet edge operand indices",
    )?;
    let mut id = ctx.copy_retained_text(stream, "f3d Fillet group stream ID")?;

    ctx.append_formatted_retained(
        &mut id,
        format_args!("{SUFFIX}{}", group.record_index),
        "f3d Fillet group ID suffix",
    )?;

    ctx.reserve_vec(out, 1, "f3d Fillet group output")?;
    out.push(DesignFilletRadiusGroup {
        id,
        scope_record_index: group.scope_record_index,
        group_ordinal,
        group_record_index: group.record_index,
        edge_operand_record_indices,
        law,
        tangency_weight_parameter_record_index,
    });
    Ok(())
}

/// Parameter source kinds that a Fillet radius law reads, in the order of
/// `FilletParameters::by_kind`.
const FILLET_PARAMETER_KINDS: [&[u8]; 9] = [
    b"Radius",
    b"TangencyWeight",
    b"ChordLen",
    b"EdgeOffset1",
    b"EdgeOffset2",
    b"StartRadius",
    b"EndRadius",
    b"MidRadius",
    b"MidParams",
];

/// The parameters owned by one Fillet scope, in owner ordinal order, and
/// the same parameters partitioned by source kind. The lists are held in
/// scoped storage for as long as they live.
struct FilletParameters<'a, 'c> {
    count: usize,
    by_kind: [Vec<&'a DesignParameter>; 9],
    _storage: ScopedReservation<'c>,
}

impl<'a> FilletParameters<'a, '_> {
    fn of_kind(&self, kind: usize) -> &[&'a DesignParameter] {
        self.by_kind.get(kind).map_or(&[], Vec::as_slice)
    }

    fn record_indices_of_kind(&self, kind: usize) -> impl Iterator<Item = u32> + '_ {
        self.of_kind(kind)
            .iter()
            .map(|parameter| parameter.record_index)
    }
}

fn fillet_parameters<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    owners: &[&DesignParameterOwner],
    parameters: &RecordIndex<'a, '_, DesignParameter>,
    stream: &str,
) -> Result<FilletParameters<'a, 'c>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d Fillet owned parameters")?;
    let mut owned = Vec::new();
    for owner in ctx.admit_iter(owners, "scan F3D Fillet owners")? {
        if let Some(parameter) = parameters.get(ctx, stream, owner.parameter_record_index())? {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut owned,
                    (owner.local_ordinal(), parameter),
                    "f3d Fillet owned parameters",
                )
            })?;
        }
    }
    ctx.stable_sort_by_key(
        &mut owned[..],
        |(ordinal, _)| *ordinal,
        Ord::cmp,
        "sort F3D Fillet owned parameters",
    )?;
    let mut by_kind: [Vec<&DesignParameter>; 9] = Default::default();
    for (_, parameter) in ctx.admit_iter(&owned, "classify F3D Fillet parameters")? {
        let source_kind = parameter.source_kind().as_bytes();
        for (kind, list) in FILLET_PARAMETER_KINDS.iter().zip(&mut by_kind) {
            if ctx.equal_bytes(source_kind, kind, "match F3D Fillet parameter kind")? {
                storage.with_storage(|| {
                    ctx.push_vec(list, *parameter, "f3d Fillet parameters by kind")
                })?;
                break;
            }
        }
    }
    Ok(FilletParameters {
        count: owned.len(),
        by_kind,
        _storage: storage,
    })
}

pub(crate) fn decode_fillet_radius_groups(
    ctx: &DecodeContext<'_>,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Result<Vec<DesignFilletRadiusGroup>, CodecError> {
    const RADIUS: usize = 0;
    const TANGENCY_WEIGHT: usize = 1;
    const CHORD_LENGTH: usize = 2;
    const EDGE_OFFSET_ONE: usize = 3;
    const EDGE_OFFSET_TWO: usize = 4;
    const START_RADIUS: usize = 5;
    const END_RADIUS: usize = 6;
    const MIDDLE_RADIUS: usize = 7;
    const MIDDLE_PARAMETER: usize = 8;

    let parameter_index = RecordIndex::build(
        ctx,
        parameters,
        |parameter| (parameter.id.as_str(), parameter.record_index),
        "index F3D Fillet parameters",
        "f3d Fillet parameter index",
    )?;
    let scope_groups_index = groups_by_scope(ctx, groups)?;
    let scope_owners_index = RecordGroups::build(
        ctx,
        owners,
        |owner| (owner.id(), owner.scope_record_index()),
        "index F3D Fillet owners by scope",
    )?;
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D Fillet scopes")? {
        if scope_family(scope) != Some(DesignFeatureFamily::Fillet) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let (mut scope_groups, _scope_groups_storage) =
            ctx.with_scoped_storage("f3d Fillet scope groups", || {
                ctx.copy_slice(
                    scope_groups_index.get(ctx, stream, scope.record_index)?,
                    "f3d Fillet scope groups",
                )
            })?;
        ctx.stable_sort_by_key(
            &mut scope_groups[..],
            |group| group.scope_reference_ordinal,
            Ord::cmp,
            "sort F3D Fillet scope groups",
        )?;
        let owned = fillet_parameters(
            ctx,
            scope_owners_index.get(ctx, stream, scope.record_index)?,
            &parameter_index,
            stream,
        )?;
        let radii = owned.of_kind(RADIUS);
        let weights = owned.of_kind(TANGENCY_WEIGHT);
        if owned.count == radii.len() + weights.len()
            && scope_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == scope_groups.len())
        {
            let groups = ctx.admit_iter(&scope_groups, "pair F3D Fillet groups")?;
            for (ordinal, (group, radius)) in groups.copied().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                push_fillet_radius_group(
                    ctx,
                    &mut out,
                    stream,
                    group_ordinal,
                    group,
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    weights.get(ordinal).map(|parameter| parameter.record_index),
                )?;
            }
            continue;
        }
        let [group] = scope_groups.as_slice() else {
            continue;
        };
        let weight = weights.first().map(|parameter| parameter.record_index);
        // TangencyWeight is optional for the chordal law; older records carry
        // only the required ChordLen input.
        if (weights.is_empty() && owned.count == 1) || (weights.len() == 1 && owned.count == 2) {
            let [chord_length] = owned.of_kind(CHORD_LENGTH) else {
                continue;
            };
            push_fillet_radius_group(
                ctx,
                &mut out,
                stream,
                0,
                group,
                DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index: chord_length.record_index,
                },
                weight,
            )?;
            continue;
        }
        if owned.count == 3 {
            if let ([offset_one], [offset_two], [_]) = (
                owned.of_kind(EDGE_OFFSET_ONE),
                owned.of_kind(EDGE_OFFSET_TWO),
                weights,
            ) {
                push_fillet_radius_group(
                    ctx,
                    &mut out,
                    stream,
                    0,
                    group,
                    DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index: offset_one.record_index,
                        offset_two_parameter_record_index: offset_two.record_index,
                    },
                    weight,
                )?;
                continue;
            }
        }
        let ([start], [end]) = (owned.of_kind(START_RADIUS), owned.of_kind(END_RADIUS)) else {
            continue;
        };
        let middle_radii = owned.of_kind(MIDDLE_RADIUS);
        let middle_parameters = owned.of_kind(MIDDLE_PARAMETER);
        // TangencyWeight is optional for the variable-radius law. Older
        // records carry only the endpoint and midpoint radius parameters.
        let variable_parameter_count =
            2 + middle_radii.len() + middle_parameters.len() + weights.len();
        if middle_radii.len() != middle_parameters.len()
            || weights.len() > 1
            || owned.count != variable_parameter_count
        {
            continue;
        }
        let middle = ctx.collect_vec(
            owned
                .record_indices_of_kind(MIDDLE_RADIUS)
                .zip(owned.record_indices_of_kind(MIDDLE_PARAMETER))
                .map(|(radius_parameter_record_index, parameter_record_index)| {
                    crate::records::topology::fillet::DesignFilletMidpoint {
                        radius_parameter_record_index,
                        parameter_record_index,
                    }
                }),
            "f3d Fillet middle parameters",
        )?;
        push_fillet_radius_group(
            ctx,
            &mut out,
            stream,
            0,
            group,
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: start.record_index,
                end_radius_parameter_record_index: end.record_index,
                middle,
            },
            weight,
        )?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 14",
    )?;
    Ok(out)
}

/// Remove fixed Fillet interpretations of frames that are indexed parameter owners.
pub(crate) fn disambiguate_fixed_fillet_parameters(
    ctx: &DecodeContext<'_>,
    scopes: &mut [DesignParameterScope],
    owners: &[DesignParameterOwner],
) -> Result<(), CodecError> {
    let owners_by_scope = RecordGroups::build(
        ctx,
        owners,
        |owner| (owner.id(), owner.scope_record_index()),
        "index F3D fixed Fillet parameter owners",
    )?;
    for position in ctx.admit_iter(&(0..scopes.len()), "scan F3D fixed Fillet scopes")? {
        let Some(scope) = scopes.get(position) else {
            continue;
        };
        if !matches!(
            scope.payload(),
            DesignScopePayload::Fillet(Some(_))
                | DesignScopePayload::Conge(Some(_))
                | DesignScopePayload::Abrundung(Some(_))
                | DesignScopePayload::Arredondamento(Some(_))
        ) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        if owners_by_scope
            .get(ctx, stream, scope.record_index)?
            .is_empty()
        {
            continue;
        }
        if let Some(
            crate::records::feature::scope::DesignScopePayloadMut::Fillet(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Conge(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Abrundung(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Arredondamento(slot),
        ) = scopes
            .get_mut(position)
            .map(DesignParameterScope::payload_mut)
        {
            *slot = None;
        }
    }
    Ok(())
}

/// Outcome of reading a scope reference member as a construction-operand group.
pub(super) enum ConstructionOperandGroupParse {
    /// The record does not open a construction-operand group.
    NotAGroup,
    /// The record opens a group the grammar does not close.
    Unclosed,
    /// A complete group.
    Complete(Box<DesignConstructionOperandGroup>),
}

impl ConstructionOperandGroupParse {
    /// The group, where the record carried a complete one.
    #[cfg(test)]
    fn complete(self) -> Option<DesignConstructionOperandGroup> {
        match self {
            Self::Complete(group) => Some(*group),
            Self::NotAGroup | Self::Unclosed => None,
        }
    }
}

/// Extrude operand role before the ordered face groups are separated into
/// start and termination uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingExtrudeRole {
    BodiesA,
    BodiesB,
    Profile,
    Faces(DesignExtrudeFaceEncoding),
}

/// Interpret the role of a counted group owned by an Extrude scope.
///
/// The `0x12` face-group role is a legacy spelling of the one-sided-to-face
/// termination group. Class-296 two-sided-to-faces scopes use the same role
/// for their termination groups. It is also a valid Thicken role, so the
/// extent and exact-layout gates are part of this admission rule rather than
/// a global role alias.
fn extrude_operand_role(
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Option<PendingExtrudeRole> {
    if scope_family(scope) != Some(DesignFeatureFamily::Extrude) {
        return None;
    }
    match role {
        DesignOperandRole::BODIES_A => Some(PendingExtrudeRole::BodiesA),
        DesignOperandRole::BODIES_B => Some(PendingExtrudeRole::BodiesB),
        DesignOperandRole::PROFILE => Some(PendingExtrudeRole::Profile),
        DesignOperandRole::FACES => {
            Some(PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::Faces))
        }
        DesignOperandRole::ROLE_0X5
            if scope.extrude_prologue().map(DesignExtrudePrologue::start)
                == Some(DesignExtrudeStart::FromFace) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::SelectedStart,
            ))
        }
        DesignOperandRole::ROLE_0X12
            if scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent)
                == Some(DesignExtrudeExtent::OneSidedToFace) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::LegacyTermination,
            ))
        }
        DesignOperandRole::ROLE_0X12 if is_class_296_two_sided_to_faces_scope(scope) => Some(
            PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::LegacyTermination),
        ),
        _ => None,
    }
}

/// Indexed frame identity and stream position.
#[derive(Clone, Debug)]
pub(super) struct RecordFrame {
    pub(super) record_index: u32,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) byte_offset: u64,
}

/// Read the construction-operand group at `header`.
pub(super) fn parse_construction_operand_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &RecordFrame,
) -> Result<ConstructionOperandGroupParse, CodecError> {
    parse_construction_operand_group_at(
        ctx,
        bytes,
        scope,
        scope_reference_ordinal,
        (header.record_index, &header.class_tag, header.byte_offset),
    )
}

/// Read the construction-operand group whose indexed header carries the
/// record index, class tag and byte offset in `header`.
///
/// The record's members are a leading-block presence byte, the property block
/// its presence byte gates, the counted member run, two optional references,
/// the counted trailing-reference run, a zero u32 and the u32 role, ten zero bytes, an
/// ordinal, a duration, and a repeat of the ordinal that one container
/// generation omits. The class-328 Move form has one null and one present
/// auxiliary reference, a zero trailing count, and a retained null trailing
/// slot before the role. The ordinary tail is a reference to record `N + 2`,
/// a flag block, a reference to record `N + 1`, a zero byte, the owning scope's
/// reference, and the same-index paired header. The class-328 Move tail has a
/// leading zero, the flag block, an unmarked `N + 1` u64 reference with three
/// zero bytes, and the owning-scope reference. The flag block's last byte is
/// zero; one container generation prefixes it with a further byte. Neither
/// the repeated ordinal nor the prefix byte is announced, so the tail settles
/// both: exactly one of the four readings reaches a paired header carrying
/// this record's own index.
fn parse_construction_operand_group_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    (record_index, class_tag, byte_offset): (u32, &crate::records::references::DesignClassTag, u64),
) -> Result<ConstructionOperandGroupParse, CodecError> {
    use ConstructionOperandGroupParse::{Complete, NotAGroup, Unclosed};

    let Ok(start) = usize::try_from(byte_offset) else {
        return Ok(NotAGroup);
    };
    // The indexed header is the three-digit class tag, the u64 entity id whose
    // low word is the record index, and the record's own empty name.
    let Some(prologue_at) = start.checked_add(19) else {
        return Ok(NotAGroup);
    };
    if !zeros_at::<8>(bytes, start.saturating_add(11)) {
        return Ok(NotAGroup);
    }
    let Some(mut cursor) = payload_prologue(bytes, prologue_at, bytes.len()) else {
        return Ok(NotAGroup);
    };
    let member_count_at = cursor;
    let Some(member_count) = View::u32_le_at(bytes, cursor) else {
        return Ok(NotAGroup);
    };
    cursor += 4;
    let Some(members) = take_counted_record_references(
        ctx,
        bytes,
        &mut cursor,
        member_count,
        "f3d construction operand members",
    )?
    else {
        return Ok(NotAGroup);
    };
    let mut auxiliary_records = Vec::new();
    let mut auxiliary_reference_slots = [false; 2];
    for present in &mut auxiliary_reference_slots {
        if bytes.get(cursor) == Some(&0) {
            cursor += 1;
            continue;
        }
        *present = true;
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return Ok(NotAGroup);
        };
        ctx.push_vec(
            &mut auxiliary_records,
            crate::records::identity::Located {
                value: record_index,
                offset,
            },
            "f3d construction operand auxiliary record",
        )?;
    }
    let Some(trailing_count) = View::u32_le_at(bytes, cursor) else {
        return Ok(NotAGroup);
    };
    cursor += 4;
    let Some(trailing_records) = take_counted_record_references(
        ctx,
        bytes,
        &mut cursor,
        trailing_count,
        "f3d construction operand trailing records",
    )?
    else {
        return Ok(NotAGroup);
    };
    let is_move = matches!(scope.payload(), DesignScopePayload::Move(_));
    let legacy_move_class_328 = is_move
        && class_tag.code() == 328
        && auxiliary_reference_slots == [false, true]
        && record_index.checked_add(13).is_some_and(
            |expected| matches!(auxiliary_records.as_slice(), [record] if record.value == expected),
        )
        && trailing_count == 0;
    if legacy_move_class_328 {
        if bytes.get(cursor) != Some(&0) {
            return Ok(NotAGroup);
        }
        cursor += 1;
    }
    // The role occupies the high word of a u64 whose low word is zero.
    let role_at = cursor;
    let (Some(0), Some(role)) = (
        View::u32_le_at(bytes, role_at),
        View::u64_le_at(bytes, role_at),
    ) else {
        return Ok(NotAGroup);
    };
    let role = DesignOperandRole::from_raw(role);
    cursor += 8;
    if !zeros_at::<10>(bytes, cursor) {
        return Ok(NotAGroup);
    }
    cursor += 10;
    let opaque_index_at = cursor;
    let (Some(opaque_index), Some(opaque_scalar)) = (
        View::u32_le_at(bytes, cursor),
        View::f64_le_at(bytes, cursor + 4),
    ) else {
        return Ok(NotAGroup);
    };
    let Some(opaque_index) = std::num::NonZeroU32::new(opaque_index) else {
        return Ok(NotAGroup);
    };
    let Some(opaque_scalar) = cadmpeg_ir::scalar::FiniteReal::new(opaque_scalar) else {
        return Ok(NotAGroup);
    };
    cursor += 12;

    // Past this point the record has opened the group grammar, so a tail that
    // does not close is a group this reader cannot name.
    let mut closed = None;
    for repeats_ordinal in [true, false] {
        let mut tail = cursor;
        if repeats_ordinal {
            if View::u32_le_at(bytes, tail) != Some(opaque_index.get()) {
                continue;
            }
            tail += 4;
        }
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != record_index.checked_add(2)
        {
            continue;
        }
        for flag_bytes in [2usize, 3] {
            let Some(flags) = bytes.get(tail..tail + flag_bytes) else {
                continue;
            };
            if flags.last() != Some(&0) {
                continue;
            }
            // The wider block prefixes the narrower one, so the variant flag is
            // always the byte before the terminating zero.
            let variant = flags[flag_bytes - 2] != 0;
            let mut after = tail + flag_bytes;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != record_index.checked_add(1)
            {
                continue;
            }
            if bytes.get(after) != Some(&0) {
                continue;
            }
            after += 1;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != Some(scope.record_index)
            {
                continue;
            }
            let Some(paired) = indexed_record_header_at(bytes, after)
                .filter(|paired| paired.record_index == record_index)
            else {
                continue;
            };
            if closed.replace((variant, after, paired.class_tag)).is_some() {
                return Ok(Unclosed);
            }
        }
    }
    if let Some(legacy_tail) = legacy_body_group_tail(
        bytes,
        scope,
        (record_index, class_tag.code()),
        cursor,
        opaque_index.get(),
    ) {
        if closed.replace(legacy_tail).is_some() {
            return Ok(Unclosed);
        }
    }
    let Some((variant, paired_at, paired_class_tag)) = closed else {
        return Ok(Unclosed);
    };

    // Face groups take their start/termination use from their ordered position
    // in the scope, which `assign_extrude_face_roles` resolves once every group
    // of the scope is decoded.
    let operand_role = match extrude_operand_role(scope, role) {
        Some(PendingExtrudeRole::BodiesA) => DesignConstructionOperandRole::ExtrudeBodiesA,
        Some(PendingExtrudeRole::BodiesB) => DesignConstructionOperandRole::ExtrudeBodiesB,
        Some(PendingExtrudeRole::Profile) => DesignConstructionOperandRole::ExtrudeProfile,
        Some(PendingExtrudeRole::Faces(_)) | None => DesignConstructionOperandRole::Other(role),
    };
    let (Ok(member_count_offset), Ok(role_offset), Ok(opaque_index_offset), Ok(paired_byte_offset)) = (
        u64::try_from(member_count_at),
        u64::try_from(role_at),
        u64::try_from(opaque_index_at),
        u64::try_from(paired_at),
    ) else {
        return Ok(Unclosed);
    };
    let Ok(opaque_scalar) = cadmpeg_ir::scalar::NonNegativeReal::try_from(opaque_scalar) else {
        return Ok(Unclosed);
    };
    let Ok(frame) = DesignConstructionOperandGroupFrame::from_parts(
        crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
            member_count_offset,
            auxiliary_records,
            auxiliary_paths: Vec::new(),
            trailing_records,
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index,
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: opaque_index_offset + 4,
            variant,
        },
    ) else {
        return Ok(Unclosed);
    };
    let paired_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *paired_class_tag,
        "copy F3D construction operand paired class tag",
    )?;
    let class_tag =
        class_tag.try_clone_for_decode(ctx, "copy F3D construction operand class tag")?;
    let Ok(group) = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: String::new(),
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset,
            class_tag,
            members,
            lost_edge_references: Vec::new(),
            frame,
            operand_role,
            role_offset,
            paired_class_tag,
            paired_byte_offset,
        },
    ) else {
        return Ok(Unclosed);
    };
    Ok(Complete(Box::new(group)))
}

/// Take `count` consecutive same-segment references at `cursor`. A reference
/// is at least one byte, so a count the remaining bytes cannot supply is no
/// run and reaches no allocator.
fn take_counted_record_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
    count: u32,
    operation: &'static str,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    let count = index_from_u32(count);
    if bytes
        .len()
        .checked_sub(*cursor)
        .is_none_or(|remaining| count > remaining)
    {
        return Ok(None);
    }
    let mut references = Vec::new();
    ctx.reserve_capacity(&mut references, count, operation)?;
    for _ in ctx.admit_iter(&(0..count), operation)? {
        let Some((value, offset)) = take_record_reference(bytes, cursor) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut references,
            crate::records::identity::Located { value, offset },
            operation,
        )?;
    }
    Ok(Some(references))
}

/// Read the legacy Move/RemoveBody tail whose two flag bytes have no
/// terminating zero. The class and feature gates keep this admission separate
/// from the terminated flag-block grammar used by other construction groups.
fn legacy_body_group_tail<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
    (record_index, class_code): (u32, u32),
    cursor: usize,
    opaque_index: u32,
) -> Option<(bool, usize, &'a [u8; 3])> {
    let is_move = matches!(scope.payload(), DesignScopePayload::Move(_));
    let body_scope = is_move || matches!(scope.payload(), DesignScopePayload::RemoveBody);
    let legacy_move_class_328 = is_move && class_code == 328;
    let (flag_pair, variant) = match class_code {
        257 | 323 | 338 if body_scope => ([1, 1], true),
        328 if is_move => ([1, 1], true),
        282 | 302 if body_scope => ([0, 1], false),
        _ => return None,
    };
    let mut tail = cursor;
    if View::u32_le_at(bytes, tail)? != opaque_index {
        return None;
    }
    tail += 4;
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
        != record_index.checked_add(2)
    {
        return None;
    }
    if legacy_move_class_328 {
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
        if bytes_at::<2>(bytes, tail) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if View::u64_le_at(bytes, tail)? != u64::from(record_index.checked_add(1)?)
            || !zeros_at::<3>(bytes, tail + 8)
        {
            return None;
        }
        tail += 11;
    } else {
        if bytes_at::<2>(bytes, tail) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != record_index.checked_add(1)
        {
            return None;
        }
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
    }
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index) != Some(scope.record_index) {
        return None;
    }
    let paired = indexed_record_header_at(bytes, tail)?;
    if (legacy_move_class_328 && paired.class_tag != b"263") || paired.record_index != record_index
    {
        return None;
    }
    Some((variant, tail, paired.class_tag))
}

/// Bind exact typed records selected by construction-group trailing runs.
pub(crate) fn bind_construction_operand_trailing_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    for position in ctx.admit_iter(&(0..groups.len()), "bind F3D construction trailing records")? {
        let Some(group) = groups.get_mut(position) else {
            continue;
        };
        group
            .frame
            .try_set_trailing_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(Vec::new())
            .map_err(CodecError::Malformed)?;
        let group = &*group;
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut trailing_transforms = Vec::new();
        let mut trailing_dual_transforms = Vec::new();
        let mut trailing_flags = Vec::new();
        for record in ctx.admit_iter(
            group.frame.trailing_records(),
            "scan F3D construction trailing records",
        )? {
            let Some(header) = headers.get(ctx, stream, record.value)? else {
                continue;
            };
            if let Some(transform) = parse_construction_operand_transform(ctx, bytes, header)? {
                ctx.push_vec(
                    &mut trailing_transforms,
                    transform,
                    "f3d construction operand trailing transforms",
                )?;
            } else if let Some(transform) =
                parse_construction_operand_dual_transform(ctx, bytes, header)?
            {
                ctx.push_vec(
                    &mut trailing_dual_transforms,
                    transform,
                    "f3d construction operand trailing dual transforms",
                )?;
            } else if let Some(flag) = parse_construction_operand_flag(ctx, bytes, header)? {
                ctx.push_vec(
                    &mut trailing_flags,
                    flag,
                    "f3d construction operand trailing flags",
                )?;
            }
        }
        let Some(group) = groups.get_mut(position) else {
            continue;
        };
        group
            .frame
            .try_set_trailing_transforms(trailing_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(trailing_dual_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(trailing_flags)
            .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

fn parse_construction_operand_flag(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Result<Option<crate::records::topology::construction::DesignConstructionOperandFlag>, CodecError>
{
    let Some((value, value_offset)) = (|| {
        let start = usize::try_from(header.byte_offset).ok()?;
        if !zeros_at::<10>(bytes, start.checked_add(11)?)
            || bytes.get(start.checked_add(21)?) != Some(&1)
            || bytes.get(start.checked_add(23)?) != Some(&0)
        {
            return None;
        }
        let value_at = start.checked_add(22)?;
        let value = match bytes.get(value_at)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some((value, u64::try_from(value_at).ok()?))
    })() else {
        return Ok(None);
    };
    Ok(Some(
        crate::records::topology::construction::DesignConstructionOperandFlag {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header
                .class_tag
                .try_clone_for_decode(ctx, "copy F3D construction operand flag class tag")?,
            value,
            value_offset,
        },
    ))
}

/// Bind exact persistent-entity path records selected by construction groups.
pub(crate) fn bind_construction_operand_paths(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    for position in ctx.admit_iter(&(0..groups.len()), "bind F3D construction auxiliary paths")? {
        let Some(group) = groups.get_mut(position) else {
            continue;
        };
        group
            .frame
            .try_set_auxiliary_paths(Vec::new())
            .map_err(CodecError::Malformed)?;
        let group = &*group;
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut auxiliary_paths = Vec::new();
        for record in ctx.admit_iter(
            &group.frame.auxiliary_records,
            "scan F3D construction auxiliary records",
        )? {
            let Some(header) = headers.get(ctx, stream, record.value)? else {
                continue;
            };
            if let Some(path) =
                parse_construction_operand_path(ctx, bytes, group.scope_record_index, header)?
            {
                ctx.push_vec(
                    &mut auxiliary_paths,
                    path,
                    "f3d construction operand auxiliary paths",
                )?;
            }
        }
        if let Some(group) = groups.get_mut(position) {
            group
                .frame
                .try_set_auxiliary_paths(auxiliary_paths)
                .map_err(CodecError::Malformed)?;
        }
    }
    Ok(())
}

fn parse_construction_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Result<Option<crate::records::topology::construction::DesignConstructionOperandPath>, CodecError>
{
    struct PathFrame<'a> {
        entity_ref: u64,
        entity_ref_offset: u64,
        placement: crate::records::topology::construction::DesignConstructionPathPlacement,
        scope_record_index: u32,
        scope_record_index_offset: u64,
        nested_record_index: u32,
        nested_record_index_offset: u64,
        following_record_index: u32,
        following_byte_offset: u64,
        following_class_tag: &'a [u8; 3],
    }
    let Some(frame) = (|| {
        let start = usize::try_from(header.byte_offset).ok()?;
        if !zeros_at::<10>(bytes, start.checked_add(11)?)
            || bytes.get(start.checked_add(21)?) != Some(&1)
        {
            return None;
        }
        let entity_ref_at = start.checked_add(22)?;
        let entity_ref = View::u64_le_at(bytes, entity_ref_at)?;
        let placement_at = start.checked_add(30)?;
        let (placement, mut cursor) = if bytes_at::<3>(bytes, placement_at)? == &[0; 3] {
            let transform = crate::design::decode::scopes::shared_frames::rigid_transform_at(
                bytes,
                start.checked_add(33)?,
            )?;
            if bytes.get(start.checked_add(161)?) != Some(&0) {
                return None;
            }
            (
                crate::records::topology::construction::DesignConstructionPathPlacement::Transform(
                    transform,
                ),
                start.checked_add(162)?,
            )
        } else {
            let variant = match bytes_at::<4>(bytes, placement_at)? {
                [0, 0, variant @ (0 | 1), 0] => *variant != 0,
                _ => return None,
            };
            (
                crate::records::topology::construction::DesignConstructionPathPlacement::Compact(
                    variant,
                ),
                start.checked_add(34)?,
            )
        };
        let (scope_record_index, scope_record_index_offset) =
            take_record_reference(bytes, &mut cursor)?;
        if scope_record_index != expected_scope_record_index {
            return None;
        }
        let (nested_record_index, nested_record_index_offset) =
            take_record_reference(bytes, &mut cursor)?;
        if nested_record_index != header.record_index.checked_add(2)?
            || !zeros_at::<6>(bytes, cursor)
        {
            return None;
        }
        let following_at = cursor.checked_add(6)?;
        let following = indexed_record_header_at(bytes, following_at)?;
        if following.record_index != header.record_index.checked_add(1)? {
            return None;
        }
        Some(PathFrame {
            entity_ref,
            entity_ref_offset: u64::try_from(entity_ref_at).ok()?,
            placement,
            scope_record_index,
            scope_record_index_offset,
            nested_record_index,
            nested_record_index_offset,
            following_record_index: following.record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: following.class_tag,
        })
    })() else {
        return Ok(None);
    };
    let following_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *frame.following_class_tag,
        "copy F3D construction path following class tag",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D construction path class tag")?;
    Ok(
        crate::records::topology::construction::DesignConstructionOperandPath::try_new(
            crate::records::topology::construction::DesignConstructionOperandPathDraft {
                record_index: header.record_index,
                byte_offset: header.byte_offset,
                class_tag,
                entity_ref: frame.entity_ref,
                entity_ref_offset: frame.entity_ref_offset,
                placement: frame.placement,
                scope_record_index: frame.scope_record_index,
                scope_record_index_offset: frame.scope_record_index_offset,
                nested_record_index: frame.nested_record_index,
                nested_record_index_offset: frame.nested_record_index_offset,
                following_record_index: frame.following_record_index,
                following_byte_offset: frame.following_byte_offset,
                following_class_tag,
            },
        )
        .ok(),
    )
}

/// Fixed fields of a construction-operand transform record.
struct ConstructionTransformFrame<'a> {
    transform: crate::records::sketch_placement::SketchPlacementMatrix,
    transform_offset: u64,
    following_record_index: u32,
    following_at: usize,
    following_class_tag: &'a [u8; 3],
}

/// Read the construction-operand transform record at `header` with fixed work.
fn construction_transform_frame<'a>(
    bytes: &'a [u8],
    header: &DesignRecordHeader,
) -> Option<ConstructionTransformFrame<'a>> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if !zeros_at::<11>(bytes, start.checked_add(11)?)
        || bytes_at::<2>(bytes, start.checked_add(150)?) != Some(&[1, 0])
    {
        return None;
    }
    let transform_at = start.checked_add(22)?;
    let transform =
        crate::design::decode::scopes::shared_frames::rigid_transform_at(bytes, transform_at)?;
    let following_at = start.checked_add(152)?;
    let following = indexed_record_header_at(bytes, following_at)?;
    if following.record_index != header.record_index.checked_add(1)? {
        return None;
    }
    Some(ConstructionTransformFrame {
        transform,
        transform_offset: u64::try_from(transform_at).ok()?,
        following_record_index: following.record_index,
        following_at,
        following_class_tag: following.class_tag,
    })
}

fn parse_construction_operand_transform(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Result<
    Option<crate::records::topology::construction::DesignConstructionOperandTransform>,
    CodecError,
> {
    let Some(frame) = construction_transform_frame(bytes, header) else {
        return Ok(None);
    };
    let Ok(following_byte_offset) = u64::try_from(frame.following_at) else {
        return Ok(None);
    };
    let following_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *frame.following_class_tag,
        "copy F3D construction transform following class tag",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D construction transform class tag")?;
    Ok(
        crate::records::topology::construction::DesignConstructionOperandTransform::try_new(
            crate::records::topology::construction::DesignConstructionOperandTransformDraft {
                record_index: header.record_index,
                byte_offset: header.byte_offset,
                class_tag,
                transform: frame.transform,
                transform_offset: frame.transform_offset,
                following_record_index: frame.following_record_index,
                following_byte_offset,
                following_class_tag,
            },
        )
        .ok(),
    )
}

fn parse_construction_operand_dual_transform(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Result<
    Option<crate::records::topology::construction::DesignConstructionOperandDualTransform>,
    CodecError,
> {
    use crate::design::decode::scopes::shared_frames::rigid_transform_at;
    let Some((first_transform, first_at, second_transform, second_at)) = (|| {
        let start = usize::try_from(header.byte_offset).ok()?;
        if !zeros_at::<10>(bytes, start.checked_add(11)?)
            || bytes.get(start.checked_add(277)?) != Some(&0)
        {
            return None;
        }
        let first_at = start.checked_add(21)?;
        let second_at = start.checked_add(149)?;
        Some((
            rigid_transform_at(bytes, first_at)?,
            u64::try_from(first_at).ok()?,
            rigid_transform_at(bytes, second_at)?,
            u64::try_from(second_at).ok()?,
        ))
    })() else {
        return Ok(None);
    };
    Ok(Some(
        crate::records::topology::construction::DesignConstructionOperandDualTransform {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header
                .class_tag
                .try_clone_for_decode(ctx, "copy F3D construction dual transform class tag")?,
            first_transform,
            first_transform_offset: first_at,
            second_transform,
            second_transform_offset: second_at,
        },
    ))
}

/// Take one reference naming a record of the same segment, advancing `at` past
/// every byte it owns. Returns the record index and the byte offset of the low
/// word of the target entity id.
fn take_record_reference(bytes: &[u8], at: &mut usize) -> Option<(u32, u64)> {
    let target_at = at.checked_add(1)?;
    let reference = take_reference(bytes, at)?;
    let (target, _) = reference.local()?;
    Some((u32::try_from(target).ok()?, u64::try_from(target_at).ok()?))
}

/// Decode the persistent identity frame named by each construction-operand group.
pub(crate) fn decode_construction_operand_identities(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandIdentity>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut seen_storage = ctx.reserve_scoped(0, "f3d construction identity seen key")?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for group in ctx.admit_iter(groups, "scan F3D construction identity groups")? {
        let Some(trailing_record_index) = group
            .frame
            .trailing_records()
            .first()
            .map(|record| record.value)
        else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let Some(wrapper_header) = headers.get(ctx, stream, trailing_record_index)? else {
            continue;
        };
        // An identity keeps the first group that names its wrapper.
        let key = (entry.name.as_str(), wrapper_header.byte_offset);
        if ctx.contains_hash_set(&seen, &key, "find F3D construction identity seen key")? {
            continue;
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(mut identity) =
            parse_construction_operand_identity(ctx, bytes, group, wrapper_header)?
        else {
            continue;
        };
        seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, key, "f3d construction identity seen key")
        })?;
        identity.id = design_record_id_charged(
            ctx,
            &entry.name,
            ":design-construction-operand-identity#",
            wrapper_header.byte_offset,
            "f3d construction operand identity ID",
        )?;
        ctx.push_vec(
            &mut out,
            identity,
            "f3d construction operand identity output",
        )?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 15",
    )?;
    Ok(out)
}

/// Lost-edge records with their native streams, ordered by stream and then by
/// record byte offset; records at one offset keep their input order. The list
/// is held in scoped storage for as long as it lives.
struct StreamLostEdges<'a, 'c> {
    edges: Vec<(&'a str, &'a LostEdgeReference)>,
    _storage: ScopedReservation<'c>,
}

impl<'a, 'c> StreamLostEdges<'a, 'c> {
    fn build(
        ctx: &'c DecodeContext<'_>,
        lost_edges: &'a [LostEdgeReference],
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "f3d lost-edge stream records")?;
        let mut edges = Vec::new();
        for edge in ctx.admit_iter(lost_edges, "scan F3D lost-edge records")? {
            let Some(stream) = record_stream(ctx, &edge.id)? else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_vec(&mut edges, (stream, edge), "f3d lost-edge stream records")
            })?;
        }
        ctx.stable_sort_by_key(
            &mut edges[..],
            |(stream, edge)| (*stream, edge.record_byte_offset()),
            Ord::cmp,
            "sort f3d design operands 16",
        )?;
        Ok(Self {
            edges,
            _storage: storage,
        })
    }

    /// The records of `stream` in byte order. The ordered list is bisected.
    fn of_stream(
        &self,
        ctx: &DecodeContext<'_>,
        stream: &str,
    ) -> Result<&[(&'a str, &'a LostEdgeReference)], CodecError> {
        const OPERATION: &str = "find F3D stream lost-edge records";
        let start = ctx.partition_point(
            &self.edges,
            |(candidate, _)| Ok(ctx.compare(*candidate, stream, OPERATION)?.is_lt()),
            OPERATION,
        )?;
        let end = ctx.partition_point(
            &self.edges,
            |(candidate, _)| Ok(ctx.compare(*candidate, stream, OPERATION)?.is_le()),
            OPERATION,
        )?;
        Ok(self.edges.get(start..end).unwrap_or(&[]))
    }
}

fn copy_lost_edge_run_ids(
    ctx: &DecodeContext<'_>,
    run: &[(&str, &LostEdgeReference)],
) -> Result<Vec<String>, CodecError> {
    let mut ids = Vec::new();
    ctx.reserve_capacity(&mut ids, run.len(), "f3d lost-edge run IDs")?;
    for (_, edge) in ctx.admit_iter(run, "copy F3D lost-edge run IDs")? {
        ctx.push_vec(
            &mut ids,
            ctx.copy_retained_text(&edge.id, "f3d lost-edge run ID text")?,
            "f3d lost-edge run IDs",
        )?;
    }
    Ok(ids)
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
pub(crate) fn bind_lost_edge_groups(
    ctx: &DecodeContext<'_>,
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) -> Result<(), CodecError> {
    const IDENTITY_OPERATION: &str = "find F3D construction identities";
    const TERMINAL_OPERATION: &str = "scan F3D lost-edge terminals";
    let stream_edges = StreamLostEdges::build(ctx, lost_edges)?;
    for position in ctx.admit_iter(&(0..groups.len()), "bind F3D lost-edge groups")? {
        let Some(group) = groups.get_mut(position) else {
            continue;
        };
        group.lost_edge_references = Vec::new();
        let group = &*group;
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let is_group_identity = |identity: &DesignConstructionOperandIdentity| {
            Ok(identity.group_record_index == group.record_index
                && in_stream(ctx, &identity.id, stream)?)
        };
        let Some(identity_at) =
            ctx.position_by(identities, is_group_identity, IDENTITY_OPERATION)?
        else {
            continue;
        };
        let later = identities.get(identity_at + 1..).unwrap_or(&[]);
        if ctx.any_by(later, is_group_identity, IDENTITY_OPERATION)? {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion construction group {} has multiple identity chains",
                    group.record_index
                ),
            ));
        }
        let Some(wrapper) = identities
            .get(identity_at)
            .and_then(|identity| identity.wrappers().first())
        else {
            continue;
        };
        let wrapper_class_code = wrapper.class_tag.code();
        let edges = stream_edges.of_stream(ctx, stream)?;
        let is_terminal = |(_, edge): &(&str, &LostEdgeReference)| {
            Ok(edge.next_record_index == wrapper.record_index
                && edge.next_byte_offset() == wrapper.byte_offset
                && edge.next_class_tag.code() == wrapper_class_code)
        };
        let Some(terminal) = ctx.position_by(edges, is_terminal, TERMINAL_OPERATION)? else {
            continue;
        };
        let later = edges.get(terminal + 1..).unwrap_or(&[]);
        if ctx.any_by(later, is_terminal, TERMINAL_OPERATION)? {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion construction group {} has multiple terminating lost-edge runs",
                    group.record_index
                ),
            ));
        }
        let (Some(preceding), Some((_, mut current))) =
            (edges.get(..terminal), edges.get(terminal))
        else {
            continue;
        };
        // The run extends backward while each record names its successor.
        let start = ctx
            .rposition_by(
                preceding,
                |(_, previous)| {
                    let chained = previous.next_byte_offset() == current.record_byte_offset()
                        && previous.next_record_index == current.record_index
                        && previous.next_class_tag.code() == current.class_tag.code();
                    current = previous;
                    Ok(!chained)
                },
                "scan F3D lost-edge run",
            )?
            .map_or(0, |broken| broken + 1);
        let Some(run) = edges.get(start..=terminal) else {
            continue;
        };
        if run.len() != group.members().len() {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                "Fusion construction group {} has {} operands but its lost-edge run has {} records",
                group.record_index,
                group.members().len(),
                run.len()
            ),
            ));
        }
        let ids = copy_lost_edge_run_ids(ctx, run)?;
        if let Some(group) = groups.get_mut(position) {
            group.lost_edge_references = ids;
        }
    }
    Ok(())
}

fn parse_construction_operand_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
) -> Result<Option<DesignConstructionOperandIdentity>, CodecError> {
    let Ok(mut current_at) = usize::try_from(wrapper_header.byte_offset) else {
        return Ok(None);
    };
    let Ok(wrapper_class_tag) = <&[u8; 3]>::try_from(wrapper_header.class_tag.as_str().as_bytes())
    else {
        return Ok(None);
    };
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_class_tag;
    let mut chain_started = false;
    if let Some(transform) = construction_transform_frame(bytes, wrapper_header) {
        current_at = transform.following_at;
        current_record_index = transform.following_record_index;
        current_class_tag = transform.following_class_tag;
        chain_started = true;
    }
    // Each wrapper is 24 bytes followed by the next indexed header, so the
    // chain advances through the stream and ends within it.
    let mut wrappers = Vec::new();
    loop {
        if construction_tracking_path_frame(bytes, current_at, current_record_index).is_some() {
            break;
        }
        let Some(prelude) = bytes_at::<10>(bytes, current_at.saturating_add(11)) else {
            return Ok(None);
        };
        if prelude != &[0; 10] {
            break;
        }
        let Some(marker) = bytes_at::<3>(bytes, current_at.saturating_add(21)) else {
            return Ok(None);
        };
        if marker != &[1, 1, 0] {
            break;
        }
        let Ok(byte_offset) = u64::try_from(current_at) else {
            return Ok(None);
        };
        let class_tag = crate::design::decode::text::retain_class_tag(
            ctx,
            *current_class_tag,
            "copy F3D construction identity wrapper class tag",
        )?;
        ctx.push_vec(
            &mut wrappers,
            crate::records::topology::construction::DesignIdentityWrapper {
                record_index: current_record_index,
                byte_offset,
                class_tag,
            },
            "f3d construction identity wrappers",
        )?;
        let Some(next_at) = current_at.checked_add(24) else {
            return Ok(None);
        };
        let Some(next) = indexed_record_header_at(bytes, next_at) else {
            return Ok(None);
        };
        current_at = next_at;
        current_record_index = next.record_index;
        current_class_tag = next.class_tag;
        chain_started = true;
    }
    let mut tracking_path = None;
    if let Some(frame) = construction_tracking_path_frame(bytes, current_at, current_record_index) {
        if let Some(path) = construction_tracking_path(ctx, &frame, *current_class_tag)? {
            current_at = frame.following_at;
            current_record_index = frame.following_record_index;
            current_class_tag = frame.following_class_tag;
            chain_started = true;
            tracking_path = Some(path);
        }
    }
    if !chain_started {
        return Ok(None);
    }
    let persistent_identity = match parse_extrude_identity_member(ctx, bytes, current_at)? {
        Some(member) => {
            let (Ok(asset_id), Ok(context_id)) =
                (member.asset_id.try_into(), member.context_id.try_into())
            else {
                return Ok(None);
            };
            DesignConstructionPersistentIdentity::try_new(
                crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                    local_id: member.local_id,
                    local_id_offset: member.local_id_offset,
                    asset_id,
                    asset_id_offset: member.asset_id_offset,
                    context_id,
                    context_id_offset: member.context_id_offset,
                    tail_slot_present: member.tail_slot_present,
                    tail_slot_offset: member.tail_slot_offset,
                    next_record_index: member.next_record_index,
                    next_byte_offset: member.next_byte_offset,
                },
            )
            .ok()
        }
        None => None,
    };
    let Ok(following_byte_offset) = u64::try_from(current_at) else {
        return Ok(None);
    };
    let following_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *current_class_tag,
        "copy F3D construction identity following class tag",
    )?;
    Ok(DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: String::new(),
            group_record_index: group.record_index,
            wrappers,
            following_record_index: current_record_index,
            following_byte_offset,
            following_class_tag,
            tracking_path,
            persistent_identity,
        },
    )
    .ok())
}

/// Fixed fields of a tracking-path wrapper and its carrier record.
struct TrackingPathFrame<'a> {
    wrapper_record_index: u32,
    wrapper_at: usize,
    carrier_record_index: u32,
    carrier_at: usize,
    carrier_class_tag: &'a [u8; 3],
    primary_identity: u64,
    selector: i32,
    kind: u32,
    first_related_identity: Option<crate::records::identity::Located<u64>>,
    second_related_identity: Option<crate::records::identity::Located<u64>>,
    following_record_index: u32,
    following_at: usize,
    following_class_tag: &'a [u8; 3],
}

/// Read the tracking-path wrapper at `wrapper_at` with fixed work.
fn construction_tracking_path_frame(
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
) -> Option<TrackingPathFrame<'_>> {
    if !zeros_at::<10>(bytes, wrapper_at.checked_add(11)?)
        || bytes.get(wrapper_at.checked_add(21)?) != Some(&1)
        || View::u64_le_at(bytes, wrapper_at.checked_add(22)?)?
            != u64::from(wrapper_record_index.checked_add(1)?)
        || !zeros_at::<3>(bytes, wrapper_at.checked_add(30)?)
    {
        return None;
    }
    let carrier_at = wrapper_at.checked_add(33)?;
    let carrier = indexed_record_header_at(bytes, carrier_at)?;
    let field = |relative: usize| carrier_at.checked_add(relative);
    if carrier.record_index != wrapper_record_index.checked_add(1)?
        || !zeros_at::<10>(bytes, field(11)?)
        || View::u32_le_at(bytes, field(21)?)? != 1
        || View::u32_le_at(bytes, field(25)?)? != 0
        || View::u32_le_at(bytes, field(29)?)? != 1
        || View::u32_le_at(bytes, field(33)?)? != 2
        || View::u64_le_at(bytes, field(45)?)? != 0
        || View::u32_le_at(bytes, field(53)?)? != 1
        || View::u64_le_at(bytes, field(65)?)? != 0
    {
        return None;
    }
    let primary_identity = View::u64_le_at(bytes, field(37)?)?;
    let selector = View::i32_le_at(bytes, field(57)?)?;
    let kind = View::u32_le_at(bytes, field(61)?)?;
    let mut cursor = field(73)?;
    let first_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let second_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let following = indexed_record_header_at(bytes, cursor)?;
    if following.record_index != carrier.record_index.checked_add(1)? {
        return None;
    }
    Some(TrackingPathFrame {
        wrapper_record_index,
        wrapper_at,
        carrier_record_index: carrier.record_index,
        carrier_at,
        carrier_class_tag: carrier.class_tag,
        primary_identity,
        selector,
        kind,
        first_related_identity,
        second_related_identity,
        following_record_index: following.record_index,
        following_at: cursor,
        following_class_tag: following.class_tag,
    })
}

/// Copy a tracking-path frame into its record. `wrapper_class_tag` is the
/// class tag of the wrapper header.
fn construction_tracking_path(
    ctx: &DecodeContext<'_>,
    frame: &TrackingPathFrame<'_>,
    wrapper_class_tag: [u8; 3],
) -> Result<Option<DesignConstructionTrackingPath>, CodecError> {
    let offset = |at: usize| u64::try_from(at).ok();
    let (
        Some(wrapper_byte_offset),
        Some(carrier_byte_offset),
        Some(primary_identity_offset),
        Some(selector_offset),
        Some(kind_offset),
        Some(following_byte_offset),
    ) = (
        offset(frame.wrapper_at),
        offset(frame.carrier_at),
        frame.carrier_at.checked_add(37).and_then(offset),
        frame.carrier_at.checked_add(57).and_then(offset),
        frame.carrier_at.checked_add(61).and_then(offset),
        offset(frame.following_at),
    )
    else {
        return Ok(None);
    };
    let copy = |tag, operation| crate::design::decode::text::retain_class_tag(ctx, tag, operation);
    Ok(DesignConstructionTrackingPath::try_new(
        crate::records::topology::construction::DesignConstructionTrackingPathDraft {
            wrapper_record_index: frame.wrapper_record_index,
            wrapper_byte_offset,
            wrapper_class_tag: copy(
                wrapper_class_tag,
                "copy F3D tracking path wrapper class tag",
            )?,
            carrier_record_index: frame.carrier_record_index,
            carrier_byte_offset,
            carrier_class_tag: copy(
                *frame.carrier_class_tag,
                "copy F3D tracking path carrier class tag",
            )?,
            primary_identity: frame.primary_identity,
            primary_identity_offset,
            selector: frame.selector,
            selector_offset,
            kind: frame.kind,
            kind_offset,
            first_related_identity: frame.first_related_identity,
            second_related_identity: frame.second_related_identity,
            following_record_index: frame.following_record_index,
            following_byte_offset,
            following_class_tag: copy(
                *frame.following_class_tag,
                "copy F3D tracking path following class tag",
            )?,
        },
    )
    .ok())
}

/// Read the tracking path whose wrapper header carries `wrapper_record_index`
/// and `wrapper_class_tag` at `wrapper_at`.
#[cfg(test)]
fn parse_construction_tracking_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
    wrapper_class_tag: &crate::records::references::DesignClassTag,
) -> Result<Option<DesignConstructionTrackingPath>, CodecError> {
    let (Some(frame), Ok(wrapper_class_tag)) = (
        construction_tracking_path_frame(bytes, wrapper_at, wrapper_record_index),
        <&[u8; 3]>::try_from(wrapper_class_tag.as_str().as_bytes()),
    ) else {
        return Ok(None);
    };
    construction_tracking_path(ctx, &frame, *wrapper_class_tag)
}

enum TrackingIdentityField {
    Invalid,
    Absent,
    Present(crate::records::identity::Located<u64>),
}

fn take_optional_tracking_identity(bytes: &[u8], cursor: &mut usize) -> TrackingIdentityField {
    match View::u32_le_at(bytes, *cursor) {
        Some(0) => {
            let Some(next) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            TrackingIdentityField::Absent
        }
        Some(1) => {
            let Some(value_at) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(value) = View::u64_le_at(bytes, value_at) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(next) = value_at.checked_add(8) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            let Ok(offset) = u64::try_from(value_at) else {
                return TrackingIdentityField::Invalid;
            };
            TrackingIdentityField::Present(crate::records::identity::Located { value, offset })
        }
        _ => TrackingIdentityField::Invalid,
    }
}

/// Fixed fields of an Extrude selection group's tail after its member run.
struct ExtrudeSelectionTail<'a> {
    opaque_index: u32,
    opaque_scalar: f64,
    variant: bool,
    paired_at: usize,
    paired_class_tag: &'a [u8; 3],
}

/// Read the selection-group tail at `position` with fixed work.
fn extrude_selection_tail(
    bytes: &[u8],
    position: usize,
    scope_record_index: u32,
    record_index: u32,
) -> Option<ExtrudeSelectionTail<'_>> {
    let field = |relative: usize| position.checked_add(relative);
    let opaque_index = View::u32_le_at(bytes, position)?;
    let opaque_scalar = View::f64_le_at(bytes, field(4)?)?;
    let variant = match bytes.get(field(28)?)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if View::u32_le_at(bytes, field(12)?)? != opaque_index
        || bytes.get(field(16)?) != Some(&1)
        || View::u32_le_at(bytes, field(17)?)? != record_index.checked_add(2)?
        || !zeros_at::<6>(bytes, field(21)?)
        || bytes.get(field(27)?) != Some(&1)
        || bytes.get(field(29)?) != Some(&0)
        || bytes.get(field(30)?) != Some(&1)
        || View::u32_le_at(bytes, field(31)?)? != record_index.checked_add(1)?
        || !zeros_at::<7>(bytes, field(35)?)
        || bytes.get(field(42)?) != Some(&1)
        || View::u32_le_at(bytes, field(43)?)? != scope_record_index
        || !zeros_at::<6>(bytes, field(47)?)
    {
        return None;
    }
    let paired_at = field(53)?;
    let paired = indexed_record_header_at(bytes, paired_at)?;
    if paired.record_index != record_index {
        return None;
    }
    Some(ExtrudeSelectionTail {
        opaque_index,
        opaque_scalar,
        variant,
        paired_at,
        paired_class_tag: paired.class_tag,
    })
}

fn parse_extrude_selection_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<Option<DesignExtrudeSelectionGroup>, CodecError> {
    const MEMBERS_OPERATION: &str = "parse F3D extrude selection members";
    const OFFSETS_OPERATION: &str = "parse F3D extrude selection member offsets";
    let Some((start, member_count, mut position)) = (|| {
        let start = usize::try_from(header.byte_offset).ok()?;
        if !zeros_at::<10>(bytes, start.checked_add(11)?)
            || bytes.get(start.checked_add(21)?) != Some(&1)
            || View::u32_le_at(bytes, start.checked_add(22)?)? != scope.record_index
            || !zeros_at::<6>(bytes, start.checked_add(26)?)
        {
            return None;
        }
        let member_count = usize::try_from(View::u32_le_at(bytes, start.checked_add(32)?)?).ok()?;
        let position = start.checked_add(36)?;
        // Each member consumes 11 bytes; a count the remaining bytes cannot
        // supply is corrupt and must not reach the allocator.
        if member_count == 0 || member_count > bytes.len().checked_sub(position)? / 11 {
            return None;
        }
        Some((start, member_count, position))
    })() else {
        return Ok(None);
    };
    let mut members = ctx.vector_storage(member_count, MEMBERS_OPERATION)?;
    let mut member_offsets = ctx.vector_storage(member_count, OFFSETS_OPERATION)?;
    for _ in ctx.admit_iter(&(0..member_count), MEMBERS_OPERATION)? {
        let (Some(member), Some(member_offset), Some(next)) = (
            position
                .checked_add(1)
                .and_then(|at| View::u32_le_at(bytes, at)),
            position
                .checked_add(1)
                .and_then(|at| u64::try_from(at).ok()),
            position.checked_add(11),
        ) else {
            return Ok(None);
        };
        if bytes.get(position) != Some(&1) || !zeros_at::<6>(bytes, position.saturating_add(5)) {
            return Ok(None);
        }
        ctx.push_vec(&mut members, member, MEMBERS_OPERATION)?;
        ctx.push_vec(&mut member_offsets, member_offset, OFFSETS_OPERATION)?;
        position = next;
    }
    let Some(tail) =
        extrude_selection_tail(bytes, position, scope.record_index, header.record_index)
    else {
        return Ok(None);
    };
    let (
        Ok(member_count_offset),
        Ok(opaque_index_offset),
        Some(opaque_scalar_offset),
        Ok(paired_byte_offset),
    ) = (
        u64::try_from(start + 32),
        u64::try_from(position),
        position
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
        u64::try_from(tail.paired_at),
    )
    else {
        return Ok(None);
    };
    let paired_class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        *tail.paired_class_tag,
        "copy F3D extrude selection paired class tag",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D extrude selection class tag")?;
    let wire = crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: class_tag.into(),
        member_count_offset,
        members,
        member_offsets,
        opaque_index: tail.opaque_index,
        opaque_index_offset,
        opaque_scalar: tail.opaque_scalar,
        opaque_scalar_offset,
        variant: tail.variant,
        paired_class_tag: paired_class_tag.into(),
        paired_byte_offset,
    };
    match DesignExtrudeSelectionGroup::from_wire_charged(ctx, wire) {
        Ok(group) => Ok(Some(group)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

/// Decode the fixed-width records named by Extrude selection groups.
pub(crate) fn decode_extrude_selection_members(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut out = Vec::new();
    for group in ctx.admit_iter(groups, "scan F3D extrude selection groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in ctx
            .admit_iter(group.members(), "scan F3D extrude selection references")?
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            if let Some(mut member) =
                parse_extrude_selection_member(ctx, bytes, group, ordinal, header)?
            {
                member.id = design_record_id_charged(
                    ctx,
                    &entry.name,
                    ":design-extrude-selection-member#",
                    header.byte_offset,
                    "f3d extrude selection member ID",
                )?;
                ctx.push_vec(&mut out, member, "f3d extrude selection member output")?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 17",
    )?;
    Ok(out)
}

/// Decode nested persistent-entity frames named by construction groups.
pub(crate) fn decode_entity_selection_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEntitySelectionOperand>, CodecError> {
    let headers = indexed_operand_headers(ctx, headers)?;
    let mut out = Vec::new();
    for group in ctx.admit_iter(groups, "scan F3D entity selection groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in ctx
            .admit_iter(group.members(), "scan F3D entity selection references")?
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(ctx, stream, record_index)? else {
                continue;
            };
            if let Some(mut operand) =
                parse_entity_selection_operand(ctx, bytes, group, ordinal, header)?
            {
                operand.id = design_record_id_charged(
                    ctx,
                    &entry.name,
                    ":design-entity-selection-operand#",
                    header.byte_offset,
                    "f3d entity selection operand ID",
                )?;
                ctx.push_vec(&mut out, operand, "f3d entity selection operand output")?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 18",
    )?;
    Ok(out)
}

pub(super) fn parse_entity_selection_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<Option<DesignEntitySelectionOperand>, CodecError> {
    let Some(frame) = parse_entity_selection_frame(
        ctx,
        bytes,
        header.record_index,
        header.byte_offset,
        header.class_tag.as_str(),
    )?
    else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id)) = (frame.asset_id.try_into(), frame.context_id.try_into())
    else {
        return Ok(None);
    };
    Ok(DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: String::new(),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: frame.record_index,
            byte_offset: frame.byte_offset,
            class_tag: frame.class_tag,
            asset_id,
            asset_id_offset: frame.asset_id_offset,
            context_id,
            context_id_offset: frame.context_id_offset,
            identity_record_index: frame.identity_record_index,
            identity_record_offset: frame.identity_record_offset,
            primary_identity: frame.primary_identity,
            primary_identity_offset: frame.primary_identity_offset,
            secondary: frame.secondary,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: frame.next_record_index,
            next_byte_offset: frame.next_byte_offset,
        },
    )
    .ok())
}

/// Persistent identity payload shared by entity-selection consumers that do
/// not belong to a construction-operand group.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::design::decode) struct EntitySelectionFrame {
    pub(super) record_index: u32,
    pub(super) byte_offset: u64,
    class_tag: crate::records::references::DesignClassTag,
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    pub(super) identity_record_index: u32,
    pub(super) identity_record_offset: u64,
    pub(super) primary_identity: u64,
    pub(super) primary_identity_offset: u64,
    pub(super) secondary: Option<
        crate::records::identity::DesignSecondaryIdentity<crate::records::identity::Located<u64>>,
    >,
    pub(super) next_record_index: u32,
    pub(super) next_byte_offset: u64,
}

pub(in crate::design::decode) struct EntitySelectionPrefix {
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    after_context_id: usize,
}

/// Copy the counted relaxed GUID at `at` that `relaxed_guid_end` accepted.
fn copy_relaxed_guid(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<String, CodecError> {
    lp_utf16_bounded_charged(ctx, bytes, at, 36..=38, "f3d Design UTF-16 text")?
        .map(|(text, _)| text)
        .ok_or_else(|| CodecError::malformed("validated F3D relaxed GUID is unreadable"))
}

/// The asset-identity length offset of a persistent selection prefix at
/// `start`. Persistent selections use a ten-byte prelude and a u32 presence
/// value. Face-recipe selections add two prelude bytes, encode presence as
/// one byte, and add three zero bytes before the first UTF-16 length.
fn entity_selection_asset_start(bytes: &[u8], start: usize, record_index: u32) -> Option<usize> {
    let at = |relative: usize| start.checked_add(relative);
    let nested_record_index = record_index.checked_add(3)?;
    if zeros_at::<10>(bytes, at(11)?)
        && bytes.get(at(coil_persist_sel::NESTED_SELECTION_MARKER)?) == Some(&1)
        && View::u32_le_at(bytes, at(coil_persist_sel::NESTED_RECORD_INDEX)?)?
            == nested_record_index
        && zeros_at::<6>(bytes, at(26)?)
        && View::u32_le_at(bytes, at(coil_persist_sel::ASSET_PRESENCE)?)? == 1
    {
        return at(coil_persist_sel::ASSET_UUID_LENGTH);
    }
    if zeros_at::<11>(bytes, at(11)?)
        && bytes.get(at(coil_modern_sel::NESTED_SELECTION_MARKER)?) == Some(&1)
        && View::u32_le_at(bytes, at(coil_modern_sel::NESTED_RECORD_INDEX)?)? == nested_record_index
        && zeros_at::<6>(bytes, at(coil_modern_sel::NESTED_RECORD_INDEX + 4)?)
        && View::u32_le_at(bytes, at(coil_modern_sel::ASSET_PRESENCE)?)? == 1
    {
        return at(coil_modern_sel::ASSET_UUID_LENGTH);
    }
    if zeros_at::<12>(bytes, at(11)?)
        && bytes.get(at(coil_face_sel::NESTED_SELECTION_MARKER)?) == Some(&1)
        && View::u32_le_at(bytes, at(coil_face_sel::NESTED_RECORD_INDEX)?)? == nested_record_index
        && zeros_at::<6>(bytes, at(28)?)
        && bytes.get(at(coil_face_sel::ASSET_PRESENCE)?) == Some(&1)
    {
        return at(coil_face_sel::ASSET_UUID_LENGTH);
    }
    None
}

pub(super) fn parse_entity_selection_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Result<Option<EntitySelectionPrefix>, CodecError> {
    let Some(asset_start) = entity_selection_asset_start(bytes, start, record_index) else {
        return Ok(None);
    };
    let Some(after_asset_id) = relaxed_guid_end(ctx, bytes, asset_start)? else {
        return Ok(None);
    };
    let Some(after_context_id) = relaxed_guid_end(ctx, bytes, after_asset_id)? else {
        return Ok(None);
    };
    let (Some(asset_id_offset), Some(context_id_offset), Some(trailer_at)) = (
        asset_start
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
        after_asset_id
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
        after_context_id.checked_add(4),
    ) else {
        return Ok(None);
    };
    if View::u32_le_at(bytes, after_context_id) != Some(2) || !zeros_at::<4>(bytes, trailer_at) {
        return Ok(None);
    }
    Ok(Some(EntitySelectionPrefix {
        asset_id: copy_relaxed_guid(ctx, bytes, asset_start)?,
        asset_id_offset,
        context_id: copy_relaxed_guid(ctx, bytes, after_asset_id)?,
        context_id_offset,
        after_context_id,
    }))
}

/// Match the selected curve identity carried by a nested entity-selection frame.
pub(in crate::design) fn entity_selection_matches_curve(
    operand: &DesignEntitySelectionOperand,
    curve: &SketchCurveIdentity,
) -> bool {
    operand.secondary().is_some_and(|secondary| {
        curve.primary_id.get() == secondary.identity.value
            && secondary
                .curve_identity
                .is_none_or(|identity| curve.secondary_id == identity.value)
    })
}

/// The five indexed headers after a persistent selection prefix: the paired
/// header, two nested headers and the identity record, which carry
/// `record_index` through `record_index + 3`, and the record after them. Each
/// header is the first one after its predecessor.
fn entity_selection_headers<'b>(
    ctx: &DecodeContext<'_>,
    bytes: &'b [u8],
    after_context_id: usize,
    record_index: u32,
) -> Result<Option<[IndexedRecordHeader<'b>; 5]>, CodecError> {
    let Some(mut position) = after_context_id.checked_add(8) else {
        return Ok(None);
    };
    let mut headers = [None; 5];
    for (delta, slot) in (0u32..).zip(headers.iter_mut()) {
        let Some(header) = next_indexed_record_header(ctx, bytes, position, |_| true)? else {
            return Ok(None);
        };
        if delta < 4 && Some(header.record_index) != record_index.checked_add(delta) {
            return Ok(None);
        }
        let Some(next) = header.offset.checked_add(indexed_header::LEN) else {
            return Ok(None);
        };
        position = next;
        *slot = Some(header);
    }
    let [Some(paired), Some(nested_one), Some(nested_two), Some(identity), Some(next)] = headers
    else {
        return Ok(None);
    };
    Ok(Some([paired, nested_one, nested_two, identity, next]))
}

/// Direct sketch-point identity carried by a `WorkPoint` input.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkPointSketchPointFrame {
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    identity_record_index: u32,
    identity_record_offset: u64,
    sketch_record_index: u32,
    sketch_record_index_offset: u64,
    point_persistent_id: u64,
    point_persistent_id_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Parse the direct sketch-point identity variant of a `WorkPoint` input.
///
/// The outer prefix is shared with persistent entity selections. Its identity
/// record is a separate four-record envelope: nine zero bytes, a one-byte
/// presence marker, two marked `u32` slots separated by zero `u32` values,
/// and the following point-data record.
fn parse_work_point_sketch_point_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
) -> Result<Option<WorkPointSketchPointFrame>, CodecError> {
    let Ok(start) = usize::try_from(byte_offset) else {
        return Ok(None);
    };
    let Some(prefix) = parse_entity_selection_prefix(ctx, bytes, start, record_index)? else {
        return Ok(None);
    };
    let Some([.., identity, next]) =
        entity_selection_headers(ctx, bytes, prefix.after_context_id, record_index)?
    else {
        return Ok(None);
    };
    let Some(fields) = (|| {
        let identity_at = identity.offset;
        let at = |relative: usize| identity_at.checked_add(relative);
        if next.record_index != record_index.checked_add(4)?
            || !zeros_at::<9>(bytes, at(indexed_header::LEN)?)
            || bytes.get(at(sketch_point_identity::PRESENCE)?) != Some(&1)
            || View::u32_le_at(bytes, at(sketch_point_identity::PRESENCE + 1)?)? != 0
            || View::u32_le_at(bytes, at(sketch_point_identity::SKETCH_RECORD_INDEX + 4)?)? != 0
            || at(sketch_point_identity::LEN)? != next.offset
        {
            return None;
        }
        let sketch_record_index_at = at(sketch_point_identity::SKETCH_RECORD_INDEX)?;
        let point_persistent_id_at = at(sketch_point_identity::POINT_PERSISTENT_ID)?;
        Some((
            record_index.checked_add(3)?,
            u64::try_from(identity_at).ok()?,
            View::u32_le_at(bytes, sketch_record_index_at)?,
            u64::try_from(sketch_record_index_at).ok()?,
            u64::from(View::u32_le_at(bytes, point_persistent_id_at)?),
            u64::try_from(point_persistent_id_at).ok()?,
            u64::try_from(next.offset).ok()?,
        ))
    })() else {
        return Ok(None);
    };
    let (
        identity_record_index,
        identity_record_offset,
        sketch_record_index,
        sketch_record_index_offset,
        point_persistent_id,
        point_persistent_id_offset,
        next_byte_offset,
    ) = fields;
    Ok(Some(WorkPointSketchPointFrame {
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index,
        identity_record_offset,
        sketch_record_index,
        sketch_record_index_offset,
        point_persistent_id,
        point_persistent_id_offset,
        next_record_index: next.record_index,
        next_byte_offset,
    }))
}

/// Primary and secondary identities of an entity-selection identity record.
type SelectionIdentities = (
    usize,
    u64,
    Option<
        crate::records::identity::DesignSecondaryIdentity<crate::records::identity::Located<u64>>,
    >,
);

/// Read the identity record at `identity_at`, which the header at `next_at`
/// follows, with fixed work. A class-338 selection whose identity record has
/// class 361 names a sketch curve.
fn entity_selection_identities(
    bytes: &[u8],
    sketch_curve: bool,
    identity_at: usize,
    (next_at, next_record_index): (usize, u32),
    record_index: u32,
) -> Option<SelectionIdentities> {
    use crate::records::identity::{DesignSecondaryIdentity, Located};
    let at = |relative: usize| identity_at.checked_add(relative);
    if sketch_curve
        && bytes_at::<9>(bytes, at(class_338_curve::ZERO_PREFIX)?)? == &[0; 9]
        && bytes.get(at(class_338_curve::PRESENCE)?) == Some(&1)
        && bytes_at::<12>(bytes, at(class_338_curve::PRESENCE + 1)?)? == &[0; 12]
        && View::u32_le_at(bytes, at(class_338_curve::OWNER_HIGH_ZERO)?)? == 0
        && View::u32_le_at(bytes, at(class_338_curve::CURVE_HIGH_ZERO)?)? == 0
        && at(class_338_curve::LEN)? == next_at
        && next_record_index == record_index.checked_add(4)?
    {
        let primary_identity_offset = at(class_338_curve::OWNER_RECORD_INDEX)?;
        let secondary_identity_offset = at(class_338_curve::CURVE_PERSISTENT_ID)?;
        return Some((
            primary_identity_offset,
            u64::from(View::u32_le_at(bytes, primary_identity_offset)?),
            Some(DesignSecondaryIdentity {
                identity: Located {
                    value: u64::from(View::u32_le_at(bytes, secondary_identity_offset)?),
                    offset: u64::try_from(secondary_identity_offset).ok()?,
                },
                curve_identity: None,
            }),
        ));
    }
    if bytes_at::<10>(bytes, at(11)?)? != &[0; 10] {
        return None;
    }
    if at(45)? == next_at && next_record_index == record_index.checked_add(4)? {
        let curve_secondary_identity_offset = at(21)?;
        let primary_identity_offset = at(29)?;
        let secondary_identity_offset = at(37)?;
        return Some((
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            Some(DesignSecondaryIdentity {
                identity: Located {
                    value: View::u64_le_at(bytes, secondary_identity_offset)?,
                    offset: u64::try_from(secondary_identity_offset).ok()?,
                },
                curve_identity: Some(Located {
                    value: View::u64_le_at(bytes, curve_secondary_identity_offset)?,
                    offset: u64::try_from(curve_secondary_identity_offset).ok()?,
                })
                .filter(|identity| identity.value != 0),
            }),
        ));
    }
    if at(29)? == next_at {
        let primary_identity_offset = at(21)?;
        return Some((
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            None,
        ));
    }
    None
}

/// Parse the nested persistent-entity frame without assigning group ownership.
pub(super) fn parse_entity_selection_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
) -> Result<Option<EntitySelectionFrame>, CodecError> {
    let Ok(start) = usize::try_from(byte_offset) else {
        return Ok(None);
    };
    let Some(prefix) = parse_entity_selection_prefix(ctx, bytes, start, record_index)? else {
        return Ok(None);
    };
    let Some([.., identity, next]) =
        entity_selection_headers(ctx, bytes, prefix.after_context_id, record_index)?
    else {
        return Ok(None);
    };
    let sketch_curve = identity.class_tag == b"361"
        && ctx.equal_bytes(
            class_tag.as_bytes(),
            b"338",
            "match F3D entity selection class tag",
        )?;
    let Some((primary_identity_offset, primary_identity, secondary)) = entity_selection_identities(
        bytes,
        sketch_curve,
        identity.offset,
        (next.offset, next.record_index),
        record_index,
    ) else {
        return Ok(None);
    };
    let (
        Some(identity_record_index),
        Ok(identity_record_offset),
        Ok(primary_identity_offset),
        Ok(next_byte_offset),
    ) = (
        record_index.checked_add(3),
        u64::try_from(identity.offset),
        u64::try_from(primary_identity_offset),
        u64::try_from(next.offset),
    )
    else {
        return Ok(None);
    };
    let Some(class_tag) = crate::design::decode::text::class_tag_from_view(ctx, class_tag)? else {
        return Ok(None);
    };
    Ok(Some(EntitySelectionFrame {
        record_index,
        byte_offset,
        class_tag,
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index,
        identity_record_offset,
        primary_identity,
        primary_identity_offset,
        secondary,
        next_record_index: next.record_index,
        next_byte_offset,
    }))
}

/// Records with their native streams, ordered by stream and then by a byte
/// offset; records at one offset keep their input order. The list is held in
/// scoped storage for as long as it lives.
struct StreamOrdered<'a, 'c, T> {
    entries: Vec<(&'a str, &'a T)>,
    _storage: ScopedReservation<'c>,
}

impl<'a, 'c, T> StreamOrdered<'a, 'c, T> {
    fn build(
        ctx: &'c DecodeContext<'_>,
        values: &'a [T],
        keep: impl Fn(&T) -> bool,
        identity: impl Fn(&'a T) -> &'a str,
        byte_offset: impl Fn(&T) -> u64,
        (scan_operation, operation): (&'static str, &'static str),
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut entries = Vec::new();
        for value in ctx.admit_iter(values, scan_operation)? {
            if !keep(value) {
                continue;
            }
            let Some(stream) = record_stream(ctx, identity(value))? else {
                continue;
            };
            storage.with_storage(|| ctx.push_vec(&mut entries, (stream, value), operation))?;
        }
        ctx.stable_sort_by_key(
            &mut entries[..],
            |(stream, value)| (*stream, byte_offset(value)),
            Ord::cmp,
            operation,
        )?;
        Ok(Self {
            entries,
            _storage: storage,
        })
    }

    /// The records of `stream` in byte order. The ordered list is bisected.
    fn of_stream(
        &self,
        ctx: &DecodeContext<'_>,
        stream: &str,
    ) -> Result<&[(&'a str, &'a T)], CodecError> {
        const OPERATION: &str = "find F3D stream records";
        let start = ctx.partition_point(
            &self.entries,
            |(candidate, _)| Ok(ctx.compare(*candidate, stream, OPERATION)?.is_lt()),
            OPERATION,
        )?;
        let end = ctx.partition_point(
            &self.entries,
            |(candidate, _)| Ok(ctx.compare(*candidate, stream, OPERATION)?.is_le()),
            OPERATION,
        )?;
        Ok(self.entries.get(start..end).unwrap_or(&[]))
    }
}

/// The position of the only value of `run` equal to `value`. The search
/// stops at a second match.
fn unique_reference_position(
    ctx: &DecodeContext<'_>,
    run: &ReferenceRun<u32>,
    value: u32,
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    fn unique<T>(
        ctx: &DecodeContext<'_>,
        values: &[T],
        matches: impl Fn(&T) -> bool,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        let Some(first) = ctx.position_by(values, |candidate| Ok(matches(candidate)), operation)?
        else {
            return Ok(None);
        };
        let later = values.get(first + 1..).unwrap_or(&[]);
        if ctx.any_by(later, |candidate| Ok(matches(candidate)), operation)? {
            return Ok(None);
        }
        Ok(Some(first))
    }
    match (run.unlocated_values(), run.located_rows()) {
        (Some(values), _) => unique(ctx, values, |candidate| *candidate == value, operation),
        (None, Some(rows)) => unique(ctx, rows, |row| row.value == value, operation),
        (None, None) => Ok(None),
    }
}

/// Decode whole-body construction operands that contain one persistent body recipe.
pub(crate) fn decode_body_recipe_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignBodyRecipeOperand>, CodecError> {
    const IDENTITY_OPERATION: &str = "count F3D body recipe operand identities";
    // A header shared by two records of one stream names neither.
    let mut header_storage = ctx.reserve_scoped(0, "f3d body recipe header index")?;
    let mut headers_by_identity = HashMap::<_, Option<&DesignRecordHeader>>::new();
    for header in ctx.admit_iter(headers, "index F3D body recipe headers")? {
        let Some(stream) = record_stream(ctx, &header.id)? else {
            continue;
        };
        let key = (stream, header.record_index);
        if let Some(existing) = ctx.get_mut_hash_map(
            &mut headers_by_identity,
            &key,
            "f3d body recipe header index",
        )? {
            *existing = None;
        } else {
            header_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut headers_by_identity,
                    key,
                    Some(header),
                    "f3d body recipe header index",
                )
            })?;
        }
    }
    let unique_header = |stream: &str, record_index: u32| {
        Ok::<_, CodecError>(
            ctx.get_hash_map(
                &headers_by_identity,
                &(stream, record_index),
                "find F3D body recipe header",
            )?
            .copied()
            .flatten(),
        )
    };
    let body_recipes = StreamOrdered::build(
        ctx,
        recipes,
        |recipe| recipe.kind == ConstructionRecipeKind::Body,
        |recipe| recipe.id.as_str(),
        |recipe| recipe.byte_offset,
        ("scan F3D body recipes", "f3d body recipe stream entries"),
    )?;
    let scopes_by_record = RecordGroups::build(
        ctx,
        scopes,
        |scope| (scope.id.as_str(), scope.record_index),
        "index F3D body recipe scopes",
    )?;
    // Candidates whose output identity is shared are all dropped.
    let mut candidate_storage = ctx.reserve_scoped(0, "f3d body recipe operand candidates")?;
    let mut candidates: Vec<((&str, u64), Option<DesignBodyRecipeOperand>)> = Vec::new();
    let mut identity_counts: HashMap<(&str, u64), u32> = HashMap::new();
    let mut add_candidate = |key, operand: DesignBodyRecipeOperand| -> Result<(), CodecError> {
        candidate_storage.with_storage(|| {
            if let Some(count) =
                ctx.get_mut_hash_map(&mut identity_counts, &key, IDENTITY_OPERATION)?
            {
                *count = count.saturating_add(1);
            } else {
                ctx.insert_hash_map(&mut identity_counts, key, 1, IDENTITY_OPERATION)?;
            }
            ctx.push_vec(
                &mut candidates,
                (key, Some(operand)),
                "f3d body recipe operand candidates",
            )
        })
    };
    let mut index_storage = ctx.reserve_scoped(0, "f3d operand stream index")?;
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for group in ctx.admit_iter(groups, "scan F3D body recipe groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        if ctx.any_by(
            scopes_by_record.get(ctx, stream, group.scope_record_index)?,
            |scope| Ok(matches!(scope.payload(), DesignScopePayload::Hole(_))),
            "find F3D body recipe scope",
        )? {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let stream_recipes = body_recipes.of_stream(ctx, stream)?;
        for (ordinal, record_index) in ctx
            .admit_iter(group.members(), "scan F3D body recipe group members")?
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = unique_header(stream, record_index)? else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(ctx, records, header, stream_recipes)?
            else {
                continue;
            };
            if let Some(operand) = parse_body_recipe_operand_with_index(
                ctx, bytes, records, group, ordinal, header, recipe,
            )? {
                add_candidate((entry.name.as_str(), header.byte_offset), operand)?;
            }
        }
    }
    for scope in ctx.admit_iter(scopes, "scan F3D body recipe scopes")? {
        let operation = scope.combine_operation();
        if operation.is_none() && !matches!(scope.payload(), DesignScopePayload::Hole(_)) {
            continue;
        }
        let Some(stream) = record_stream(ctx, &scope.id)? else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let cache = &mut record_offset_index;
        let records = index_storage
            .with_storage(move || cached_borrowed_record_offsets(ctx, cache, stream, bytes))?;
        let stream_recipes = body_recipes.of_stream(ctx, stream)?;
        let mut decode_record_index = |record_index: u32| -> Result<(), CodecError> {
            let Some(scope_reference_ordinal) = unique_reference_position(
                ctx,
                scope.reference_members(),
                record_index,
                "find F3D body recipe scope reference ordinal",
            )?
            .and_then(|ordinal| u32::try_from(ordinal).ok()) else {
                return Ok(());
            };
            if operation.is_some() && scope_reference_ordinal.is_multiple_of(2) {
                return Ok(());
            }
            let Some(header) = unique_header(stream, record_index)? else {
                return Ok(());
            };
            let Some(recipe) = unique_body_recipe_with_index(ctx, records, header, stream_recipes)?
            else {
                return Ok(());
            };
            let owner = DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            };
            if let Some(operand) = parse_body_recipe_operand_frame_with_index(
                ctx,
                bytes,
                records,
                scope.record_index,
                owner,
                header,
                recipe,
            )? {
                add_candidate((entry.name.as_str(), header.byte_offset), operand)?;
            }
            Ok(())
        };
        if let Some(combine_operation) = operation {
            decode_record_index(combine_operation.target_record_index)?;
            decode_record_index(combine_operation.tools.first.record_index)?;
            for tool in ctx.admit_iter(
                &combine_operation.tools.additional,
                "scan F3D combine tools",
            )? {
                decode_record_index(tool.record_index)?;
            }
        } else {
            for record_index in admit_reference_values(
                ctx,
                scope.reference_members(),
                "scan F3D body recipe scope references",
            )? {
                decode_record_index(*record_index)?;
            }
        }
    }
    let mut out = Vec::new();
    for position in ctx.admit_iter(&(0..candidates.len()), "keep F3D body recipe operands")? {
        let Some(((stream, offset), slot)) = candidates.get_mut(position) else {
            continue;
        };
        if ctx.get_hash_map(&identity_counts, &(*stream, *offset), IDENTITY_OPERATION)? != Some(&1)
        {
            continue;
        }
        let Some(operand) = slot.take() else {
            continue;
        };
        push_body_recipe_operand(ctx, &mut out, operand, stream, *offset)?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design operands 20",
    )?;
    Ok(out)
}

fn push_body_recipe_operand(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<DesignBodyRecipeOperand>,
    mut operand: DesignBodyRecipeOperand,
    stream: &str,
    offset: u64,
) -> Result<(), CodecError> {
    operand.id = design_record_id_charged(
        ctx,
        stream,
        ":design-body-recipe-operand#",
        offset,
        "f3d body recipe operand ID",
    )?;

    ctx.reserve_vec(out, 1, "f3d body recipe operand output")?;
    out.push(operand);
    Ok(())
}

/// Select the sole body recipe in the structural interval after the `N+3`
/// header and before the enclosing `N+4` header. The bounded Design stream,
/// rather than an arbitrary byte distance, limits the interval.
#[cfg(test)]
fn unique_body_recipe<'a>(
    bytes: &[u8],
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let recipes = recipes
        .iter()
        .map(|recipe| ("", *recipe))
        .collect::<Vec<_>>();
    crate::design::test_support::with_test_decode_context(|ctx| {
        unique_body_recipe_with_index(ctx, &records, header, &recipes)
    })
    .ok()
    .flatten()
}

/// `recipes` is in byte order, so the interval is bisected at both ends.
fn unique_body_recipe_with_index<'a>(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
    recipes: &[(&str, &'a ConstructionRecipe)],
) -> Result<Option<&'a ConstructionRecipe>, CodecError> {
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return Ok(None);
    };
    let Some(prologue_end) =
        body_recipe_prologue_end_with_index(ctx, records, start, header.record_index)?
    else {
        return Ok(None);
    };
    let Some(closing_index) = header.record_index.checked_add(4) else {
        return Ok(None);
    };
    let Some(next_at) = records.first_at_or_after(ctx, prologue_end, closing_index)? else {
        return Ok(None);
    };
    let lower = u64_from_index(prologue_end);
    let upper = u64_from_index(next_at);
    let first = ctx.partition_point(
        recipes,
        |(_, recipe)| Ok(recipe.byte_offset < lower),
        "find F3D body recipe interval",
    )?;
    let end = ctx.partition_point(
        recipes,
        |(_, recipe)| Ok(recipe.byte_offset < upper),
        "find F3D body recipe interval",
    )?;
    let Some([(_, recipe)]) = recipes.get(first..end) else {
        return Ok(None);
    };
    Ok(Some(recipe))
}

/// Offset past the four consecutively indexed records that open a body-recipe
/// operand, or `None` when the records after `start` do not carry
/// `record_index` through `record_index + 3` in order.
///
/// The prologue depends only on the operand header, so a caller weighing many
/// candidate recipes against one header resolves it once.
#[cfg(test)]
fn body_recipe_prologue_end(bytes: &[u8], start: usize, record_index: u32) -> Option<usize> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    crate::design::test_support::with_test_decode_context(|ctx| {
        body_recipe_prologue_end_with_index(ctx, &records, start, record_index)
    })
    .ok()
    .flatten()
}

fn body_recipe_prologue_end_with_index(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    start: usize,
    record_index: u32,
) -> Result<Option<usize>, CodecError> {
    let Some(position) = start.checked_add(11) else {
        return Ok(None);
    };
    Ok(records
        .consecutive_headers::<4>(ctx, position, record_index)?
        .and_then(|[.., last]| last.checked_add(11)))
}

/// Offset of the record carrying `record_index + 4` that closes a body-recipe
/// operand whose recipe sits at `recipe_at`. The recipe must follow the
/// prologue that ends at `prologue_end`.
#[cfg(test)]
fn body_recipe_operand_end(
    bytes: &[u8],
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<usize> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    crate::design::test_support::with_test_decode_context(|ctx| {
        body_recipe_operand_end_with_index(ctx, &records, prologue_end, record_index, recipe_at)
    })
    .ok()
    .flatten()
}

fn body_recipe_operand_end_with_index(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(closing_index) = record_index.checked_add(4) else {
        return Ok(None);
    };
    if recipe_at < prologue_end {
        return Ok(None);
    }
    records.first_at_or_after(ctx, recipe_at, closing_index)
}

#[cfg(test)]
fn parse_body_recipe_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Result<Option<DesignBodyRecipeOperand>, CodecError> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    parse_body_recipe_operand_with_index(
        ctx,
        bytes,
        &records,
        group,
        group_member_ordinal,
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_with_index(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Result<Option<DesignBodyRecipeOperand>, CodecError> {
    parse_body_recipe_operand_frame_with_index(
        ctx,
        bytes,
        records,
        group.scope_record_index,
        DesignOperandOwner::Group {
            group_record_index: group.record_index,
            group_member_ordinal,
        },
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_frame_with_index(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner: DesignOperandOwner,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Result<Option<DesignBodyRecipeOperand>, CodecError> {
    let (Ok(start), Ok(recipe_at)) = (
        usize::try_from(header.byte_offset),
        usize::try_from(recipe.byte_offset),
    ) else {
        return Ok(None);
    };
    let Some(prologue_end) =
        body_recipe_prologue_end_with_index(ctx, records, start, header.record_index)?
    else {
        return Ok(None);
    };
    let Some(next_at) = body_recipe_operand_end_with_index(
        ctx,
        records,
        prologue_end,
        header.record_index,
        recipe_at,
    )?
    else {
        return Ok(None);
    };
    // The legacy Combine form permits an empty persistent-reference table;
    // its marker then starts at the ordinary post-count cursor. The history
    // binder keeps that operand native because an empty identity cannot prove
    // a body selection.
    let Some((reference_count, mut cursor)) = (|| {
        let reference_count =
            usize::try_from(View::u32_le_at(bytes, start.checked_add(21)?)?).ok()?;
        if start >= recipe_at
            || recipe_at >= next_at
            || bytes_at::<10>(bytes, start.checked_add(11)?)? != &[0; 10]
        {
            return None;
        }
        let cursor = start.checked_add(25)?;
        // Each reference consumes 12 bytes; a count the remaining bytes cannot
        // supply is corrupt and must not reach the allocator.
        (reference_count <= bytes.len().checked_sub(cursor)? / 12)
            .then_some((reference_count, cursor))
    })() else {
        return Ok(None);
    };
    let mut references = Vec::new();
    ctx.reserve_capacity(
        &mut references,
        reference_count,
        "f3d body recipe references",
    )?;
    for _ in ctx.admit_iter(&(0..reference_count), "f3d body recipe references")? {
        let Some((reference, next)) = (|| {
            let form_at = cursor.checked_add(8)?;
            Some((
                DesignBodyRecipeReference {
                    design_reference: View::u64_le_at(bytes, cursor)?,
                    design_reference_offset: u64::try_from(cursor).ok()?,
                    form: View::u32_le_at(bytes, form_at)?,
                    form_offset: u64::try_from(form_at).ok()?,
                    candidate_faces: Vec::new(),
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: Vec::new(),
                },
                cursor.checked_add(12)?,
            ))
        })() else {
            return Ok(None);
        };
        ctx.push_vec(&mut references, reference, "f3d body recipe references")?;
        cursor = next;
    }
    let Some((nested_record_index, asset_id_at)) = (|| {
        if bytes.get(cursor) != Some(&1)
            || bytes_at::<2>(bytes, cursor.checked_add(9)?)? != &[0; 2]
            || View::u32_le_at(bytes, cursor.checked_add(11)?)? != 1
        {
            return None;
        }
        Some((
            View::u64_le_at(bytes, cursor.checked_add(1)?)?,
            cursor.checked_add(15)?,
        ))
    })() else {
        return Ok(None);
    };
    let Some(after_asset_id) = relaxed_guid_end(ctx, bytes, asset_id_at)? else {
        return Ok(None);
    };
    let Some(after_context_id) = relaxed_guid_end(ctx, bytes, after_asset_id)? else {
        return Ok(None);
    };
    let Some((selector_tail, selector_tail_at)) = (|| {
        let selector_tail_at = after_context_id.checked_add(4)?;
        let selector_tail = *bytes_at::<4>(bytes, selector_tail_at)?;
        let selector_tail_is_valid = match header.class_tag.code() {
            // The class-365 member is a four-byte generation-dependent value.
            // Retain it until DR-24 identifies its neutral meaning.
            365 => true,
            367 => selector_tail == [1, 0, 0, 0],
            _ => selector_tail == [0; 4],
        };
        (View::u32_le_at(bytes, after_context_id)? == 2
            && selector_tail_is_valid
            && nested_record_index == u64::from(header.record_index.checked_add(3)?))
        .then_some((selector_tail, selector_tail_at))
    })() else {
        return Ok(None);
    };
    let offset = |at: Option<usize>| at.and_then(|at| u64::try_from(at).ok());
    let (
        Some(asset_id_offset),
        Some(context_id_offset),
        Some(selector_tail_offset),
        Some(nested_record_index_offset),
        Some(next_record_index),
        Some(next_byte_offset),
    ) = (
        offset(asset_id_at.checked_add(4)),
        offset(after_asset_id.checked_add(4)),
        offset(Some(selector_tail_at)),
        offset(cursor.checked_add(1)),
        header.record_index.checked_add(4),
        offset(Some(next_at)),
    )
    else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id)) = (
        copy_relaxed_guid(ctx, bytes, asset_id_at)?.try_into(),
        copy_relaxed_guid(ctx, bytes, after_asset_id)?.try_into(),
    ) else {
        return Ok(None);
    };
    let recipe_id = ctx.copy_retained_text(&recipe.id, "f3d body recipe operand recipe ID")?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D body recipe operand class tag")?;
    Ok(DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: String::new(),
            scope_record_index,
            owner,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            asset_id,
            asset_id_offset,
            context_id,
            context_id_offset,
            selector_tail: Some(crate::records::identity::Located {
                value: selector_tail,
                offset: selector_tail_offset,
            }),
            references,
            nested_record_index,
            nested_record_index_offset,
            recipe_id,
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index,
            next_byte_offset,
        },
    )
    .ok())
}

/// Join body-recipe Design references to solved persistent face tags.
pub(crate) fn bind_body_recipe_operand_candidates(
    ctx: &DecodeContext<'_>,
    operands: &mut [DesignBodyRecipeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
    scopes: &[DesignParameterScope],
) -> Result<(), CodecError> {
    use cadmpeg_ir::attributes::AttributeTarget;
    const OPERATION: &str = "bind F3D body recipe candidates";

    // A recipe identity shared by two recipes names neither.
    let mut recipe_storage = ctx.reserve_scoped(0, "f3d body recipe candidate index")?;
    let mut recipes_by_id = HashMap::<_, Option<&ConstructionRecipe>>::new();
    for recipe in ctx.admit_iter(recipes, "index F3D body candidate recipes")? {
        let key = recipe.id.as_str();
        if let Some(existing) =
            ctx.get_mut_hash_map(&mut recipes_by_id, key, "f3d body recipe candidate index")?
        {
            *existing = None;
        } else {
            recipe_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut recipes_by_id,
                    key,
                    Some(recipe),
                    "f3d body recipe candidate index",
                )
            })?;
        }
    }
    let scopes_by_record = RecordGroups::build(
        ctx,
        scopes,
        |scope| (scope.id.as_str(), scope.record_index),
        "index F3D body candidate scopes",
    )?;
    for position in ctx.admit_iter(&(0..operands.len()), OPERATION)? {
        let Some(operand) = operands.get_mut(position) else {
            continue;
        };
        // The recipe selector is a persistent-tag selector for Combine's
        // form-three clauses. In the class-365 body-member grammar it names
        // the enclosing N+4 record instead, so each clause joins by its own
        // Design reference and the history pass performs the body proof.
        let owning_scopes = match record_stream(ctx, &operand.id)? {
            Some(stream) => scopes_by_record.get(ctx, stream, operand.scope_record_index)?,
            None => &[],
        };
        let form_three_uses_recipe_selector = match owning_scopes {
            [scope] => scope.combine_operation().is_some(),
            _ => true,
        };
        let tag_selector = ctx
            .get_hash_map(
                &recipes_by_id,
                operand.recipe_id.as_str(),
                "find F3D body candidate recipe",
            )?
            .copied()
            .flatten()
            .and_then(|recipe| recipe.design.as_ref()?.selector)
            .map(|selector| i64::from(selector.value));
        let (occurrence_tags, _occurrence_storage) =
            ctx.with_scoped_storage("f3d body recipe occurrence tags", || {
                let mut occurrence_tags = Vec::new();
                for tag in ctx.admit_iter(tags, "scan F3D body recipe tags")? {
                    if matches!(tag.target, AttributeTarget::Face(_))
                        && crate::ids::same_native_occurrence(&tag.id, &operand.id)
                    {
                        ctx.push_vec(&mut occurrence_tags, tag, "f3d body recipe occurrence tags")?;
                    }
                }
                Ok::<_, CodecError>(occurrence_tags)
            })?;
        let reference_count = operand.references().len();
        for (_, reference) in ctx
            .admit_iter(&(0..reference_count), OPERATION)?
            .zip(operand.reference_bindings_mut())
        {
            let mut faces = Vec::new();
            if let Ok(design_reference) = i64::try_from(reference.design_reference) {
                let uses_selector = reference.form == 3 && form_three_uses_recipe_selector;
                for tag in
                    ctx.admit_iter(&occurrence_tags, "scan F3D body recipe occurrence tags")?
                {
                    if uses_selector && tag_selector != Some(tag.selector) {
                        continue;
                    }
                    if !ctx.contains(
                        &tag.design_references,
                        &design_reference,
                        "find F3D body recipe Design reference",
                    )? {
                        continue;
                    }
                    if let AttributeTarget::Face(face) = &tag.target {
                        push_operand_face_candidate(ctx, &mut faces, face)?;
                    }
                }
                sort_and_dedup_faces(ctx, &mut faces)?;
            }
            *reference.candidate_faces = faces;
        }
    }
    Ok(())
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub(crate) fn bind_extrude_selection_geometry(
    ctx: &DecodeContext<'_>,
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> Result<(), CodecError> {
    const POINT_OPERATION: &str = "find F3D Extrude selection points";
    const CURVE_OPERATION: &str = "find F3D Extrude selection curves";
    let scopes_by_record = RecordGroups::build(
        ctx,
        scopes,
        |scope| (scope.id.as_str(), scope.record_index),
        "index F3D selected Sketch scopes",
    )?;
    let mut sketch_storage = ctx.reserve_scoped(0, "f3d selected Extrude sketch index")?;
    let mut selected_sketches = HashMap::new();
    for group in ctx.admit_iter(groups, "scan F3D Extrude selection groups")? {
        let Some(stream) = record_stream(ctx, &group.id)? else {
            continue;
        };
        let Some(profile) = scopes_by_record
            .get(ctx, stream, group.scope_record_index)?
            .first()
            .and_then(|scope| scope.extrude_profile())
        else {
            continue;
        };
        sketch_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut selected_sketches,
                (stream, group.record_index),
                profile.entity_id.suffix(),
                "f3d selected Extrude sketch index",
            )
        })?;
    }
    for position in ctx.admit_iter(&(0..members.len()), "bind F3D Extrude selection geometry")? {
        let Some(member) = members.get_mut(position) else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &member.id)? else {
            continue;
        };
        let Some(entity_suffix) = ctx
            .get_hash_map(
                &selected_sketches,
                &(stream, member.group_record_index),
                "find F3D selected Extrude sketch",
            )?
            .and_then(|suffix| u32::try_from(*suffix).ok())
        else {
            continue;
        };
        let local_id = member.local_id;
        let is_point = |point: &SketchPoint| -> Result<bool, CodecError> {
            Ok(point.owner_reference == Some(entity_suffix)
                && point.persistent_id() == Some(local_id)
                && in_stream(ctx, &point.id, stream)?)
        };
        let is_curve = |curve: &SketchCurveIdentity| -> Result<bool, CodecError> {
            Ok(curve.owner_reference == Some(entity_suffix)
                && (curve.primary_id.get() == local_id
                    || curve.secondary_id != 0 && curve.secondary_id == local_id)
                && in_stream(ctx, &curve.id, stream)?)
        };
        // The member resolves only when exactly one point or curve matches.
        let resolved = match ctx.position_by(points, is_point, POINT_OPERATION)? {
            Some(at) => {
                let later = points.get(at + 1..).unwrap_or(&[]);
                if ctx.any_by(later, is_point, POINT_OPERATION)?
                    || ctx.any_by(curves, is_curve, CURVE_OPERATION)?
                {
                    None
                } else {
                    points.get(at).map(|point| SketchRelationOperand::Point {
                        record_index: point.record_index,
                        persistent_id: point.persistent_id(),
                    })
                }
            }
            None => match ctx.position_by(curves, is_curve, CURVE_OPERATION)? {
                Some(at) => {
                    let later = curves.get(at + 1..).unwrap_or(&[]);
                    if ctx.any_by(later, is_curve, CURVE_OPERATION)? {
                        None
                    } else {
                        curves.get(at).map(|curve| SketchRelationOperand::Curve {
                            record_index: curve.record_index,
                            primary_id: curve.primary_id.get(),
                            secondary_id: curve.secondary_id,
                        })
                    }
                }
                None => None,
            },
        };
        if let Some(resolved) = resolved {
            member.resolved_geometry = Some(resolved);
        }
    }
    Ok(())
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub(crate) fn bind_extrude_selection_identities(
    ctx: &DecodeContext<'_>,
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) -> Result<(), CodecError> {
    const OPERATION: &str = "scan F3D Extrude identity records";
    let identities_by_following = RecordGroups::build(
        ctx,
        identities,
        |identity| (identity.id.as_str(), identity.following_record_index()),
        "index F3D Extrude identity records",
    )?;
    for position in ctx.admit_iter(&(0..members.len()), "bind F3D Extrude identities")? {
        let Some(member) = members.get_mut(position) else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &member.id)? else {
            continue;
        };
        let (mut matches, _matches_storage) =
            ctx.with_scoped_storage("f3d Extrude identity matches", || {
                let mut matches = Vec::new();
                for identity in ctx.admit_iter(
                    identities_by_following.get(ctx, stream, member.record_index())?,
                    OPERATION,
                )? {
                    if identity.following_byte_offset() != member.byte_offset() {
                        continue;
                    }
                    let Some(persistent) = identity.persistent_identity() else {
                        continue;
                    };
                    if persistent.local_id == member.local_id
                        && ctx.equal(&persistent.asset_id, &member.asset_id, OPERATION)?
                        && ctx.equal(&persistent.context_id, &member.context_id, OPERATION)?
                    {
                        ctx.push_vec(&mut matches, *identity, "f3d Extrude identity matches")?;
                    }
                }
                Ok::<_, CodecError>(matches)
            })?;
        ctx.stable_sort_by_key(
            &mut matches[..],
            |identity| {
                identity
                    .wrappers()
                    .first()
                    .map(|wrapper| wrapper.byte_offset)
            },
            Ord::cmp,
            "sort f3d design operands 22",
        )?;
        let mut ids = Vec::new();
        ctx.reserve_capacity(&mut ids, matches.len(), "f3d Extrude identity IDs")?;
        for identity in ctx.admit_iter(&matches, "copy F3D Extrude identity matches")? {
            ctx.push_vec(
                &mut ids,
                ctx.copy_retained_text(&identity.id, "f3d Extrude identity ID text")?,
                "f3d Extrude identity IDs",
            )?;
        }
        member.operand_identity_ids = ids;
    }
    Ok(())
}

fn parse_extrude_selection_member(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<Option<DesignExtrudeSelectionMember>, CodecError> {
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return Ok(None);
    };
    let Some(member) = parse_extrude_identity_member(ctx, bytes, start)? else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id)) = (member.asset_id.try_into(), member.context_id.try_into())
    else {
        return Ok(None);
    };
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D extrude selection member class tag")?;
    Ok(DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: String::new(),
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id,
            asset_id_offset: member.asset_id_offset,
            context_id,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        },
    )
    .ok())
}

struct ParsedExtrudeIdentityMember {
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    tail_slot_present: bool,
    tail_slot_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Fixed fields of an Extrude identity member after its two GUIDs.
struct ExtrudeIdentityTail {
    tail_slot_present: bool,
    tail_slot_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Read the identity-member tail after the context GUID that ends at
/// `after_context_id`, for a member whose fixed frame ends at `fixed_end`.
fn extrude_identity_tail(
    bytes: &[u8],
    after_context_id: usize,
    fixed_end: usize,
) -> Option<ExtrudeIdentityTail> {
    let tail_slot_offset = after_context_id.checked_add(4)?;
    let tail_slot_present = match bytes.get(tail_slot_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if View::u32_le_at(bytes, after_context_id)? != 2 {
        return None;
    }
    let (next_record_index, next_byte_offset) =
        if View::u32_le_at(bytes, tail_slot_offset.checked_add(1)?) == Some(0)
            && after_context_id.checked_add(9)? == fixed_end
        {
            if fixed_end == bytes.len() {
                (0, u64::try_from(fixed_end).ok()?)
            } else {
                let (_, after_next_tag) =
                    lp_ascii_filtered_view(bytes, fixed_end, 0..=2000, u8::is_ascii_graphic)?;
                (
                    View::u32_le_at(bytes, after_next_tag)?,
                    u64::try_from(fixed_end).ok()?,
                )
            }
        } else if bytes_at::<3>(bytes, tail_slot_offset.checked_add(1)?)? == &[0; 3] {
            let mut cursor = tail_slot_offset.checked_add(4)?;
            let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
            let next_at = cursor;
            let (_, after_next_tag) =
                lp_ascii_filtered_view(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
            if View::u32_le_at(bytes, after_next_tag)? != next_record_index {
                return None;
            }
            (next_record_index, u64::try_from(next_at).ok()?)
        } else {
            return None;
        };
    Some(ExtrudeIdentityTail {
        tail_slot_present,
        tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
        next_record_index,
        next_byte_offset,
    })
}

fn parse_extrude_identity_member(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<ParsedExtrudeIdentityMember>, CodecError> {
    let Some((local_id, asset_at, fixed_end, local_id_offset, asset_id_offset)) = (|| {
        if bytes_at::<10>(bytes, start.checked_add(extrude_member::ZERO_RUN_10)?)? != &[0; 10] {
            return None;
        }
        let local_id_at = start.checked_add(extrude_member::LOCAL_IDENTITY)?;
        Some((
            View::u64_le_at(bytes, local_id_at)?,
            start.checked_add(extrude_member::ASSET_UUID_LENGTH)?,
            start.checked_add(extrude_member::LEN)?,
            u64::try_from(local_id_at).ok()?,
            u64::try_from(start.checked_add(extrude_member::ASSET_UUID_UTF16)?).ok()?,
        ))
    })() else {
        return Ok(None);
    };
    let Some(after_asset_id) = relaxed_guid_end(ctx, bytes, asset_at)? else {
        return Ok(None);
    };
    let Some(after_context_id) = relaxed_guid_end(ctx, bytes, after_asset_id)? else {
        return Ok(None);
    };
    let (Some(tail), Some(context_id_offset)) = (
        extrude_identity_tail(bytes, after_context_id, fixed_end),
        after_asset_id
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
    ) else {
        return Ok(None);
    };
    Ok(Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset,
        asset_id: copy_relaxed_guid(ctx, bytes, asset_at)?,
        asset_id_offset,
        context_id: copy_relaxed_guid(ctx, bytes, after_asset_id)?,
        context_id_offset,
        tail_slot_present: tail.tail_slot_present,
        tail_slot_offset: tail.tail_slot_offset,
        next_record_index: tail.next_record_index,
        next_byte_offset: tail.next_byte_offset,
    }))
}

struct ParsedEdgeIdentityMember {
    layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout,
    local_id: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
}

fn parse_edge_identity_member(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<ParsedEdgeIdentityMember>, CodecError> {
    use crate::records::topology::edge_identity::DesignEdgeIdentityLayout;
    let Some((layout, local_id, asset_at)) = (|| {
        let prelude_at = start.checked_add(11)?;
        let layout = if zeros_at::<12>(bytes, prelude_at) {
            DesignEdgeIdentityLayout::Full
        } else if zeros_at::<11>(bytes, prelude_at) {
            DesignEdgeIdentityLayout::Compact
        } else if zeros_at::<10>(bytes, prelude_at) {
            DesignEdgeIdentityLayout::Shortest
        } else {
            return None;
        };
        let marker_at = start.checked_add(usize::try_from(layout.marker_offset()).ok()?)?;
        if bytes.get(marker_at) != Some(&1)
            || bytes_at::<6>(bytes, marker_at.checked_add(5)?)? != &[0; 6]
            || View::u32_le_at(bytes, marker_at.checked_add(11)?)? != 1
        {
            return None;
        }
        Some((
            u64::from(View::u32_le_at(bytes, marker_at.checked_add(1)?)?),
            marker_at.checked_add(15)?,
        ))
        .map(|(local_id, asset_at)| (layout, local_id, asset_at))
    })() else {
        return Ok(None);
    };
    let Some(after_asset_id) = relaxed_guid_end(ctx, bytes, asset_at)? else {
        return Ok(None);
    };
    if relaxed_guid_end(ctx, bytes, after_asset_id)?.is_none() {
        return Ok(None);
    }
    let (Some(asset_id_offset), Some(context_id_offset)) = (
        asset_at
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
        after_asset_id
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
    ) else {
        return Ok(None);
    };
    Ok(Some(ParsedEdgeIdentityMember {
        layout,
        local_id,
        asset_id: copy_relaxed_guid(ctx, bytes, asset_at)?,
        asset_id_offset,
        context_id: copy_relaxed_guid(ctx, bytes, after_asset_id)?,
        context_id_offset,
    }))
}

/// A counted UTF-16 decimal of at most 256 code units, with an optional
/// leading `+`.
fn utf16_decimal_u64(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !(1..=256).contains(&count) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    bytes.get(start..end)?;
    let mut value = 0u64;
    let mut digits = 0usize;
    for index in 0..count {
        let unit = View::u16_le_at(bytes, start.checked_add(index.checked_mul(2)?)?)?;
        if index == 0 && unit == u16::from(b'+') {
            continue;
        }
        let digit = u8::try_from(unit).ok()?.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
        digits += 1;
    }
    (digits > 0).then_some((value, end))
}

pub(in crate::design) fn parse_sketch_profile(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Result<Option<DesignSketchProfileOperand>, CodecError> {
    const ENTITY_OPERATION: &str = "find F3D sketch profile entity";
    let Some((start, asset_at)) = (|| {
        let start = usize::try_from(header.byte_offset).ok()?;
        let at = |relative: usize| start.checked_add(relative);
        if bytes_at::<10>(bytes, at(11)?)? != &[0; 10]
            || bytes.get(at(21)?) != Some(&1)
            || View::u32_le_at(bytes, at(22)?)? != header.record_index.checked_add(3)?
            || bytes_at::<6>(bytes, at(26)?)? != &[0; 6]
            || View::u32_le_at(bytes, at(32)?)? != 1
        {
            return None;
        }
        Some((start, at(36)?))
    })() else {
        return Ok(None);
    };
    let Some(after_asset_id) = relaxed_guid_end(ctx, bytes, asset_at)? else {
        return Ok(None);
    };
    let Some((entity_suffix, after_entity_suffix)) = utf16_decimal_u64(bytes, after_asset_id)
    else {
        return Ok(None);
    };
    let Some(paired) = next_indexed_record_header(ctx, bytes, start.saturating_add(11), |_| true)?
    else {
        return Ok(None);
    };
    let paired_at = paired.offset;
    let Some(tail_length) = paired_at.checked_sub(after_entity_suffix) else {
        return Ok(None);
    };
    if paired.record_index != header.record_index || !matches!(tail_length, 89 | 93 | 94) {
        return Ok(None);
    }
    if matches!(tail_length, 89 | 93) {
        let tail = after_entity_suffix;
        let (nested_two_at, nested_one_at, scope_at) = if tail_length == 89 {
            (53, 66, 78)
        } else {
            (57, 70, 82)
        };
        if bytes_at::<8>(bytes, tail) != Some(&[1, 0, 0, 0, 0, 0, 0, 0])
            || View::u32_le_at(bytes, tail + 8) != Some(1)
            || marked_record_reference(bytes, tail + nested_two_at)
                != header.record_index.checked_add(2)
            || bytes_at::<2>(bytes, tail + nested_one_at - 2) != Some(&[0; 2])
            || marked_record_reference(bytes, tail + nested_one_at)
                != header.record_index.checked_add(1)
            || bytes.get(tail + scope_at - 1) != Some(&0)
            || marked_record_reference(bytes, tail + scope_at).is_none()
            || View::u32_le_at(bytes, tail + 41) == Some(0)
            || (tail_length == 93
                && View::u32_le_at(bytes, tail + 41) != View::u32_le_at(bytes, tail + 53))
        {
            return Ok(None);
        }
    }
    let is_entity = |entity: &DesignEntityHeader| -> Result<bool, CodecError> {
        Ok(entity.in_sketch_module()
            && entity.entity_id.suffix() == entity_suffix
            && in_stream(ctx, &entity.id, stream)?)
    };
    let Some(entity_at) = ctx.position_by(entities, is_entity, ENTITY_OPERATION)? else {
        return Ok(None);
    };
    let later = entities.get(entity_at + 1..).unwrap_or(&[]);
    let (Some(entity), false) = (
        entities.get(entity_at),
        ctx.any_by(later, is_entity, ENTITY_OPERATION)?,
    ) else {
        return Ok(None);
    };
    let region_selection =
        parse_sketch_profile_region_selection(ctx, bytes, header.record_index, paired_at)?;
    let (Some(asset_id_offset), Some(entity_reference_offset), Ok(paired_byte_offset)) = (
        start.checked_add(40).and_then(|at| u64::try_from(at).ok()),
        after_asset_id
            .checked_add(4)
            .and_then(|at| u64::try_from(at).ok()),
        u64::try_from(paired_at),
    ) else {
        return Ok(None);
    };
    let Ok(asset_id) = copy_relaxed_guid(ctx, bytes, asset_at)?.try_into() else {
        return Ok(None);
    };
    let entity_id_text =
        ctx.copy_retained_text(entity.entity_id.as_str(), "f3d sketch profile entity ID")?;
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id_text) else {
        return Ok(None);
    };
    let paired_class_tag =
        paired.retain_class_tag(ctx, "copy F3D sketch profile paired class tag")?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D sketch profile class tag")?;
    Ok(DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            asset_id,
            asset_id_offset,
            entity_id,
            entity_reference_offset,
            region_selection,
            paired_class_tag,
            paired_byte_offset,
        },
    )
    .ok())
}

/// The first indexed header at or after `position` when it carries
/// `record_index`.
fn next_header_with_index<'b>(
    ctx: &DecodeContext<'_>,
    bytes: &'b [u8],
    position: usize,
    record_index: u32,
) -> Result<Option<IndexedRecordHeader<'b>>, CodecError> {
    Ok(next_indexed_record_header(ctx, bytes, position, |_| true)?
        .filter(|header| header.record_index == record_index))
}

fn parse_sketch_profile_region_selection(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    profile_record_index: u32,
    paired_at: usize,
) -> Result<Option<DesignSketchProfileRegionSelection>, CodecError> {
    const REGION_MARKER_LEN: usize = 1;
    const REGION_COUNT_LEN: usize = 4;
    const TERMINATOR_LEN: usize = 5;
    const REGIONS_OPERATION: &str = "f3d sketch profile regions";
    const MEMBERS_OPERATION: &str = "f3d sketch profile region members";

    let (
        Some(nested_one_search),
        Some(nested_one_index),
        Some(nested_two_index),
        Some(selection_record_index),
    ) = (
        paired_at.checked_add(indexed_header::LEN),
        profile_record_index.checked_add(1),
        profile_record_index.checked_add(2),
        profile_record_index.checked_add(3),
    )
    else {
        return Ok(None);
    };
    let Some(nested_one) = next_header_with_index(ctx, bytes, nested_one_search, nested_one_index)?
    else {
        return Ok(None);
    };
    let Some(nested_two) = next_header_with_index(
        ctx,
        bytes,
        nested_one.offset.saturating_add(indexed_header::LEN),
        nested_two_index,
    )?
    else {
        return Ok(None);
    };
    let Some(selection) = next_header_with_index(
        ctx,
        bytes,
        nested_two.offset.saturating_add(indexed_header::LEN),
        selection_record_index,
    )?
    else {
        return Ok(None);
    };
    let selection_at = selection.offset;
    let Some((region_count, mut cursor)) = (|| {
        let at = |relative: usize| selection_at.checked_add(relative);
        if bytes_at::<10>(bytes, at(region_selection::ZERO_RUN_10)?)? != &[0; 10]
            || marked_record_reference(bytes, at(region_selection::PROFILE_REFERENCE_MARKER)?)
                != Some(profile_record_index)
            || bytes_at::<6>(bytes, at(region_selection::ZERO_RUN_6)?)? != &[0; 6]
            || View::u32_le_at(bytes, at(region_selection::FORMAT_VERSION)?)? != 1
        {
            return None;
        }
        let region_count =
            usize::try_from(View::u32_le_at(bytes, at(region_selection::REGION_COUNT)?)?).ok()?;
        if region_count == 0 {
            return None;
        }
        let minimum_regions_len = region_count
            .checked_mul(REGION_COUNT_LEN.checked_add(region_member::LEN)?)?
            .checked_add(
                region_count
                    .checked_sub(1)?
                    .checked_mul(REGION_MARKER_LEN)?,
            )?
            .checked_add(TERMINATOR_LEN)?
            .checked_add(indexed_header::LEN)?;
        let cursor = at(region_selection::LEN)?;
        (cursor.checked_add(minimum_regions_len)? <= bytes.len()).then_some((region_count, cursor))
    })() else {
        return Ok(None);
    };

    let mut regions = Vec::new();
    ctx.reserve_capacity(&mut regions, region_count, REGIONS_OPERATION)?;
    for region_ordinal in ctx.admit_iter(&(0..region_count), REGIONS_OPERATION)? {
        let Some((member_count_offset, member_count)) = (|| {
            if region_ordinal != 0 {
                if bytes.get(cursor) != Some(&1) {
                    return None;
                }
                cursor = cursor.checked_add(1)?;
            }
            let member_count_offset = cursor;
            let member_count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
            cursor = cursor.checked_add(REGION_COUNT_LEN)?;
            if member_count == 0 {
                return None;
            }
            let remaining_regions = region_count.checked_sub(region_ordinal.checked_add(1)?)?;
            let trailing_minimum_len = remaining_regions
                .checked_mul(
                    REGION_MARKER_LEN
                        .checked_add(REGION_COUNT_LEN)?
                        .checked_add(region_member::LEN)?,
                )?
                .checked_add(TERMINATOR_LEN)?
                .checked_add(indexed_header::LEN)?;
            let member_run_len = member_count.checked_mul(region_member::LEN)?;
            (cursor
                .checked_add(member_run_len)?
                .checked_add(trailing_minimum_len)?
                <= bytes.len())
            .then_some((u64::try_from(member_count_offset).ok()?, member_count))
        })() else {
            return Ok(None);
        };

        let mut members = Vec::new();
        ctx.reserve_capacity(&mut members, member_count, MEMBERS_OPERATION)?;
        for _ in ctx.admit_iter(&(0..member_count), MEMBERS_OPERATION)? {
            let Some(member) = sketch_profile_region_member(bytes, cursor) else {
                return Ok(None);
            };
            ctx.push_vec(&mut members, member, MEMBERS_OPERATION)?;
            let Some(next) = cursor.checked_add(region_member::LEN) else {
                return Ok(None);
            };
            cursor = next;
        }
        ctx.push_vec(
            &mut regions,
            DesignSketchProfileRegion {
                member_count_offset,
                members,
            },
            REGIONS_OPERATION,
        )?;
    }
    let Some(companion_at) = cursor.checked_add(TERMINATOR_LEN) else {
        return Ok(None);
    };
    let Some(companion) = (|| {
        if bytes_at::<TERMINATOR_LEN>(bytes, cursor)? != &[0; TERMINATOR_LEN] {
            return None;
        }
        indexed_record_header_at(bytes, companion_at)
            .filter(|companion| companion.record_index == selection_record_index)
    })() else {
        return Ok(None);
    };
    let (Ok(byte_offset), Some(region_count_offset), Ok(companion_byte_offset)) = (
        u64::try_from(selection_at),
        selection_at
            .checked_add(region_selection::REGION_COUNT)
            .and_then(|at| u64::try_from(at).ok()),
        u64::try_from(companion_at),
    ) else {
        return Ok(None);
    };
    Ok(Some(DesignSketchProfileRegionSelection {
        record_index: selection_record_index,
        byte_offset,
        class_tag: selection
            .retain_class_tag(ctx, "copy F3D sketch profile region selection class tag")?,
        region_count_offset,
        regions,
        companion_class_tag: companion
            .retain_class_tag(ctx, "copy F3D sketch profile region companion class tag")?,
        companion_byte_offset,
    }))
}

/// Read the fixed-width sketch-profile region member at `at`.
fn sketch_profile_region_member(
    bytes: &[u8],
    at: usize,
) -> Option<DesignSketchProfileRegionMember> {
    use crate::records::topology::sketch_profile::DesignRegionIncidence;
    let kind = View::u32_le_at(bytes, at)?;
    let curve_primary_id_at = at.checked_add(region_member::CURVE_PRIMARY_ID)?;
    let curve_primary_id = std::num::NonZeroU32::new(View::u32_le_at(bytes, curve_primary_id_at)?)?;
    let incidence_words_offset = at.checked_add(region_member::ZERO_WORDS_3)?;
    let mut incidence_words = [0; 8];
    for (ordinal, word) in incidence_words.iter_mut().enumerate() {
        *word = View::u32_le_at(
            bytes,
            incidence_words_offset.checked_add(ordinal.checked_mul(4)?)?,
        )?;
    }
    if kind != 3
        || incidence_words[..3] != [0; 3]
        || !matches!(incidence_words[3], 0 | 1)
        || incidence_words[6..] != [0; 2]
    {
        return None;
    }
    Some(DesignSketchProfileRegionMember {
        kind_offset: u64::try_from(at).ok()?,
        curve_primary_id,
        curve_primary_id_offset: u64::try_from(curve_primary_id_at).ok()?,
        incidence_flag: incidence_words[3] == 1,
        incidence_values: [
            DesignRegionIncidence::try_from(incidence_words[4]).ok()?,
            DesignRegionIncidence::try_from(incidence_words[5]).ok()?,
        ],
        incidence_words_offset: u64::try_from(incidence_words_offset).ok()?,
    })
}

struct ParsedRecipeOperand {
    paired_byte_offset: u64,
    paired_class_tag: crate::records::references::DesignClassTag,
    recipe_record_index: u32,
    recipe_record_byte_offset: u64,
    recipe_id: String,
    recipe_prefix_offset: u64,
    recipe_prefix_bytes: Vec<u8>,
    recipe_references: Vec<crate::records::dimensions::DesignRecipeReference>,
    recipe_program_offset: u64,
    recipe_program: Vec<i32>,
    next_record_index: u32,
    next_byte_offset: u64,
}

#[derive(Clone, Copy)]
enum RecipeOperandTerminator {
    RecordDelta(u32),
    NextIndexedAfterRecipe { limit: u64 },
}

/// Parse one exact persistent vertex-recipe envelope.
fn parse_vertex_recipe(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignVertexRecipe>, CodecError> {
    let Some(parsed) = parse_recipe_operand(
        ctx,
        bytes,
        records,
        stream,
        header,
        recipes,
        (
            ConstructionRecipeKind::Vertex,
            RecipeOperandTerminator::RecordDelta(5),
        ),
    )?
    else {
        return Ok(None);
    };
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D vertex recipe class tag")?;
    Ok(DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes,
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            resolution: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok())
}

/// The only recipe of `stream` whose byte offset lies strictly between
/// `after` and `before` and whose kind `kind` accepts. The search stops at a
/// second match.
fn unique_stream_recipe<'r>(
    ctx: &DecodeContext<'_>,
    recipes: &'r [ConstructionRecipe],
    stream: &str,
    kind: impl Fn(ConstructionRecipeKind) -> bool,
    (after, before): (u64, u64),
) -> Result<Option<&'r ConstructionRecipe>, CodecError> {
    const OPERATION: &str = "find F3D operand recipe";
    let is_match = |recipe: &ConstructionRecipe| -> Result<bool, CodecError> {
        Ok(kind(recipe.kind)
            && recipe.byte_offset > after
            && recipe.byte_offset < before
            && in_stream(ctx, &recipe.id, stream)?)
    };
    let Some(at) = ctx.position_by(recipes, is_match, OPERATION)? else {
        return Ok(None);
    };
    let later = recipes.get(at + 1..).unwrap_or(&[]);
    if ctx.any_by(later, is_match, OPERATION)? {
        return Ok(None);
    }
    Ok(recipes.get(at))
}

/// The indexed headers at five offsets that indexed record searches found.
fn five_headers_at(bytes: &[u8], offsets: [usize; 5]) -> Option<[IndexedRecordHeader<'_>; 5]> {
    let [first, second, third, fourth, fifth] =
        offsets.map(|offset| indexed_record_header_at(bytes, offset));
    Some([first?, second?, third?, fourth?, fifth?])
}

/// Parse the indexed-record envelope shared by topology recipe operands.
fn parse_recipe_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    (recipe_kind, terminator): (ConstructionRecipeKind, RecipeOperandTerminator),
) -> Result<Option<ParsedRecipeOperand>, CodecError> {
    let Some(family_name_len) = crate::design::RECIPES
        .iter()
        .find_map(|(name, kind)| (*kind == recipe_kind).then_some(name.len()))
    else {
        return Ok(None);
    };
    let Some(prologue_search) = usize::try_from(header.byte_offset)
        .ok()
        .and_then(|start| start.checked_add(11))
    else {
        return Ok(None);
    };
    let Some(prologue) =
        records.consecutive_headers::<4>(ctx, prologue_search, header.record_index)?
    else {
        return Ok(None);
    };
    let Some(position) = prologue[3].checked_add(11) else {
        return Ok(None);
    };
    let Ok(recipe_record_byte_offset) = u64::try_from(prologue[3]) else {
        return Ok(None);
    };
    let next_at = match terminator {
        RecipeOperandTerminator::RecordDelta(delta) => {
            let Some(next_index) = header.record_index.checked_add(delta) else {
                return Ok(None);
            };
            records.first_at_or_after(ctx, position, next_index)?
        }
        RecipeOperandTerminator::NextIndexedAfterRecipe { limit } => {
            // The terminator is the first header after the earliest recipe
            // that follows the recipe record.
            let earliest = ctx.fold(
                recipes,
                None::<&ConstructionRecipe>,
                |earliest, recipe| {
                    let candidate = recipe.kind == recipe_kind
                        && recipe.byte_offset > recipe_record_byte_offset
                        && recipe.byte_offset < limit
                        && earliest
                            .is_none_or(|earliest| recipe.byte_offset < earliest.byte_offset)
                        && in_stream(ctx, &recipe.id, stream)?;
                    Ok(if candidate { Some(recipe) } else { earliest })
                },
                "find F3D earliest operand recipe",
            )?;
            let Some(program_at) = earliest
                .and_then(|recipe| usize::try_from(recipe.byte_offset).ok())
                .and_then(|at| at.checked_add(family_name_len))
            else {
                return Ok(None);
            };
            next_indexed_record_offset(ctx, bytes, program_at)?
                .filter(|next| u64::try_from(*next).is_ok_and(|next| next <= limit))
        }
    };
    let Some(next_at) = next_at else {
        return Ok(None);
    };
    let offsets = [prologue[0], prologue[1], prologue[2], prologue[3], next_at];
    let Some(indexed) = five_headers_at(bytes, offsets) else {
        return Ok(None);
    };
    let Some(recipe_record_index) = header.record_index.checked_add(3) else {
        return Ok(None);
    };
    let prefix_matches = (0u32..4)
        .zip(&indexed)
        .all(|(delta, found)| Some(found.record_index) == header.record_index.checked_add(delta));
    let next_record_index = indexed[4].record_index;
    if !prefix_matches
        || matches!(terminator, RecipeOperandTerminator::RecordDelta(delta)
            if Some(next_record_index) != header.record_index.checked_add(delta))
    {
        return Ok(None);
    }
    let Ok(next_byte_offset) = u64::try_from(next_at) else {
        return Ok(None);
    };
    let Some(recipe) = unique_stream_recipe(
        ctx,
        recipes,
        stream,
        |kind| kind == recipe_kind,
        (recipe_record_byte_offset, next_byte_offset),
    )?
    else {
        return Ok(None);
    };
    let Some((recipe_prefix_at, recipe_prefix_bytes, recipe_program_at)) = (|| {
        let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
        let (prefix_at, prefix_bytes) =
            recipe_record_prefix(bytes, prologue[3], recipe_at, family_name_len)?;
        let program_at = recipe_at.checked_add(family_name_len)?;
        (next_at.checked_sub(program_at)? <= 64 * 1024).then_some((
            prefix_at,
            prefix_bytes,
            program_at,
        ))
    })() else {
        return Ok(None);
    };
    let (Ok(recipe_prefix_offset), Ok(recipe_program_offset), Ok(paired_byte_offset)) = (
        u64::try_from(recipe_prefix_at),
        u64::try_from(recipe_program_at),
        u64::try_from(prologue[0]),
    ) else {
        return Ok(None);
    };
    let recipe_references =
        decode_recipe_references_charged(ctx, recipe_prefix_bytes, recipe_prefix_offset)?;
    let Some(recipe_program) =
        contiguous_i32_program(ctx, bytes, recipe_program_at, next_at).transpose()?
    else {
        return Ok(None);
    };
    let recipe_prefix_bytes =
        ctx.copy_retained(recipe_prefix_bytes, "f3d recipe operand prefix")?;
    let recipe_id = ctx.copy_retained_text(&recipe.id, "f3d recipe operand recipe ID")?;
    let paired_class_tag =
        indexed[0].retain_class_tag(ctx, "copy F3D recipe operand paired class tag")?;
    Ok(Some(ParsedRecipeOperand {
        paired_byte_offset,
        paired_class_tag,
        recipe_record_index,
        recipe_record_byte_offset,
        recipe_id,
        recipe_prefix_offset,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset,
        recipe_program,
        next_record_index,
        next_byte_offset,
    }))
}

fn parse_edge_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (scope_reference_ordinal, header): (u32, &DesignRecordHeader),
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Result<Option<DesignEdgeOperand>, CodecError> {
    let next_record_delta = terminal_delta(
        scope_family(scope),
        matches!(scope.payload(), DesignScopePayload::WorkPoint(_)),
    );
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let parse = |terminator| {
        parse_recipe_operand(
            ctx,
            bytes,
            records,
            stream,
            header,
            recipes,
            (ConstructionRecipeKind::Edge, terminator),
        )
    };
    let parsed = match parse(RecipeOperandTerminator::RecordDelta(next_record_delta))? {
        Some(parsed) => parsed,
        None => match terminal_group_limit {
            Some(limit) => {
                let Some(parsed) =
                    parse(RecipeOperandTerminator::NextIndexedAfterRecipe { limit })?
                else {
                    return Ok(None);
                };
                parsed
            }
            None => return Ok(None),
        },
    };
    let recipe_structure = edge_recipe_structure_with_context(ctx, &parsed.recipe_program)?;
    let surface_patch_recipe_structure =
        if matches!(scope.payload(), DesignScopePayload::SurfacePatch(_)) {
            surface_patch_recipe_structure_with_context(
                ctx,
                &parsed.recipe_program,
                parsed.recipe_references.len(),
            )?
        } else {
            None
        };
    let local_topology_references = match recipe_structure.as_ref() {
        Some(structure) => edge_recipe_local_topology_references_with_context(
            ctx,
            structure,
            parsed.recipe_references.len(),
        )?,
        None => None,
    };
    let id_stream = ctx
        .strip_prefix(
            stream,
            crate::ids::SCHEME_PREFIX,
            "strip F3D edge operand scheme",
        )?
        .unwrap_or(stream);
    let id = design_record_id_charged(
        ctx,
        id_stream,
        ":design-edge-operand#",
        header.byte_offset,
        "f3d edge operand ID",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D edge operand class tag")?;
    Ok(DesignEdgeOperand::try_new(
        crate::records::topology::edge_identity::DesignEdgeOperandDraft {
            id,
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes,
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            recipe_structure,
            surface_patch_recipe_structure,
            local_topology_references,
            candidate_faces: Vec::new(),
            result_candidate_faces: Vec::new(),
            result_boundary_edge_slots: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            terminal_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            preceding_boundary_edge_slots: Vec::new(),
            terminal_boundary_edge_slots: Vec::new(),
            changed_boundary_edge_slots: Vec::new(),
            deleted_boundary_edge_slots: Vec::new(),
            updated_boundary_edge_slots: Vec::new(),
            treatment_radius_candidates: Vec::new(),
            changed_boundary_edge_contexts: Vec::new(),
            terminal_boundary_edge_contexts: Vec::new(),
            terminal_reference_edge_slots: Vec::new(),
            recipe_reference_contexts: Vec::new(),
            recipe_selectors: Vec::new(),
            recipe_state_id: None,
            resolved_edge_slot: None,
            resolved_axis: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok())
}
pub(crate) fn edge_recipe_structure_with_context(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some(tail) = program.get(7..) else {
        return Ok(None);
    };
    edge_recipe_structure_tail(ctx, tail)
}

/// Decode the alternate two-clause edge-recipe grammar owned by `SurfacePatch`.
///
/// The six fields in each clause are delimiter-bounded by `-1`. The first and
/// second fields name face references, the third and fifth fields name edge
/// references, and the fourth and sixth fields are zero pairs. The final field
/// is a counted sequence of the standard eight-word topology entries.
pub(crate) fn surface_patch_recipe_structure_with_context(
    ctx: &DecodeContext<'_>,
    program: &[i32],
    reference_count: usize,
) -> Result<
    Option<crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    CodecError,
> {
    let Some(mut remaining) = program.get(7..) else {
        return Ok(None);
    };
    let Some((&root, tail)) = remaining.split_first() else {
        return Ok(None);
    };
    if root != 2 {
        return Ok(None);
    }
    remaining = tail;
    let mut clauses = ctx.vector_storage(2, "collect F3D SurfacePatch clauses")?;
    for _ in 0..2 {
        let mut fields = ctx.vector_storage(6, "collect F3D SurfacePatch fields")?;
        for _ in 0..6 {
            // A field holds one or two words, so its delimiter is one of the
            // first three words.
            let Some(delimiter_at) = remaining.iter().take(3).position(|word| *word == -1) else {
                return Ok(None);
            };
            let (Some(field), Some(tail)) = (
                remaining.get(..delimiter_at),
                remaining.get(delimiter_at + 1..),
            ) else {
                return Ok(None);
            };
            if field.is_empty() || field.iter().any(|word| *word < 0) {
                return Ok(None);
            }
            let field = ctx.copy_slice(field, "collect F3D SurfacePatch field words")?;
            remaining = tail;
            ctx.push_vec(&mut fields, field, "collect F3D SurfacePatch fields")?;
        }
        let Some((&payload_entry_count, tail)) = remaining.split_first() else {
            return Ok(None);
        };
        let Ok(payload_entry_count) = u32::try_from(payload_entry_count) else {
            return Ok(None);
        };
        let Some(payload_word_count) = usize::try_from(payload_entry_count)
            .ok()
            .and_then(|count| count.checked_mul(8))
        else {
            return Ok(None);
        };
        let Some(payload) = tail.get(..payload_word_count) else {
            return Ok(None);
        };
        let Some(entries) = edge_recipe_entries_with_context(ctx, payload)? else {
            return Ok(None);
        };
        if u32::try_from(entries.len()).ok() != Some(payload_entry_count) {
            return Ok(None);
        }
        let Some((&delimiter, tail)) = tail
            .get(payload_word_count..)
            .and_then(|tail| tail.split_first())
        else {
            return Ok(None);
        };
        if delimiter != -1 {
            return Ok(None);
        }
        remaining = tail;
        let [first, second, third, fourth, fifth, sixth] = fields.as_slice() else {
            return Ok(None);
        };
        if !(first.len() == 1 || first.len() == 2 && first[0] == 2)
            || second.len() != 1
            || third.len() != 2
            || third[0] != 2
            || fourth.as_slice() != [0, 0]
            || fifth.len() != 1
            || sixth.as_slice() != [0, 0]
        {
            return Ok(None);
        }
        let ordinal = |field: &[i32], position: usize| {
            let ordinal = usize::try_from(*field.get(position)?).ok()?;
            (ordinal < reference_count).then_some(u32::try_from(ordinal).ok()?)
        };
        let (Some(first_face), Some(second_face), Some(first_edge), Some(second_edge)) = (
            ordinal(first, first.len() - 1),
            ordinal(second, 0),
            ordinal(third, 1),
            ordinal(fifth, 0),
        ) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut clauses,
            crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                fields,
                face_reference_ordinals: [first_face, second_face],
                edge_reference_ordinals: [first_edge, second_edge],
                entries,
            },
            "collect F3D SurfacePatch clauses",
        )?;
    }
    if let Some((&delimiter, tail)) = remaining.split_first() {
        if delimiter != 0 {
            return Ok(None);
        }
        remaining = tail;
    }
    if !remaining.is_empty() {
        return Ok(None);
    }
    Ok(clauses.try_into().ok().map(|clauses| {
        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure { clauses }
    }))
}

#[cfg(test)]
fn edge_recipe_local_topology_references(
    structure: &crate::records::topology::edge_recipe::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    crate::test_support::with_decode_context(|decode_ctx| {
        edge_recipe_local_topology_references_with_context(decode_ctx, structure, reference_count)
            .ok()
            .flatten()
    })
}

fn edge_recipe_local_topology_references_with_context(
    ctx: &DecodeContext<'_>,
    structure: &crate::records::topology::edge_recipe::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Result<Option<Vec<std::num::NonZeroU32>>, CodecError> {
    topology_recipe_references(ctx, &structure.sides, reference_count)
}

/// Take the only side sequence that `accepts`. The search stops at a second
/// accepted sequence.
fn only_side_sequence<'w>(
    ctx: &DecodeContext<'_>,
    mut sequences: Vec<RecipeSideSequence<'w>>,
    accepts: impl Fn(&RecipeSideSequence<'w>) -> bool,
) -> Result<Option<RecipeSideSequence<'w>>, CodecError> {
    const OPERATION: &str = "select F3D recipe side sequence";
    let Some(at) = ctx.position_by(&sequences, |sequence| Ok(accepts(sequence)), OPERATION)? else {
        return Ok(None);
    };
    let later = sequences.get(at + 1..).unwrap_or(&[]);
    if ctx.any_by(later, |sequence| Ok(accepts(sequence)), OPERATION)? {
        return Ok(None);
    }
    Ok(Some(sequences.swap_remove(at)))
}

fn edge_recipe_structure_tail(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some((&root, remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Ok(side_count) = usize::try_from(root) else {
        return Ok(None);
    };
    if side_count == 0 {
        return Ok(None);
    }
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let sequences = edge_recipe_side_sequences(ctx, remaining, side_count)?;
    Ok(only_side_sequence(ctx, sequences, |sequence| {
        matches!(sequence.remaining, [] | [-1 | 0])
    })?
    .map(
        |sequence| crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root,
            sides: sequence.sides,
        },
    ))
}

struct RecipeSideSequence<'a> {
    sides: Vec<DesignTopologyRecipeSide>,
    remaining: &'a [i32],
}

fn edge_recipe_side_sequences<'w>(
    ctx: &DecodeContext<'_>,
    words: &'w [i32],
    side_count: usize,
) -> Result<Vec<RecipeSideSequence<'w>>, CodecError> {
    let _depth = Some(ctx.enter_nested("f3d recipe side recursion")?);
    if side_count == 0 {
        let mut empty = Vec::new();
        ctx.reserve_vec(&mut empty, 1, "f3d recipe empty side sequence")?;
        empty.push(RecipeSideSequence {
            sides: Vec::new(),
            remaining: words,
        });
        return Ok(empty);
    }
    let candidates = edge_recipe_counted_side_candidates(ctx, words)?;
    let mut out = Vec::new();
    for (side, tail) in ctx.admit_iter(&candidates, "scan F3D recipe side candidates")? {
        let remaining = if side_count == 1 {
            *tail
        } else if let Some(remaining) = recipe_delimiter(tail) {
            remaining
        } else {
            continue;
        };
        let mut following = edge_recipe_side_sequences(ctx, remaining, side_count - 1)?;
        for sequence in ctx.admit_iter(&(0..following.len()), "extend F3D recipe side sequences")? {
            let Some(sequence) = following.get_mut(sequence) else {
                continue;
            };
            let mut sides = std::mem::take(&mut sequence.sides);
            ctx.push_vec(
                &mut sides,
                copy_recipe_side(ctx, side)?,
                "f3d recipe following side",
            )?;
            ctx.rotate_right(&mut sides, 1, "f3d recipe following side")?;
            ctx.push_vec(
                &mut out,
                RecipeSideSequence {
                    sides,
                    remaining: sequence.remaining,
                },
                "f3d recipe side sequence",
            )?;
        }
    }
    Ok(out)
}

fn copy_recipe_side(
    ctx: &DecodeContext<'_>,
    side: &DesignTopologyRecipeSide,
) -> Result<DesignTopologyRecipeSide, CodecError> {
    Ok(DesignTopologyRecipeSide {
        header_value: side.header_value,
        scalars: ctx.copy_slice(&side.scalars, "f3d recipe copied scalars")?,
        payload_prefix: ctx.copy_slice(&side.payload_prefix, "f3d recipe copied payload prefix")?,
        entries: ctx.copy_slice(&side.entries, "f3d recipe copied entries")?,
    })
}

fn recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1 | 0)).then(|| &words[1..])
}

fn complete_recipe_payload_prefix(
    ctx: &DecodeContext<'_>,
    prefix: &[i32],
) -> Result<bool, CodecError> {
    if prefix == [0] {
        return Ok(true);
    }
    let mut remaining = prefix;
    let mut field_count = 0;
    while !remaining.is_empty() {
        let Some(delimiter_at) = ctx.position_by(
            remaining,
            |word| Ok(*word == -1),
            "scan F3D recipe payload prefix delimiters",
        )?
        else {
            return Ok(false);
        };
        let (Some(field), Some(after)) = (
            remaining.get(..delimiter_at),
            remaining.get(delimiter_at + 1..),
        ) else {
            return Ok(false);
        };
        if field.first().is_none_or(|word| *word <= 0)
            || ctx.any_by(
                field,
                |word| Ok(*word < 0),
                "validate F3D recipe payload prefix words",
            )?
        {
            return Ok(false);
        }
        let Some(([0, 0, -1], after)) = after.split_first_chunk::<3>() else {
            return Ok(false);
        };
        remaining = after;
        field_count += 1;
    }
    Ok(field_count > 0)
}

fn edge_recipe_counted_side_candidates<'w>(
    ctx: &DecodeContext<'_>,
    words: &'w [i32],
) -> Result<Vec<(DesignTopologyRecipeSide, &'w [i32])>, CodecError> {
    let Some(field_count) = words
        .first()
        .and_then(|word| u32::try_from(*word).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Ok(Vec::new());
    };
    if field_count.get() < 2 {
        return Ok(Vec::new());
    }
    let Some(scalar_count) = usize::try_from(field_count.get())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return Ok(Vec::new());
    };
    let Some(&header_value) = words.get(1) else {
        return Ok(Vec::new());
    };
    let Some(mut remaining) = words.get(2..).and_then(recipe_delimiter) else {
        return Ok(Vec::new());
    };
    // Each scalar consumes at least one remaining word; a larger count is
    // corrupt and must not reach the allocator.
    if scalar_count > remaining.len() {
        return Ok(Vec::new());
    }
    let mut scalar_storage = ctx.reserve_scoped(0, "f3d recipe scalars")?;
    let mut scalars = Vec::new();
    scalar_storage
        .with_storage(|| ctx.reserve_capacity(&mut scalars, scalar_count, "f3d recipe scalars"))?;
    for _ in ctx.admit_iter(&(0..scalar_count), "f3d recipe scalars")? {
        let Some((&scalar, tail)) = remaining.split_first() else {
            return Ok(Vec::new());
        };
        scalar_storage.with_storage(|| ctx.push_vec(&mut scalars, scalar, "f3d recipe scalars"))?;
        let Some(tail) = recipe_delimiter(tail) else {
            return Ok(Vec::new());
        };
        remaining = tail;
    }
    let mut candidates = Vec::new();
    for (entry_count_at, _) in ctx
        .admit_iter(remaining, "f3d recipe payload candidates")?
        .enumerate()
    {
        let Some(payload_prefix) = remaining.get(..entry_count_at) else {
            continue;
        };
        if !complete_recipe_payload_prefix(ctx, payload_prefix)? {
            continue;
        }
        let candidate = (|| {
            let payload_entry_count = u32::try_from(*remaining.get(entry_count_at)?).ok()?;
            if entry_count_at != 1 && payload_entry_count == 0 {
                return None;
            }
            let payload_len = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
            let entries_at = entry_count_at.checked_add(1)?;
            let entries_end = entries_at.checked_add(payload_len)?;
            Some((
                remaining.get(entries_at..entries_end)?,
                remaining.get(entries_end..)?,
            ))
        })();
        let Some((payload, tail)) = candidate else {
            continue;
        };
        let Some(entries) = edge_recipe_entries_with_context(ctx, payload)? else {
            continue;
        };
        let side = DesignTopologyRecipeSide {
            header_value,
            scalars: ctx.copy_slice(&scalars, "f3d recipe candidate scalars")?,
            payload_prefix: ctx.copy_slice(payload_prefix, "f3d recipe payload prefix")?,
            entries,
        };
        ctx.push_vec(&mut candidates, (side, tail), "f3d recipe side candidate")?;
    }
    Ok(candidates)
}

pub(crate) fn face_recipe_structure_with_context(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::face::DesignFaceRecipeStructure>, CodecError> {
    let Some((&root, remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&first_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&second_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let postlude = |tail: &[i32]| match tail {
        [] | [-1 | 0] => Some(None),
        [-1, value, -1, 0, 0, -1] => Some(Some(*value)),
        _ => None,
    };
    let sequences = edge_recipe_side_sequences(ctx, remaining, 2)?;
    let Some(sequence) = only_side_sequence(ctx, sequences, |sequence| {
        sequence.sides.len() == 2 && postlude(sequence.remaining).is_some()
    })?
    else {
        return Ok(None);
    };
    let (Some(postlude_value), Ok(sides)) =
        (postlude(sequence.remaining), sequence.sides.try_into())
    else {
        return Ok(None);
    };
    Ok(Some(
        crate::records::topology::face::DesignFaceRecipeStructure {
            root,
            prelude: [first_prelude, second_prelude],
            sides,
            postlude_value,
        },
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceRecipeProgramKind {
    Terminal,
    Counted { header_value: usize },
}

pub(crate) fn face_recipe_program_kind(program: &[i32]) -> Option<FaceRecipeProgramKind> {
    if matches!(program, [0, -1 | 0]) {
        return Some(FaceRecipeProgramKind::Terminal);
    }
    if !matches!(program.get(0..2), Some([0, -1 | 0])) {
        return None;
    }
    let header_value = usize::try_from(*program.get(2)?).ok()?;
    (header_value > 0 && header_value <= 100_000)
        .then_some(FaceRecipeProgramKind::Counted { header_value })
}

fn face_recipe_nodes_with_context(
    ctx: &DecodeContext<'_>,
    recipe_program: &[i32],
    recipe_program_at: usize,
    program_kind: FaceRecipeProgramKind,
) -> Result<Option<Vec<crate::records::topology::face::DesignFaceRecipeNode>>, CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "f3d face recipe node index")?;
    let mut recipe_node_indices = Vec::new();
    let marker_width = std::num::NonZeroUsize::new(3)
        .ok_or_else(|| CodecError::malformed("F3D face recipe marker window is empty"))?;
    for (index, values) in ctx
        .admit_iter(recipe_program, "scan F3D face recipe marker windows")?
        .windows(marker_width)
        .enumerate()
    {
        if values != [-1, -1, 2] {
            continue;
        }
        index_storage.with_storage(|| {
            ctx.push_vec(
                &mut recipe_node_indices,
                index,
                "f3d face recipe node index",
            )
        })?;
    }
    if recipe_node_indices.first().is_some_and(|index| *index < 3) {
        return Ok(None);
    }
    if program_kind == FaceRecipeProgramKind::Terminal && !recipe_node_indices.is_empty() {
        return Ok(None);
    }
    let node_starts = ctx
        .admit_iter(
            &recipe_node_indices,
            "scan F3D face recipe node range starts",
        )?
        .copied();
    let node_ends = ctx
        .admit_iter(&recipe_node_indices, "scan F3D face recipe node range ends")?
        .copied()
        .skip(1)
        .chain(std::iter::once(recipe_program.len()));
    let mut recipe_nodes = Vec::new();
    for (start, end) in node_starts.zip(node_ends) {
        let Some(program) = recipe_program.get(start..end) else {
            return Ok(None);
        };
        let Some(byte_offset) = start
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let Some(end_byte_offset) = end
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let recipe_structure = match program.get(3..) {
            Some(tail) => face_recipe_structure_with_context(ctx, tail)?,
            None => None,
        };
        let program = ctx.copy_slice(program, "f3d face recipe node program")?;
        ctx.push_vec(
            &mut recipe_nodes,
            crate::records::topology::face::DesignFaceRecipeNode {
                byte_offset,
                end_byte_offset,
                program,
                recipe_structure,
            },
            "f3d face recipe node",
        )?;
    }
    Ok(Some(recipe_nodes))
}

fn topology_recipe_references(
    ctx: &DecodeContext<'_>,
    sides: &[DesignTopologyRecipeSide],
    reference_count: usize,
) -> Result<Option<Vec<std::num::NonZeroU32>>, CodecError> {
    let mut references = Vec::new();
    let mut accept_word = |word: i32| -> Result<bool, CodecError> {
        if word == 0 {
            return Ok(true);
        }
        let Some(ordinal) = u32::try_from(word).ok().and_then(std::num::NonZeroU32::new) else {
            return Ok(false);
        };
        if usize::try_from(ordinal.get())
            .ok()
            .is_none_or(|index| index > reference_count)
        {
            return Ok(false);
        }
        ctx.push_vec(&mut references, ordinal, "f3d recipe topology reference")?;
        Ok(true)
    };
    let complete = ctx.all_by(
        sides,
        |side| {
            Ok(accept_word(side.header_value)?
                && ctx.all_by(
                    &side.scalars,
                    |word| accept_word(*word),
                    "scan F3D edge recipe side scalars",
                )?)
        },
        "scan F3D edge recipe topology sides",
    )?;
    Ok(complete.then_some(references))
}

#[cfg(test)]
fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    crate::test_support::with_decode_context(|decode_ctx| {
        edge_recipe_entries_with_context(decode_ctx, words)
            .ok()
            .flatten()
    })
}

/// Read one eight-word topology entry.
fn edge_recipe_entry(entry: &[i32; 8]) -> Option<DesignTopologyRecipeEntry> {
    let [selector, boundary_edge_count, ..] = entry;
    if *selector < 0 {
        return None;
    }
    let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(*boundary_edge_count).ok()?)?;
    let topology_triplets = [
        edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
        edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
    ];
    topology_triplets
        .iter()
        .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
        .then_some(DesignTopologyRecipeEntry {
            selector: *selector,
            boundary_edge_count,
            topology_triplets,
        })
}

/// Read eight-word topology entries with strictly increasing selectors. A
/// trailing partial entry is ignored.
fn edge_recipe_entries_with_context(
    ctx: &DecodeContext<'_>,
    words: &[i32],
) -> Result<Option<Vec<DesignTopologyRecipeEntry>>, CodecError> {
    let (chunks, _) = words.as_chunks::<8>();
    let mut entries: Vec<DesignTopologyRecipeEntry> = Vec::new();
    let complete = ctx.all_by(
        chunks,
        |entry| {
            let Some(parsed) = edge_recipe_entry(entry) else {
                return Ok(false);
            };
            if entries
                .last()
                .is_some_and(|last| last.selector >= parsed.selector)
            {
                return Ok(false);
            }
            ctx.push_vec(&mut entries, parsed, "f3d recipe topology entry")?;
            Ok(true)
        },
        "scan F3D topology recipe entry words",
    )?;
    Ok(complete.then_some(entries))
}

fn edge_recipe_topology_triplet(
    words: &[i32],
    boundary_edge_count: std::num::NonZeroU32,
) -> Option<DesignTopologyRecipeTriplet> {
    let [outer, middle, repeated_outer] = words else {
        return None;
    };
    if outer != repeated_outer {
        return None;
    }
    let outer = std::num::NonZeroU32::new(u32::try_from(*outer).ok()?)?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let incident = if *middle == i32::try_from(outer.get()).ok()? {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        ))
    } else if *middle >= 0 && middle.checked_add(1) == i32::try_from(outer.get()).ok() {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        ))
    } else {
        None
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle: *middle,
        incident: incident.map(|(side, ordinal)| {
            crate::records::topology::edge_recipe::DesignTopologyIncident { ordinal, side }
        }),
    })
}

/// Find the indexed header that terminates a face-recipe member.
///
/// The ordinary envelope terminates at `N+4`. One serialized generation
/// omits that header and terminates at `N+5`. When neither expected index is
/// present within the enclosing boundary, the first following valid indexed
/// header terminates the envelope; its record index has no fixed delta from
/// `N`. Select an expected continuation before applying that fallback.
fn face_recipe_next_boundary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
    record_index: u32,
    limit: Option<u64>,
) -> Result<Option<(usize, u32)>, CodecError> {
    // A header that opens at or before `limit` lies within `limit + 11` bytes,
    // so the search reads no further.
    let (window_end, limit) = match limit {
        None => (bytes.len(), usize::MAX),
        Some(limit) => {
            let Ok(limit) = usize::try_from(limit) else {
                return Ok(None);
            };
            let end = limit
                .checked_add(11)
                .map_or(bytes.len(), |end| end.min(bytes.len()));
            (end, limit)
        }
    };
    let Some(window) = bytes.get(..window_end) else {
        return Ok(None);
    };
    let Some((plus_four, plus_five)) = record_index.checked_add(4).zip(record_index.checked_add(5))
    else {
        return Ok(None);
    };
    // The search for an expected continuation passes the first following
    // header first, which is the fallback terminator.
    let mut first_following = None;
    let expected = next_indexed_record_header(ctx, window, position, |header| {
        first_following.get_or_insert(*header);
        header.record_index == plus_four || header.record_index == plus_five
    })?;
    let Some(header) = expected
        .filter(|header| header.offset <= limit)
        .or(first_following)
    else {
        return Ok(None);
    };
    Ok((header.offset <= limit).then_some((header.offset, header.record_index)))
}

#[derive(Clone, Copy)]
pub(crate) struct FaceOperandFrame<'a> {
    pub(crate) scope: &'a DesignParameterScope,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) group_ownership: Option<(u32, u32)>,
    pub(crate) next_byte_offset: Option<u64>,
    pub(crate) header: &'a DesignRecordHeader,
}

pub(super) fn parse_face_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    input: FaceOperandFrame<'_>,
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignFaceOperand>, CodecError> {
    let FaceOperandFrame {
        scope,
        scope_reference_ordinal,
        group_ownership,
        next_byte_offset,
        header,
    } = input;
    let Some(prologue_search) = usize::try_from(header.byte_offset)
        .ok()
        .and_then(|start| start.checked_add(11))
    else {
        return Ok(None);
    };
    let Some(prologue) =
        records.consecutive_headers::<4>(ctx, prologue_search, header.record_index)?
    else {
        return Ok(None);
    };
    let Some(position) = prologue[3].checked_add(11) else {
        return Ok(None);
    };
    let Some((immediate_next, next_record_index)) =
        face_recipe_next_boundary(ctx, bytes, position, header.record_index, next_byte_offset)?
    else {
        return Ok(None);
    };
    let offsets = [
        prologue[0],
        prologue[1],
        prologue[2],
        prologue[3],
        immediate_next,
    ];
    let Some(indexed) = five_headers_at(bytes, offsets) else {
        return Ok(None);
    };
    let Some(recipe_record_index) = header.record_index.checked_add(3) else {
        return Ok(None);
    };
    let prefix_matches = (0u32..4)
        .zip(&indexed)
        .all(|(delta, found)| Some(found.record_index) == header.record_index.checked_add(delta));
    if !prefix_matches || indexed[4].record_index != next_record_index {
        return Ok(None);
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let (Ok(recipe_start), Ok(next_byte_offset)) =
        (u64::try_from(prologue[3]), u64::try_from(immediate_next))
    else {
        return Ok(None);
    };
    let Some(recipe) = unique_stream_recipe(
        ctx,
        recipes,
        stream,
        |kind| {
            matches!(
                kind,
                ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
            )
        },
        (recipe_start, next_byte_offset),
    )?
    else {
        return Ok(None);
    };
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return Ok(None),
    };
    let Some((recipe_prefix_at, recipe_prefix_bytes, recipe_program_at)) = (|| {
        let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
        let (prefix_at, prefix_bytes) =
            recipe_record_prefix(bytes, prologue[3], recipe_at, family_name_len)?;
        let program_at = recipe_at.checked_add(family_name_len)?;
        (immediate_next.checked_sub(program_at)? <= 64 * 1024).then_some((
            prefix_at,
            prefix_bytes,
            program_at,
        ))
    })() else {
        return Ok(None);
    };
    let (Ok(recipe_prefix_offset), Ok(recipe_program_offset), Ok(paired_byte_offset)) = (
        u64::try_from(recipe_prefix_at),
        u64::try_from(recipe_program_at),
        u64::try_from(prologue[0]),
    ) else {
        return Ok(None);
    };
    let recipe_references =
        decode_recipe_references_charged(ctx, recipe_prefix_bytes, recipe_prefix_offset)?;
    let Some(recipe_program) =
        contiguous_i32_program(ctx, bytes, recipe_program_at, immediate_next).transpose()?
    else {
        return Ok(None);
    };
    let Some(program_kind) = face_recipe_program_kind(&recipe_program) else {
        return Ok(None);
    };
    let Some(recipe_nodes) =
        face_recipe_nodes_with_context(ctx, &recipe_program, recipe_program_at, program_kind)?
    else {
        return Ok(None);
    };
    let recipe_prefix_bytes = ctx.copy_retained(recipe_prefix_bytes, "f3d face operand prefix")?;
    let recipe_id = ctx.copy_retained_text(&recipe.id, "f3d face operand recipe ID")?;
    let id_stream = ctx
        .strip_prefix(
            stream,
            crate::ids::SCHEME_PREFIX,
            "strip F3D face operand scheme",
        )?
        .unwrap_or(stream);
    let id = design_record_id_charged(
        ctx,
        id_stream,
        ":design-face-operand#",
        header.byte_offset,
        "f3d face operand ID",
    )?;
    let class_tag = header
        .class_tag
        .try_clone_for_decode(ctx, "copy F3D face operand class tag")?;
    let paired_class_tag =
        indexed[0].retain_class_tag(ctx, "copy F3D face operand paired class tag")?;
    Ok(
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id,
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            group: group_ownership.map(|(group_record_index, group_member_ordinal)| {
                crate::records::topology::body_recipe::DesignOperandGroup {
                    group_record_index,
                    group_member_ordinal,
                }
            }),
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag,
            paired_byte_offset,
            paired_class_tag,
            recipe_record_index,
            recipe_record_byte_offset: recipe_start,
            recipe_id,
            recipe_prefix_offset,
            recipe_prefix_bytes,
            recipe_references,
            recipe_kind: recipe.kind,
            recipe_program_offset,
            recipe_program,
            recipe_nodes,
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index,
            next_byte_offset,
        })
        .ok(),
    )
}

#[cfg(test)]
pub(in crate::design) fn has_typed_edge_treatment_group(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    )
}

/// Apply the selection-identity completeness rule to a parsed group candidate.
///
/// A localized edge-treatment reference table can contain selections that do
/// not use counted groups. Such a reference is a group only when its parsed
/// candidate also resolves through one of the selection-identity grammars.
pub(crate) fn construction_operand_group_is_retained(
    scope_kind: Option<&crate::records::feature::scope::DesignFeatureKind>,
    has_selection_identity: bool,
) -> bool {
    !scope_kind.is_some_and(crate::design::is_localized_edge_treatment_kind)
        || has_selection_identity
}

#[cfg(test)]
mod tests;
