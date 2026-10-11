// SPDX-License-Identifier: Apache-2.0
//! Project parameter-design features and dispatch per feature family.

use crate::design::decode::operands::entity_selection_matches_curve;
use crate::design::dimensions::expression_identifiers;
use crate::design::edge_resolve::{
    project_fixed_fillet_with_corners, resolved_edge_flange_group, resolved_edge_group,
    resolved_edge_treatment_group_with_corners, resolved_surface_patch_edge_group,
};
use crate::design::face_resolve::{
    design_angle, resolved_body_recipe_selection, resolved_direct_face_selection,
    resolved_face_group, resolved_historical_face_group, resolved_historical_face_operand,
    resolved_historical_split_face_target_group_with_updated_faces,
    resolved_loft_edge_profile_group, resolved_profile_face_group,
};

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids;

use crate::records::{
    bodies::DesignBodyBinding,
    entity_header::DesignFeatureTimeline,
    feature::{
        extrude::DesignExtrudeOperation,
        scope::DesignParameterScope,
        surface_ops::{
            DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport, DesignSurfaceTrimOperation,
        },
        work_geometry::DesignEdgeTreatmentVertexOperand,
    },
    parameters::{DesignParameter, DesignParameterKind, DesignParameterOwner},
    recipes::ConstructionRecipeKind,
    sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity},
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeIdentityOperand, edge_identity::DesignEdgeOperand,
        entity_selection::DesignLoftLegacyBodyCarrier, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand, fillet::DesignFilletRadiusGroup, fillet::DesignFilletRadiusLaw,
    },
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;

pub(crate) mod form_cages;

const EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9: f64 = 1.0e-9;

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

mod coil;
mod extrude;
mod fixed_geometry;
mod parameter_order;
mod patterns;
pub(super) mod spatial_profiles;

pub(super) fn unique_feature_match_for_decode<T: Copy>(
    ctx: &DecodeContext<'_>,
    source: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(T) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<T>, CodecError> {
    let mut source = source.into_iter();
    let Some(first) = ctx.find_by(&mut source, |value| predicate(*value), operation)? else {
        return Ok(None);
    };
    if ctx.any_by(source, predicate, operation)? {
        return Ok(None);
    }
    Ok(Some(first))
}

pub(super) fn feature_stream_matches(
    ctx: &DecodeContext<'_>,
    id: &str,
    expected: Option<&str>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let actual = ids::native_stream_charged(ctx, id, operation)?;
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            ctx.equal_bytes(actual.as_bytes(), expected.as_bytes(), operation)
        }
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

// A raw-reference walk exposes empty references rather than charging a hidden
// flat_map next. The callback returns false to stop at the reached candidate.
fn visit_revolve_face_candidates<'source>(
    ctx: &DecodeContext<'_>,
    operand: &'source DesignFaceOperand,
    mut visit: impl FnMut(&'source cadmpeg_ir::ids::FaceId) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let use_referenced = operand.recipe_kind == ConstructionRecipeKind::Face
        && ctx.any_by(
            &operand.recipe_references,
            |reference| {
                Ok(!reference.candidate_faces.is_empty()
                    || !reference.alternate_selector_faces.is_empty())
            },
            "select F3D Revolve candidate lane",
        )?;
    if use_referenced {
        ctx.all_by(
            &operand.recipe_references,
            |reference| {
                Ok(ctx.all_by(
                    &reference.candidate_faces,
                    &mut visit,
                    "visit F3D Revolve face candidate",
                )? && ctx.all_by(
                    &reference.alternate_selector_faces,
                    &mut visit,
                    "visit F3D Revolve face candidate",
                )?)
            },
            "visit F3D Revolve candidate reference",
        )
    } else {
        ctx.all_by(
            crate::design::face_resolve::face_operand_candidates(operand),
            visit,
            "visit F3D Revolve face candidate",
        )
    }
}

fn insert_feature_dependency(
    ctx: &DecodeContext<'_>,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    dependency: &cadmpeg_ir::features::FeatureId,
) -> Result<(), CodecError> {
    if ctx.contains(
        dependencies.as_slice(),
        dependency,
        "f3d feature dependency membership",
    )? {
        return Ok(());
    }
    let id = (dependency).try_clone_for_decode(ctx, "f3d feature dependency id")?;
    dependencies.insert(ctx, id, "f3d feature dependency")?;
    Ok(())
}

/// Design record slices projected together into the neutral construction
/// history: the parameter, owner, and scope tables plus the construction
/// operand, fillet-radius, edge, edge-identity, face, and whole-body recipe
/// operand records and the sketch placements and body bindings each feature
/// scope resolves against.
#[cfg_attr(test, derive(Default))]
pub(crate) struct ProjectInputs<'a> {
    pub(crate) native: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) timelines: &'a [DesignFeatureTimeline],
    pub(crate) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(crate) fillet_radius_groups: &'a [DesignFilletRadiusGroup],
    pub(crate) edge_operands: &'a [DesignEdgeOperand],
    pub(crate) edge_identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) edge_treatment_vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) entity_selection_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) face_operands: &'a [DesignFaceOperand],
    pub(crate) body_recipe_operands: &'a [DesignBodyRecipeOperand],
    pub(crate) legacy_loft_body_carriers: &'a [DesignLoftLegacyBodyCarrier],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    pub(crate) component_naming_spaces: &'a [crate::records::recipes::DesignComponentNamingSpace],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
}

/// Authored construction ordinal of every parameter scope represented by a
/// neutral top-level feature. All input scopes must share one Design stream.
pub(super) fn authored_scope_ordinals<'a>(
    ctx: &DecodeContext<'_>,
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<BTreeMap<(&'a str, u32), u64>, CodecError> {
    let Some(first_scope) = scopes.first() else {
        return Ok(BTreeMap::new());
    };
    let stream =
        ids::native_stream_charged(ctx, &first_scope.id, "resolve F3D authored scope stream")?
            .unwrap_or(ids::DEFAULT_STREAM);
    if ctx.any_by(
        scopes,
        |scope| {
            Ok(!ctx.equal(
                ids::native_stream_charged(ctx, &scope.id, "resolve F3D authored scope stream")?
                    .unwrap_or(ids::DEFAULT_STREAM),
                stream,
                "f3d authored scope stream comparison",
            )?)
        },
        "f3d authored scope stream scan",
    )? {
        return Err(CodecError::NotImplemented(
            "independent Design scope streams have no shared authored timeline order".into(),
        ));
    }
    let mut scope_storage = ctx.reserve_scoped(0, "f3d authored scope scratch")?;
    let mut stream_scopes = Vec::new();
    for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
        ctx.push_scoped_vec(
            &mut scope_storage,
            &mut stream_scopes,
            scope,
            "f3d authored stream scope",
        )?;
    }
    authored_scope_ordinals_for_stream(ctx, &stream_scopes, timelines)
}

/// Authored scope ordinals evaluated independently for every Design stream.
pub(crate) fn authored_scope_ordinals_per_stream<'a>(
    ctx: &DecodeContext<'_>,
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<BTreeMap<(&'a str, u32), u64>, CodecError> {
    let mut stream_storage = ctx.reserve_scoped(0, "f3d authored stream storage")?;
    let streams = stream_storage.with_storage(|| -> Result<_, CodecError> {
        let mut streams = BTreeMap::<&str, Vec<&DesignParameterScope>>::new();
        for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
            let stream =
                ids::native_stream_charged(ctx, &scope.id, "resolve F3D authored scope stream")?
                    .unwrap_or(ids::DEFAULT_STREAM);
            ctx.push_btree_group(
                &mut streams,
                stream,
                scope,
                "f3d authored stream index",
                "f3d authored stream scope",
            )?;
        }
        Ok(streams)
    })?;
    let mut out = BTreeMap::new();
    for (_, stream_scopes) in ctx.admit_iter(streams, "f3d authored stream scan")? {
        let (ordinals, ordinal_storage) = ctx
            .with_scoped_storage("f3d authored ordinal merge scratch", || {
                authored_scope_ordinals_for_stream(ctx, &stream_scopes, timelines)
            })?;
        for (key, ordinal) in ctx.admit_iter(ordinals, "f3d authored ordinal merge scan")? {
            if ctx
                .insert_btree_map(&mut out, key, ordinal, "f3d authored scope ordinal")?
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
        drop(ordinal_storage);
    }
    Ok(out)
}

fn authored_scope_ordinals_for_stream<'a>(
    ctx: &DecodeContext<'_>,
    scopes: &[&'a DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<BTreeMap<(&'a str, u32), u64>, CodecError> {
    let mut out = BTreeMap::new();
    let Some(first_scope) = scopes.first().copied() else {
        return Ok(out);
    };
    let stream =
        ids::native_stream_charged(ctx, &first_scope.id, "resolve F3D authored scope stream")?
            .unwrap_or(ids::DEFAULT_STREAM);
    let mut index_storage = ctx.reserve_scoped(0, "f3d authored order scratch")?;
    let mut scopes_by_record = HashMap::<u32, &DesignParameterScope>::new();
    for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
        if index_storage
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut scopes_by_record,
                    scope.record_index,
                    *scope,
                    "f3d authored scope record index",
                )
            })?
            .is_some()
        {
            return Err(CodecError::Malformed(
                "Design scope record identity is not unique".into(),
            ));
        }
    }
    for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let Some(target) = ctx.get_hash_map(
            &scopes_by_record,
            &target_record_index,
            "f3d authored assembly target lookup",
        )?
        else {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has no JointOrigin target".into(),
            ));
        };
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Assemble
            || target.kind() != crate::records::feature::scope::DesignFeatureKind::JointOrigin
            || target.joint_origin_transform().is_none()
        {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has an invalid JointOrigin target".into(),
            ));
        }
    }

    let mut stream_timelines = Vec::new();
    for timeline in ctx.admit_iter(timelines, "f3d authored timeline scan")? {
        if !ctx.equal(
            ids::native_stream_charged(ctx, timeline.id(), "resolve F3D authored timeline stream")?
                .unwrap_or(ids::DEFAULT_STREAM),
            stream,
            "f3d authored timeline stream comparison",
        )? {
            continue;
        }
        ctx.push_scoped_vec(
            &mut index_storage,
            &mut stream_timelines,
            timeline,
            "f3d authored stream timeline",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut stream_timelines[..],
        |value| {
            let timeline = value;
            {
                timeline.source_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design feature_project 1",
    )?;
    if stream_timelines.is_empty() {
        let first_family = design_feature_family(&first_scope.kind());
        let homogeneous = ctx.all_by(
            scopes,
            |scope| {
                Ok(first_family.map_or_else(
                    || scope.kind() == first_scope.kind(),
                    |family| design_feature_family(&scope.kind()) == Some(family),
                ))
            },
            "f3d authored homogeneous family scan",
        )?;
        let mut ordered = Vec::new();
        for &scope in ctx.admit_iter(scopes, "f3d authored scope order scan")? {
            ctx.push_scoped_vec(
                &mut index_storage,
                &mut ordered,
                scope,
                "f3d authored scope order",
            )?;
        }
        ctx.stable_sort_by_key(
            &mut ordered[..],
            |value| {
                let scope = value;
                scope.feature_ordinal
            },
            Ord::cmp,
            "sort f3d design feature_project 2",
        )?;
        let complete_ordinals = ctx.all_by(
            ordered.iter().enumerate(),
            |(ordinal, scope)| {
                Ok({
                    u32::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| ordinal.checked_add(1))
                        == Some(scope.feature_ordinal.get())
                })
            },
            "f3d authored complete ordinal scan",
        )?;
        if !homogeneous || !complete_ordinals {
            return Err(CodecError::NotImplemented(
                "Design scopes have no complete authored timeline order".into(),
            ));
        }
        for (ordinal, scope) in ctx
            .admit_iter(ordered, "f3d authored ordered scope scan")?
            .enumerate()
        {
            let ordinal = u64::try_from(ordinal)
                .map_err(|_| CodecError::Malformed("Design feature ordinal exceeds u64".into()))?;
            if ctx
                .insert_btree_map(
                    &mut out,
                    (
                        ids::native_stream_charged(
                            ctx,
                            &scope.id,
                            "resolve F3D authored scope stream",
                        )?
                        .unwrap_or(ids::DEFAULT_STREAM),
                        scope.record_index,
                    ),
                    ordinal,
                    "f3d authored scope ordinal",
                )?
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
        return Ok(out);
    }

    if !ctx.all_by(
        stream_timelines.iter().enumerate(),
        |(ordinal, timeline)| Ok(u32::try_from(ordinal).ok() == Some(timeline.source_ordinal)),
        "f3d authored contiguous timeline scan",
    )? {
        return Err(CodecError::Malformed(
            "Design timeline-record ordinals are not contiguous".into(),
        ));
    }
    let mut selected = stream_timelines.iter();
    if ctx
        .find_by(
            selected.by_ref(),
            |timeline| Ok(!timeline.frame().items().is_empty()),
            "f3d authored nonempty timeline scan",
        )?
        .is_some()
        && ctx.any_by(
            selected,
            |timeline| Ok(!timeline.frame().items().is_empty()),
            "f3d authored nonempty timeline scan",
        )?
    {
        return Err(CodecError::NotImplemented(
            "multiple nonempty Design timelines have no shared authored order".into(),
        ));
    }
    let mut item_ordinals = HashMap::<u64, u64>::new();
    let mut next_ordinal = 0_u64;
    for timeline in ctx.admit_iter(stream_timelines, "f3d authored selected timeline scan")? {
        for item in ctx
            .admit_iter(timeline.frame().items(), "f3d authored timeline item scan")?
            .map(|item| item.value)
        {
            if index_storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut item_ordinals,
                        item,
                        next_ordinal,
                        "f3d authored timeline item ordinal",
                    )
                })?
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design timeline item identity is not unique".into(),
                ));
            }
            next_ordinal = next_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("Design feature ordinal exceeds u64".into())
            })?;
        }
    }
    for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
        if let Some(ordinal) = ctx
            .get_hash_map(
                &item_ordinals,
                &u64::from(scope.record_index),
                "f3d authored item ordinal lookup",
            )?
            .copied()
        {
            // discarded-value: each scope is visited once in this stream.
            let _ = ctx.insert_btree_map(
                &mut out,
                (stream, scope.record_index),
                ordinal,
                "f3d authored scope ordinal",
            )?;
        }
    }
    for scope in ctx.admit_iter(scopes, "f3d authored scope scan")? {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let target = ctx
            .get_hash_map(
                &scopes_by_record,
                &target_record_index,
                "f3d authored assembly target lookup",
            )?
            .ok_or_else(|| {
                CodecError::Malformed(
                    "Design assembly datum envelope has no JointOrigin target".into(),
                )
            })?;
        let source_key = (stream, scope.record_index);
        let Some(source_ordinal) =
            ctx.remove_btree_map(&mut out, &source_key, "f3d authored scope ordinal removal")?
        else {
            continue;
        };
        let target_key = (stream, target.record_index);
        if ctx.contains_key_hash_map(
            &item_ordinals,
            &u64::from(target.record_index),
            "f3d authored target item lookup",
        )? {
            continue;
        }
        if ctx
            .insert_btree_map(
                &mut out,
                target_key,
                source_ordinal,
                "f3d authored scope ordinal",
            )?
            .is_some()
        {
            return Err(CodecError::Malformed(
                "Design JointOrigin target has multiple authored timeline positions".into(),
            ));
        }
    }
    Ok(out)
}

/// Result of following one scope's preceding state through internal scopes.
pub(crate) enum ScopeHistoryPredecessor<'a> {
    /// The state chain reaches no projected parameter scope.
    None,
    /// The chain reaches one projected parameter scope.
    Scope(&'a DesignParameterScope),
    /// The state or history identity does not select one scope.
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum ComponentHistoryNamespace {
    Aggregate,
    Component(u64),
}

impl cadmpeg_core::decode::cost::DecodeCost for ComponentHistoryNamespace {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

enum ScopeHistoryBinding {
    Absent,
    Bound(HashMap<String, String>),
}

/// History-state index qualified by component-local Design and ASM history.
pub(crate) struct ScopeHistoryGraph<'a> {
    binding: ScopeHistoryBinding,
    component_namespaces: HashMap<&'a str, ComponentHistoryNamespace>,
    scopes_by_state:
        HashMap<(String, ComponentHistoryNamespace, String, i64), Vec<&'a DesignParameterScope>>,
}

impl<'a> ScopeHistoryGraph<'a> {
    pub(crate) fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        scopes: &'a [DesignParameterScope],
        body_bindings: &[DesignBodyBinding],
        body_recipe_operands: &[DesignBodyRecipeOperand],
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
        histories: &[crate::history_records::AsmHistory],
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        ctx.with_scoped_storage(
            "f3d scope history graph scratch",
            || -> Result<Self, CodecError> {
                let binding = if histories.is_empty() {
                    ScopeHistoryBinding::Absent
                } else {
                    ScopeHistoryBinding::Bound(crate::history::bind_scope_histories(
                        ctx,
                        scopes,
                        body_bindings,
                        body_recipe_operands,
                        histories,
                    )?)
                };
                let mut naming_storage =
                    ctx.reserve_scoped(0, "f3d component naming-space scratch")?;
                let mut spaces_by_stream = BTreeMap::<&str, BTreeSet<u64>>::new();
                if !scopes.is_empty() {
                    for space in ctx
                        .admit_iter(component_naming_spaces, "f3d component naming-space source")?
                    {
                        let Some(stream) = ids::native_stream_charged(
                            ctx,
                            &space.id,
                            "resolve F3D component naming-space stream",
                        )?
                        else {
                            continue;
                        };
                        naming_storage.with_storage(|| -> Result<(), CodecError> {
                            if let Some(records) = ctx.get_mut_btree_map(
                                &mut spaces_by_stream,
                                stream,
                                "f3d component naming-space stream lookup",
                            )? {
                                // discarded-value: Duplicate component records share the same namespace.
                                let _ = ctx.insert_btree_set(
                                    records,
                                    space.component_record_index,
                                    "f3d component naming-space record index",
                                )?;
                            } else {
                                let mut records = BTreeSet::new();
                                // discarded-value: The component record set is empty.
                                let _ = ctx.insert_btree_set(
                                    &mut records,
                                    space.component_record_index,
                                    "f3d component naming-space record index",
                                )?;
                                // discarded-value: The stream lookup proved the key absent.
                                let _ = ctx.insert_btree_map(
                                    &mut spaces_by_stream,
                                    stream,
                                    records,
                                    "f3d component naming-space stream index",
                                )?;
                            }
                            Ok(())
                        })?;
                    }
                }
                let mut component_namespaces = HashMap::new();
                for scope in ctx.admit_iter(scopes, "f3d component history scope source")? {
                    let Some(stream) = ids::native_stream_charged(
                        ctx,
                        &scope.id,
                        "resolve F3D component history scope stream",
                    )?
                    else {
                        continue;
                    };
                    let namespace = match ctx.get_btree_map(
                        &spaces_by_stream,
                        stream,
                        "f3d component namespace stream lookup",
                    )? {
                        None => Some(ComponentHistoryNamespace::Aggregate),
                        Some(records) => ctx
                            .find_by(
                                records.iter().rev(),
                                |record| Ok(**record <= u64::from(scope.record_index)),
                                "f3d component namespace record query",
                            )?
                            .map(|record| ComponentHistoryNamespace::Component(*record)),
                    };
                    if let Some(namespace) = namespace {
                        // discarded-value: A repeated scope ID keeps its last namespace.
                        let _ = ctx.insert_hash_map(
                            &mut component_namespaces,
                            scope.id.as_str(),
                            namespace,
                            "f3d component history namespace",
                        )?;
                    }
                }
                drop(spaces_by_stream);
                drop(naming_storage);
                let mut scopes_by_state = HashMap::new();
                for scope in ctx.admit_iter(scopes, "f3d history state scope source")? {
                    let (Some(stream), Some(state_id)) = (
                        ids::native_stream_charged(
                            ctx,
                            &scope.id,
                            "resolve F3D history state scope stream",
                        )?,
                        scope.history_state_id(),
                    ) else {
                        continue;
                    };
                    let history_id = match &binding {
                        ScopeHistoryBinding::Absent => String::new(),
                        ScopeHistoryBinding::Bound(bound) => {
                            let Some(history_id) = ctx.get_hash_map(
                                bound,
                                scope.id.as_str(),
                                "f3d history state binding lookup",
                            )?
                            else {
                                continue;
                            };
                            ctx.copy_retained_text(history_id, "f3d history state binding id")?
                        }
                    };
                    let Some(namespace) = ctx.get_hash_map(
                        &component_namespaces,
                        scope.id.as_str(),
                        "f3d history state component lookup",
                    )?
                    else {
                        continue;
                    };
                    let key = (
                        ctx.copy_retained_text(stream, "f3d history state stream")?,
                        *namespace,
                        history_id,
                        state_id,
                    );
                    ctx.push_hash_group(
                        &mut scopes_by_state,
                        key,
                        scope,
                        "f3d history state index",
                        "f3d history state scope",
                    )?;
                }
                Ok(Self {
                    binding,
                    component_namespaces,
                    scopes_by_state,
                })
            },
        )
    }

    fn history_id(
        &self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
    ) -> Result<Option<&str>, CodecError> {
        match &self.binding {
            ScopeHistoryBinding::Absent => Ok(Some("")),
            ScopeHistoryBinding::Bound(bound) => Ok(ctx
                .get_hash_map(bound, scope.id.as_str(), "f3d scope history binding lookup")?
                .map(String::as_str)),
        }
    }

    pub(super) fn state_key(
        &self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        state_id: i64,
    ) -> Result<Option<(String, ComponentHistoryNamespace, String, i64)>, CodecError> {
        let (Some(stream), Some(namespace), Some(history_id)) = (
            ids::native_stream_charged(ctx, &scope.id, "resolve F3D history lookup scope stream")?,
            ctx.get_hash_map(
                &self.component_namespaces,
                scope.id.as_str(),
                "f3d history component lookup",
            )?,
            self.history_id(ctx, scope)?,
        ) else {
            return Ok(None);
        };
        Ok(Some((
            ctx.copy_retained_text(stream, "f3d history lookup stream")?,
            *namespace,
            ctx.copy_retained_text(history_id, "f3d history lookup id")?,
            state_id,
        )))
    }

    /// Follow `scope.previous_history_state_id()` until a scope accepted by
    /// `projected` is reached. Internal scopes preserve state continuity but
    /// are not themselves authored top-level features.
    pub(crate) fn predecessor<F>(
        &self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        projected: F,
    ) -> Result<ScopeHistoryPredecessor<'a>, CodecError>
    where
        F: Fn(&DesignParameterScope) -> Result<bool, CodecError>,
    {
        let mut storage = ctx.reserve_scoped(0, "f3d predecessor lookup storage")?;
        storage.with_storage(|| {
            let Some(mut state_id) = scope.previous_history_state_id() else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            let Some(stream) =
                ids::native_stream_charged(ctx, &scope.id, "resolve F3D predecessor scope stream")?
            else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            let Some(history_id) = self.history_id(ctx, scope)? else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            let mut visited = HashSet::new();
            loop {
                let Some(component_namespace) = ctx.get_hash_map(
                    &self.component_namespaces,
                    scope.id.as_str(),
                    "f3d predecessor component lookup",
                )?
                else {
                    return Ok(ScopeHistoryPredecessor::Ambiguous);
                };
                let mut key_storage = ctx.reserve_scoped(0, "hold F3D predecessor lookup key")?;
                let key = key_storage.with_storage(|| -> Result<_, CodecError> {
                    Ok((
                        ctx.copy_retained_text(stream, "f3d predecessor stream")?,
                        *component_namespace,
                        ctx.copy_retained_text(history_id, "f3d predecessor history id")?,
                        state_id,
                    ))
                })?;
                let Some(candidates) =
                    ctx.get_hash_map(&self.scopes_by_state, &key, "f3d predecessor state lookup")?
                else {
                    return Ok(ScopeHistoryPredecessor::None);
                };
                drop(key);
                drop(key_storage);
                let [candidate] = candidates.as_slice() else {
                    return Ok(ScopeHistoryPredecessor::Ambiguous);
                };
                if ctx.equal(
                    &candidate.id,
                    &scope.id,
                    "f3d predecessor scope identity comparison",
                )? {
                    if visited.is_empty() {
                        return Ok(ScopeHistoryPredecessor::None);
                    }
                    return Err(CodecError::Malformed(
                        "Design scope history-state dependency is cyclic".into(),
                    ));
                }
                if projected(candidate)? {
                    return Ok(ScopeHistoryPredecessor::Scope(candidate));
                }
                if !ctx.insert_hash_set(
                    &mut visited,
                    candidate.id.as_str(),
                    "f3d predecessor visited scope",
                )? {
                    return Err(CodecError::Malformed(
                        "Design scope history-state dependency is cyclic".into(),
                    ));
                }
                let Some(previous_state_id) = candidate.previous_history_state_id() else {
                    return Ok(ScopeHistoryPredecessor::None);
                };
                if candidate.history_state_id() == Some(previous_state_id) {
                    return Ok(ScopeHistoryPredecessor::None);
                }
                state_id = previous_state_id;
            }
        })
    }
}

pub(super) fn ensure_feature_dependencies_precede(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d feature dependency index storage")?;
    storage.with_storage(|| {
        let mut ordinals = HashMap::new();
        for feature in ctx.admit_iter(features, "f3d feature dependency ordinal source")? {
            // discarded-value: The distinct-size check below rejects repeated feature IDs.
            let _ = ctx.insert_hash_map(&mut ordinals, &feature.id, feature.ordinal, "f3d feature dependency ordinal index")?;
        }
        if ordinals.len() != features.len() { return Err(CodecError::Malformed("projected Design feature identity is not unique".into())); }
        let mut unique_ordinals = HashSet::new();
        for feature in ctx.admit_iter(features, "f3d feature dependency source")? {
            if !ctx.insert_hash_set(&mut unique_ordinals, feature.ordinal, "f3d feature unique ordinal index")? {
                return Err(CodecError::Malformed("projected Design feature ordinal is not unique".into()));
            }
            if let Some((dependency, dependency_ordinal)) = ctx.find_map(feature.dependencies.iter(), |dependency| {
                Ok(ctx.get_hash_map(&ordinals, dependency, "f3d feature dependency ordinal lookup")?.filter(|ordinal| **ordinal >= feature.ordinal).map(|ordinal| (dependency, ordinal)))
            }, "f3d feature dependency query")? {
                return Err(crate::design::text::malformed_design(ctx, format_args!("Design feature dependency does not precede its authored timeline position: {dependency} at ordinal {dependency_ordinal} -> {} at ordinal {}", feature.id, feature.ordinal)));
            }
        }
        Ok(())
    })
}

/// Project Design parameters and feature scopes, including fixed edge identities.
pub(crate) fn project_parameter_design_with_edge_identities(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'_>,
) -> Result<
    (
        Vec<cadmpeg_ir::features::Feature>,
        Vec<cadmpeg_ir::features::DesignParameter>,
    ),
    CodecError,
> {
    use cadmpeg_ir::features::{
        patterns::PatternKind, DesignParameter as NeutralParameter, DimensionDisplay, Feature,
        FeatureDefinition, FeatureOperation, ParameterValue,
    };
    use cadmpeg_ir::scalar::{Angle, Length};
    use std::collections::BTreeMap;

    let &ProjectInputs {
        native,
        owners,
        scopes,
        timelines,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        edge_treatment_vertex_operands,
        entity_selection_operands,
        curve_identities,
        face_operands,
        body_recipe_operands,
        legacy_loft_body_carriers,
        placements,
        body_bindings,
        component_naming_spaces,
        histories,
        ..
    } = inputs;

    let mut lookup_storage = ctx.reserve_scoped(0, "f3d parameter projection lookup storage")?;
    let source_ordinals =
        lookup_storage.with_storage(|| authored_scope_ordinals(ctx, scopes, timelines))?;

    let mut scope_ids = HashMap::new();
    lookup_storage.with_storage(|| -> Result<(), CodecError> {
        for scope in ctx.admit_iter(scopes, "f3d projected scope source")? {
            let Some(stream) =
                ids::native_stream_charged(ctx, &scope.id, "f3d projected scope stream")?
            else {
                continue;
            };
            if ctx.contains_key_btree_map(
                &source_ordinals,
                &(stream, scope.record_index),
                "f3d projected scope ordinal lookup",
            )? {
                // discarded-value: duplicate scope keys retain the last feature ID.
                let _ = ctx.insert_hash_map(
                    &mut scope_ids,
                    (stream, scope.record_index),
                    crate::design::identity::neutral_feature_id(ctx, scope)?,
                    "f3d projected scope id index",
                )?;
            }
        }
        Ok(())
    })?;
    let mut owners_by_index = HashMap::new();
    lookup_storage.with_storage(|| -> Result<(), CodecError> {
        for owner in ctx.admit_iter(owners, "f3d projected owner index source")? {
            let Some(stream) =
                ids::native_stream_charged(ctx, owner.id(), "f3d projected owner stream")?
            else {
                continue;
            };
            // discarded-value: duplicate owner keys retain the last source record.
            let _ = ctx.insert_hash_map(
                &mut owners_by_index,
                (stream, owner.record_index()),
                owner,
                "f3d projected parameter owner index",
            )?;
        }
        Ok(())
    })?;
    let mut features = ctx.admit_iter(scopes, "f3d projected feature scope source")?
        .map(|scope| -> Result<Option<Feature>, CodecError> {
            let native_scope = ids::native_stream_charged(ctx, &scope.id, "f3d projected feature scope stream")?.unwrap_or(ids::DEFAULT_STREAM);
            let Some(&ordinal) = ctx.get_btree_map(&source_ordinals, &(native_scope, scope.record_index), "f3d projected feature ordinal lookup")? else { return Ok(None); };
            let mut parameter_storage = ctx.reserve_scoped(0, "f3d projected scope parameter storage")?;
            let mut parameters = Vec::new();
            for owner in ctx.admit_iter(owners, "f3d projected scope owner source")? {
                if !feature_stream_matches(ctx, owner.id(), Some(native_scope), "f3d projected scope owner stream")? || owner.scope_record_index() != scope.record_index { continue; }
                if let Some(parameter) = ctx.find_by(native, |parameter| {
                    Ok(feature_stream_matches(ctx, &parameter.id, Some(native_scope), "f3d projected scope parameter stream")? && parameter.record_index == owner.parameter_record_index())
                }, "f3d projected scope parameter source")? {
                    parameter_storage.with_storage(|| ctx.push_vec(&mut parameters, (owner.local_ordinal(), parameter), "f3d projected scope parameter"))?;
                }
            }
            let family = design_feature_family(&scope.kind());
            let mut inserted_bodies: Vec<cadmpeg_ir::ids::BodyId> = Vec::new();
            let definition = match family {
                Some(DesignFeatureFamily::Sketch) => FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved }),
                Some(DesignFeatureFamily::Assemble) => scope
                    .assembly_alignment()
                    .filter(|alignment| {
                        matches!(
                            alignment.form,
                            Some(
                                crate::records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 { .. }
                                    | crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(_)
                            )
                        )
                    })
                    .map_or_else(
                        || native_scope_definition(ctx, scope, &parameters),
                        |_| Ok(FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                            joint: crate::design::identity::neutral_assembly_joint_id(ctx, scope)?,
                        })),
                    )?,
                Some(DesignFeatureFamily::Extrude) => extrude::project_extrude(
                    ctx,
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    placements,
                    body_recipe_operands,
                )?
                .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Fillet) => {
                    project_fillet_arm(ctx, inputs, scope, parameters.as_slice(), native_scope)?
                }
                Some(DesignFeatureFamily::Chamfer) => {
                    let mut chamfer = if parameters.is_empty() {
                        project_fixed_chamfer(
                            scope,
                            construction_groups,
                            edge_operands,
                            edge_identity_operands,
                            edge_treatment_vertex_operands,
                            histories,
                            ctx,
                        )?
                    } else {
                        None
                    };
                    if chamfer.is_none() {
                        chamfer = project_chamfer(
                            scope,
                            &parameters,
                            inputs,
                            ctx,
                        )?;
                    }
                    chamfer.map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Combine) => project_combine(ctx, scope, native_scope)?
                    .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })),
                Some(DesignFeatureFamily::Draft) => project_draft(
                    ctx,
                    scope,
                    scopes,
                    construction_groups,
                    entity_selection_operands,
                    face_operands,
                    histories,
                )?
                .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: scope.kind_name().into(),
                    parameters: BTreeMap::new(),
                })),
                Some(DesignFeatureFamily::ReplaceFace) => project_replace_face(
                    ctx,
                    scope,
                    construction_groups,
                    face_operands,
                    body_recipe_operands,
                )?
                .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Revolve) => project_fixed_revolve_with_entities(
ctx,
scope,
construction_groups,
edge_operands,
entity_selection_operands,
face_operands,
(placements, curve_identities),
)?
                .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: scope.kind_name().into(),
                    parameters: BTreeMap::new(),
                })),
                Some(DesignFeatureFamily::Loft) => project_fixed_loft(
                    scope,
                    construction_groups,
                    legacy_loft_body_carriers,
                    edge_operands,
                    edge_identity_operands,
                    face_operands,
                    ctx,
                )?
                .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: scope.kind_name().into(),
                    parameters: BTreeMap::new(),
                })),
                Some(DesignFeatureFamily::Sweep) => project_fixed_sweep(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    entity_selection_operands,
                    face_operands,
                    ctx,
                )?
                .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: scope.kind_name().into(),
                    parameters: BTreeMap::new(),
                })),
                Some(DesignFeatureFamily::Pipe) => project_fixed_pipe(
                    scope,
                    &parameters,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    ctx,
                )
                ?
                .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::SurfacePatch) => project_surface_patch(
                    ctx,
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                )?
                .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: scope.kind_name().into(),
                    parameters: BTreeMap::new(),
                })),
                Some(DesignFeatureFamily::SurfaceExtend) => {
                    if let Some((operation, distance)) = scope.surface_extend_operation().and_then(|operation| {
                        cadmpeg_ir::scalar::PositiveLength::new(operation.distance.get() * 10.0).map(|distance| (operation, distance))
                    }) {
                            use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
                            use cadmpeg_ir::features::{FaceSelection, SurfaceExtension};

                            let method = match operation.method {
                                DesignSurfaceExtendMethod::Natural => SurfaceExtension::Natural,
                                DesignSurfaceExtendMethod::Tangent => SurfaceExtension::Linear,
                                DesignSurfaceExtendMethod::Perpendicular => {
                                    SurfaceExtension::Perpendicular
                                }
                            };
                            FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                                faces: FaceSelection::Native(ctx.format_retained(format_args!("{}{}{}", native_scope, ":design-record#", u64::from(operation.boundary_record_index)), "f3d surface extend boundary id")?),
                                distance: Some(distance),
                                method,
                            })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }
                }
                Some(DesignFeatureFamily::SurfaceOffset) => scope
                    .surface_offset_operation()
                    .map(|operation| {
                        project_surface_offset(ctx, scope, operation, construction_groups, face_operands)
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })),
                Some(DesignFeatureFamily::SurfaceRuled) => project_ruled_surface(
                    scope,
                    owners,
                    native,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    ctx,
                )?
                .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::SurfaceTrim) => {
                    project_surface_trim(ctx, scope, construction_groups, body_recipe_operands)?
                        .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        }))
                }
                Some(DesignFeatureFamily::BoundaryFill) => {
                    project_boundary_fill(ctx, scope, construction_groups)?.unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    })
                }
                Some(DesignFeatureFamily::Hole) => project_hole(ctx, scope, &parameters, face_operands)?
                    .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Split) => {
                    project_split(ctx, scope, construction_groups, face_operands)?.unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    })
                }
                Some(DesignFeatureFamily::CircularPattern) => {
                    patterns::project_circular_pattern(ctx, scope, construction_groups, face_operands)?
                        .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::UNRESOLVED_CIRCULAR,
                        }))
                }
                Some(DesignFeatureFamily::RectangularPattern) => {
                    patterns::project_rectangular_pattern_scalars(ctx, scope, construction_groups, face_operands)?
                        .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::UNRESOLVED_LINEAR,
                        }))
                }
                Some(DesignFeatureFamily::Mirror) => {
                    patterns::project_mirror(ctx, scope, construction_groups, face_operands, scopes)?
                        .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::UNRESOLVED_MIRROR,
                        }))
                }
                Some(DesignFeatureFamily::OffsetFaces) => {
                    project_offset_faces(ctx, scope, &parameters, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Move) => project_move(ctx, scope, construction_groups)?
                    .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Shell) => {
                    project_shell(ctx, scope, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Thicken) => {
                    project_thicken(ctx, scope, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Coil) => {
                    coil::project_coil(ctx, scope, &parameters, construction_groups)?
                        .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Scale) => if let Some(operation) = scope.scale_operation() {
                        let factor = cadmpeg_ir::scalar::NonZeroReal::from(operation.uniform_factor);
                        let body_group = ctx.find_by(construction_groups, |group| {
                            Ok(feature_stream_matches(ctx, &group.id, Some(native_scope), "f3d Scale body group stream")?
                                && group.scope_record_index == scope.record_index
                                && group.record_index == operation.body_group_record_index)
                        }, "f3d Scale body group source")?;
                        let bodies = match body_group {
                            Some(group) => cadmpeg_ir::features::BodySelection::Native(
                                ctx.copy_retained_text(&group.id, "f3d Scale body group id")?),
                            None => cadmpeg_ir::features::BodySelection::Unresolved,
                        };
                        let center = match operation.center_position.and_then(|center| {
                            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                center.value[0].get() * 10.0, center.value[1].get() * 10.0,
                                center.value[2].get() * 10.0,
                            ))
                        }) {
                            Some(center) => cadmpeg_ir::features::ScaleCenter::Point(center),
                            None => cadmpeg_ir::features::ScaleCenter::Native(
                                ctx.format_retained(format_args!("{}{}{}", native_scope, ":design-record#", u64::from(operation.center_record_index)), "f3d Scale center id")?),
                        };
                        FeatureDefinition::Operation(FeatureOperation::Scale {
                            bodies,
                            center: Some(center),
                            factors: cadmpeg_ir::features::ScaleFactors::Uniform { factor },
                        })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                        })
                    },
                Some(DesignFeatureFamily::Thread) => scope
                    .thread_construction()
                    .and_then(|construction| {
                        construction
                            .nominal_size
                            .value()
                            .ok()
                            .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                            .map(|size| (construction, size))
                    })
                    .map_or_else(
                        || native_scope_definition(ctx, scope, &parameters),
                        |(construction, nominal_size)| {
                            let face = project_thread_face_selection(
                                ctx,
                                scope,
                                &construction.face_group_record_indices,
                                construction_groups,
                                face_operands,
                            )?;
                            let has_parameter_owners = ctx.any_by(owners, |owner| {
                                Ok(feature_stream_matches(ctx, owner.id(), Some(native_scope), "f3d Thread parameter owner stream")?
                                    && owner.scope_record_index() == scope.record_index)
                            }, "f3d Thread parameter owner source")?;
                            let face_reference_count = construction.face_group_record_indices.len().checked_mul(2);
                            let full_face_extent = !has_parameter_owners
                                && Some(scope.reference_members().len()) == face_reference_count
                                && ctx.all_by(construction.face_group_record_indices.iter().enumerate(), |(group_ordinal, group_record_index)| {
                                    let Some(pair_at) = group_ordinal.checked_mul(2) else { return Ok(false); };
                                    let Some(member_at) = pair_at.checked_add(1) else { return Ok(false); };
                                    let Some(member_record_index) = scope.reference_members().values().nth(member_at).copied() else { return Ok(false); };
                                    Ok(scope.reference_members().values().nth(pair_at) == Some(group_record_index)
                                        && ctx.any_by(construction_groups, |group| {
                                            Ok(feature_stream_matches(ctx, &group.id, Some(native_scope), "f3d Thread full extent group stream")?
                                                && group.scope_record_index == scope.record_index
                                                && group.record_index == *group_record_index
                                                && group.role() == DesignOperandRole::ROLE_0X10
                                                && matches!(group.members(), [member] if member.value == member_record_index))
                                        }, "f3d Thread full extent group source")?)
                                }, "f3d Thread full extent reference source")?;
                            let extent = ctx.find_by(&parameters, |(ordinal, _)| Ok(*ordinal == 1), "f3d Thread extent parameter source")?
                                .and_then(|(_, parameter)| design_positive_length(parameter))
                                .map(|length| cadmpeg_ir::features::CosmeticThreadExtent::Blind {
                                    length,
                                })
                                .or_else(|| {
                                    full_face_extent.then_some(
                                        cadmpeg_ir::features::CosmeticThreadExtent::Through {},
                                    )
                                });
                            Ok(FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                                face,
                                diameter: Some(nominal_size),
                                extent,
                            }))
                        },
                    )?,
                Some(DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    project_edge_flange(scope, inputs, ctx)?
                        .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::SheetMetalHem) => project_hem(scope, inputs, ctx)?
                    .map_or_else(|| native_scope_definition(ctx, scope, &parameters), Ok)?,
                None => {
                    if let Some(primitive) = fixed_geometry::project_solid_primitive(scope) {
                        primitive
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::JointOrigin {
                        scope.joint_origin_transform().and_then(|transform| cadmpeg_ir::features::FeatureCoordinateFrame::new(Point3::new(
                                    transform[0][3] * 10.0,
                                    transform[1][3] * 10.0,
                                    transform[2][3] * 10.0,
                                ), Vector3::new(
                                    transform[0][0],
                                    transform[1][0],
                                    transform[2][0],
                                ), Vector3::new(
                                    transform[0][1],
                                    transform[1][1],
                                    transform[2][1],
                                ), Vector3::new(
                                    transform[0][2],
                                    transform[1][2],
                                    transform[2][2],
                                ))).map_or_else(
                            || native_scope_definition(ctx, scope, &parameters),
                            |frame| Ok(FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame })),
                        )?
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane {
                        scope.work_plane_transform().map_or_else(
                            || native_scope_definition(ctx, scope, &parameters),
                            |transform| project_work_plane(ctx, scope, transform.into()),
                        )?
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkAxis {
                        scope
                            .work_axis_construction()
                            .and_then(|construction| {
                                let displacement = Vector3::new(
                                    construction.displacement[0].get(),
                                    construction.displacement[1].get(),
                                    construction.displacement[2].get(),
                                );
                                Some((
                                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                        construction.origin[0].get() * 10.0,
                                        construction.origin[1].get() * 10.0,
                                        construction.origin[2].get() * 10.0,
                                    ))?,
                                    cadmpeg_ir::features::FeatureDirection3::from(cadmpeg_ir::units::UnitVector3::normalized(displacement)?),
                                ))
                            })
                            .map_or_else(
                                || native_scope_definition(ctx, scope, &parameters),
                                |(origin, direction)| Ok(FeatureDefinition::Operation(FeatureOperation::DatumAxis { origin, direction })),
                            )?
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint {
                        scope.work_point_construction().and_then(|construction| Some((
                            construction,
                            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                construction.position[0].get() * 10.0,
                                construction.position[1].get() * 10.0,
                                construction.position[2].get() * 10.0,
                            ))?,
                        ))).map_or_else(
                            || native_scope_definition(ctx, scope, &parameters),
                            |(construction, position)| Ok(FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                                position,
                                construction: project_work_point_construction(
                                    ctx,
                                    scope,
                                    construction,
                                    &parameters,
                                    edge_operands,
                                    &scope_ids,
                                )?
                                .map(Box::new),
                            })),
                        )?
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange {
                        project_base_flange(ctx, scope, construction_groups, placements)?.unwrap_or_else(
                            || FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            }),
                        )
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::RemoveBody {
                        project_remove_body(ctx, scope, construction_groups)?.unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch {
                        project_surface_stitch(ctx, scope, construction_groups)?.unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace {
                        project_split_face(
                            ctx,
                            scope,
                            scopes,
                            construction_groups,
                            entity_selection_operands,
                            face_operands,
                            histories,
                        )?
                        .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        }))
                    } else if matches!(
                        scope.kind(),
                        crate::records::feature::scope::DesignFeatureKind::DeleteFace
                            | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                    ) {
                        project_delete_face(ctx, scope, construction_groups, face_operands)?
                            .unwrap_or_else(|| FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            }))
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies {
                        if let Some(operation) = scope.copy_paste_bodies_operation() {
                            let selection = design_body_selection(
                                ctx,
                                scope,
                                operation.bodies().iter().map(|body| u64::from(body.copied.value)),
                                body_bindings,
                            )?;
                            let bodies = match selection {
                                cadmpeg_ir::features::BodySelection::Resolved { bodies, native } => {
                                    for body in ctx.admit_iter(bodies.as_slice(), "f3d moved body selection source")? {
                                        let id = (body).try_clone_for_decode(ctx, "f3d copied body output id")?;
                                        ctx.push_vec(&mut inserted_bodies, id, "f3d copied body output")?;
                                    }
                                    cadmpeg_ir::features::InsertedBodies::Resolved { native }
                                }
                                _ => cadmpeg_ir::features::InsertedBodies::Native(
                                    ctx.copy_retained_text(&scope.id, "f3d copied body native selection")?),
                            };
                            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::CopyPaste {
                        scope.copy_paste_component_operation().map_or_else(
                            || FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            }),
                            |operation| FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                                occurrence: crate::ids::neutral_component_occurrence_id(
                                    &operation.copied_occurrence_guid,
                                ),
                            }),
                        )
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFeature {
                        if let Some(construction) = scope.base_feature_construction() {
                            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                                bodies: design_body_selection(ctx, scope,
                                    construction.body_entity_suffixes(), body_bindings)?,
                            })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else {
                        native_scope_definition(ctx, scope, &parameters)?
                    }
                }
            };
            let outputs = inserted_bodies;
            Ok(Some(Feature {
                id: ctx.get_hash_map(&scope_ids, &(native_scope, scope.record_index), "f3d projected feature id lookup")?.ok_or_else(|| CodecError::malformed("projected Design scope has no feature ID"))?.try_clone_for_decode(ctx, "f3d projected feature id")?,
                ordinal,
                name: Some(ctx.format_retained(format_args!("{} {}", scope.kind_name(), scope.feature_ordinal), "f3d projected feature name")?),
                suppressed: Some(
                    matches!(
                        family,
                        Some(
                            DesignFeatureFamily::Extrude
                                | DesignFeatureFamily::Fillet
                                | DesignFeatureFamily::Chamfer
                        )
                    ) && scope.history_state_id().is_none()
                        && scope.previous_history_state_id().is_none(),
                ),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: if matches!(&definition, FeatureDefinition::Operation(FeatureOperation::Native { .. })) {
                    scope_properties(ctx, scope, native_scope, placements)?
                } else {
                    BTreeMap::new()
                },
                source_tag: Some(ctx.copy_retained_text(scope.kind_name(), "f3d projected feature source tag")?),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                    definition,
                    cadmpeg_ir::features::DistinctMembers::try_from(outputs, ctx)
                        .map_err(cadmpeg_core::CodecError::from)?,
                ),
                native_ref: Some(ctx.copy_retained_text(&scope.id, "f3d projected feature native reference")?),
            }))
        })
        .try_fold(Vec::new(), |mut features, feature| {
            if let Some(feature) = feature? { ctx.push_vec(&mut features, feature, "f3d projected feature output")?; }
            Ok::<_, CodecError>(features)
        })?;
    let (scope_history, _scope_history_storage) = ScopeHistoryGraph::new(
        ctx,
        scopes,
        body_bindings,
        body_recipe_operands,
        component_naming_spaces,
        histories,
    )?;
    for feature in ctx.admit_iter(&mut features, "f3d projected feature dependency source")? {
        let scope = if let Some(native_ref) = feature.native_ref.as_deref() {
            ctx.find_by(
                scopes,
                |scope| {
                    ctx.equal(
                        scope.id.as_str(),
                        native_ref,
                        "f3d predecessor native scope comparison",
                    )
                },
                "f3d predecessor native scope source",
            )?
        } else {
            None
        };
        let Some(scope) = scope else {
            continue;
        };
        let ScopeHistoryPredecessor::Scope(predecessor_scope) =
            scope_history.predecessor(ctx, scope, |candidate| {
                let stream = ids::native_stream_charged(
                    ctx,
                    &candidate.id,
                    "f3d projected predecessor stream",
                )?
                .unwrap_or(ids::DEFAULT_STREAM);
                ctx.contains_key_hash_map(
                    &scope_ids,
                    &(stream, candidate.record_index),
                    "f3d projected predecessor membership",
                )
            })?
        else {
            continue;
        };
        let stream = ids::native_stream_charged(
            ctx,
            &predecessor_scope.id,
            "f3d projected predecessor_scope stream",
        )?
        .unwrap_or(ids::DEFAULT_STREAM);
        let Some(predecessor) = ctx.get_hash_map(
            &scope_ids,
            &(stream, predecessor_scope.record_index),
            "f3d projected predecessor id lookup",
        )?
        else {
            continue;
        };
        if !ctx.equal(
            predecessor,
            &feature.id,
            "f3d projected predecessor identity comparison",
        )? {
            insert_feature_dependency(ctx, &mut feature.dependencies, predecessor)?;
        }
    }
    for feature in ctx.admit_iter(&mut features, "f3d projected feature dependency source")? {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for dependency in ctx
            .admit_iter(seeds.as_slice(), "f3d projected pattern seed source")?
            .filter_map(|seed| match seed {
                cadmpeg_ir::features::patterns::PatternSeed::Feature(feature) => Some(feature),
                _ => None,
            })
        {
            if !ctx.equal(
                dependency,
                &feature.id,
                "f3d projected seed dependency comparison",
            )? {
                insert_feature_dependency(ctx, &mut feature.dependencies, dependency)?;
            }
        }
    }
    for feature in ctx.admit_iter(&mut features, "f3d projected feature dependency source")? {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut add_dependency = |dependency: &cadmpeg_ir::features::FeatureId| {
            if !ctx.equal(
                dependency,
                feature_id,
                "f3d projected construction dependency comparison",
            )? {
                insert_feature_dependency(ctx, dependencies, dependency)?;
            }
            Ok::<_, CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Draft { anchor, .. }) => {
                if let Some(plane) = anchor.pull().and_then(|pull| pull.plane.as_ref()) {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Plane { plane },
                ..
            }) => add_dependency(plane)?,
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
                ..
            }) => {
                for plane in ctx.admit_iter(&planes[..], "f3d projected split plane source")? {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                use cadmpeg_ir::features::{
                    DatumPlaneReference, DatumPointConstruction, VertexSelection,
                };
                match construction.as_ref() {
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        for plane in
                            ctx.admit_iter(&planes[..], "f3d feature plane dependency source")?
                        {
                            if let DatumPlaneReference::Feature { feature } = plane {
                                add_dependency(feature)?;
                            }
                        }
                    }
                    DatumPointConstruction::EdgePlaneIntersection {
                        plane: DatumPlaneReference::Feature { feature },
                        ..
                    } => add_dependency(feature)?,
                    DatumPointConstruction::Vertex {
                        vertex: VertexSelection::Generated { vertex, .. },
                    } => add_dependency(&vertex.feature)?,
                    _ => {}
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane {
                points, ..
            }) => {
                for point in ctx.admit_iter(&points[..], "f3d feature point dependency source")? {
                    if let cadmpeg_ir::features::VertexSelection::Generated { vertex, .. } = point {
                        add_dependency(&vertex.feature)?;
                    }
                }
            }
            _ => {}
        }
    }
    let mut history_state_features = HashMap::<
        (String, ComponentHistoryNamespace, String, i64),
        Option<cadmpeg_ir::features::FeatureId>,
    >::new();
    lookup_storage.with_storage(|| -> Result<(), CodecError> {
        for scope in ctx.admit_iter(scopes, "f3d projected scope source")? {
            let Some(state_id) = scope.history_state_id() else {
                continue;
            };
            let mut key_storage = ctx.reserve_scoped(0, "hold F3D history state lookup key")?;
            let Some(key) =
                key_storage.with_storage(|| scope_history.state_key(ctx, scope, state_id))?
            else {
                continue;
            };
            let stream = ids::native_stream_charged(ctx, &scope.id, "f3d projected scope stream")?
                .unwrap_or(ids::DEFAULT_STREAM);
            let Some(feature_id) = ctx.get_hash_map(
                &scope_ids,
                &(stream, scope.record_index),
                "f3d feature history scope id lookup",
            )?
            else {
                continue;
            };
            if let Some(candidate) = ctx.get_mut_hash_map(
                &mut history_state_features,
                &key,
                "f3d feature history state lookup",
            )? {
                *candidate = None;
            } else {
                let id = (feature_id).try_clone_for_decode(ctx, "f3d feature history state id")?;
                let key = key_storage.commit_value(key)?;
                // discarded-value: the vacant-key check admits this state.
                let _ = ctx.insert_hash_map(
                    &mut history_state_features,
                    key,
                    Some(id),
                    "f3d feature history state index",
                )?;
            }
        }
        Ok(())
    })?;
    let mut work_point_features = features.iter_mut();
    while let Some(feature) = ctx.next_charged(
        &mut work_point_features,
        "visit F3D WorkPoint dependency features",
    )? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = ctx.find_map(
            scopes,
            |scope| {
                Ok(ctx
                    .equal_bytes(
                        scope.id.as_bytes(),
                        native_ref.as_bytes(),
                        "compare F3D WorkPoint dependency scope",
                    )?
                    .then_some(scope))
            },
            "visit F3D WorkPoint dependency scopes",
        )?
        else {
            continue;
        };
        let Some(construction) = scope.work_point_construction() else {
            continue;
        };
        visit_work_point_history_states(ctx, scope, construction, edge_operands, |state_id| {
            let mut key_storage = ctx.reserve_scoped(0, "hold F3D WorkPoint history lookup key")?;
            let Some(key) =
                key_storage.with_storage(|| scope_history.state_key(ctx, scope, state_id))?
            else {
                return Ok(true);
            };
            let Some(Some(dependency)) = ctx.get_hash_map(
                &history_state_features,
                &key,
                "f3d WorkPoint history dependency lookup",
            )?
            else {
                return Ok(true);
            };
            if !ctx.equal_bytes(
                dependency.as_str().as_bytes(),
                feature.id.as_str().as_bytes(),
                "compare F3D WorkPoint dependency identity",
            )? {
                insert_feature_dependency(ctx, &mut feature.dependencies, dependency)?;
            }
            Ok(true)
        })?;
    }
    ctx.stable_sort_by(
        &mut features[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design feature_project 3",
    )?;

    let mut parameters = ctx
        .admit_iter(native, "f3d projected parameter source")?
        .map(|parameter| -> Result<NeutralParameter, CodecError> {
            let stream =
                ids::native_stream_charged(ctx, &parameter.id, "f3d projected parameter stream")?
                    .unwrap_or(ids::DEFAULT_STREAM);
            let native_owner = parameter
                .owner_record_index()
                .map(|record_index| {
                    ctx.get_hash_map(
                        &owners_by_index,
                        &(stream, record_index),
                        "f3d projected parameter owner lookup",
                    )
                })
                .transpose()?
                .flatten();
            let owner = native_owner
                .map(|owner| {
                    ctx.get_hash_map(
                        &scope_ids,
                        &(stream, owner.scope_record_index()),
                        "f3d projected parameter scope lookup",
                    )
                })
                .transpose()?
                .flatten();
            let mut properties = BTreeMap::new();
            if parameter.kind() != DesignParameterKind::User {
                ctx.insert_btree_map(
                    &mut properties,
                    cadmpeg_core::nonblank_literal!("source_kind"),
                    ctx.copy_retained_text(
                        parameter.source_kind(),
                        "f3d projected parameter source kind",
                    )?,
                    "f3d projected parameter property",
                )
                .map(|_| ())?;
            }
            if let (Some(owner_record_index), None) = (parameter.owner_record_index(), owner) {
                ctx.insert_btree_map(
                    &mut properties,
                    cadmpeg_core::nonblank_literal!("owner_record_index"),
                    ctx.format_retained(
                        format_args!("{owner_record_index}"),
                        "f3d projected parameter owner record text",
                    )?,
                    "f3d projected parameter property",
                )
                .map(|_| ())?;
            }
            let value = match parameter.unit().map(|field| field.value.as_str()) {
                Some(unit) if design_length_unit(unit) => {
                    Length::new(parameter.evaluated_value().get() * 10.0)
                        .map(ParameterValue::Length)
                }
                Some(unit) if design_angle_unit(unit) => {
                    Angle::new(parameter.evaluated_value().get()).map(ParameterValue::Angle)
                }
                None => Some(ParameterValue::Real(parameter.evaluated_value())),
                Some(unit) => {
                    ctx.insert_btree_map(
                        &mut properties,
                        cadmpeg_core::nonblank_literal!("unit"),
                        ctx.copy_retained_text(unit, "f3d projected parameter unit")?,
                        "f3d projected parameter property",
                    )
                    .map(|_| ())?;
                    ctx.insert_btree_map(
                        &mut properties,
                        cadmpeg_core::nonblank_literal!("evaluated_scalar"),
                        ctx.format_retained(
                            format_args!("{}", parameter.evaluated_value().get()),
                            "f3d projected parameter evaluated scalar text",
                        )?,
                        "f3d projected parameter property",
                    )
                    .map(|_| ())?;
                    None
                }
            };
            Ok(NeutralParameter {
                id: crate::design::identity::neutral_parameter_id(ctx, parameter)?,
                owner: owner
                    .map(|id| (id).try_clone_for_decode(ctx, "f3d projected parameter owner id"))
                    .transpose()?,
                ordinal: owner
                    .zip(native_owner)
                    .map_or(parameter.source_ordinal, |(_, owner)| owner.local_ordinal()),
                name: ctx.copy_retained_text(parameter.name(), "f3d projected parameter name")?,
                expression: ctx.copy_retained_text(
                    parameter.expression(),
                    "f3d projected parameter expression",
                )?,
                display: if ctx.contains_text(
                    parameter.source_kind(),
                    "Diameter Dimension",
                    "f3d parameter diameter display kind",
                )? {
                    Some(DimensionDisplay::Diameter)
                } else if ctx.contains_text(
                    parameter.source_kind(),
                    "Radius Dimension",
                    "f3d parameter radius display kind",
                )? {
                    Some(DimensionDisplay::Radius)
                } else {
                    None
                },
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties,
                pmi: None,
                native_ref: Some(ctx.copy_retained_text(
                    &parameter.id,
                    "f3d projected parameter native reference",
                )?),
            })
        })
        .try_fold(Vec::new(), |mut projected, parameter| {
            ctx.push_vec(&mut projected, parameter?, "f3d projected parameter output")?;
            Ok::<_, CodecError>(projected)
        })?;
    let (
        parameter_scopes,
        document_aliases,
        feature_aliases,
        owned_aliases,
        parameter_owners,
        feature_order,
    ) = lookup_storage.with_storage(|| -> Result<_, CodecError> {
        let mut parameter_scopes = HashMap::new();
        for (source, parameter) in ctx
            .admit_iter(native, "f3d parameter scope source")?
            .zip(&parameters)
        {
            if let Some(stream) =
                ids::native_stream_charged(ctx, &source.id, "resolve F3D parameter scope stream")?
            {
                let id = parameter
                    .id
                    .try_clone_for_decode(ctx, "f3d parameter scope index id")?;
                // discarded-value: A repeated parameter ID keeps its last source stream.
                let _ = ctx.insert_hash_map(
                    &mut parameter_scopes,
                    id,
                    stream,
                    "f3d parameter scope index",
                )?;
            }
        }
        let mut document_aliases = HashMap::<(&str, &str), Option<usize>>::new();
        let mut feature_aliases =
            HashMap::<(&str, cadmpeg_ir::features::FeatureId, &str), Option<usize>>::new();
        let mut owned_aliases = HashMap::<(&str, &str), Vec<usize>>::new();
        for (index, (source, parameter)) in ctx
            .admit_iter(native, "f3d parameter alias source")?
            .zip(&parameters)
            .enumerate()
        {
            let scope = *ctx
                .get_hash_map(
                    &parameter_scopes,
                    &parameter.id,
                    "f3d parameter alias scope lookup",
                )?
                .ok_or_else(|| CodecError::malformed("Design parameter has no source stream"))?;
            if let Some(owner) = &parameter.owner {
                let key = (
                    scope,
                    owner.try_clone_for_decode(ctx, "f3d feature alias owner id")?,
                    source.name(),
                );
                if let Some(candidate) =
                    ctx.get_mut_hash_map(&mut feature_aliases, &key, "f3d feature alias lookup")?
                {
                    *candidate = None;
                } else {
                    // discarded-value: The alias lookup proved the key absent.
                    let _ = ctx.insert_hash_map(
                        &mut feature_aliases,
                        key,
                        Some(index),
                        "f3d feature alias index",
                    )?;
                }
                ctx.push_hash_group(
                    &mut owned_aliases,
                    (scope, source.name()),
                    index,
                    "f3d owned alias index",
                    "f3d owned alias member",
                )?;
            } else {
                let key = (scope, source.name());
                if let Some(candidate) =
                    ctx.get_mut_hash_map(&mut document_aliases, &key, "f3d document alias lookup")?
                {
                    *candidate = None;
                } else {
                    // discarded-value: The alias lookup proved the key absent.
                    let _ = ctx.insert_hash_map(
                        &mut document_aliases,
                        key,
                        Some(index),
                        "f3d document alias index",
                    )?;
                }
            }
        }
        let mut parameter_owners = HashMap::new();
        for parameter in ctx.admit_iter(&parameters, "f3d parameter owner index source")? {
            let key = parameter
                .id
                .try_clone_for_decode(ctx, "f3d parameter owner index id")?;
            let owner = parameter
                .owner
                .as_ref()
                .map(|id| id.try_clone_for_decode(ctx, "f3d parameter owner index owner id"))
                .transpose()?;
            // discarded-value: A repeated parameter ID keeps its last owner.
            let _ = ctx.insert_hash_map(
                &mut parameter_owners,
                key,
                owner,
                "f3d parameter owner index",
            )?;
        }
        let mut feature_order = HashMap::new();
        for feature in ctx.admit_iter(&features, "f3d feature order index source")? {
            // discarded-value: A repeated feature ID keeps its last ordinal.
            let _ = ctx.insert_hash_map(
                &mut feature_order,
                &feature.id,
                feature.ordinal,
                "f3d feature order index",
            )?;
        }
        Ok((
            parameter_scopes,
            document_aliases,
            feature_aliases,
            owned_aliases,
            parameter_owners,
            feature_order,
        ))
    })?;
    for index in ctx.admit_iter(0..parameters.len(), "f3d parameter dependency source")? {
        let parameter = &parameters[index];
        let scope = *ctx
            .get_hash_map(
                &parameter_scopes,
                &parameter.id,
                "f3d parameter dependency scope lookup",
            )?
            .ok_or_else(|| CodecError::malformed("Design parameter has no source stream"))?;
        if ctx.contains_key_btree_map(
            &parameter.properties,
            "owner_record_index",
            "f3d parameter unresolved owner lookup",
        )? {
            continue;
        }
        let mut dependency_storage =
            ctx.reserve_scoped(0, "f3d parameter dependency membership scratch")?;
        let mut seen = HashSet::new();
        let mut dependencies = Vec::new();
        for identifier in expression_identifiers(ctx, &parameter.expression) {
            let identifier = identifier?;
            let alias_key = (scope, identifier);
            let preceding_owned = || -> Result<Option<usize>, CodecError> {
                let Some(consumer) = parameter.owner.as_ref() else {
                    return Ok(None);
                };
                let Some(consumer_order) = ctx.get_hash_map(
                    &feature_order,
                    consumer,
                    "f3d expression consumer order lookup",
                )?
                else {
                    return Ok(None);
                };
                let Some(candidates) = ctx.get_hash_map(
                    &owned_aliases,
                    &alias_key,
                    "f3d expression owned alias lookup",
                )?
                else {
                    return Ok(None);
                };
                unique_feature_match_for_decode(
                    ctx,
                    candidates.iter().copied(),
                    |candidate| {
                        let Some(Some(owner)) = ctx.get_hash_map(
                            &parameter_owners,
                            &parameters[candidate].id,
                            "f3d expression candidate owner lookup",
                        )?
                        else {
                            return Ok(false);
                        };
                        Ok(ctx
                            .get_hash_map(
                                &feature_order,
                                owner,
                                "f3d expression candidate order lookup",
                            )?
                            .is_some_and(|order| order < consumer_order))
                    },
                    "f3d expression preceding owned alias query",
                )
            };
            let candidate = if let Some(owner) = &parameter.owner {
                let mut owner_storage = ctx.reserve_scoped(0, "f3d expression owner lookup")?;
                let owner_key = owner_storage.with_storage(|| {
                    owner.try_clone_for_decode(ctx, "f3d expression owner lookup")
                })?;
                match ctx.get_hash_map(
                    &feature_aliases,
                    &(scope, owner_key, identifier),
                    "f3d expression feature alias lookup",
                )? {
                    Some(None) => None,
                    Some(Some(local)) => Some(*local),
                    None => match ctx.get_hash_map(
                        &document_aliases,
                        &alias_key,
                        "f3d expression document alias lookup",
                    )? {
                        Some(Some(document)) => Some(*document),
                        Some(None) => None,
                        None => preceding_owned()?,
                    },
                }
            } else {
                ctx.get_hash_map(
                    &document_aliases,
                    &alias_key,
                    "f3d expression document alias lookup",
                )?
                .copied()
                .flatten()
            };
            let Some(candidate) = candidate else {
                continue;
            };
            let candidate = &parameters[candidate].id;
            let dependency_owner = ctx.get_hash_map(
                &parameter_owners,
                candidate,
                "f3d parameter dependency owner lookup",
            )?;
            let allowed = match (dependency_owner, &parameter.owner) {
                (Some(Some(dependency_owner)), Some(consumer_owner)) => {
                    ctx.equal(
                        dependency_owner,
                        consumer_owner,
                        "f3d parameter dependency owner comparison",
                    )? || ctx
                        .get_hash_map(
                            &feature_order,
                            dependency_owner,
                            "f3d parameter dependency feature order lookup",
                        )?
                        .zip(ctx.get_hash_map(
                            &feature_order,
                            consumer_owner,
                            "f3d parameter consumer feature order lookup",
                        )?)
                        .is_some_and(|(dependency, consumer)| dependency < consumer)
                }
                (Some(Some(_)), None) => false,
                (Some(_), _) => true,
                (None, _) => false,
            };
            if !allowed
                || ctx.equal(
                    candidate,
                    &parameter.id,
                    "f3d parameter self dependency comparison",
                )?
                || !dependency_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut seen, candidate, "f3d parameter dependency membership")
                })?
            {
                continue;
            }
            let dependency = candidate.try_clone_for_decode(ctx, "f3d parameter dependency id")?;
            ctx.push_vec(&mut dependencies, dependency, "f3d parameter dependency")?;
        }
        drop(seen);
        drop(dependency_storage);
        parameters[index].dependencies =
            cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)?;
    }

    drop(feature_order);
    parameter_order::normalize_parameter_ordinals(ctx, &mut parameters, &parameter_owners)?;
    for feature in ctx.admit_iter(&mut features, "f3d parameter feature dependency source")? {
        for parameter in ctx.admit_iter(&parameters, "f3d owned parameter dependency source")? {
            let Some(owner) = &parameter.owner else {
                continue;
            };
            if !ctx.equal(owner, &feature.id, "f3d owned parameter feature comparison")? {
                continue;
            }
            for dependency in ctx.admit_iter(
                parameter.dependencies.as_slice(),
                "f3d parameter feature dependency member source",
            )? {
                if let Some(Some(owner)) = ctx.get_hash_map(
                    &parameter_owners,
                    dependency,
                    "f3d parameter feature dependency owner lookup",
                )? {
                    if !ctx.equal(
                        owner,
                        &feature.id,
                        "f3d parameter feature dependency owner comparison",
                    )? {
                        insert_feature_dependency(ctx, &mut feature.dependencies, owner)?;
                    }
                }
            }
        }
    }
    ensure_feature_dependencies_precede(ctx, &features)?;
    ctx.stable_sort_by(
        &mut parameters[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design feature_project 4",
    )?;
    Ok((features, parameters))
}

fn work_point_edge_operand<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    input: &crate::records::feature::work_geometry::DesignWorkPointInput,
    edge_operands: &'a [DesignEdgeOperand],
) -> Result<Option<&'a DesignEdgeOperand>, CodecError> {
    let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
        operand_id,
    } = or_none!(input.carrier())
    else {
        return Ok(None);
    };
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D WorkPoint scope stream"
    )?);
    unique_feature_match_for_decode(
        ctx,
        edge_operands.iter(),
        |operand| {
            Ok(ctx.equal_bytes(
                operand.id.as_bytes(),
                operand_id.as_bytes(),
                "compare F3D WorkPoint operand identity",
            )? && feature_stream_matches(
                ctx,
                &operand.id,
                Some(stream),
                "compare F3D WorkPoint operand stream",
            )? && operand.scope_record_index == scope.record_index
                && operand.record_index() == input.record_index())
        },
        "visit F3D WorkPoint edge operands",
    )
}

fn visit_work_point_history_states(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction: &crate::records::feature::work_geometry::DesignWorkPointConstruction,
    edge_operands: &[DesignEdgeOperand],
    mut visit: impl FnMut(i64) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    use crate::records::feature::work_geometry::{
        DesignWorkPointInputCarrier, DesignWorkPointRuleForm,
    };

    // Supported rules have fixed arity. Only Native carries a counted run.
    let counted = matches!(
        construction.rule.form(),
        DesignWorkPointRuleForm::Native { .. }
    );
    let mut inputs = construction.rule.inputs().iter();
    while let Some(input) = if counted {
        ctx.next_charged(&mut inputs, "visit F3D WorkPoint history inputs")?
    } else {
        inputs.next()
    } {
        let state = match input.carrier() {
            Some(DesignWorkPointInputCarrier::EdgeRecipe { .. }) => {
                work_point_edge_operand(ctx, scope, input, edge_operands)?
                    .and_then(|operand| operand.recipe_state_id)
            }
            Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) => {
                recipe.resolution.map(|resolution| resolution.state_id)
            }
            Some(
                DesignWorkPointInputCarrier::WorkPlane { .. }
                | DesignWorkPointInputCarrier::SketchPoint { .. },
            )
            | None => None,
        };
        if let Some(state) = state {
            let result = visit(state);

            if !result? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(crate) fn work_point_recipe_state_id(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    edge_operands: &[DesignEdgeOperand],
) -> Result<Option<i64>, CodecError> {
    let construction = or_none!(scope.work_point_construction());
    let mut state = None;
    let consistent =
        visit_work_point_history_states(ctx, scope, construction, edge_operands, |candidate| {
            if let Some(state) = state {
                Ok(candidate == state)
            } else {
                state = Some(candidate);
                Ok(true)
            }
        })?;
    Ok(if consistent { state } else { None })
}

pub(crate) fn work_plane_recipe_state_id(scope: &DesignParameterScope) -> Option<i64> {
    scope.work_plane_construction()?.inputs()[0]
        .resolution
        .map(|resolution| resolution.state_id)
}

fn project_work_point_construction(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction: &crate::records::feature::work_geometry::DesignWorkPointConstruction,
    parameters: &[(u32, &DesignParameter)],
    edge_operands: &[DesignEdgeOperand],
    scope_ids: &HashMap<(&str, u32), cadmpeg_ir::features::FeatureId>,
) -> Result<Option<cadmpeg_ir::features::DatumPointConstruction>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(
        "f3d WorkPoint accepted construction",
        || -> Result<Option<cadmpeg_ir::features::DatumPointConstruction>, CodecError> {
            use crate::records::feature::work_geometry::{
                DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRuleForm,
            };
            use cadmpeg_ir::features::{
                DatumPlaneReference, DatumPointConstruction, EdgeSelection, VertexSelection,
            };

            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "f3d WorkPoint scope stream"
            )?);
            let edge = |input: &DesignWorkPointInput| -> Result<Option<EdgeSelection>, CodecError> {
                let Some(operand) = work_point_edge_operand(ctx, scope, input, edge_operands)?
                else {
                    return Ok(None);
                };
                let resolved = match operand.recipe_state_id {
                    Some(state_id) => {
                        crate::design::edge_resolve::resolved_edge_operand(operand, ctx)?
                            .map(|edge_slot| (state_id, edge_slot))
                    }
                    None => None,
                };
                let Some((state_id, edge_slot)) = resolved else {
                    return Ok(Some(EdgeSelection::Native(ctx.copy_retained_text(
                        &operand.id,
                        "f3d WorkPoint native edge operand id",
                    )?)));
                };
                let ((feature_id, prefix), _identity_storage) =
                    ctx.with_scoped_storage("f3d WorkPoint identity storage", || {
                        let feature_id = crate::design::identity::neutral_feature_id(ctx, scope)?;
                        let feature_key =
                            crate::design::identity::identity_key(ctx, feature_id.as_str())?;
                        let prefix = crate::design::identity::history_input_prefix(
                            ctx,
                            feature_key,
                            state_id,
                        )?;
                        Ok::<_, CodecError>((feature_id, prefix))
                    })?;
                Ok(Some(match cadmpeg_ir::features::EdgeSelection::historical(
                    crate::design::identity::feature_input_topology_id(ctx, &feature_id, state_id)?,
                    vec![crate::design::identity::history_input_edge_id(
                        ctx,
                        &prefix,
                        edge_slot,
                        "f3d historical edge identifier",
                    )?],
                    ctx.copy_retained_text(
                        &operand.id,
                        "f3d WorkPoint historical edge operand id",
                    )?,
                    ctx,
                )? {
                    Ok(selection) => selection,
                    Err(_) => EdgeSelection::Native(ctx.copy_retained_text(
                        &operand.id,
                        "f3d WorkPoint fallback edge operand id",
                    )?),
                }))
            };
            let plane =
                |input: &DesignWorkPointInput| -> Result<Option<DatumPlaneReference>, CodecError> {
                    let Some(DesignWorkPointInputCarrier::WorkPlane { selection }) =
                        input.carrier()
                    else {
                        return Ok(None);
                    };
                    ctx.get_hash_map(
                        scope_ids,
                        &(stream, selection.work_plane_scope_record_index),
                        "f3d WorkPoint plane scope lookup",
                    )?
                    .map(|feature| {
                        (feature)
                            .try_clone_for_decode(ctx, "f3d WorkPoint plane feature id")
                            .map(|feature| DatumPlaneReference::Feature { feature })
                    })
                    .transpose()
                };

            Ok(Some(match construction.rule.form() {
                DesignWorkPointRuleForm::CircleCenter { input } => {
                    DatumPointConstruction::CircleCenter {
                        edge: or_none!(edge(input)?),
                    }
                }
                DesignWorkPointRuleForm::TwoEdgeIntersection { inputs } => {
                    DatumPointConstruction::TwoEdgeIntersection {
                        edges: [or_none!(edge(&inputs[0])?), or_none!(edge(&inputs[1])?)],
                    }
                }
                DesignWorkPointRuleForm::ThreePlaneIntersection { inputs } => {
                    DatumPointConstruction::ThreePlaneIntersection {
                        planes: Box::new([
                            or_none!(plane(&inputs[0])?),
                            or_none!(plane(&inputs[1])?),
                            or_none!(plane(&inputs[2])?),
                        ]),
                    }
                }
                DesignWorkPointRuleForm::Vertex { input } => {
                    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) =
                        input.carrier()
                    else {
                        return Ok(None);
                    };
                    let vertex = match recipe.resolution {
                        None => VertexSelection::native(
                            ctx.copy_retained_text(
                                &recipe.recipe_id,
                                "f3d WorkPoint native vertex recipe id",
                            )?,
                            ctx,
                        )?
                        .unwrap_or(VertexSelection::Unresolved),
                        Some(resolution) => {
                            let state_id = resolution.state_id;
                            let vertex_slot = resolution.vertex_slot();
                            let ((feature_id, prefix), _identity_storage) = ctx
                                .with_scoped_storage(
                                    "f3d WorkPoint vertex identity storage",
                                    || {
                                        let feature_id =
                                            crate::design::identity::neutral_feature_id(
                                                ctx, scope,
                                            )?;
                                        let feature_key = crate::design::identity::identity_key(
                                            ctx,
                                            feature_id.as_str(),
                                        )?;
                                        let prefix = crate::design::identity::history_input_prefix(
                                            ctx,
                                            feature_key,
                                            state_id,
                                        )?;
                                        Ok::<_, CodecError>((feature_id, prefix))
                                    },
                                )?;
                            match VertexSelection::historical(
                                crate::design::identity::feature_input_topology_id(
                                    ctx,
                                    &feature_id,
                                    state_id,
                                )?,
                                crate::design::identity::history_input_vertex_id(
                                    ctx,
                                    &prefix,
                                    vertex_slot,
                                    "f3d historical vertex identifier",
                                )?,
                                ctx.copy_retained_text(
                                    &recipe.recipe_id,
                                    "f3d WorkPoint historical vertex recipe id",
                                )?,
                                ctx,
                            )? {
                                Ok(selection) => selection,
                                Err(_) => VertexSelection::Unresolved,
                            }
                        }
                    };
                    DatumPointConstruction::Vertex { vertex }
                }
                DesignWorkPointRuleForm::EdgePlaneIntersection { inputs } => {
                    DatumPointConstruction::EdgePlaneIntersection {
                        edge: or_none!(edge(&inputs[0])?),
                        plane: or_none!(plane(&inputs[1])?),
                    }
                }
                DesignWorkPointRuleForm::DistanceOnEdge { input } => {
                    let distance = or_none!(unique_feature_match_for_decode(
                        ctx,
                        parameters.iter().map(|(_, parameter)| *parameter),
                        |parameter| Ok(parameter.source_kind() == "PathDistance"),
                        "f3d WorkPoint path distance parameter source"
                    )?);
                    if !(0.0..=1.0).contains(&distance.evaluated_value().get()) {
                        return Ok(None);
                    }
                    DatumPointConstruction::DistanceOnEdge {
                        edge: or_none!(edge(input)?),
                        fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(
                            distance.evaluated_value().get()
                        )),
                    }
                }
                DesignWorkPointRuleForm::Native { .. } => return Ok(None),
            }))
        },
    )?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

fn project_work_plane(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    transform: [[f64; 4]; 4],
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, UnresolvedFamily, VertexSelection,
    };

    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let normal = Vector3::new(transform[0][2], transform[1][2], transform[2][2]);
    let u_axis = Vector3::new(transform[0][0], transform[1][0], transform[2][0]);
    let Some(frame) = cadmpeg_ir::features::FeatureDatumPlaneFrame::new(origin, normal, u_axis)
    else {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    };
    let Some(construction) = scope.work_plane_construction() else {
        return Ok(FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame,
        }));
    };
    let Some(state_id) = work_plane_recipe_state_id(scope) else {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    };
    let [first, second, third] = construction.inputs();
    if [first, second, third]
        .into_iter()
        .any(|recipe| recipe.resolution.is_none())
    {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    }
    let ((feature_id, prefix), _identity_storage) =
        ctx.with_scoped_storage("f3d WorkPlane identity storage", || {
            let feature_id = crate::design::identity::neutral_feature_id(ctx, scope)?;
            let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
            let prefix = crate::design::identity::history_input_prefix(ctx, feature_key, state_id)?;
            Ok::<_, CodecError>((feature_id, prefix))
        })?;
    let vertex = |recipe: &crate::records::feature::work_geometry::DesignVertexRecipe|
        -> Result<Option<VertexSelection>, CodecError> {
            let Some(resolution) = recipe.resolution else { return Ok(None); };
            let native = ctx.copy_retained_text(&recipe.recipe_id, "f3d WorkPlane vertex recipe id")?;
            let selection = match VertexSelection::historical(
                    crate::design::identity::feature_input_topology_id(ctx, &feature_id, state_id)?,
                    crate::design::identity::history_input_vertex_id(ctx, &prefix, resolution.vertex_slot(), "f3d historical vertex identifier")?,
                    native,
                    ctx,
                )? {
                Ok(selection) => selection,
                Err(_) => VertexSelection::Unresolved,
            };
            Ok(Some(selection))
    };
    let (Some(first), Some(second), Some(third)) =
        (vertex(first)?, vertex(second)?, vertex(third)?)
    else {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    };
    let Ok(points) =
        cadmpeg_ir::features::ThreePointSelection::new(Box::new([first, second, third]), ctx)?
    else {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::DatumThreePointPlane { frame, points },
    ))
}

pub(super) fn project_combine(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    native_scope: &str,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(
        "f3d Combine accepted output",
        || -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
            use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

            let Some(operation) = scope.combine_operation() else {
                return Ok(None);
            };
            let selection = |record_index, operation| {
                ctx.format_retained(
                    format_args!(
                        "{}{}{}",
                        native_scope,
                        ":design-record#",
                        u64::from(record_index)
                    ),
                    operation,
                )
            };
            let target = BodySelection::Native(selection(
                operation.target_record_index,
                "f3d Combine target id",
            )?);
            let tools = if operation.tools.additional.is_empty() {
                BodySelection::Native(selection(
                    operation.tools.first.record_index,
                    "f3d Combine single tool id",
                )?)
            } else {
                let mut selected = Vec::new();
                ctx.push_vec(
                    &mut selected,
                    selection(
                        operation.tools.first.record_index,
                        "f3d Combine tool set id",
                    )?,
                    "f3d Combine tool selection",
                )?;
                for tool in
                    ctx.admit_iter(&operation.tools.additional, "f3d Combine tool source")?
                {
                    ctx.push_vec(
                        &mut selected,
                        selection(tool.record_index, "f3d Combine tool set id")?,
                        "f3d Combine tool selection",
                    )?;
                }
                let members = cadmpeg_ir::features::NativeSelections::new(
                    selected,
                    ctx,
                    "f3d Combine tool uniqueness",
                )?;
                let Ok(members) = members else {
                    return Ok(None);
                };
                BodySelection::NativeSet(members)
            };
            let Ok(operands) = cadmpeg_ir::features::CombineOperands::new(target, tools, ctx)?
            else {
                return Ok(None);
            };
            Ok(Some(FeatureDefinition::Operation(
                FeatureOperation::Combine {
                    operands,
                    op: operation.operation,
                    keep_tools: operation.keep_tools,
                },
            )))
        },
    )?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

fn scope_properties(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    native_scope: &str,
    placements: &[DesignSketchPlacement],
) -> Result<std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = std::collections::BTreeMap::new();
    let mut references = scope.reference_members().values().enumerate();
    while let Some((ordinal, record_index)) =
        ctx.next_charged(&mut references, "f3d scope property reference source")?
    {
        ctx.insert_btree_map(
            &mut properties,
            cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                ctx.format_retained(
                    format_args!("reference:{ordinal}"),
                    "f3d scope reference property key",
                )?,
                "validate nonblank text",
            )?
            .ok_or_else(|| CodecError::malformed("reference property key is blank"))?,
            ctx.format_retained(
                format_args!("{record_index}"),
                "f3d scope reference property value",
            )?,
            "f3d scope reference property",
        )
        .map(|_| ())?;
    }
    if let Some(profile) = scope.extrude_profile().or(scope.base_flange_profile()) {
        if let Some(placement) = ctx.find_by(
            placements,
            |placement| {
                Ok(feature_stream_matches(
                    ctx,
                    &placement.id,
                    Some(native_scope),
                    "f3d scope property placement stream",
                )? && placement.entity_id == profile.entity_id)
            },
            "f3d scope property placement source",
        )? {
            let id = crate::design::identity::neutral_sketch_id(ctx, placement)?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!("profile"),
                id.into_string(),
                "f3d scope profile property",
            )
            .map(|_| ())?;
        }
    }
    Ok(properties)
}

/// The native definition for a scope this codec does not type.
///
/// It states the scope's own kind name and every parameter expression the
/// scope carries, so a feature the codec will not type still names what the
/// source states about it.
///
/// # Errors
///
/// Names the scope whose parameter set states a blank parameter name.
fn native_scope_definition(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut properties = std::collections::BTreeMap::new();
    let mut source = parameters.iter();
    while let Some((_, parameter)) =
        ctx.next_charged(&mut source, "f3d native parameter property source")?
    {
        let name = ctx.copy_retained_text(parameter.name(), "f3d native parameter name")?;
        let Some(name) =
            cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")?
        else {
            return Err(cadmpeg_core::text::NamedEntryError::Blank {
                record: ctx.copy_retained_text(&scope.id, "f3d native property error scope")?,
            }
            .into());
        };
        if ctx.contains_key_btree_map(&properties, &name, "f3d native parameter name lookup")? {
            return Err(cadmpeg_core::text::NamedEntryError::Restated {
                record: ctx.copy_retained_text(&scope.id, "f3d native property error scope")?,
                key: name,
            }
            .into());
        }
        let expression =
            ctx.copy_retained_text(parameter.expression(), "f3d native parameter expression")?;
        ctx.insert_btree_map(
            &mut properties,
            name,
            expression,
            "f3d native parameter property",
        )
        .map(|_| ())?;
    }

    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: ctx
            .copy_retained_text(scope.kind_name(), "f3d native feature kind")?
            .into(),
        parameters: properties,
    }))
}

fn project_fillet_arm(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    native_scope: &str,
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        EdgeSelection, FeatureDefinition, FeatureOperation,
    };

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    if parameters.is_empty() {
        if let Some(definition) = project_full_round_fillet(
            ctx,
            scope,
            inputs.construction_groups,
            inputs.face_operands,
            inputs.owners,
            inputs.fillet_radius_groups,
            inputs.histories,
        )? {
            return Ok(definition);
        }
    }

    let mut assignments = Vec::new();
    for assignment in ctx.admit_iter(inputs.fillet_radius_groups, "f3d Fillet assignment source")? {
        if feature_stream_matches(
            ctx,
            &assignment.id,
            Some(native_scope),
            "f3d Fillet assignment stream",
        )? && assignment.scope_record_index == scope.record_index
        {
            scratch_storage.with_storage(|| {
                ctx.push_vec(&mut assignments, assignment, "f3d Fillet scope assignment")
            })?;
        }
    }
    ctx.stable_sort_by_key(
        &mut assignments[..],
        |value| {
            let assignment = value;
            {
                assignment.group_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design feature_project 5",
    )?;
    if !assignments.is_empty() {
        let mut assignment_storage = ctx.reserve_scoped(0, "F3D resolved Fillet rows")?;
        let Some(assignments) =
            resolved_fillet_assignments(ctx, &mut assignment_storage, &assignments, parameters)?
        else {
            return native_scope_definition(ctx, scope, parameters);
        };
        let mut groups = Vec::new();
        for resolved in ctx.admit_iter(assignments, "f3d Fillet resolved assignment source")? {
            let edge_radius = match &resolved.radius {
                RadiusSpec::Constant { radius } => Some(radius.get()),
                _ => None,
            };
            let edges = if let Some(group) = ctx.find_by(
                inputs.construction_groups,
                |group| {
                    Ok(feature_stream_matches(
                        ctx,
                        &group.id,
                        Some(native_scope),
                        "f3d Fillet assignment group stream",
                    )? && group.record_index == resolved.assignment.group_record_index)
                },
                "f3d Fillet assignment group source",
            )? {
                {
                    let (selection_feature_id, _selection_feature_storage) = ctx
                        .with_scoped_storage("f3d edge selection feature key", || {
                            crate::design::identity::neutral_feature_id(ctx, scope)
                        })?;
                    resolved_edge_treatment_group_with_corners(
                        group,
                        crate::design::edge_resolve::EdgeTreatmentInputs {
                            groups: inputs.construction_groups,
                            operands: inputs.edge_operands,
                            identity_operands: inputs.edge_identity_operands,
                            vertex_operands: inputs.edge_treatment_vertex_operands,
                            histories: inputs.histories,
                            previous_state_id: scope.previous_history_state_id(),
                            feature_id: &selection_feature_id,
                            treatment_radius: edge_radius,
                        },
                        ctx,
                    )
                }?
            } else {
                EdgeSelection::Native(ctx.copy_retained_text(
                    &resolved.assignment.id,
                    "f3d Fillet fallback edge group ID",
                )?)
            };
            resolved.radius_storage.commit()?;
            ctx.push_vec(
                &mut groups,
                FilletGroup {
                    edges,
                    radius: resolved.radius,
                    tangency_weight: resolved.tangency_weight,
                },
                "f3d Fillet projected group",
            )?;
        }
        let groups = groups.try_into().ok();
        return groups.map_or_else(
            || native_scope_definition(ctx, scope, parameters),
            |groups| {
                Ok(FeatureDefinition::Operation(FeatureOperation::Fillet {
                    groups,
                }))
            },
        );
    }
    if let Some(definition) = project_variable_fillet(scope, parameters, inputs, ctx)? {
        return Ok(definition);
    }
    if parameters.is_empty() {
        if let Some(definition) = project_fixed_fillet_with_corners(
            scope,
            inputs.construction_groups,
            inputs.edge_operands,
            inputs.edge_identity_operands,
            inputs.edge_treatment_vertex_operands,
            inputs.histories,
            ctx,
        )? {
            return Ok(definition);
        }
    }
    let [(_, parameter)] = parameters else {
        return native_scope_definition(ctx, scope, parameters);
    };
    let Some(radius) = (parameter.source_kind() == "Radius")
        .then(|| design_positive_length(parameter))
        .flatten()
    else {
        return native_scope_definition(ctx, scope, parameters);
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
            edges: EdgeSelection::Native(
                ctx.copy_retained_text(&scope.id, "f3d Fillet single radius edge scope ID")?,
            ),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }),
    }))
}

struct ResolvedFilletAssignment<'a, 'ctx> {
    assignment: &'a DesignFilletRadiusGroup,
    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec,
    radius_storage: ScopedReservation<'ctx>,
    tangency_weight: Option<cadmpeg_ir::scalar::FiniteReal>,
}

fn resolved_fillet_assignments<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    row_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    assignments: &[&'a DesignFilletRadiusGroup],
    parameters: &[(u32, &DesignParameter)],
) -> Result<Option<Vec<ResolvedFilletAssignment<'a, 'ctx>>>, CodecError> {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;

    let mut index_storage = ctx.reserve_scoped(0, "F3D projection index scratch")?;

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let mut by_record = std::collections::BTreeMap::new();
    for (_, parameter) in ctx.admit_iter(parameters, "f3d Fillet parameter index source")? {
        // discarded-value: duplicate record indices keep the last parameter.
        let _ = index_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut by_record,
                parameter.record_index,
                *parameter,
                "f3d Fillet assignment parameter index",
            )
        })?;
    }
    if by_record.len() != parameters.len() {
        return Ok(None);
    }
    let mut assigned = Vec::new();
    for assignment in ctx.admit_iter(assignments, "f3d Fillet assigned parameter source")? {
        let (first, second, middle) = match &assignment.law {
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            } => (Some(*radius_parameter_record_index), None, None),
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            } => (Some(*chord_length_parameter_record_index), None, None),
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => (
                Some(*offset_one_parameter_record_index),
                Some(*offset_two_parameter_record_index),
                None,
            ),
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle,
            } => (
                Some(*start_radius_parameter_record_index),
                Some(*end_radius_parameter_record_index),
                Some(middle.as_slice()),
            ),
        };
        for record in [first, second].into_iter().flatten() {
            scratch_storage.with_storage(|| {
                ctx.push_vec(&mut assigned, record, "f3d Fillet assigned parameter")
            })?;
        }
        let middle = middle.unwrap_or(&[]);
        for row in ctx.admit_iter(middle, "f3d Fillet radius record source")? {
            scratch_storage.with_storage(|| {
                ctx.push_vec(
                    &mut assigned,
                    row.radius_parameter_record_index,
                    "f3d Fillet assigned parameter",
                )
            })?;
        }
        for row in ctx.admit_iter(middle, "f3d Fillet position record source")? {
            scratch_storage.with_storage(|| {
                ctx.push_vec(
                    &mut assigned,
                    row.parameter_record_index,
                    "f3d Fillet assigned parameter",
                )
            })?;
        }
        if let Some(record) = assignment.tangency_weight_parameter_record_index {
            scratch_storage.with_storage(|| {
                ctx.push_vec(&mut assigned, record, "f3d Fillet assigned parameter")
            })?;
        }
    }
    ctx.sort_unstable_by(
        &mut assigned,
        |value| value,
        Ord::cmp,
        "f3d Fillet assigned parameter sort",
    )?;
    let mut keys = by_record.keys();
    if assigned.len() != by_record.len()
        || !ctx.all_by(
            &assigned,
            |record| {
                Ok(ctx
                    .next_charged(&mut keys, "f3d Fillet parameter key source")?
                    .is_some_and(|key| key == record))
            },
            "f3d Fillet assigned parameter comparison",
        )?
    {
        return Ok(None);
    }
    let parameter = |record, kind| -> Result<Option<&DesignParameter>, CodecError> {
        Ok(ctx
            .get_btree_map(
                &by_record,
                &record,
                "f3d Fillet assignment parameter lookup",
            )?
            .copied()
            .filter(|parameter| parameter.source_kind() == kind))
    };
    let length = |record, kind| -> Result<Option<cadmpeg_ir::scalar::PositiveLength>, CodecError> {
        Ok(parameter(record, kind)?.and_then(design_positive_length))
    };
    let mut resolved = Vec::new();
    let mut source = assignments.iter().copied();
    while let Some(assignment) =
        ctx.next_charged(&mut source, "f3d Fillet assignment resolution source")?
    {
        let tangency_weight = match assignment.tangency_weight_parameter_record_index {
            Some(record) => {
                let Some(parameter) = parameter(record, "TangencyWeight")? else {
                    return Ok(None);
                };
                Some(parameter.evaluated_value())
            }
            None => None,
        };
        let mut radius_storage = ctx.reserve_scoped(0, "f3d Fillet assignment radius storage")?;
        let radius =
            radius_storage.with_storage(|| -> Result<Option<RadiusSpec>, CodecError> {
                let radius = match &assignment.law {
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index,
                    } => RadiusSpec::Constant {
                        radius: match length(*radius_parameter_record_index, "Radius")? {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                    },
                    DesignFilletRadiusLaw::Chordal {
                        chord_length_parameter_record_index,
                    } => RadiusSpec::Chordal {
                        chord_length: match length(
                            *chord_length_parameter_record_index,
                            "ChordLen",
                        )? {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                    },
                    DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index,
                        offset_two_parameter_record_index,
                    } => RadiusSpec::Asymmetric {
                        offset_one: match length(*offset_one_parameter_record_index, "EdgeOffset1")?
                        {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                        offset_two: match length(*offset_two_parameter_record_index, "EdgeOffset2")?
                        {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                    },
                    DesignFilletRadiusLaw::Variable {
                        start_radius_parameter_record_index,
                        end_radius_parameter_record_index,
                        middle,
                    } => {
                        let Some(start) =
                            parameter(*start_radius_parameter_record_index, "StartRadius")?
                        else {
                            return Ok(None);
                        };
                        let Some(end) = parameter(*end_radius_parameter_record_index, "EndRadius")?
                        else {
                            return Ok(None);
                        };
                        let mut control_storage =
                            ctx.reserve_scoped(0, "f3d Fillet row control storage")?;
                        let mut controls = Vec::new();
                        control_storage.with_storage(|| {
                            ctx.push_vec(&mut controls, (0, start), "f3d Fillet variable control")
                        })?;
                        control_storage.with_storage(|| {
                            ctx.push_vec(&mut controls, (1, end), "f3d Fillet variable control")
                        })?;
                        let mut rows = middle.iter().enumerate();
                        while let Some((ordinal, row)) =
                            ctx.next_charged(&mut rows, "f3d Fillet variable row source")?
                        {
                            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                                return Ok(None);
                            };
                            let Some(radius) =
                                parameter(row.radius_parameter_record_index, "MidRadius")?
                            else {
                                return Ok(None);
                            };
                            let Some(position) =
                                parameter(row.parameter_record_index, "MidParams")?
                            else {
                                return Ok(None);
                            };
                            control_storage.with_storage(|| {
                                ctx.push_vec(
                                    &mut controls,
                                    (ordinal, radius),
                                    "f3d Fillet variable control",
                                )
                            })?;
                            control_storage.with_storage(|| {
                                ctx.push_vec(
                                    &mut controls,
                                    (ordinal, position),
                                    "f3d Fillet variable control",
                                )
                            })?;
                        }
                        let Some((points, _)) = variable_fillet_law(ctx, &controls)? else {
                            return Ok(None);
                        };
                        RadiusSpec::Variable { points }
                    }
                };
                Ok(Some(radius))
            })?;
        let Some(radius) = radius else {
            return Ok(None);
        };
        row_storage.with_storage(|| {
            ctx.push_vec(
                &mut resolved,
                ResolvedFilletAssignment {
                    assignment,
                    radius,
                    radius_storage,
                    tangency_weight,
                },
                "f3d Fillet resolved assignment",
            )
        })?;
    }
    Ok(Some(resolved))
}

fn project_thread_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    face_group_record_indices: &[u32],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    if face_group_record_indices.is_empty() || groups.is_empty() {
        return Ok(FaceSelection::Unresolved);
    }
    let Some(stream) =
        ids::native_stream_charged(ctx, &scope.id, "resolve F3D Thread face query scope stream")?
    else {
        return Ok(FaceSelection::Unresolved);
    };
    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;
    let mut ordered_groups = Vec::new();
    let mut source = face_group_record_indices.iter();
    while let Some(record_index) =
        ctx.next_charged(&mut source, "f3d Thread group record source")?
    {
        let Some(group) = unique_feature_match_for_decode(
            ctx,
            groups.iter(),
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "match F3D Thread face group stream",
                )? && group.scope_record_index == scope.record_index
                    && group.record_index == *record_index
                    && group.role() == DesignOperandRole::ROLE_0X10)
            },
            "visit F3D Thread face group source",
        )?
        else {
            return Ok(FaceSelection::Unresolved);
        };
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut ordered_groups, group, "f3d Thread face group"))?;
    }
    let native_source = if let [group] = ordered_groups.as_slice() {
        group.id.as_str()
    } else {
        scope.id.as_str()
    };
    let fallback = || {
        Ok(FaceSelection::Native(ctx.copy_retained_text(
            native_source,
            "f3d Thread face native id",
        )?))
    };
    let mut output_storage = ctx.reserve_scoped(0, "f3d Thread accepted face selection")?;
    let mut state = None;
    let mut faces = Vec::new();
    let mut source = ordered_groups.into_iter();
    while let Some(group) = ctx.next_charged(&mut source, "f3d Thread selected group source")? {
        let (selection, group_storage) =
            ctx.with_scoped_storage("f3d Thread group selection storage", || {
                resolved_historical_face_group(
                    ctx,
                    scope,
                    scope.previous_history_state_id(),
                    group,
                    face_operands,
                )
            })?;
        let Some(FaceSelection::Historical {
            state: group_state,
            faces: group_faces,
            ..
        }) = selection
        else {
            drop(faces);
            drop(state);
            drop(output_storage);
            drop(group_storage);
            return fallback();
        };
        if match state.as_ref() {
            Some(expected) => !ctx.equal(
                expected,
                &group_state,
                "f3d Thread historical state comparison",
            )?,
            None => false,
        } {
            drop(faces);
            drop(state);
            drop(output_storage);
            drop(group_faces);
            drop(group_state);
            drop(group_storage);
            return fallback();
        }
        if state.is_none() {
            state = Some(output_storage.with_storage(|| {
                group_state.try_clone_for_decode(ctx, "f3d Thread historical state id")
            })?);
        }
        for face in ctx.admit_iter(group_faces.as_slice(), "f3d Thread selected face source")? {
            if faces.is_empty()
                || !ctx.any_by(
                    &faces,
                    |candidate: &cadmpeg_ir::ids::HistoricalFaceId| {
                        ctx.equal_bytes(
                            candidate.as_str().as_bytes(),
                            face.as_str().as_bytes(),
                            "compare F3D Thread selected face identity",
                        )
                    },
                    "visit F3D Thread selected face membership",
                )?
            {
                output_storage.with_storage(|| -> Result<(), CodecError> {
                    let face = face.try_clone_for_decode(ctx, "f3d Thread historical face id")?;
                    ctx.push_vec(&mut faces, face, "f3d Thread historical face")?;
                    Ok(())
                })?;
            }
        }
    }
    let Some(state) = state else {
        drop(faces);
        drop(output_storage);
        return fallback();
    };
    let selected = output_storage.with_storage(|| -> Result<_, CodecError> {
        let native = ctx.copy_retained_text(native_source, "f3d Thread historical native id")?;
        Ok(FaceSelection::historical(state, faces, native, ctx)?)
    })?;
    match selected {
        Ok(selection) => output_storage.commit_value(selection),
        Err(_) => {
            drop(output_storage);
            fallback()
        }
    }
}

/// Project Fusion's role-`0x4` full-round face construction.
///
/// This form has no radius parameter. Its one compact member identifies the
/// center face; the trailing true flag requests automatic inference of both
/// side-face sets. A role-`0x4` group with any other shape remains available to
/// the regular feature projectors.
fn project_full_round_fillet(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    owners: &[DesignParameterOwner],
    fillet_radius_groups: &[DesignFilletRadiusGroup],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(
        "f3d full round accepted output",
        || -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "resolve F3D full-round query scope stream"
            )?);
            if ctx.any_by(
                owners,
                |owner| {
                    Ok(feature_stream_matches(
                        ctx,
                        owner.id(),
                        Some(stream),
                        "f3d full round owner stream",
                    )? && owner.scope_record_index() == scope.record_index)
                },
                "f3d full round owner source",
            )? || ctx.any_by(
                fillet_radius_groups,
                |assignment| {
                    Ok(feature_stream_matches(
                        ctx,
                        &assignment.id,
                        Some(stream),
                        "f3d full round radius stream",
                    )? && assignment.scope_record_index == scope.record_index)
                },
                "f3d full round radius source",
            )? {
                return Ok(None);
            }
            let group = or_none!(unique_feature_match_for_decode(
                ctx,
                construction_groups.iter(),
                |group| {
                    Ok(feature_stream_matches(
                        ctx,
                        &group.id,
                        Some(stream),
                        "match F3D full-round group stream",
                    )? && group.scope_record_index == scope.record_index)
                },
                "visit F3D full-round group source",
            )?);
            if group.role() != DesignOperandRole::BODIES_A
                || group.members().len() != 1
                || group.frame.trailing_records().len() != 1
                || group.frame.trailing_flags().len() != 1
                || group.frame.trailing_records()[0].value
                    != group.frame.trailing_flags()[0].record_index
                || !group.frame.trailing_flags()[0].value
                || group.frame.variant
            {
                return Ok(None);
            }
            let [crate::records::identity::Located { value: member, .. }] = group.members() else {
                return Ok(None);
            };
            let operand = or_none!(unique_feature_match_for_decode(
                ctx,
                face_operands.iter(),
                |operand| {
                    Ok(feature_stream_matches(
                        ctx,
                        &operand.id,
                        Some(stream),
                        "match F3D full-round face operand stream",
                    )? && operand.scope_record_index == scope.record_index
                        && operand.group_record_index() == Some(group.record_index)
                        && operand.group_member_ordinal() == Some(0)
                        && operand.record_index() == *member)
                },
                "visit F3D full-round face operand source",
            )?);
            if operand.recipe_kind != ConstructionRecipeKind::BoundedFace {
                return Ok(None);
            }
            if operand.resolved_face_slots.is_empty() {
                return Ok(None);
            }
            let center_faces = project_face_selection(ctx, scope, group, face_operands, histories)?;
            if matches!(center_faces, cadmpeg_ir::features::FaceSelection::Native(_)) {
                return Ok(None);
            }
            Ok(Some(cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::FullRoundFillet {
                    groups: cadmpeg_ir::features::NonEmptyMembers::one(or_none!(
                cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                    center_faces,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                    ctx,
                )?
                .ok()
            )),
                },
            )))
        },
    )?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

fn design_body_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    mut entity_suffixes: impl ExactSizeIterator<Item = u64>,
    body_bindings: &[DesignBodyBinding],
) -> Result<cadmpeg_ir::features::BodySelection, CodecError> {
    use cadmpeg_ir::features::{BodySelection, DistinctMembers, FeatureCollectionError};

    let stream = ids::native_stream_charged(ctx, &scope.id, "resolve F3D body selection stream")?
        .unwrap_or(ids::DEFAULT_STREAM);
    let expected_count = entity_suffixes.len();
    let mut candidate_storage = ctx.reserve_scoped(0, "hold F3D body selection candidate")?;
    let mut bodies = Vec::new();
    let mut complete = true;
    while entity_suffixes.len() != 0 {
        let Some(suffix) =
            ctx.next_charged(&mut entity_suffixes, "visit F3D body selection suffix")?
        else {
            break;
        };
        let mut bindings = body_bindings.iter();
        let body = ctx.find_map(
            &mut bindings,
            |binding| {
                let Some(binding_stream) = ids::native_stream_charged(
                    ctx,
                    binding.id(),
                    "resolve F3D body binding stream",
                )?
                else {
                    return Ok(None);
                };
                if !ctx.equal_bytes(
                    binding_stream.as_bytes(),
                    stream.as_bytes(),
                    "compare F3D body binding stream",
                )? || binding.entity_suffix != suffix
                {
                    return Ok(None);
                }
                Ok(binding.body.as_ref())
            },
            "visit F3D body selection binding",
        )?;
        let Some(body) = body else {
            complete = false;
            break;
        };
        if !ctx.all_by(
            bindings,
            |binding| {
                let Some(binding_stream) = ids::native_stream_charged(
                    ctx,
                    binding.id(),
                    "resolve F3D body binding stream",
                )?
                else {
                    return Ok(true);
                };
                if !ctx.equal_bytes(
                    binding_stream.as_bytes(),
                    stream.as_bytes(),
                    "compare F3D body binding stream",
                )? || binding.entity_suffix != suffix
                {
                    return Ok(true);
                }
                let Some(candidate) = binding.body.as_ref() else {
                    return Ok(true);
                };
                ctx.equal_bytes(
                    candidate.as_str().as_bytes(),
                    body.as_str().as_bytes(),
                    "compare F3D body selection identity",
                )
            },
            "visit F3D body selection binding",
        )? {
            complete = false;
            break;
        }
        ctx.push_scoped_vec(
            &mut candidate_storage,
            &mut bodies,
            body,
            "f3d body selection body proof",
        )?;
    }
    if complete && bodies.len() == expected_count {
        let distinct = candidate_storage
            .with_storage(|| Ok::<_, CodecError>(DistinctMembers::try_from(bodies, ctx)))?;
        match distinct {
            Ok(bodies) => {
                let mut resolved = Vec::new();
                for body in ctx.admit_iter(bodies.as_slice(), "copy accepted F3D body selection")? {
                    let copied = body.try_clone_for_decode(ctx, "f3d body selection body id")?;
                    ctx.push_vec(&mut resolved, copied, "f3d body selection body")?;
                }
                let bodies = match DistinctMembers::try_from(resolved, ctx) {
                    Ok(bodies) => bodies,
                    Err(FeatureCollectionError::Resource(limit)) => return Err(limit.into()),
                    Err(FeatureCollectionError::Invalid(_)) => {
                        return Ok(BodySelection::Native(
                            ctx.copy_retained_text(&scope.id, "f3d body selection native id")?,
                        ))
                    }
                };
                let native =
                    ctx.copy_retained_text(&scope.id, "f3d body selection resolved native id")?;
                return Ok(BodySelection::Resolved { bodies, native });
            }
            Err(FeatureCollectionError::Invalid(_)) => {}
            Err(FeatureCollectionError::Resource(limit)) => return Err(limit.into()),
        }
    } else {
        drop(bodies);
    }
    drop(candidate_storage);
    Ok(BodySelection::Native(ctx.copy_retained_text(
        &scope.id,
        "f3d body selection native id",
    )?))
}

/// Bind each Sketch history node to geometry in exactly one neutral sketch arena.
pub(crate) fn bind_sketch_feature_geometry(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, LoftSection, PathRef,
        PlanarProfileRef, ProfileRef, SketchPointSelection,
    };

    for feature in ctx.admit_iter(&mut *features, "f3d sketch binding feature source")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                if !matches!(
                    definition,
                    FeatureDefinition::Operation(
                        FeatureOperation::Sketch { .. } | FeatureOperation::SpatialSketch { .. }
                    )
                ) {
                    return Ok(());
                }
                let Some(native_ref) = feature.native_ref.as_deref() else {
                    return Ok(());
                };
                let Some(scope) = ctx.find_by(
                    scopes,
                    |scope| {
                        ctx.equal(
                            scope.id.as_str(),
                            native_ref,
                            "f3d sketch binding scope comparison",
                        )
                    },
                    "f3d sketch binding scope source",
                )?
                else {
                    return Ok(());
                };
                let stream = ids::native_stream_charged(
                    ctx,
                    &scope.id,
                    "resolve F3D sketch binding query scope stream",
                )?;
                let Some(placement) = unique_feature_match_for_decode(
                    ctx,
                    placements.iter(),
                    |placement| {
                        Ok(feature_stream_matches(
                            ctx,
                            &placement.id,
                            stream,
                            "match F3D sketch binding placement stream",
                        )? && placement.scope_record_index == Some(scope.record_index))
                    },
                    "visit F3D sketch binding placement source",
                )?
                else {
                    return Ok(());
                };
                let (planar, planar_storage) = ctx
                    .with_scoped_storage("f3d planar sketch binding identity storage", || {
                        crate::design::identity::neutral_sketch_id(ctx, placement)
                    })?;
                let (spatial, spatial_storage) = ctx
                    .with_scoped_storage("f3d spatial sketch binding identity storage", || {
                        crate::design::identity::neutral_spatial_sketch_id(ctx, placement)
                    })?;
                let has_planar = ctx.any_by(
                    sketches,
                    |sketch| ctx.equal(&sketch.id, &planar, "f3d planar sketch binding comparison"),
                    "f3d planar sketch binding source",
                )?;
                let has_spatial = ctx.any_by(
                    spatial_sketches,
                    |sketch| {
                        ctx.equal(
                            &sketch.id,
                            &spatial,
                            "f3d spatial sketch binding comparison",
                        )
                    },
                    "f3d spatial sketch binding source",
                )?;
                *definition = match (has_planar, has_spatial) {
                    (true, false) => FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                            planar_storage.commit_value(planar)?,
                        )),
                    }),
                    (false, true) => {
                        FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                            sketch: Some(spatial_storage.commit_value(spatial)?),
                        })
                    }
                    _ => FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                    }),
                };
                Ok(())
            })();
        });
        edit_result?;
    }
    for feature in ctx.admit_iter(&mut *features, "f3d sketch binding feature source")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(native_ref) = feature.native_ref.as_deref() else {
                        return Ok(());
                    };
                    let Some(scope) = ctx.find_by(
                        scopes,
                        |scope| {
                            ctx.equal(
                                scope.id.as_str(),
                                native_ref,
                                "f3d sketch binding scope comparison",
                            )
                        },
                        "f3d sketch binding scope source",
                    )?
                    else {
                        break 'feature_edit;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                        definition
                    else {
                        break 'feature_edit;
                    };
                    let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) = profile else {
                        break 'feature_edit;
                    };
                    if ctx.any_by(
                        sketches,
                        |candidate| {
                            ctx.equal(
                                &candidate.id,
                                sketch,
                                "f3d Extrude planar sketch binding comparison",
                            )
                        },
                        "f3d Extrude planar sketch binding source",
                    )? {
                        break 'feature_edit;
                    }
                    let mut spatial = None;
                    let mut placement_source = placements.iter();
                    while let Some(placement) = ctx.next_charged(
                        &mut placement_source,
                        "f3d Extrude spatial placement source",
                    )? {
                        let (planar, _planar_storage) = ctx.with_scoped_storage(
                            "f3d Extrude planar placement identity storage",
                            || crate::design::identity::neutral_sketch_id(ctx, placement),
                        )?;
                        if !ctx.equal(&planar, sketch, "f3d Extrude planar placement comparison")? {
                            continue;
                        }
                        let (spatial_id, _spatial_storage) = ctx.with_scoped_storage(
                            "f3d Extrude spatial placement identity storage",
                            || crate::design::identity::neutral_spatial_sketch_id(ctx, placement),
                        )?;
                        if let Some(candidate) = ctx.find_by(
                            spatial_sketches,
                            |candidate| {
                                ctx.equal(
                                    &candidate.id,
                                    &spatial_id,
                                    "f3d Extrude spatial placement comparison",
                                )
                            },
                            "f3d Extrude spatial placement sketch source",
                        )? {
                            if spatial.replace(candidate).is_some() {
                                break 'feature_edit;
                            }
                        }
                    }
                    let Some(spatial) = spatial else {
                        break 'feature_edit;
                    };
                    if spatial.profiles.is_empty() {
                        let Some(profile_operand) = scope.extrude_profile() else {
                            break 'feature_edit;
                        };
                        let Some(stream) = ids::native_stream_charged(
                            ctx,
                            &scope.id,
                            "f3d Extrude spatial binding stream",
                        )?
                        else {
                            break 'feature_edit;
                        };
                        // The spatial carrier has no closed loop that can be represented
                        // by a profile index. Keep the exact profile frame as a native
                        // selection instead of retaining the provisional planar ID.
                        let sketch_id = spatial
                            .id
                            .try_clone_for_decode(ctx, "f3d extrude spatial sketch id")?;
                        let selection = ctx.format_retained(
                            format_args!(
                                "{}{}{}",
                                stream,
                                ":design-record-header#",
                                profile_operand.byte_offset()
                            ),
                            "f3d extrude spatial selection ref",
                        )?;
                        *profile = match ProfileRef::spatial_sketch_selection(
                            sketch_id,
                            vec![selection],
                            ctx,
                        )? {
                            Ok(profile) => profile,
                            Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                ctx.copy_retained_text(
                                    &scope.id,
                                    "f3d extrude spatial fallback id",
                                )?,
                            )),
                        };
                        break 'feature_edit;
                    }
                    let Ok(profile_count) = u32::try_from(spatial.profiles.len()) else {
                        break 'feature_edit;
                    };
                    let mut profiles = Vec::new();
                    for profile_index in
                        ctx.admit_iter(0..profile_count, "f3d Extrude spatial profile source")?
                    {
                        ctx.push_vec(
                            &mut profiles,
                            profile_index,
                            "f3d extrude spatial profile index",
                        )?;
                    }
                    *profile = match ProfileRef::spatial_sketch_profiles(
                        spatial
                            .id
                            .try_clone_for_decode(ctx, "f3d extrude spatial sketch id")?,
                        profiles,
                        ctx,
                    )? {
                        Ok(profile) => profile,
                        Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                            ctx.copy_retained_text(&scope.id, "f3d extrude spatial fallback id")?,
                        )),
                    };
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    let mut sketch_storage = ctx.reserve_scoped(0, "F3D sketch feature lookups")?;
    let mut sketch_features = HashMap::new();
    let mut spatial_sketch_features = HashMap::new();
    sketch_storage.with_storage(|| {
        for feature in ctx.admit_iter(&*features, "f3d sketch feature index source")? {
            let (index, sketch) = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }) => (&mut sketch_features, sketch.as_str()),
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(sketch),
                }) => (&mut spatial_sketch_features, sketch.as_str()),
                _ => continue,
            };
            let key = ctx.copy_retained_text(sketch, "f3d sketch feature index key")?;
            let value = feature
                .id
                .try_clone_for_decode(ctx, "f3d sketch feature index id")?;
            // discarded-value: duplicate sketch bindings keep the last feature.
            let _ = ctx.insert_hash_map(index, key, value, "f3d sketch feature index")?;
        }
        Ok::<_, CodecError>(())
    })?;
    let planar_profile_dependency = |profile: &PlanarProfileRef| match profile {
        PlanarProfileRef::Sketch(sketch)
        | PlanarProfileRef::SketchProfiles { sketch, .. }
        | PlanarProfileRef::SketchRegions { sketch, .. }
        | PlanarProfileRef::SketchEntities { sketch, .. }
        | PlanarProfileRef::SketchSelection { sketch, .. } => ctx.get_hash_map(
            &sketch_features,
            sketch.as_str(),
            "f3d planar sketch feature dependency lookup",
        ),
        _ => Ok(None),
    };
    let profile_dependency = |profile: &ProfileRef| match profile {
        ProfileRef::Planar(
            PlanarProfileRef::Sketch(sketch)
            | PlanarProfileRef::SketchProfiles { sketch, .. }
            | PlanarProfileRef::SketchRegions { sketch, .. }
            | PlanarProfileRef::SketchEntities { sketch, .. }
            | PlanarProfileRef::SketchSelection { sketch, .. },
        ) => ctx.get_hash_map(
            &sketch_features,
            sketch.as_str(),
            "f3d planar sketch feature dependency lookup",
        ),
        ProfileRef::SpatialSketchProfiles { sketch, .. }
        | ProfileRef::SpatialSketchSelection { sketch, .. } => ctx.get_hash_map(
            &spatial_sketch_features,
            sketch.as_str(),
            "f3d spatial sketch feature dependency lookup",
        ),
        ProfileRef::Planar(_) => Ok(None),
    };
    let path_dependency = |path: &PathRef| match path {
        PathRef::Sketch(sketch) | PathRef::SketchCurves { sketch, .. } => ctx.get_hash_map(
            &sketch_features,
            sketch.as_str(),
            "f3d planar sketch feature dependency lookup",
        ),
        PathRef::SpatialSketchSelection { sketch, .. }
        | PathRef::SpatialSketchCurves { sketch, .. } => ctx.get_hash_map(
            &spatial_sketch_features,
            sketch.as_str(),
            "f3d spatial sketch feature dependency lookup",
        ),
        _ => Ok(None),
    };
    let sketch_point_dependency = |point: &SketchPointSelection| match point {
        SketchPointSelection::Planar { sketch, .. } => ctx.get_hash_map(
            &sketch_features,
            sketch.as_str(),
            "f3d planar sketch feature dependency lookup",
        ),
        SketchPointSelection::Spatial { sketch, .. } => ctx.get_hash_map(
            &spatial_sketch_features,
            sketch.as_str(),
            "f3d spatial sketch feature dependency lookup",
        ),
        SketchPointSelection::Unresolved | SketchPointSelection::Native(_) => Ok(None),
    };
    for feature in ctx.admit_iter(&mut *features, "f3d sketch binding feature source")? {
        let mut add_dependency = |dependency: Option<&cadmpeg_ir::features::FeatureId>| {
            if let Some(dependency) = dependency {
                if !ctx.equal(
                    dependency,
                    &feature.id,
                    "f3d sketch feature dependency identity comparison",
                )? {
                    insert_feature_dependency(ctx, &mut feature.dependencies, dependency)?;
                }
            }
            Ok::<(), CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                add_dependency(profile_dependency(profile)?)?;
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                ..
            }) => {
                add_dependency(planar_profile_dependency(profile)?)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
                add_dependency(
                    construction
                        .profile()
                        .map(planar_profile_dependency)
                        .transpose()?
                        .flatten(),
                )?;
                add_dependency(
                    construction
                        .axis()
                        .and_then(|axis| axis.reference.as_ref())
                        .map(path_dependency)
                        .transpose()?
                        .flatten(),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path,
                guide_rail,
                ..
            }) => {
                add_dependency(
                    shape
                        .referenced_profile()
                        .map(planar_profile_dependency)
                        .transpose()?
                        .flatten(),
                )?;
                match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { sections, .. }
                    | cadmpeg_ir::features::SweepShape::Surface { sections, .. } => {
                        for section in ctx.admit_iter(
                            &sections[..],
                            "f3d sketch feature dependency sections source",
                        )? {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .map(planar_profile_dependency)
                                    .transpose()?
                                    .flatten(),
                            )?;
                        }
                    }
                    cadmpeg_ir::features::SweepShape::Solid { sections, .. } => {
                        for section in ctx.admit_iter(
                            &sections[..],
                            "f3d sketch feature dependency sections source",
                        )? {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .map(planar_profile_dependency)
                                    .transpose()?
                                    .flatten(),
                            )?;
                        }
                    }
                }
                add_dependency(path.as_ref().map(path_dependency).transpose()?.flatten())?;
                add_dependency(
                    guide_rail
                        .as_ref()
                        .map(|guide| path_dependency(&guide.path))
                        .transpose()?
                        .flatten(),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections, guidance, ..
            }) => {
                for section in ctx.admit_iter(
                    &sections[..],
                    "f3d sketch feature dependency sections source",
                )? {
                    if let LoftSection::Profile(profile) = section {
                        add_dependency(profile_dependency(profile)?)?;
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(paths) => {
                        for path in ctx
                            .admit_iter(&paths[..], "f3d sketch feature dependency paths source")?
                        {
                            add_dependency(path_dependency(path)?)?;
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                        add_dependency(path_dependency(path)?)?;
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                if let DatumPointConstruction::SketchPoint { point } = construction.as_ref() {
                    add_dependency(sketch_point_dependency(point)?)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Bind `WorkPoint` inputs that select a sketch point after the sketch arenas
/// have been projected. The Design selection identifies a native point record;
/// the neutral point identity depends on whether that record belongs to a
/// planar or model-space sketch.
pub(crate) fn bind_work_point_sketch_point_constructions(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    spatial_sketch_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, SketchPointSelection,
    };

    for feature in ctx.admit_iter(&mut *features, "f3d sketch binding feature source")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
        edit_result = (|| -> Result<(), CodecError> {
        'feature_edit: {
            let Some(native_ref) = feature.native_ref.as_deref() else { return Ok(()); };
                    let Some(scope) = ctx.find_by(scopes, |scope| ctx.equal(scope.id.as_str(), native_ref, "f3d sketch binding scope comparison"), "f3d sketch binding scope source")? else {
                break 'feature_edit;
            };
            let FeatureDefinition::Operation(FeatureOperation::DatumPoint { construction, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if construction.is_some() {
                break 'feature_edit;
            }
            let Some(point) = scope.work_point_construction() else {
                break 'feature_edit;
            };
            let crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { input } =
                point.rule.form()
            else {
                break 'feature_edit;
            };
            let Some(carrier) = input.carrier() else {
                break 'feature_edit;
            };
            let record_index = input.record_index();
            let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                selection,
            } = carrier
            else {
                break 'feature_edit;
            };
            let stream = ids::native_stream_charged(ctx, &scope.id, "f3d WorkPoint sketch binding stream")?.unwrap_or(ids::DEFAULT_STREAM);
            if let Some(entity) = ctx.find_by(sketch_entities, |entity| {
                if let Some(native) = entity.native_ref.as_deref() { ctx.equal(native, selection.point_native_id.as_str(), "f3d WorkPoint sketch entity comparison") } else { Ok(false) }
            }, "f3d WorkPoint sketch_entities binding source")? {
                let native = ctx.format_retained(format_args!("{}{}{}", stream, ":design-record#", u64::from(record_index)), "f3d work point sketch native ref")?;
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Planar {
                        sketch: (entity.sketch).try_clone_for_decode(ctx, "f3d work point planar sketch id")?,
                        point: (entity.id()).try_clone_for_decode(ctx, "f3d work point planar point id")?,
                        native,
                    },
                }));
            } else if let Some(entity) = ctx.find_by(spatial_sketch_entities, |entity| {
                if let Some(native) = entity.native_ref.as_deref() { ctx.equal(native, selection.point_native_id.as_str(), "f3d WorkPoint sketch entity comparison") } else { Ok(false) }
            }, "f3d WorkPoint spatial sketch entity binding source")? {
                let native = ctx.format_retained(format_args!("{}{}{}", stream, ":design-record#", u64::from(record_index)), "f3d work point sketch native ref")?;
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Spatial {
                        sketch: (entity.sketch).try_clone_for_decode(ctx, "f3d work point spatial sketch id")?,
                        point: (entity.id()).try_clone_for_decode(ctx, "f3d work point spatial point id")?,
                        native,
                    },
                }));
            }
        }
        Ok(())
        })();
        });
        edit_result?;
    }
    Ok(())
}

fn project_surface_offset(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operation: &DesignSurfaceOffsetOperation,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let Some(stream) = ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D surface offset query scope stream",
    )?
    else {
        return Ok(None);
    };
    let Some(distance) = cadmpeg_ir::scalar::Length::new(operation.distance.get() * 10.0) else {
        return Ok(None);
    };
    let DesignSurfaceOffsetSupport::FaceGroups {
        group_record_indices,
    } = &operation.support
    else {
        let DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index,
            ..
        } = &operation.support
        else {
            return Ok(None);
        };
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::OffsetSurface {
                faces: FaceSelection::Native(ctx.format_retained(
                    format_args!(
                        "{}{}{}",
                        stream,
                        ":design-record#",
                        u64::from(*boundary_record_index)
                    ),
                    "f3d surface offset boundary id",
                )?),
                distance: Some(distance),
            },
        )));
    };
    let mut output_storage = ctx.reserve_scoped(0, "f3d offset surface accepted selection")?;
    let mut faces = Vec::new();
    let mut source = group_record_indices.iter();
    while let Some(group_record_index) =
        ctx.next_charged(&mut source, "f3d surface offset group record source")?
    {
        let Some(group) = unique_feature_match_for_decode(
            ctx,
            groups.iter(),
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "match F3D surface offset group stream",
                )? && group.scope_record_index == scope.record_index
                    && group.record_index == *group_record_index
                    && group.role() == DesignOperandRole::PROFILE
                    && !group.members().is_empty())
            },
            "visit F3D surface offset group source",
        )?
        else {
            return Ok(None);
        };
        let (selection, _group_storage) = ctx
            .with_scoped_storage("f3d offset surface group selection storage", || {
                resolved_face_group(ctx, group, face_operands)
            })?;
        let Some(FaceSelection::Resolved {
            faces: group_faces, ..
        }) = selection
        else {
            return Ok(None);
        };
        for face in ctx.admit_iter(&group_faces, "f3d surface offset selected face source")? {
            if faces.is_empty()
                || !ctx.any_by(
                    &faces,
                    |candidate: &cadmpeg_ir::ids::FaceId| {
                        ctx.equal_bytes(
                            candidate.as_str().as_bytes(),
                            face.as_str().as_bytes(),
                            "compare F3D surface offset face identity",
                        )
                    },
                    "visit F3D surface offset face membership",
                )?
            {
                output_storage.with_storage(|| -> Result<(), CodecError> {
                    let face = face.try_clone_for_decode(ctx, "f3d offset surface face id")?;
                    ctx.push_vec(&mut faces, face, "f3d offset surface face")?;
                    Ok(())
                })?;
            }
        }
    }
    if faces.is_empty() {
        return Ok(None);
    }
    let definition = output_storage.with_storage(|| -> Result<_, CodecError> {
        let native = ctx.copy_retained_text(&scope.id, "f3d offset surface native id")?;
        Ok(FeatureDefinition::Operation(
            FeatureOperation::OffsetSurface {
                faces: FaceSelection::Resolved { faces, native },
                distance: Some(distance),
            },
        ))
    })?;
    Ok(Some(output_storage.commit_value(definition)?))
}

/// Derive the neutral material-side flag from F3D's signed Draft angle.
///
/// F3D does not store a second outward bit. Keeping this rule in one helper
/// makes every Draft projection branch use the same convention.
const fn draft_outward(angle: f64) -> bool {
    angle < 0.0
}

fn project_draft(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let construction = or_none!(scope.draft_operation());
    let angle = or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
        construction.angle.get()
    ));
    let faces = or_none!(single_operand_group(
        ctx,
        groups,
        scope,
        DesignOperandRole::ROLE_0X10
    )?);
    let stream = ids::native_stream_charged(ctx, &scope.id, "resolve F3D Draft role scope stream")?;
    let mut role_groups = [None; 2];
    let mut source = groups.iter();
    while let Some(group) = ctx.next_charged(&mut source, "visit F3D Draft role group")? {
        if !feature_stream_matches(ctx, &group.id, stream, "match F3D Draft role group stream")?
            || group.scope_record_index != scope.record_index
            || group.role() != DesignOperandRole::ROLE_0X21
            || group.members().is_empty()
        {
            continue;
        }
        if role_groups[0].is_none() {
            role_groups[0] = Some(group);
        } else if role_groups[1].is_none() {
            role_groups[1] = Some(group);
        } else {
            return Ok(None);
        }
    }
    if role_groups[0].is_none() {
        return Ok(None);
    }
    let member_of_scope = |group: &DesignConstructionOperandGroup| {
        ctx.all_by(
            group.members(),
            |member| {
                ctx.any_by(
                    scope.reference_members().values(),
                    |value| Ok(*value == member.value),
                    "visit F3D Draft scope member",
                )
            },
            "visit F3D Draft group member",
        )
    };
    if !ctx.any_by(
        scope.reference_members().values(),
        |value| Ok(*value == faces.record_index),
        "visit F3D Draft face group reference",
    )? || !member_of_scope(faces)?
    {
        return Ok(None);
    }
    Ok(match role_groups {
        [Some(neutral_plane), None] if member_of_scope(neutral_plane)? => {
            if group_has_entity_selection(ctx, scope, neutral_plane, entity_selection_operands)? {
                if let Some(neutral_plane) = selected_work_plane(
                    ctx,
                    scope,
                    neutral_plane,
                    entity_selection_operands,
                    scopes,
                )? {
                    let transform = or_none!(neutral_plane.work_plane_transform());
                    let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                        Vector3::new(transform[0][2], transform[1][2], transform[2][2],)
                    ));
                    return Ok(Some(FeatureDefinition::Operation(
                        FeatureOperation::Draft {
                            faces: project_draft_face_selection(
                                ctx,
                                scope,
                                faces,
                                face_operands,
                                histories,
                            )?,
                            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                                plane: cadmpeg_ir::features::FaceSelection::Native(
                                    crate::design::identity::neutral_feature_id(
                                        ctx,
                                        neutral_plane,
                                    )?
                                    .into_string(),
                                ),
                                pull: Some(cadmpeg_ir::features::DraftPull {
                                    direction: cadmpeg_ir::features::FeatureDirection3::from(
                                        pull_direction,
                                    ),
                                    plane: Some(crate::design::identity::neutral_feature_id(
                                        ctx,
                                        neutral_plane,
                                    )?),
                                }),
                            },
                            angle: Some(angle),
                            outward: Some(draft_outward(construction.angle.get())),
                        },
                    )));
                }
                let neutral_plane = selected_historical_face_selection(
                    ctx,
                    scope,
                    neutral_plane,
                    entity_selection_operands,
                    histories,
                )?;
                let neutral_plane = or_none!(neutral_plane);
                Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                    faces: project_draft_face_selection(
                        ctx,
                        scope,
                        faces,
                        face_operands,
                        histories,
                    )?,
                    anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                        plane: neutral_plane,
                        pull: None,
                    },
                    angle: Some(angle),
                    outward: Some(draft_outward(construction.angle.get())),
                }))
            } else {
                Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                    faces: project_draft_face_selection(
                        ctx,
                        scope,
                        faces,
                        face_operands,
                        histories,
                    )?,
                    anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                        plane: project_draft_face_selection(
                            ctx,
                            scope,
                            neutral_plane,
                            face_operands,
                            histories,
                        )?,
                        pull: None,
                    },
                    angle: Some(angle),
                    outward: Some(draft_outward(construction.angle.get())),
                }))
            }
        }
        [Some(first), Some(second)] if member_of_scope(first)? && member_of_scope(second)? => {
            let first_plane =
                selected_work_plane(ctx, scope, first, entity_selection_operands, scopes)?;
            let second_plane =
                selected_work_plane(ctx, scope, second, entity_selection_operands, scopes)?;
            let (parting_tool, pull_plane) = match (first_plane, second_plane) {
                (Some(plane), None)
                    if !group_has_entity_selection(
                        ctx,
                        scope,
                        second,
                        entity_selection_operands,
                    )? =>
                {
                    (second, plane)
                }
                (None, Some(plane))
                    if !group_has_entity_selection(
                        ctx,
                        scope,
                        first,
                        entity_selection_operands,
                    )? =>
                {
                    (first, plane)
                }
                _ => return Ok(None),
            };
            let transform = or_none!(pull_plane.work_plane_transform());
            let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                Vector3::new(transform[0][2], transform[1][2], transform[2][2],)
            ));
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(ctx, scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::PartingLine {
                    tool: project_draft_face_selection(
                        ctx,
                        scope,
                        parting_tool,
                        face_operands,
                        histories,
                    )?,
                    pull: cadmpeg_ir::features::DraftPull {
                        direction: cadmpeg_ir::features::FeatureDirection3::from(pull_direction),
                        plane: Some(crate::design::identity::neutral_feature_id(
                            ctx, pull_plane,
                        )?),
                    },
                },
                angle: Some(angle),
                outward: Some(draft_outward(construction.angle.get())),
            }))
        }
        _ => None,
    })
}

fn selected_historical_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    let previous_state_id = or_none!(crate::history::effective_scope_previous_history_state_id(
        ctx, scope, histories
    )?);
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D Draft historical query scope stream"
    )?);
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return Ok(None);
    };
    let Some(selection) = unique_feature_match_for_decode(
        ctx,
        entity_selection_operands.iter(),
        |operand| {
            Ok(feature_stream_matches(
                ctx,
                &operand.id,
                Some(stream),
                "match F3D Draft historical operand stream",
            )? && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index() == *member)
        },
        "visit F3D Draft historical operand source",
    )?
    else {
        return Ok(None);
    };
    if selection.secondary().is_some() {
        return Ok(None);
    }
    let mut candidates = selection.historical_face_candidates.iter();
    let face_slot = or_none!(ctx.find_map(
        &mut candidates,
        |candidate| {
            Ok(ctx
                .any_by(
                    &candidate.historical.state_ids,
                    |state_id| Ok(*state_id == previous_state_id),
                    "visit F3D Draft historical candidate state",
                )?
                .then_some(candidate.face_slot))
        },
        "visit F3D Draft historical face candidate",
    )?);
    if !ctx.all_by(
        candidates,
        |candidate| {
            Ok(!ctx.any_by(
                &candidate.historical.state_ids,
                |state_id| Ok(*state_id == previous_state_id),
                "visit F3D Draft historical candidate state",
            )? || candidate.face_slot == face_slot)
        },
        "visit F3D Draft historical face candidate",
    )? {
        return Ok(None);
    }
    let ((feature, prefix), _identity_storage) =
        ctx.with_scoped_storage("f3d Draft face identity storage", || {
            let feature = crate::design::identity::neutral_feature_id(ctx, scope)?;
            let key = crate::design::identity::identity_key(ctx, feature.as_str())?;
            let prefix =
                crate::design::identity::history_input_prefix(ctx, key, previous_state_id)?;
            Ok::<_, CodecError>((feature, prefix))
        })?;
    Ok(Some(match cadmpeg_ir::features::FaceSelection::historical(
        crate::design::identity::feature_input_topology_id(ctx, &feature, previous_state_id)?,
        vec![crate::design::identity::history_input_face_id(
            ctx,
            &prefix,
            face_slot,
            "f3d historical face identifier",
        )?],
        ctx.copy_retained_text(&group.id, "f3d Draft historical face group id")?,
        ctx,
    )? {
        Ok(selection) => selection,
        Err(_) => cadmpeg_ir::features::FaceSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d Draft fallback face group id")?,
        ),
    }))
}

fn project_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let historical = if let Some(previous_state_id) =
        crate::history::effective_scope_previous_history_state_id(ctx, scope, histories)?
    {
        let updated_face_slots = match scope.history_state_id() {
            Some(state_id) => match crate::history::unique_history_state_pair(
                ctx,
                histories,
                state_id,
                previous_state_id,
            )? {
                Some((_, state, _)) => state.transition.as_ref().map_or(&[][..], |transition| {
                    transition.topology.faces.updated.as_slice()
                }),
                None => &[][..],
            },
            None => &[][..],
        };
        if let Some(selection) = resolved_historical_face_group(
            ctx,
            scope,
            Some(previous_state_id),
            group,
            face_operands,
        )? {
            Some(selection)
        } else {
            resolved_historical_split_face_target_group_with_updated_faces(
                ctx,
                scope,
                Some(previous_state_id),
                group,
                face_operands,
                updated_face_slots,
            )?
        }
    } else {
        None
    };
    if let Some(selection) = historical {
        return Ok(selection);
    }
    match resolved_face_group(ctx, group, face_operands)? {
        Some(selection) => Ok(selection),
        None => Ok(cadmpeg_ir::features::FaceSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d face selection native fallback")?,
        )),
    }
}

fn project_draft_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let (selection, storage) = ctx
        .with_scoped_storage("f3d Draft candidate face storage", || {
            project_face_selection(ctx, scope, group, face_operands, histories)
        })?;
    if matches!(&selection, cadmpeg_ir::features::FaceSelection::Native(_)) {
        if let Some(explicit) = crate::design::face_resolve::resolved_explicit_bounded_face_group(
            ctx,
            group,
            face_operands,
        )? {
            return Ok(explicit);
        }
    }
    storage.commit_value(selection)
}

fn group_has_entity_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<bool, CodecError> {
    if group.members().is_empty() || entity_selection_operands.is_empty() {
        return Ok(false);
    }
    let Some(stream) =
        ids::native_stream_charged(ctx, &scope.id, "resolve F3D group entity scope stream")?
    else {
        return Ok(false);
    };
    ctx.any_by(
        group.members().iter().enumerate(),
        |(ordinal, member)| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(false);
            };
            ctx.any_by(
                entity_selection_operands,
                |operand| {
                    Ok(feature_stream_matches(
                        ctx,
                        &operand.id,
                        Some(stream),
                        "match F3D group entity operand stream",
                    )? && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == ordinal
                        && operand.record_index() == member.value)
                },
                "visit F3D group entity operand",
            )
        },
        "visit F3D group entity member",
    )
}

fn selected_work_plane<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Result<Option<&'a DesignParameterScope>, CodecError> {
    let _plane_storage;
    let (planes, held_storage) = ctx.with_scoped_storage("F3D work plane rows", || {
        selected_work_planes(ctx, scope, group, entity_selection_operands, scopes)
    })?;
    _plane_storage = held_storage;
    let Some(planes) = planes else {
        return Ok(None);
    };
    let [plane] = planes.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*plane))
}

fn selected_work_planes<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Result<Option<Vec<&'a DesignParameterScope>>, CodecError> {
    if group.members().is_empty() {
        return Ok(None);
    }
    let mut index_storage = ctx.reserve_scoped(0, "F3D projection index scratch")?;

    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D selected work plane stream"
    )?);
    let mut plane_storage = ctx.reserve_scoped(0, "hold F3D selected work plane rows")?;
    let mut planes = Vec::new();
    let mut target_record_indices = HashSet::new();
    let mut member_source = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut member_source, "f3d selected work plane member scan")?
    {
        let member = &member.value;
        let ordinal = or_none!(u32::try_from(ordinal).ok());
        let selection = unique_feature_match_for_decode(
            ctx,
            entity_selection_operands.iter(),
            |operand| {
                let Some(operand_stream) = ids::native_stream_charged(
                    ctx,
                    &operand.id,
                    "resolve F3D selected work plane operand stream",
                )?
                else {
                    return Ok(false);
                };
                Ok(ctx.equal_bytes(
                    operand_stream.as_bytes(),
                    stream.as_bytes(),
                    "compare F3D selected work plane operand stream",
                )? && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == *member)
            },
            "visit F3D selected work plane operand",
        )?;
        let Some(selection) = selection else {
            return Ok(None);
        };
        if selection.secondary().is_some() {
            return Ok(None);
        }
        let target_record_index =
            or_none!(or_none!(u32::try_from(selection.primary_identity).ok()).checked_add(1));
        if !index_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut target_record_indices,
                target_record_index,
                "f3d selected work plane target index",
            )
        })? {
            return Ok(None);
        }
        let target = unique_feature_match_for_decode(
            ctx,
            scopes.iter(),
            |candidate| {
                Ok(feature_stream_matches(
                    ctx,
                    &candidate.id,
                    Some(stream),
                    "compare F3D unique feature stream",
                )? && candidate.record_index == target_record_index
                    && matches!(
                        candidate.payload(),
                        crate::records::feature::scope::DesignScopePayload::WorkPlane(_)
                    )
                    && candidate.work_plane_transform().is_some())
            },
            "visit F3D unique feature source",
        )?;
        let Some(target) = target else {
            return Ok(None);
        };
        ctx.push_scoped_vec(
            &mut plane_storage,
            &mut planes,
            target,
            "f3d selected work plane",
        )?;
    }
    plane_storage.commit_value(Some(planes))
}

fn resolved_split_face_path(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::PathRef>, CodecError> {
    use cadmpeg_ir::features::PathRef;

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let previous_state_id = or_none!(crate::history::effective_scope_previous_history_state_id(
        ctx, scope, histories
    )?);
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D SplitFace path query scope stream"
    )?);
    let mut edge_slots = Vec::new();
    let mut member_source = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut member_source, "f3d SplitFace path member source")?
    {
        let member = &member.value;
        let ordinal = or_none!(u32::try_from(ordinal).ok());
        let selection = or_none!(unique_feature_match_for_decode(
            ctx,
            entity_selection_operands.iter(),
            |selection| {
                Ok(feature_stream_matches(
                    ctx,
                    &selection.id,
                    Some(stream),
                    "match F3D SplitFace path operand stream",
                )? && selection.scope_record_index == scope.record_index
                    && selection.group_record_index == group.record_index
                    && selection.group_member_ordinal == ordinal
                    && selection.record_index() == *member)
            },
            "visit F3D SplitFace path operand source",
        )?);
        if selection.secondary().is_some() {
            return Ok(None);
        }
        let edge_slot = or_none!(selection.resolved_edge_slot);
        if !edge_slots.is_empty()
            && ctx.any_by(
                &edge_slots,
                |candidate| Ok(*candidate == edge_slot),
                "visit F3D SplitFace path edge membership",
            )?
        {
            return Ok(None);
        }
        scratch_storage.with_storage(|| {
            ctx.push_vec(&mut edge_slots, edge_slot, "f3d SplitFace path edge slot")
        })?;
    }
    if edge_slots.is_empty() {
        return Ok(None);
    }
    let ((feature, prefix), _identity_storage) =
        ctx.with_scoped_storage("f3d SplitFace path identity storage", || {
            let feature = crate::design::identity::neutral_feature_id(ctx, scope)?;
            let key = crate::design::identity::identity_key(ctx, feature.as_str())?;
            let prefix =
                crate::design::identity::history_input_prefix(ctx, key, previous_state_id)?;
            Ok::<_, CodecError>((feature, prefix))
        })?;
    let mut edges = Vec::new();
    for edge_slot in ctx.admit_iter(edge_slots, "f3d SplitFace selected edge slot source")? {
        let edge = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            edge_slot,
            "f3d SplitFace historical edge id",
        )?;
        ctx.push_vec(&mut edges, edge, "f3d SplitFace historical edge")?;
    }
    Ok(PathRef::historical_edges(
        crate::design::identity::feature_input_topology_id(ctx, &feature, previous_state_id)?,
        edges,
        ctx.copy_retained_text(&group.id, "f3d SplitFace path group id")?,
        ctx,
    )?
    .ok())
}

/// Return the unique non-empty construction operand group in `scope` carrying
/// `role`. Yields `None` unless exactly one such group exists.
fn single_operand_group<'a>(
    ctx: &DecodeContext<'_>,
    groups: &'a [DesignConstructionOperandGroup],
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Result<Option<&'a DesignConstructionOperandGroup>, CodecError> {
    if groups.is_empty() {
        return Ok(None);
    }
    let stream = ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D single operand group scope stream",
    )?;
    unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                stream,
                "match F3D single operand group stream",
            )? && group.scope_record_index == scope.record_index
                && group.role() == role
                && !group.members().is_empty())
        },
        "find F3D single operand group",
    )
}

pub(super) fn project_offset_faces(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceMotion, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::scalar::Length;

    let parameter_distance = match parameters {
        [] => None,
        [(_, distance)] if distance.source_kind() == "distance" => {
            Some(or_none!(design_length(distance)))
        }
        _ => return Ok(None),
    };
    let fixed_distance = match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::OffsetFaces(value)
        | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(value) => {
            match value.as_ref() {
                Some(value) => Some(or_none!(Length::new(value.distance.get() * 10.0))),
                None => None,
            }
        }
        _ => return Ok(None),
    };
    let distance = match (parameter_distance, fixed_distance) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs()
                <= EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9 =>
        {
            parameter
        }
        (Some(distance), None) | (None, Some(distance)) => distance,
        _ => return Ok(None),
    };
    let faces = if let Some(faces) = direct_face_selection(ctx, scope, operands)? {
        faces
    } else {
        let group = or_none!(single_operand_group(
            ctx,
            groups,
            scope,
            DesignOperandRole::ROLE_0X10
        )?);
        cadmpeg_ir::features::FaceSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d OffsetFaces native group id")?,
        )
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces,
            motion: FaceMotion::Offset { distance },
        },
    )))
}

pub(super) fn project_thicken(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, ThickenSide};

    let crate::records::feature::scope::DesignScopePayload::Thicken(Some(
        crate::records::feature::direct_face::DesignThickenOperation {
            signed_thickness, ..
        },
    )) = &scope.payload()
    else {
        return Ok(None);
    };
    let accepted_thickness = or_none!(cadmpeg_ir::scalar::PositiveLength::new(
        signed_thickness.get().abs() * 10.0
    ));
    let faces = if let Some(faces) = direct_face_selection(ctx, scope, operands)? {
        faces
    } else {
        if groups.is_empty() {
            return Ok(None);
        }
        let stream =
            ids::native_stream_charged(ctx, &scope.id, "resolve F3D Thicken query scope stream")?;
        let group = or_none!(unique_feature_match_for_decode(
            ctx,
            groups.iter(),
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    stream,
                    "match F3D Thicken group stream",
                )? && group.scope_record_index == scope.record_index
                    && matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                    )
                    && !group.members().is_empty())
            },
            "visit F3D Thicken group source",
        )?);
        FaceSelection::Native(ctx.copy_retained_text(&group.id, "f3d Thicken native group id")?)
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Thicken {
            faces,
            thickness: Some(accepted_thickness),
            side: Some(if signed_thickness.get() > 0.0 {
                ThickenSide::Forward
            } else {
                ThickenSide::Reverse
            }),
        },
    )))
}

pub(super) fn project_shell(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

    let (crate::records::feature::scope::DesignScopePayload::Shell(Some(
        crate::records::feature::direct_face::DesignShellOperation {
            thickness, outward, ..
        },
    ))
    | crate::records::feature::scope::DesignScopePayload::Schale(Some(
        crate::records::feature::direct_face::DesignShellOperation {
            thickness, outward, ..
        },
    ))) = &scope.payload()
    else {
        return Ok(None);
    };
    let accepted_thickness = or_none!(cadmpeg_ir::scalar::PositiveLength::new(
        thickness.get() * 10.0
    ));
    let bodies = single_operand_group(ctx, groups, scope, DesignOperandRole::BODIES_A)?
        .map(|group| {
            ctx.copy_retained_text(&group.id, "f3d Shell native body group id")
                .map(BodySelection::Native)
        })
        .transpose()?;
    let removed_faces = if let Some(faces) = direct_face_selection(ctx, scope, operands)? {
        faces
    } else if let Some(group) =
        single_operand_group(ctx, groups, scope, DesignOperandRole::ROLE_0X10)?
    {
        FaceSelection::Native(ctx.copy_retained_text(&group.id, "f3d Shell native face group id")?)
    } else if bodies.is_some() {
        FaceSelection::Faces(Vec::new())
    } else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness: Some(accepted_thickness),
            outward: Some(*outward),
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        },
    )))
}

fn project_move(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = or_none!(scope.move_operation());
    let group = or_none!(single_operand_group(
        ctx,
        groups,
        scope,
        DesignOperandRole::BODIES_A
    )?);
    let translation = or_none!(cadmpeg_ir::features::FiniteVector3::new(Vector3::new(
        operation.transform[0][3] * 10.0,
        operation.transform[1][3] * 10.0,
        operation.transform[2][3] * 10.0,
    )));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveBody {
            bodies: BodySelection::Native(
                ctx.copy_retained_text(&group.id, "f3d Move body group id")?,
            ),
            translation,
            rotation: fixed_geometry::matrix_axis_angle(operation.transform.as_ref()),
            copies: 0,
        },
    )))
}

pub(super) fn project_remove_body(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let group = or_none!(single_operand_group(
        ctx,
        groups,
        scope,
        DesignOperandRole::BODIES_A
    )?);
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(
                ctx.copy_retained_text(&group.id, "f3d RemoveBody group id")?,
            ),
            mode: BodyRetentionMode::DeleteSelected,
        },
    )))
}

fn project_base_flange(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    placements: &[DesignSketchPlacement],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, PlanarProfileRef, SheetMetalThicknessSide,
    };

    let operation = or_none!(scope.base_flange_operation());
    if groups.is_empty() {
        return Ok(None);
    }
    let stream =
        ids::native_stream_charged(ctx, &scope.id, "resolve F3D base flange query scope stream")?;
    let Some(profile_group) = unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                stream,
                "match F3D base flange group stream",
            )? && group.scope_record_index == scope.record_index)
        },
        "visit F3D base flange group source",
    )?
    else {
        return Ok(None);
    };
    if profile_group.scope_reference_ordinal != 0
        || profile_group.record_index != operation.profile_group_record_index
        || profile_group.role() != DesignOperandRole::PROFILE
        || !profile_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.profile_record_index])
    {
        return Ok(None);
    }
    let profile = or_none!(scope.base_flange_profile());
    if profile.scope_reference_ordinal != 1
        || profile.record_index != operation.profile_record_index
    {
        return Ok(None);
    }
    let placement = or_none!(ctx.find_by(
        placements,
        |placement| {
            Ok(feature_stream_matches(
                ctx,
                &placement.id,
                stream,
                "f3d base flange placement stream",
            )? && placement.entity_id == profile.entity_id)
        },
        "f3d base flange placement source"
    )?);
    let accepted_thickness = or_none!(cadmpeg_ir::scalar::PositiveLength::new(
        operation.thickness.get() * 10.0
    ));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalBaseFlange {
            profile: PlanarProfileRef::Sketch(crate::design::identity::neutral_sketch_id(
                ctx, placement,
            )?),
            thickness: accepted_thickness,
            side: SheetMetalThicknessSide::Forward,
        },
    )))
}

/// Project a sheet-metal `EdgeFlange` scope onto its neutral operation.
///
/// The typed operation supplies the bend position, height datum, and inside
/// radius. Owner parameters supply the height, angle, and width. A to-object
/// height resolves its target entity to a known neutral construction feature;
/// otherwise the source selection remains explicit in the neutral height law.
fn project_edge_flange(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage("f3d EdgeFlange accepted output", || -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {

    use crate::records::feature::sheet_metal::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeWidthParameterSource,
        DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalBendPosition, SheetMetalFlangeHeight,
        SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
        SheetMetalHeightDatum,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let ProjectInputs {
        native: parameters,
        owners,
        scopes,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        entity_selection_operands,
        ..
    } = inputs;
    let Some((operation, stream, height, angle, width, height_datum, bend_position)) =
        (|| -> Result<Option<_>, CodecError> {
            let operation = or_none!(scope.edge_flange_operation());
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "resolve F3D EdgeFlange query scope stream"
            )?);
            let parameter = |owner_record_index,
                             source_kind: &str|
             -> Result<Option<&DesignParameter>, CodecError> {
                let Some(owner) = unique_feature_match_for_decode(
                    ctx,
                    owners.iter(),
                    |owner| {
                        Ok(feature_stream_matches(
                            ctx,
                            owner.id(),
                            Some(stream),
                            "f3d feature parameter owner stream",
                        )? && owner.scope_record_index() == scope.record_index
                            && owner.record_index() == owner_record_index)
                    },
                    "f3d feature parameter owner source",
                )?
                else {
                    return Ok(None);
                };
                ctx.find_by(
                    *parameters,
                    |parameter| {
                        Ok(feature_stream_matches(
                            ctx,
                            &parameter.id,
                            Some(stream),
                            "f3d feature owned parameter stream",
                        )? && parameter.record_index == owner.parameter_record_index()
                            && parameter.source_kind() == source_kind)
                    },
                    "f3d feature owned parameter source",
                )
            };

            let height = match &operation.selection.shape().height() {
                DesignEdgeFlangeHeightExtent::Distance => {
                    SheetMetalFlangeHeight::Distance(or_none!(design_positive_length(or_none!(
                        parameter(operation.height_owner_record_index, "FlangeHeight",)?
                    ))))
                }
                DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } => {
                    let target_group = or_none!(unique_feature_match_for_decode(
                        ctx,
                        groups.iter(),
                        |group| {
                            Ok(feature_stream_matches(
                                ctx,
                                &group.id,
                                Some(stream),
                                "match F3D EdgeFlange target group stream",
                            )? && group.scope_record_index == scope.record_index
                                && group.record_index == *target_group_record_index
                                && group.role() == DesignOperandRole::ROLE_0X21
                                && group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([*target_operand_record_index]))
                        },
                        "visit F3D EdgeFlange target group source",
                    )?);
                    let target_selection = or_none!(unique_feature_match_for_decode(
                        ctx,
                        entity_selection_operands.iter(),
                        |operand| {
                            Ok(feature_stream_matches(
                                ctx,
                                &operand.id,
                                Some(stream),
                                "match F3D EdgeFlange target operand stream",
                            )? && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == target_group.record_index
                                && operand.group_member_ordinal == 0
                                && operand.record_index() == *target_operand_record_index)
                        },
                        "visit F3D EdgeFlange target operand source",
                    )?);
                    let target_record_index =
                        or_none!(u32::try_from(target_selection.primary_identity)
                            .ok()
                            .and_then(|index| index.checked_add(1)));
                    let mut target_scopes = scopes.iter();
                    let mut matches_target =
                        |candidate: &&DesignParameterScope| -> Result<bool, CodecError> {
                            Ok(feature_stream_matches(ctx, &candidate.id, Some(stream), "f3d EdgeFlange height scope stream")?
                            && candidate.record_index == target_record_index
                            && matches!(candidate.kind(), crate::records::feature::scope::DesignFeatureKind::WorkPlane | crate::records::feature::scope::DesignFeatureKind::WorkPoint))
                        };
                    let first = ctx.find_by(
                        &mut target_scopes,
                        &mut matches_target,
                        "f3d EdgeFlange height scope source",
                    )?;
                    let target = match first {
                        Some(target_scope) => {
                            if ctx.any_by(
                                target_scopes,
                                |candidate| matches_target(&candidate),
                                "f3d EdgeFlange height scope source",
                            )? {
                                return Ok(None);
                            }
                            SheetMetalFlangeHeightTarget::Feature(
                                crate::design::identity::neutral_feature_id(ctx, target_scope)?,
                            )
                        }
                        None => SheetMetalFlangeHeightTarget::Native(ctx.copy_retained_text(
                            &target_selection.id,
                            "f3d EdgeFlange native height target id",
                        )?),
                    };
                    let offset = or_none!(design_length(or_none!(parameter(
                        *offset_owner_record_index,
                        "ToObjectOffset"
                    )?)));
                    SheetMetalFlangeHeight::ToObject { target, offset }
                }
            };
            let angle = or_none!(design_angle(or_none!(parameter(
                operation.angle_owner_record_index,
                "FlangeAngle",
            )?)));

            let width = match &operation.selection.shape() {
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                    ..
                } => SheetMetalFlangeWidth::FullEdge,
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                    owner,
                    ..
                } => SheetMetalFlangeWidth::Symmetric {
                    width: or_none!(design_positive_length(or_none!(parameter(
                        *owner,
                        "EdgeWidth"
                    )?))),
                },
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                    edges,
                ) => {
                    let mut widths = edges.iter();
                    let row = or_none!(
                        ctx.next_charged(&mut widths, "f3d EdgeFlange symmetric width source")?
                    );
                    let first = or_none!(
                        parameter(row.owners, "EdgeWidth")?.and_then(design_positive_length)
                    );
                    if !ctx.all_by(
                        widths,
                        |row| {
                            Ok(
                                parameter(row.owners, "EdgeWidth")?
                                    .and_then(design_positive_length)
                                    == Some(first),
                            )
                        },
                        "f3d EdgeFlange symmetric width source",
                    )? {
                        return Ok(None);
                    }
                    SheetMetalFlangeWidth::Symmetric { width: first }
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                    edges,
                    source,
                } => {
                    let (first_kind, second_kind) = match source {
                        DesignEdgeFlangeWidthParameterSource::EdgeWidth => {
                            ("EdgeWidth_1", "EdgeWidth_2")
                        }
                        DesignEdgeFlangeWidthParameterSource::EdgeOffset => {
                            ("EdgeOffset_1", "EdgeOffset_2")
                        }
                    };
                    let width_length =
                        |owner, kind| -> Result<Option<PositiveLength>, CodecError> {
                            let Some(length) = parameter(owner, kind)?.and_then(design_length)
                            else {
                                return Ok(None);
                            };
                            Ok(PositiveLength::new(match source {
                                DesignEdgeFlangeWidthParameterSource::EdgeWidth => length.get(),
                                DesignEdgeFlangeWidthParameterSource::EdgeOffset => {
                                    length.get().abs()
                                }
                            }))
                        };
                    let mut widths = Vec::new();
                    for row in ctx.admit_iter(edges, "f3d EdgeFlange two sided width source")? {
                        let width = SheetMetalFlangeTwoSidedWidth {
                            first: or_none!(width_length(row.owners[0], first_kind)?),
                            second: or_none!(width_length(row.owners[1], second_kind)?),
                        };
                        ctx.push_vec(&mut widths, width, "f3d EdgeFlange two-sided edge width")?;
                    }
                    SheetMetalFlangeWidth::TwoSidesPerEdge {
                        widths: or_none!(cadmpeg_ir::features::SheetMetalFlangeEdgeWidths::new(
                            widths
                        )
                        .ok()),
                    }
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                    owners: [first, second],
                    ..
                } => SheetMetalFlangeWidth::TwoSides {
                    first: or_none!(design_positive_length(or_none!(parameter(
                        *first,
                        "EdgeWidth_1"
                    )?))),
                    second: or_none!(design_positive_length(or_none!(parameter(
                        *second,
                        "EdgeWidth_2"
                    )?))),
                },
            };

            let height_datum = match operation.height_datum {
                DesignSheetMetalHeightDatum::InnerFaces => SheetMetalHeightDatum::InnerFaces,
                DesignSheetMetalHeightDatum::OuterFaces => SheetMetalHeightDatum::OuterFaces,
                DesignSheetMetalHeightDatum::Unknown(_) => return Ok(None),
            };
            let bend_position = match operation.bend_position {
                DesignBendPosition::Outside => SheetMetalBendPosition::Outside,
                DesignBendPosition::Inside => SheetMetalBendPosition::Inside,
                DesignBendPosition::Adjacent => SheetMetalBendPosition::Adjacent,
                DesignBendPosition::TangentToSide => SheetMetalBendPosition::TangentToSide,
                DesignBendPosition::Unknown(_) => return Ok(None),
            };
            Ok(Some((
                operation,
                stream,
                height,
                angle,
                width,
                height_datum,
                bend_position,
            )))
        })()?
    else {
        return Ok(None);
    };

    // Each role-`0x08` group carries one selected edge. The aggregate role-`0x43`
    // group repeats them, so it contributes no separate selection.
    if operation.selection.shape().edges().next().is_none() {
        return Ok(None);
    }
    let mut group_storage = ctx.reserve_scoped(0, "f3d EdgeFlange proven group storage")?;
    let mut edge_groups = Vec::new();
    let mut edge_source = operation.selection.shape().edges();
    while let Some(edge) =
        ctx.next_charged(&mut edge_source, "f3d EdgeFlange selected edge source")?
    {
        let Some(edge_group) = unique_feature_match_for_decode(
            ctx,
            groups.iter(),
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "match F3D EdgeFlange edge group stream",
                )? && group.scope_record_index == scope.record_index
                    && group.record_index == edge.group_record_index.get())
            },
            "visit F3D EdgeFlange edge group source",
        )?
        else {
            return Ok(None);
        };
        if edge_group.role() != DesignOperandRole::BODIES_B || edge_group.members().len() != 1 {
            return Ok(None);
        }
        ctx.push_scoped_vec(&mut group_storage, &mut edge_groups, edge_group, "f3d EdgeFlange proven edge group")?;
    }
    let resolve_group = |edge_group: &DesignConstructionOperandGroup| {
        let (selection_feature_id, _selection_feature_storage) = ctx.with_scoped_storage("f3d edge selection feature key", || {
            crate::design::identity::neutral_feature_id(ctx, scope)
        })?;
        resolved_edge_flange_group(edge_group, groups, edge_operands, edge_identity_operands, scope.previous_history_state_id(), &selection_feature_id, ctx)
    };
    let edges = if let [group] = edge_groups.as_slice() {
        resolve_group(group)?
    } else {
        let (selections, _selection_storage) = ctx.with_scoped_storage("f3d EdgeFlange input selection storage", || {
            let mut selections = Vec::new();
            for group in ctx.admit_iter(&edge_groups, "f3d EdgeFlange proven group source")? {
                ctx.push_vec(&mut selections, resolve_group(group)?, "f3d edge flange selection")?;
            }
            Ok::<_, CodecError>(selections)
        })?;
        merge_edge_selections(ctx, scope, &selections)?
    };

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalEdgeFlange {
            edges,
            height,
            angle,
            height_datum,
            bend_position,
            width,
            bend_radius: or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                operation.bend_radius.get() * 10.0,
            )),
        },
    )))

    })?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

/// Project a sheet-metal `Hem` scope onto its neutral operation.
///
/// The owner layout distinguishes the rolled and teardrop forms from the
/// shared gap-and-length layout. Fold direction is recovered from the signed
/// placement of the inserted bend carriers against the preceding source face;
/// an incomplete transition keeps it unresolved.
fn project_hem(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::sheet_metal::DesignHemParameterOwners;
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalHemDirection, SheetMetalHemForm,
    };

    let ProjectInputs {
        native: parameters,
        owners,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        histories,
        ..
    } = inputs;
    let operation = or_none!(scope.hem_operation());
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d Hem scope stream"
    )?);
    let parameter = |owner_record_index: u32,
                     source_kind: &str|
     -> Result<Option<&DesignParameter>, CodecError> {
        let Some(owner) = unique_feature_match_for_decode(
            ctx,
            owners.iter(),
            |owner| {
                Ok(
                    feature_stream_matches(ctx, owner.id(), Some(stream), "f3d Hem owner stream")?
                        && owner.scope_record_index() == scope.record_index
                        && owner.record_index() == owner_record_index,
                )
            },
            "f3d Hem owner source",
        )?
        else {
            return Ok(None);
        };
        unique_feature_match_for_decode(
            ctx,
            parameters.iter(),
            |parameter| {
                Ok(feature_stream_matches(
                    ctx,
                    &parameter.id,
                    Some(stream),
                    "f3d Hem parameter stream",
                )? && parameter.record_index == owner.parameter_record_index()
                    && parameter.source_kind() == source_kind)
            },
            "f3d Hem parameter source",
        )
    };
    let form = match &operation.parameter_owners {
        DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        } => SheetMetalHemForm::GapLength {
            gap: or_none!(cadmpeg_ir::scalar::NonNegativeLength::try_from(or_none!(
                design_length(or_none!(parameter(*gap_owner_record_index, "HemGap")?),)
            ))
            .ok()),
            length: or_none!(design_positive_length(or_none!(parameter(
                *length_owner_record_index,
                "HemLength",
            )?))),
        },
        DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        } => SheetMetalHemForm::Rolled {
            radius: or_none!(design_positive_length(or_none!(parameter(
                *radius_owner_record_index,
                "HemRadius",
            )?))),
            angle: or_none!(design_angle(or_none!(parameter(
                *angle_owner_record_index,
                "HemAngle"
            )?))),
        },
        DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        } => SheetMetalHemForm::Teardrop {
            gap: or_none!(cadmpeg_ir::scalar::NonNegativeLength::try_from(or_none!(
                design_length(or_none!(parameter(*gap_owner_record_index, "HemGap")?),)
            ))
            .ok()),
            length: or_none!(design_positive_length(or_none!(parameter(
                *length_owner_record_index,
                "HemLength",
            )?))),
            radius: or_none!(design_positive_length(or_none!(parameter(
                *radius_owner_record_index,
                "HemRadius",
            )?))),
        },
    };

    let edge_group = or_none!(unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(
                feature_stream_matches(ctx, &group.id, Some(stream), "f3d Hem edge group stream")?
                    && group.scope_record_index == scope.record_index
                    && group.record_index == operation.edge_group_record_index.get(),
            )
        },
        "f3d Hem edge group source"
    )?);
    if edge_group.role() != DesignOperandRole::BODIES_B
        || !edge_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.edge_operand_record_index()])
    {
        return Ok(None);
    }
    let aggregate_group = or_none!(unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "f3d Hem aggregate group stream",
            )? && group.scope_record_index == scope.record_index
                && group.record_index == operation.aggregate_group_record_index.get())
        },
        "f3d Hem aggregate group source"
    )?);
    if aggregate_group.role() != DesignOperandRole::ROLE_0X43
        || !aggregate_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.aggregate_operand_record_index()])
    {
        return Ok(None);
    }

    let bend_radius = or_none!(cadmpeg_ir::scalar::PositiveLength::new(
        operation.bend_radius.get() * 10.0
    ));
    let edges = {
        let (selection_feature_id, _selection_feature_storage) = ctx
            .with_scoped_storage("f3d edge selection feature key", || {
                crate::design::identity::neutral_feature_id(ctx, scope)
            })?;
        crate::design::edge_resolve::resolved_hem_edge_group(
            edge_group,
            groups,
            edge_operands,
            edge_identity_operands,
            crate::history::effective_scope_previous_history_state_id(ctx, scope, histories)?,
            &selection_feature_id,
            ctx,
        )
    }?;

    let edge_slot = match unique_feature_match_for_decode(
        ctx,
        edge_operands.iter(),
        |operand| {
            Ok({
                let operand_stream =
                    ids::native_stream_charged(ctx, &operand.id, "resolve F3D Hem operand stream")?;
                let group_stream = ids::native_stream_charged(
                    ctx,
                    &edge_group.id,
                    "resolve F3D Hem group stream",
                )?;
                match (operand_stream, group_stream) {
                    (Some(operand), Some(group)) => ctx.equal_bytes(
                        operand.as_bytes(),
                        group.as_bytes(),
                        "compare F3D Hem operand stream",
                    )?,
                    (None, None) => true,
                    _ => false,
                }
            } && operand.scope_record_index == edge_group.scope_record_index
                && operand.record_index() == operation.edge_operand_record_index())
        },
        "visit F3D unique feature source",
    )? {
        Some(operand) => crate::design::edge_resolve::resolved_hem_edge_slot(
            operand,
            crate::history::effective_scope_previous_history_state_id(ctx, scope, histories)?,
            ctx,
        )?,
        None => None,
    };
    let semantics = edge_slot
        .map(|edge_slot| crate::history::hem_geometry_semantics(ctx, scope, edge_slot, histories))
        .transpose()?;
    let form = match (
        form,
        semantics.and_then(|semantics| semantics.gap_length_form),
    ) {
        (
            SheetMetalHemForm::GapLength { gap: _, length },
            Some(crate::history::HemGapLengthForm::Flat),
        ) => SheetMetalHemForm::Flat { length },
        (
            SheetMetalHemForm::GapLength { gap, length },
            Some(crate::history::HemGapLengthForm::Open),
        ) => SheetMetalHemForm::Open { gap, length },
        (form, _) => form,
    };
    let direction = semantics
        .and_then(|semantics| semantics.direction)
        .unwrap_or(SheetMetalHemDirection::Unresolved);

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalHem {
            edges,
            form,
            direction,
            bend_radius,
        },
    )))
}

pub(super) fn project_surface_stitch(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let operation = or_none!(scope.surface_stitch_operation());
    let input_end = or_none!(scope.reference_members().len().checked_sub(2));
    let native_scope =
        ids::native_stream_charged(ctx, &scope.id, "f3d SurfaceStitch scope stream")?;
    let mut matching = Vec::new();
    for group in ctx.admit_iter(groups, "f3d SurfaceStitch group source")? {
        if !feature_stream_matches(
            ctx,
            &group.id,
            native_scope,
            "f3d SurfaceStitch group stream",
        )? || group.scope_record_index != scope.record_index
        {
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut matching, group, "f3d SurfaceStitch group"))?;
    }
    ctx.stable_sort_by_key(
        &mut matching[..],
        |value| {
            let group = value;
            {
                group.scope_reference_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design feature_project 6",
    )?;
    let mut references = scope.reference_members().values();
    if or_none!(matching.len().checked_mul(2)) != input_end
        || ctx.any_by(
            matching.iter().enumerate(),
            |(ordinal, group)| {
                let group_reference =
                    ctx.next_charged(&mut references, "f3d SurfaceStitch group reference")?;
                let member_reference =
                    ctx.next_charged(&mut references, "f3d SurfaceStitch member reference")?;
                Ok(
                    u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                        || group_reference != Some(&group.record_index)
                        || member_reference.is_none_or(|reference| {
                            !group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([*reference])
                        })
                        || group.role() != DesignOperandRole::ROLE_0X5,
                )
            },
            "f3d SurfaceStitch member proof",
        )?
    {
        return Ok(None);
    }
    let gap_tolerance = or_none!(cadmpeg_ir::scalar::NonNegativeLength::new(
        operation.gap_tolerance.get() * 10.0
    ));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(
                ctx.copy_retained_text(&scope.id, "f3d SurfaceStitch native id")?,
            ),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(gap_tolerance),
        },
    )))
}

fn project_ruled_surface(
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
    parameters: &[DesignParameter],
    groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::surface_ops::{
        DesignRuledSurfaceCorner, DesignRuledSurfaceMethod,
    };
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, RuledSurfaceCorner, RuledSurfaceMode,
    };

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let operation = or_none!(scope.ruled_surface_operation());
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D ruled surface query scope stream"
    )?);
    let parameter =
        |owner_record_index, source_kind: &str| -> Result<Option<&DesignParameter>, CodecError> {
            let Some(owner) = unique_feature_match_for_decode(
                ctx,
                owners.iter(),
                |owner| {
                    Ok(feature_stream_matches(
                        ctx,
                        owner.id(),
                        Some(stream),
                        "f3d feature parameter owner stream",
                    )? && owner.scope_record_index() == scope.record_index
                        && owner.record_index() == owner_record_index)
                },
                "f3d feature parameter owner source",
            )?
            else {
                return Ok(None);
            };
            ctx.find_by(
                parameters,
                |parameter| {
                    Ok(feature_stream_matches(
                        ctx,
                        &parameter.id,
                        Some(stream),
                        "f3d feature owned parameter stream",
                    )? && parameter.record_index == owner.parameter_record_index()
                        && parameter.source_kind() == source_kind)
                },
                "f3d feature owned parameter source",
            )
        };
    let distance = or_none!(design_positive_length(or_none!(parameter(
        operation.distance_owner_record_index,
        "ruledDistance",
    )?)));
    let angle = or_none!(design_angle(or_none!(parameter(
        operation.angle_owner_record_index,
        "ruledAngle"
    )?)));
    let mode = match operation.method {
        DesignRuledSurfaceMethod::Tangent => RuledSurfaceMode::Tangent { distance },
        DesignRuledSurfaceMethod::Normal => RuledSurfaceMode::Normal { distance },
        DesignRuledSurfaceMethod::Direction => return Ok(None),
    };
    let mut ordered_groups = Vec::new();
    for record_index in ctx.admit_iter(
        &operation.edge_group_record_indices,
        "f3d ruled surface group record source",
    )? {
        let group = or_none!(unique_feature_match_for_decode(
            ctx,
            groups.iter(),
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "match F3D ruled surface edge group stream",
                )? && group.scope_record_index == scope.record_index
                    && group.record_index == *record_index)
            },
            "visit F3D ruled surface edge group source",
        )?);
        if group.role() != DesignOperandRole::BODIES_B || group.members().len() != 1 {
            return Ok(None);
        }
        let reference_ordinal = or_none!(usize::try_from(group.scope_reference_ordinal).ok());
        if scope.reference_members().values().nth(reference_ordinal) != Some(record_index)
            || scope
                .reference_members()
                .values()
                .nth(reference_ordinal + 1)
                != group.members().first().map(|member| &member.value)
        {
            return Ok(None);
        }
        scratch_storage.with_storage(|| {
            ctx.push_vec(&mut ordered_groups, group, "f3d ruled surface edge group")
        })?;
    }
    let (selections, _selection_storage) =
        ctx.with_scoped_storage("f3d ruled surface input selection storage", || {
            let mut selections = Vec::new();
            for group in
                ctx.admit_iter(&ordered_groups, "f3d ruled surface selected group source")?
            {
                let selection = {
                    let (selection_feature_id, _selection_feature_storage) = ctx
                        .with_scoped_storage("f3d edge selection feature key", || {
                            crate::design::identity::neutral_feature_id(ctx, scope)
                        })?;
                    resolved_edge_group(
                        group,
                        groups,
                        edge_operands,
                        edge_identity_operands,
                        scope.previous_history_state_id(),
                        &selection_feature_id,
                        ctx,
                    )
                }?;
                ctx.push_vec(
                    &mut selections,
                    selection,
                    "f3d ruled surface edge selection",
                )?;
            }
            Ok::<_, CodecError>(selections)
        })?;
    let edges = merge_edge_selections(ctx, scope, &selections)?;
    let support_native =
        ctx.copy_retained_text(&scope.id, "f3d ruled surface support native id")?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::RuledSurface {
            edges,
            support_faces: FaceSelection::Native(support_native),
            mode,
            angle: Some(angle),
            alternate_face: Some(operation.alternate_face),
            corner: Some(match operation.corner {
                DesignRuledSurfaceCorner::Rounded => RuledSurfaceCorner::Rounded,
                DesignRuledSurfaceCorner::Mitered => RuledSurfaceCorner::Mitered,
            }),
        },
    )))
}

fn merge_edge_selections(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    selections: &[cadmpeg_ir::features::EdgeSelection],
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let native = || -> Result<EdgeSelection, CodecError> {
        Ok(EdgeSelection::Native(ctx.copy_retained_text(
            &scope.id,
            "f3d merged edge native id",
        )?))
    };
    if ctx.all_by_limit(
        selections,
        |selection| {
            Ok(matches!(
                selection,
                EdgeSelection::Edges(_) | EdgeSelection::Resolved { .. }
            ))
        },
        "f3d merged edge selection scan",
    )? {
        let mut candidate_storage = ctx.reserve_scoped(0, "F3D merged direct edge candidate")?;
        let mut resolved = Vec::new();
        let mut members = selections.iter();
        while let Some(selection) =
            ctx.next_charged(&mut members, "f3d merged edge member source")?
        {
            let (EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. }) = selection
            else {
                drop(resolved);
                drop(candidate_storage);
                return native();
            };
            let mut source = edges.iter();
            while let Some(edge) =
                ctx.next_charged(&mut source, "f3d merged selected edge source")?
            {
                if !ctx.all_by_limit(
                    &resolved,
                    |candidate: &cadmpeg_ir::ids::EdgeId| {
                        Ok(!ctx.equal_bytes_limit(
                            candidate.as_str().as_bytes(),
                            edge.as_str().as_bytes(),
                            "f3d merged direct edge identity comparison",
                        )?)
                    },
                    "f3d merged edge duplicate scan",
                )? {
                    drop(resolved);
                    drop(candidate_storage);
                    return native();
                }
                let edge = candidate_storage
                    .with_storage(|| edge.try_clone_for_decode(ctx, "f3d merged direct edge id"))?;
                candidate_storage
                    .with_storage(|| ctx.push_vec(&mut resolved, edge, "f3d merged direct edge"))?;
            }
        }
        let selected_native = candidate_storage
            .with_storage(|| ctx.copy_retained_text(&scope.id, "f3d merged direct native id"))?;
        return candidate_storage.commit_value(EdgeSelection::Resolved {
            edges: resolved,
            native: selected_native,
        });
    }
    if let Some(EdgeSelection::Historical { state, .. }) = selections.first() {
        if ctx.all_by_limit(
            selections,
            |selection| match selection {
                EdgeSelection::Historical {
                    state: candidate, ..
                } => ctx.equal_bytes_limit(
                    candidate.as_str().as_bytes(),
                    state.as_str().as_bytes(),
                    "f3d merged historical state identity comparison",
                ),
                _ => Ok(false),
            },
            "f3d merged historical state scan",
        )? {
            let mut candidate_storage =
                ctx.reserve_scoped(0, "F3D merged historical edge candidate")?;
            let state = candidate_storage.with_storage(|| {
                state.try_clone_for_decode(ctx, "f3d merged historical edge state id")
            })?;
            let mut resolved = Vec::new();
            let mut members = selections.iter();
            while let Some(selection) =
                ctx.next_charged(&mut members, "f3d merged edge member source")?
            {
                let EdgeSelection::Historical { edges, .. } = selection else {
                    drop(resolved);
                    drop(state);
                    drop(candidate_storage);
                    return native();
                };
                let mut source = edges.iter();
                while let Some(edge) =
                    ctx.next_charged(&mut source, "f3d merged selected edge source")?
                {
                    if !ctx.all_by_limit(
                        &resolved,
                        |candidate: &cadmpeg_ir::ids::HistoricalEdgeId| {
                            Ok(!ctx.equal_bytes_limit(
                                candidate.as_str().as_bytes(),
                                edge.as_str().as_bytes(),
                                "f3d merged historical edge identity comparison",
                            )?)
                        },
                        "f3d merged historical edge duplicate scan",
                    )? {
                        drop(resolved);
                        drop(state);
                        drop(candidate_storage);
                        return native();
                    }
                    let edge = candidate_storage.with_storage(|| {
                        edge.try_clone_for_decode(ctx, "f3d merged historical edge id")
                    })?;
                    candidate_storage.with_storage(|| {
                        ctx.push_vec(&mut resolved, edge, "f3d merged historical edge")
                    })?;
                }
            }
            let selected_native = candidate_storage.with_storage(|| {
                ctx.copy_retained_text(&scope.id, "f3d merged historical native id")
            })?;
            let selected = candidate_storage.with_storage(|| -> Result<_, CodecError> {
                Ok(EdgeSelection::historical(
                    state,
                    resolved,
                    selected_native,
                    ctx,
                )?)
            })?;
            return match selected {
                Ok(selection) => candidate_storage.commit_value(selection),
                Err(_) => {
                    drop(candidate_storage);
                    native()
                }
            };
        }
    }
    native()
}

pub(crate) fn direct_face_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    if operands.is_empty() {
        return Ok(None);
    }
    let stream = ids::native_stream_charged(ctx, &scope.id, "f3d direct face scope stream")?;
    let mut matching_storage = ctx.reserve_scoped(0, "f3d direct face matching storage")?;
    let mut matching = Vec::new();
    for operand in ctx.admit_iter(operands, "f3d direct face operand source")? {
        if ctx.equal(
            &ids::native_stream_charged(ctx, &operand.id, "f3d direct face operand stream")?,
            &stream,
            "f3d direct face stream comparison",
        )? && operand.scope_record_index == scope.record_index
        {
            ctx.push_scoped_vec(
                &mut matching_storage,
                &mut matching,
                operand,
                "f3d direct face operand",
            )?;
        }
    }
    ctx.stable_sort_by_key(
        &mut matching,
        |operand| operand.scope_reference_ordinal,
        Ord::cmp,
        "sort f3d design feature_project 7",
    )?;
    if matching.is_empty() {
        return Ok(None);
    }
    let Some(previous_state_id) = scope.previous_history_state_id() else {
        return Ok(Some(FaceSelection::Native(
            ctx.copy_retained_text(&scope.id, "f3d direct native id")?,
        )));
    };
    let complete = ctx.all_by(
        &matching,
        |operand| Ok(!operand.resolved_face_slots.is_empty()),
        "f3d direct face complete member source",
    )?;
    if !complete
        && !ctx.any_by(
            &matching,
            |operand| Ok(!operand.resolved_face_slots.is_empty()),
            "f3d direct face resolved member source",
        )?
    {
        return Ok(Some(FaceSelection::Native(
            ctx.copy_retained_text(&scope.id, "f3d direct native id")?,
        )));
    }
    let ((feature_id, prefix), _identity_storage) =
        ctx.with_scoped_storage("f3d direct face identity storage", || {
            let feature_id = crate::design::identity::neutral_feature_id(ctx, scope)?;
            let key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
            let prefix =
                crate::design::identity::history_input_prefix(ctx, key, previous_state_id)?;
            Ok::<_, CodecError>((feature_id, prefix))
        })?;
    let mut distinct_storage = ctx.reserve_scoped(0, "f3d direct face distinct slot storage")?;
    let mut seen = HashSet::new();
    let (selection, selection_storage) =
        ctx.with_scoped_storage("f3d direct face selection storage", || {
            let mut resolved = Vec::new();
            let mut unresolved = Vec::new();
            for operand in ctx.admit_iter(&matching, "f3d direct face selected member source")? {
                if operand.resolved_face_slots.is_empty() {
                    let identity =
                        ctx.copy_retained_text(&operand.id, "f3d direct unresolved face id")?;
                    ctx.push_vec(&mut unresolved, identity, "f3d direct unresolved face")?;
                }
                for &slot in ctx.admit_iter(
                    &operand.resolved_face_slots,
                    "f3d direct face selected slot source",
                )? {
                    if !distinct_storage.with_storage(|| {
                        ctx.insert_hash_set(&mut seen, slot, "f3d direct face distinct slot")
                    })? {
                        continue;
                    }
                    let face = crate::design::identity::history_input_face_id(
                        ctx,
                        &prefix,
                        slot,
                        "f3d direct historical face id",
                    )?;
                    ctx.push_vec(
                        &mut resolved,
                        face,
                        if complete {
                            "f3d direct historical face"
                        } else {
                            "f3d direct partial historical face"
                        },
                    )?;
                }
            }
            let state = crate::design::identity::feature_input_topology_id(
                ctx,
                &feature_id,
                previous_state_id,
            )?;
            let native = ctx.copy_retained_text(
                &scope.id,
                if complete {
                    "f3d direct historical native id"
                } else {
                    "f3d direct partial native id"
                },
            )?;
            let selection = if complete {
                FaceSelection::historical(state, resolved, native, ctx)?
            } else {
                FaceSelection::historical_partial(state, resolved, unresolved, native, ctx)?
            };
            Ok::<_, CodecError>(selection)
        })?;
    drop((seen, distinct_storage));
    match selection {
        Ok(selection) => selection_storage.commit_value(Some(selection)),
        Err(_) => {
            drop(selection_storage);
            Ok(Some(FaceSelection::Native(ctx.copy_retained_text(
                &scope.id,
                if complete {
                    "f3d direct historical fallback id"
                } else {
                    "f3d direct partial fallback id"
                },
            )?)))
        }
    }
}

fn design_positive_length(
    parameter: &DesignParameter,
) -> Option<cadmpeg_ir::scalar::PositiveLength> {
    cadmpeg_ir::scalar::PositiveLength::try_from(design_length(parameter)?).ok()
}

pub(super) fn design_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::scalar::Length> {
    let value = parameter.evaluated_value().get() * 10.0;
    (parameter
        .unit()
        .map(|field| field.value.as_str())
        .is_some_and(design_length_unit))
    .then_some(cadmpeg_ir::scalar::Length::new(value)?)
}

pub(crate) fn design_length_unit(unit: &str) -> bool {
    matches!(unit, "mm" | "cm" | "m" | "in" | "ft")
}

pub(super) fn design_angle_unit(unit: &str) -> bool {
    matches!(unit, "deg" | "rad")
}

pub(super) fn design_dimension_unit(parameter: &DesignParameter) -> bool {
    let unit = parameter.unit().map(|field| field.value.as_str());
    if parameter.source_kind().starts_with("Linear Dimension")
        || parameter.source_kind().starts_with("Radius Dimension")
        || parameter.source_kind().starts_with("Radial Dimension")
        || parameter.source_kind().starts_with("Diameter Dimension")
    {
        return unit.is_some_and(design_length_unit);
    }
    if parameter.source_kind().starts_with("Angular Dimension") {
        return unit.is_some_and(design_angle_unit);
    }
    if parameter.source_kind().starts_with("Tangent Dimension") {
        return unit.is_some_and(design_length_unit);
    }
    false
}

fn project_variable_fillet(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D variable Fillet query scope stream"
    )?);
    let Some(group) = unique_feature_match_for_decode(
        ctx,
        construction_groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "match F3D variable Fillet group stream",
            )? && group.scope_record_index == scope.record_index)
        },
        "visit F3D variable Fillet group source",
    )?
    else {
        return Ok(None);
    };
    let (radius, radius_storage) = ctx
        .with_scoped_storage("f3d variable Fillet accepted radius storage", || {
            variable_fillet_law(ctx, parameters)
        })?;
    let (points, tangency_weight) = or_none!(radius);
    let definition = FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
            edges: {
                let (selection_feature_id, _selection_feature_storage) = ctx
                    .with_scoped_storage("f3d edge selection feature key", || {
                        crate::design::identity::neutral_feature_id(ctx, scope)
                    })?;
                resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: construction_groups,
                        operands: edge_operands,
                        identity_operands: edge_identity_operands,
                        vertex_operands: edge_treatment_vertex_operands,
                        histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &selection_feature_id,
                        treatment_radius: None,
                    },
                    ctx,
                )
            }?,
            radius: RadiusSpec::Variable { points },
            tangency_weight,
        }),
    });
    radius_storage.commit()?;
    Ok(Some(definition))
}

fn variable_fillet_law(
    ctx: &DecodeContext<'_>,
    parameters: &[(u32, &DesignParameter)],
) -> Result<
    Option<(
        cadmpeg_ir::features::edge_treatments::VariableRadii,
        Option<cadmpeg_ir::scalar::FiniteReal>,
    )>,
    CodecError,
> {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D variable Fillet scratch")?;

    let unique_parameter = |kind: &str| {
        unique_feature_match_for_decode(
            ctx,
            parameters.iter().map(|(_, parameter)| *parameter),
            |parameter| Ok(parameter.source_kind() == kind),
            "f3d variable Fillet parameter source",
        )
    };
    let start = or_none!(unique_parameter("StartRadius")?.and_then(design_length));
    let end = or_none!(unique_parameter("EndRadius")?.and_then(design_length));
    let mut tangencies = parameters.iter();
    let tangency_weight = match ctx.find_by(
        &mut tangencies,
        |(_, parameter)| Ok(parameter.source_kind() == "TangencyWeight"),
        "f3d variable Fillet tangency source",
    )? {
        None => None,
        Some((_, parameter)) => {
            if ctx.any_by(
                tangencies,
                |(_, parameter)| Ok(parameter.source_kind() == "TangencyWeight"),
                "f3d variable Fillet tangency source",
            )? {
                return Ok(None);
            }
            Some(parameter.evaluated_value())
        }
    };
    let mut middle_radii = Vec::new();
    let mut middle_parameters = Vec::new();
    for (ordinal, parameter) in
        ctx.admit_iter(parameters, "f3d variable Fillet middle parameter source")?
    {
        match parameter.source_kind() {
            "MidRadius" => scratch_storage.with_storage(|| {
                ctx.push_vec(
                    &mut middle_radii,
                    (*ordinal, *parameter),
                    "f3d variable Fillet middle radii",
                )
            })?,
            "MidParams" => scratch_storage.with_storage(|| {
                ctx.push_vec(
                    &mut middle_parameters,
                    (*ordinal, *parameter),
                    "f3d variable Fillet middle parameters",
                )
            })?,
            _ => {}
        }
    }
    ctx.stable_sort_by_key(
        &mut middle_radii[..],
        |value| {
            let (ordinal, _) = value;
            *ordinal
        },
        Ord::cmp,
        "sort f3d design feature_project 9",
    )?;
    ctx.stable_sort_by_key(
        &mut middle_parameters[..],
        |value| {
            let (ordinal, _) = value;
            *ordinal
        },
        Ord::cmp,
        "sort f3d design feature_project 10",
    )?;
    if middle_radii.len() != middle_parameters.len()
        || ctx.any_by(
            parameters,
            |(_, parameter)| {
                Ok(!matches!(
                    parameter.source_kind(),
                    "StartRadius" | "EndRadius" | "MidRadius" | "MidParams" | "TangencyWeight"
                ))
            },
            "f3d variable Fillet parameter kind source",
        )?
    {
        return Ok(None);
    }
    let mut sample_storage = ctx.reserve_scoped(0, "f3d variable Fillet radius point")?;
    let mut points = Vec::new();
    ctx.push_scoped_vec(
        &mut sample_storage,
        &mut points,
        VariableRadius {
            parameter: 0.0,
            radius: start,
        },
        "f3d variable Fillet radius point",
    )?;
    for ((_, radius), (_, parameter)) in ctx
        .admit_iter(middle_radii, "f3d variable Fillet radius conversion")?
        .zip(ctx.admit_iter(middle_parameters, "f3d variable Fillet position conversion")?)
    {
        let Some(radius) = design_length(radius) else {
            return Ok(None);
        };
        let parameter = parameter.evaluated_value().get();
        ctx.push_scoped_vec(
            &mut sample_storage,
            &mut points,
            VariableRadius { parameter, radius },
            "f3d variable Fillet radius point",
        )?;
    }
    ctx.push_scoped_vec(
        &mut sample_storage,
        &mut points,
        VariableRadius {
            parameter: 1.0,
            radius: end,
        },
        "f3d variable Fillet radius point",
    )?;
    let mut candidate_storage =
        ctx.reserve_scoped(0, "F3D variable fillet typed radius candidate")?;
    let converted = candidate_storage
        .with_storage(|| cadmpeg_ir::features::edge_treatments::VariableRadii::new(points, ctx))?;
    match converted {
        Ok(points) => candidate_storage.commit_value(Some((points, tangency_weight))),
        Err(_) => Ok(None),
    }
}

/// Count parameters whose unit token has no settled neutral quantity kind.
pub(crate) fn untyped_parameter_unit_count(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
) -> Result<usize, CodecError> {
    Ok(ctx
        .admit_iter(parameters, "f3d parameter unit scan")?
        .filter(|parameter| {
            parameter
                .unit()
                .map(|field| field.value.as_str())
                .is_some_and(|unit| !design_length_unit(unit) && !design_angle_unit(unit))
        })
        .count())
}

/// Select and order one Chamfer parameter lane. Only the returned reference
/// list is published; the ordinal/reference sort buffer remains temporary.
fn ordered_chamfer_parameters<'a>(
    ctx: &DecodeContext<'_>,
    parameters: &[(u32, &'a DesignParameter)],
    matches_kind: impl Fn(&str) -> bool,
) -> Result<Vec<&'a DesignParameter>, CodecError> {
    let mut matches_storage = ctx.reserve_scoped(0, "f3d chamfer ordered parameter entries")?;
    let mut matches = Vec::new();
    for parameter in ctx
        .admit_iter(parameters, "scan F3D chamfer parameter kinds")?
        .filter(|(_, parameter)| matches_kind(parameter.source_kind()))
        .copied()
    {
        matches_storage.with_storage(|| {
            ctx.push_vec(
                &mut matches,
                parameter,
                "f3d chamfer ordered parameter entries",
            )
        })?;
    }
    ctx.stable_sort_by_key(
        &mut matches[..],
        |(ordinal, _)| *ordinal,
        Ord::cmp,
        "sort f3d design feature_project 12",
    )?;
    let mut output_storage = ctx.reserve_scoped(0, "f3d chamfer ordered parameter output")?;
    let mut out = Vec::new();
    for (_, parameter) in ctx.admit_iter(matches, "f3d chamfer ordered output source")? {
        output_storage.with_storage(|| {
            ctx.push_vec(&mut out, parameter, "f3d chamfer ordered parameter output")
        })?;
    }
    drop(matches_storage);
    output_storage.commit_value(out)
}

fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    enum Lanes<'a> {
        Distance(&'a [&'a DesignParameter]),
        TwoDistances(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
        DistanceAngle(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
    }
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let native_scope = ids::native_stream_charged(ctx, &scope.id, "f3d Chamfer scope stream")?;
    let mut edge_groups = Vec::new();
    for group in ctx.admit_iter(construction_groups, "f3d Chamfer group source")? {
        if !feature_stream_matches(ctx, &group.id, native_scope, "f3d Chamfer group stream")?
            || group.scope_record_index != scope.record_index
            || group.extrude_role().is_some()
        {
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut edge_groups, group, "f3d chamfer edge groups"))?;
    }
    ctx.stable_sort_by_key(
        &mut edge_groups[..],
        |value| {
            let group = value;
            {
                group.scope_reference_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design feature_project 11",
    )?;
    let group_count = edge_groups.len();
    let mut ordered_parameters =
        |matches_kind: &dyn Fn(&str) -> bool| -> Result<Vec<&DesignParameter>, CodecError> {
            scratch_storage
                .with_storage(|| ordered_chamfer_parameters(ctx, parameters, matches_kind))
        };
    let distances = ordered_parameters(&|kind| kind == "Distance")?;
    let first_distances = ordered_parameters(&|kind| kind == "Distance 1")?;
    let second_distances = ordered_parameters(&|kind| kind == "Distance 2")?;
    let left_distances = ordered_parameters(&|kind| kind == "leftDistance")?;
    let right_distances = ordered_parameters(&|kind| kind == "rightDistance")?;
    let angles =
        ordered_parameters(&|kind| matches!(kind, "Angle" | "Rotate Angle" | "rotateAngle"))?;
    if !ctx.all_by(
        parameters,
        |(_, parameter)| {
            Ok(matches!(
                parameter.source_kind(),
                "Distance"
                    | "Distance 1"
                    | "Distance 2"
                    | "leftDistance"
                    | "rightDistance"
                    | "Angle"
                    | "Rotate Angle"
                    | "rotateAngle"
            ))
        },
        "f3d Chamfer parameter kind source",
    )? {
        return Ok(None);
    }

    let lanes = if !left_distances.is_empty() || !right_distances.is_empty() {
        if !distances.is_empty() || !first_distances.is_empty() || !second_distances.is_empty() {
            return Ok(None);
        }
        if right_distances.is_empty() && !angles.is_empty() {
            if left_distances.len() != group_count || angles.len() != group_count {
                return Ok(None);
            }
            Lanes::DistanceAngle(&left_distances, &angles)
        } else if right_distances.is_empty() {
            if left_distances.len() != group_count {
                return Ok(None);
            }
            Lanes::Distance(&left_distances)
        } else {
            if !angles.is_empty()
                || left_distances.len() != group_count
                || right_distances.len() != group_count
            {
                return Ok(None);
            }
            Lanes::TwoDistances(&left_distances, &right_distances)
        }
    } else if !first_distances.is_empty() || !second_distances.is_empty() {
        if !distances.is_empty()
            || !angles.is_empty()
            || first_distances.len() != group_count
            || second_distances.len() != group_count
        {
            return Ok(None);
        }
        Lanes::TwoDistances(&first_distances, &second_distances)
    } else if !angles.is_empty() {
        if distances.len() != group_count || angles.len() != group_count {
            return Ok(None);
        }
        Lanes::DistanceAngle(&distances, &angles)
    } else if !distances.is_empty() {
        if distances.len() != group_count {
            return Ok(None);
        }
        Lanes::Distance(&distances)
    } else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    let mut ordinals = 0..group_count;
    while let Some(ordinal) = ctx.next_charged(&mut ordinals, "f3d Chamfer specification source")? {
        let spec = match lanes {
            Lanes::Distance(distance) => design_positive_length(distance[ordinal])
                .map(|distance| ChamferSpec::Distance { distance }),
            Lanes::TwoDistances(first, second) => design_positive_length(first[ordinal])
                .zip(design_positive_length(second[ordinal]))
                .map(|(first, second)| ChamferSpec::TwoDistances { first, second }),
            Lanes::DistanceAngle(distance, angle) => design_positive_length(distance[ordinal])
                .zip(
                    design_angle(angle[ordinal])
                        .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::try_from(value).ok()),
                )
                .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle }),
        };
        let spec = or_none!(spec);
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut candidates, spec, "f3d chamfer specifications"))?;
    }
    let mut groups = Vec::new();
    for (spec, group) in ctx
        .admit_iter(candidates, "f3d Chamfer output specification source")?
        .zip(ctx.admit_iter(edge_groups, "f3d Chamfer output group source")?)
    {
        let group = ChamferGroup {
            edges: {
                let (selection_feature_id, _selection_feature_storage) = ctx
                    .with_scoped_storage("f3d edge selection feature key", || {
                        crate::design::identity::neutral_feature_id(ctx, scope)
                    })?;
                resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: construction_groups,
                        operands: edge_operands,
                        identity_operands: edge_identity_operands,
                        vertex_operands: edge_treatment_vertex_operands,
                        histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &selection_feature_id,
                        treatment_radius: None,
                    },
                    ctx,
                )
            }?,
            spec,
        };
        ctx.push_vec(&mut groups, group, "f3d chamfer output groups")?;
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: or_none!(groups.try_into().ok()),
            flip_direction: false,
        },
    )))
}

fn project_fixed_chamfer(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let fixed = or_none!(scope.fixed_chamfer_parameters());
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D unique feature stream"
    )?);
    let group = or_none!(unique_feature_match_for_decode(
        ctx,
        construction_groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && group.scope_record_index == scope.record_index)
        },
        "visit F3D unique feature source"
    )?);
    let spec = match fixed {
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::EqualDistance { distance } => {
            ChamferSpec::Distance {
                distance: or_none!(cadmpeg_ir::scalar::PositiveLength::new(distance.value.get() * 10.0)),
            }
        }
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::TwoDistances { first, second } => {
            ChamferSpec::TwoDistances {
                first: or_none!(cadmpeg_ir::scalar::PositiveLength::new(first.value.get() * 10.0)),
                second: or_none!(cadmpeg_ir::scalar::PositiveLength::new(second.value.get() * 10.0)),
            }
        }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: {
                    let (selection_feature_id, _selection_feature_storage) = ctx
                        .with_scoped_storage("f3d edge selection feature key", || {
                            crate::design::identity::neutral_feature_id(ctx, scope)
                        })?;
                    resolved_edge_treatment_group_with_corners(
                        group,
                        crate::design::edge_resolve::EdgeTreatmentInputs {
                            groups: construction_groups,
                            operands: edge_operands,
                            identity_operands: edge_identity_operands,
                            vertex_operands: edge_treatment_vertex_operands,
                            histories,
                            previous_state_id: scope.previous_history_state_id(),
                            feature_id: &selection_feature_id,
                            treatment_radius: None,
                        },
                        ctx,
                    )
                }?,
                spec,
            }),
            flip_direction: false,
        },
    )))
}

fn fixed_boolean_operation(operation: DesignExtrudeOperation) -> cadmpeg_ir::features::BooleanOp {
    match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    }
}

pub(super) fn project_fixed_revolve_with_entities(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    (placements, curve_identities): (&[DesignSketchPlacement], &[SketchCurveIdentity]),
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        AngularTermination, FeatureDefinition, FeatureOperation, PlanarProfileRef, RevolutionAxis,
        RevolveConstruction, RevolveExtent,
    };

    let crate::records::feature::scope::DesignScopePayload::Revolve(Some(
        crate::records::feature::path_features::DesignRevolveConstruction {
            operation, angle, ..
        },
    )) = &scope.payload()
    else {
        return Ok(None);
    };
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d project_fixed_revolve_with_entities scope stream"
    )?);
    let mut profile = None;
    let mut axis_group = None;
    let mut body_count = 0;
    let mut group_count = 0;
    for group in ctx.admit_iter(
        construction_groups,
        "f3d project_fixed_revolve_with_entities group source",
    )? {
        if !feature_stream_matches(
            ctx,
            &group.id,
            Some(stream),
            "f3d project_fixed_revolve_with_entities group stream",
        )? || group.scope_record_index != scope.record_index
        {
            continue;
        }
        group_count += 1;
        match group.role() {
            DesignOperandRole::PROFILE => {
                if profile.replace(group).is_some() {
                    return Ok(None);
                }
            }
            DesignOperandRole::ROLE_0X21 => {
                if axis_group.replace(group).is_some() {
                    return Ok(None);
                }
            }
            DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B => body_count += 1,
            _ => {}
        }
    }
    let profile = or_none!(profile);
    let axis_group = or_none!(axis_group);
    let expected_body_groups = usize::from(*operation != DesignExtrudeOperation::NewBody);
    if body_count != expected_body_groups || group_count != 2 + expected_body_groups {
        return Ok(None);
    }
    let [_] = profile.members() else {
        return Ok(None);
    };
    let [crate::records::identity::Located {
        value: axis_member, ..
    }] = axis_group.members()
    else {
        return Ok(None);
    };
    let mut operands = edge_operands.iter();
    let mut matching = || {
        ctx.find_by(
            &mut operands,
            |operand| {
                Ok(feature_stream_matches(
                    ctx,
                    &operand.id,
                    Some(stream),
                    "f3d Revolve axis operand stream",
                )? && operand.scope_record_index == scope.record_index
                    && operand.record_index() == *axis_member)
            },
            "f3d Revolve axis operand source",
        )
    };
    let first = matching()?;
    let second = matching()?;
    let axis = if let (Some(axis_operand), None) = (first, second) {
        Some(RevolutionAxis {
            origin: or_none!(axis_operand.resolved_axis).origin,
            direction: cadmpeg_ir::features::FeatureDirection3::from(
                or_none!(axis_operand.resolved_axis).direction,
            ),
            reference: None,
        })
    } else if first.is_none() {
        let resolved = resolve_sketch_axis_selection(
            ctx,
            scope,
            axis_group,
            *axis_member,
            entity_selection_operands,
            placements,
            curve_identities,
        )?;
        if resolved.is_none()
            && !unresolved_historical_face_axis_selection(
                ctx,
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
            )?
            && revolve_face_axis_operand(ctx, scope, axis_group, *axis_member, face_operands)?
                .is_none()
        {
            return Ok(None);
        }
        resolved
    } else {
        return Ok(None);
    };
    let revolve_profile: cadmpeg_ir::features::PlanarProfileRef = PlanarProfileRef::Native(
        ctx.copy_retained_text(&profile.id, "f3d Revolve native profile id")?,
    );
    let extent = RevolveExtent::OneSided {
        termination: AngularTermination::Angle { angle: *angle },
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Revolve {
            construction: match axis {
                Some(axis) => RevolveConstruction::Resolved {
                    profile: revolve_profile,
                    axis,
                    extent,
                    solid: None,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
                None => RevolveConstruction::Unresolved(
                    cadmpeg_ir::features::PartialRevolveConstruction::Axis {
                        profile: revolve_profile,
                        extent: Some(extent),
                        solid: None,
                        face_maker: None,
                        fuse_order: None,
                        allow_multi_profile_faces: None,
                    },
                ),
            },
            op: fixed_boolean_operation(*operation),
        },
    )))
}

fn unresolved_historical_face_axis_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<bool, CodecError> {
    let stream = ids::native_stream_charged(ctx, &scope.id, "resolve F3D unique feature stream")?;
    let selection = unique_feature_match_for_decode(
        ctx,
        entity_selection_operands.iter(),
        |operand| {
            Ok(feature_stream_matches(
                ctx,
                &operand.id,
                stream,
                "compare F3D unique feature stream",
            )? && operand.scope_record_index == scope.record_index
                && operand.group_record_index == axis_group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index() == axis_member)
        },
        "visit F3D unique feature source",
    )?;
    Ok(selection.is_some_and(|selection| !selection.historical_face_candidates.is_empty()))
}

fn revolve_face_axis_operand<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    face_operands: &'a [crate::records::topology::face::DesignFaceOperand],
) -> Result<Option<&'a crate::records::topology::face::DesignFaceOperand>, CodecError> {
    let stream = ids::native_stream_charged(ctx, &scope.id, "resolve F3D unique feature stream")?;
    let operand = or_none!(unique_feature_match_for_decode(
        ctx,
        face_operands.iter(),
        |operand| {
            Ok(feature_stream_matches(
                ctx,
                &operand.id,
                stream,
                "compare F3D unique feature stream",
            )? && operand.scope_record_index == scope.record_index
                && operand.group_record_index() == Some(axis_group.record_index)
                && operand.group_member_ordinal() == Some(0)
                && operand.record_index() == axis_member)
        },
        "visit F3D unique feature source"
    )?);
    let mut found = false;
    visit_revolve_face_candidates(ctx, operand, |_| {
        found = true;
        Ok(false)
    })?;
    Ok(found.then_some(operand))
}

/// Resolve Revolve axes selected through history-qualified analytic faces.
pub(crate) fn bind_revolve_face_axes(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut scope_storage = ctx.reserve_scoped(0, "F3D Revolve scope index")?;
    let mut scopes_by_id = HashMap::new();
    let mut scope_indexed = false;
    let mut features = features.iter_mut();
    while let Some(feature) = ctx.next_charged(&mut features, "visit F3D Revolve axis feature")? {
        let native_ref = feature.native_ref.as_deref();
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) = definition
                else {
                    return Ok(());
                };
                if construction.axis().is_some() {
                    return Ok(());
                }
                let Some(native_ref) = native_ref else {
                    return Ok(());
                };
                if !scope_indexed {
                    for scope in ctx.admit_iter(scopes, "f3d Revolve scope index scan")? {
                        if !ctx.contains_key_hash_map(
                            &scopes_by_id,
                            scope.id.as_str(),
                            "f3d Revolve scope index lookup",
                        )? {
                            // discarded-value: the first source scope owns this identity.
                            let _ = scope_storage.with_storage(|| {
                                ctx.insert_hash_map(
                                    &mut scopes_by_id,
                                    scope.id.as_str(),
                                    scope,
                                    "f3d Revolve scope index insertion",
                                )
                            })?;
                        }
                    }
                    scope_indexed = true;
                }
                let Some(&scope) =
                    ctx.get_hash_map(&scopes_by_id, native_ref, "f3d Revolve scope lookup")?
                else {
                    return Ok(());
                };
                let Some(stream) = ids::native_stream_charged(
                    ctx,
                    &scope.id,
                    "resolve F3D unique feature stream",
                )?
                else {
                    return Ok(());
                };
                let Some(group) = unique_feature_match_for_decode(
                    ctx,
                    construction_groups.iter(),
                    |group| {
                        Ok(feature_stream_matches(
                            ctx,
                            &group.id,
                            Some(stream),
                            "compare F3D unique feature stream",
                        )? && group.scope_record_index == scope.record_index
                            && group.role() == DesignOperandRole::ROLE_0X21)
                    },
                    "visit F3D unique feature source",
                )?
                else {
                    return Ok(());
                };
                let [crate::records::identity::Located { value: member, .. }] = group.members()
                else {
                    return Ok(());
                };
                let selection = unique_feature_match_for_decode(
                    ctx,
                    entity_selection_operands.iter(),
                    |operand| {
                        Ok(feature_stream_matches(
                            ctx,
                            &operand.id,
                            Some(stream),
                            "compare F3D unique feature stream",
                        )? && operand.scope_record_index == scope.record_index
                            && operand.group_record_index == group.record_index
                            && operand.group_member_ordinal == 0
                            && operand.record_index() == *member)
                    },
                    "visit F3D unique feature source",
                )?;
                let entity_face_slot = match selection {
                    Some(selection) => match selection.historical_face_candidates.first() {
                        Some(first)
                            if ctx.all_by(
                                &selection.historical_face_candidates,
                                |candidate| Ok(candidate.face_slot == first.face_slot),
                                "visit F3D Revolve entity face candidate",
                            )? =>
                        {
                            Some(first.face_slot)
                        }
                        _ => None,
                    },
                    None => None,
                };
                let entity_axis = match entity_face_slot {
                    Some(face_slot) => {
                        let mut identity_storage =
                            ctx.reserve_scoped(0, "hold F3D Revolve selected face identity")?;
                        let face_id = identity_storage.with_storage(|| {
                            cadmpeg_ir::ids::FaceId::mint(ctx.format_retained(
                                format_args!("f3d:brep:entity#{face_slot}"),
                                "F3D Revolve selected face identity",
                            )?)
                            .map_err(CodecError::malformed)
                        })?;
                        analytic_axis_for_face(ctx, &face_id, faces, surfaces)?
                    }
                    None => None,
                };
                let recipe_axis =
                    match revolve_face_axis_operand(ctx, scope, group, *member, face_operands)? {
                        Some(operand) => (|| -> Result<Option<_>, CodecError> {
                            let mut minimum: Option<&cadmpeg_ir::ids::FaceId> = None;
                            visit_revolve_face_candidates(ctx, operand, |face| {
                                let replace = match minimum {
                                    Some(first) => {
                                        let left = face.as_str().as_bytes();
                                        let right = first.as_str().as_bytes();
                                        let ordering = ctx
                                            .find_map(
                                                left.iter().zip(right),
                                                |(left, right)| {
                                                    Ok((left != right).then(|| left.cmp(right)))
                                                },
                                                "compare F3D Revolve candidate identity",
                                            )?
                                            .unwrap_or_else(|| left.len().cmp(&right.len()));
                                        // Iterator::min_by retains the last equal minimum.
                                        ordering != std::cmp::Ordering::Greater
                                    }
                                    None => true,
                                };
                                if replace {
                                    minimum = Some(face);
                                }
                                Ok(true)
                            })?;
                            let first_face = or_none!(minimum);
                            let first =
                                or_none!(analytic_axis_for_face(ctx, first_face, faces, surfaces)?);
                            let consistent =
                                visit_revolve_face_candidates(ctx, operand, |face_id| {
                                    Ok(analytic_axis_for_face(ctx, face_id, faces, surfaces)?
                                        .is_some_and(|axis| {
                                            crate::history::same_axis_line(
                                                (first.origin.get(), first.direction.get()),
                                                (axis.origin.get(), axis.direction.get()),
                                            )
                                        }))
                                })?;
                            Ok(consistent.then_some(first))
                        })()?,
                        None => None,
                    };
                construction.set_axis(match (entity_axis, recipe_axis) {
                    (Some(entity), Some(recipe))
                        if crate::history::same_axis_line(
                            (entity.origin.get(), entity.direction.get()),
                            (recipe.origin.get(), recipe.direction.get()),
                        ) =>
                    {
                        Some(entity)
                    }
                    (Some(axis), None) | (None, Some(axis)) => Some(axis),
                    _ => None,
                });
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn analytic_axis_for_face(
    ctx: &DecodeContext<'_>,
    face_id: &cadmpeg_ir::ids::FaceId,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    let face = or_none!(unique_feature_match_for_decode(
        ctx,
        faces.iter(),
        |face| {
            ctx.equal_bytes(
                face.id.as_str().as_bytes(),
                face_id.as_str().as_bytes(),
                "compare F3D Revolve face identity",
            )
        },
        "visit F3D unique feature source"
    )?);
    let surface = or_none!(unique_feature_match_for_decode(
        ctx,
        surfaces.iter(),
        |surface| {
            ctx.equal_bytes(
                surface.id.as_str().as_bytes(),
                face.surface.as_str().as_bytes(),
                "compare F3D Revolve surface identity",
            )
        },
        "visit F3D unique feature source"
    )?);
    Ok(analytic_surface_axis(&surface.geometry))
}

fn analytic_surface_axis(
    geometry: &cadmpeg_ir::geometry::SurfaceGeometry,
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let (origin, direction) = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            (plane_surface.origin(), *plane_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            (cylinder_surface.origin(), *cylinder_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            (cone_surface.origin(), *cone_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            (torus_surface.center(), *torus_surface.frame().axis())
        }
        _ => return None,
    };
    Some(cadmpeg_ir::features::RevolutionAxis {
        origin,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
        reference: None,
    })
}

fn resolve_sketch_axis_selection(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
    curve_identities: &[SketchCurveIdentity],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D unique feature stream"
    )?);
    let selection = or_none!(unique_feature_match_for_decode(
        ctx,
        entity_selection_operands.iter(),
        |operand| {
            Ok(feature_stream_matches(
                ctx,
                &operand.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && operand.scope_record_index == scope.record_index
                && operand.group_record_index == axis_group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index() == axis_member)
        },
        "visit F3D unique feature source"
    )?);
    let owner_reference = or_none!(u32::try_from(selection.primary_identity).ok());
    let placement = or_none!(unique_feature_match_for_decode(
        ctx,
        placements.iter(),
        |placement| {
            Ok(feature_stream_matches(
                ctx,
                &placement.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && placement.entity_id.suffix() == selection.primary_identity)
        },
        "visit F3D unique feature source"
    )?);
    let curve = or_none!(unique_feature_match_for_decode(
        ctx,
        curve_identities.iter(),
        |curve| {
            Ok(feature_stream_matches(
                ctx,
                &curve.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && curve.owner_reference == Some(owner_reference)
                && entity_selection_matches_curve(selection, curve))
        },
        "visit F3D unique feature source"
    )?);
    let SketchCurveGeometry::Line {
        start, direction, ..
    } = or_none!(curve.geometry.as_ref())
    else {
        return Ok(None);
    };
    let start = start.as_raw();
    let direction = direction.as_raw();
    let origin_scale = crate::design::face_resolve::placement_origin_scale(placement);
    let origin = Point3::new(
        placement.transform()[0][0] * start.x
            + placement.transform()[0][1] * start.y
            + placement.transform()[0][2] * start.z
            + placement.transform()[0][3] * origin_scale,
        placement.transform()[1][0] * start.x
            + placement.transform()[1][1] * start.y
            + placement.transform()[1][2] * start.z
            + placement.transform()[1][3] * origin_scale,
        placement.transform()[2][0] * start.x
            + placement.transform()[2][1] * start.y
            + placement.transform()[2][2] * start.z
            + placement.transform()[2][3] * origin_scale,
    );
    let direction = Vector3::new(
        placement.transform()[0][0] * direction.x
            + placement.transform()[0][1] * direction.y
            + placement.transform()[0][2] * direction.z,
        placement.transform()[1][0] * direction.x
            + placement.transform()[1][1] * direction.y
            + placement.transform()[1][2] * direction.z,
        placement.transform()[2][0] * direction.x
            + placement.transform()[2][1] * direction.y
            + placement.transform()[2][2] * direction.z,
    );
    Ok(Some(cadmpeg_ir::features::RevolutionAxis {
        origin: or_none!(cadmpeg_ir::features::FinitePoint3::new(origin)),
        direction: cadmpeg_ir::features::FeatureDirection3::from(or_none!(
            cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(direction)
        )),
        reference: None,
    }))
}

pub(super) fn project_fixed_loft(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    legacy_body_carriers: &[DesignLoftLegacyBodyCarrier],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    face_operands: &[DesignFaceOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(
        "f3d Loft accepted output",
        || -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
            use cadmpeg_ir::features::{
                FeatureDefinition, FeatureOperation, LoftPointSection, LoftSection,
                PlanarProfileRef, ProfileRef,
            };

            let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

            let crate::records::feature::scope::DesignScopePayload::Loft(Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation, ..
                },
            )) = &scope.payload()
            else {
                return Ok(None);
            };
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "f3d project_fixed_loft scope stream"
            )?);
            let mut groups = Vec::new();
            for group in
                ctx.admit_iter(construction_groups, "f3d project_fixed_loft group source")?
            {
                if !feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d project_fixed_loft group stream",
                )? || group.scope_record_index != scope.record_index
                {
                    continue;
                }
                scratch_storage
                    .with_storage(|| ctx.push_vec(&mut groups, group, "f3d Loft scope group"))?;
            }
            ctx.stable_sort_by_key(
                &mut groups[..],
                |value| {
                    let group = value;
                    group.scope_reference_ordinal
                },
                Ord::cmp,
                "sort f3d design feature_project 13",
            )?;
            let mut carriers = legacy_body_carriers.iter();
            let mut matching_legacy_carriers = || {
                ctx.find_by(
                    &mut carriers,
                    |carrier| {
                        Ok(feature_stream_matches(
                            ctx,
                            &carrier.id,
                            Some(stream),
                            "f3d Loft carrier stream",
                        )? && carrier.scope_record_index == scope.record_index)
                    },
                    "f3d Loft carrier source",
                )
            };
            let legacy_body_group_identity =
                match (matching_legacy_carriers()?, matching_legacy_carriers()?) {
                    (None, _) => None,
                    (Some(_), None) => {
                        if ctx.any_by(
                            &groups,
                            |group| Ok(group.role() == DesignOperandRole::BODIES_A),
                            "f3d Loft legacy body role source",
                        )? {
                            return Ok(None);
                        }
                        let body_group = or_none!(unique_feature_match_for_decode(
                            ctx,
                            groups.iter(),
                            |group| {
                                Ok(group.role() == DesignOperandRole::BODIES_B
                                    && group.scope_reference_ordinal == 1)
                            },
                            "visit F3D legacy Loft body group source",
                        )?);
                        Some((body_group.record_index, body_group.scope_reference_ordinal))
                    }
                    _ => return Ok(None),
                };
            let is_body_group = |group: &DesignConstructionOperandGroup| {
                group.role() == DesignOperandRole::BODIES_A
                    || legacy_body_group_identity
                        == Some((group.record_index, group.scope_reference_ordinal))
            };
            let body_count = ctx
                .admit_iter(&groups, "f3d Loft body count source")?
                .filter(|group| is_body_group(group))
                .count();
            let expected_body_count = usize::from(*operation != DesignExtrudeOperation::NewBody);
            if body_count != expected_body_count {
                return Ok(None);
            }
            let mut operands = Vec::new();
            for group in ctx
                .admit_iter(&groups, "f3d Loft operand group source")?
                .filter(|group| !is_body_group(group))
            {
                scratch_storage.with_storage(|| {
                    ctx.push_vec(&mut operands, *group, "f3d Loft operand group")
                })?;
            }
            let mut profile_groups = Vec::new();
            for group in ctx
                .admit_iter(&operands, "f3d Loft profile group source")?
                .filter(|group| {
                    matches!(
                        group.role(),
                        DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                    )
                })
            {
                scratch_storage.with_storage(|| {
                    ctx.push_vec(&mut profile_groups, *group, "f3d Loft profile group")
                })?;
            }
            let (sections, guides, centerline) = if profile_groups.len() >= 2 {
                if ctx.any_by(
                    &operands,
                    |group| {
                        Ok(!matches!(
                            group.role(),
                            DesignOperandRole::PROFILE
                                | DesignOperandRole::ROLE_0X43
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::ROLE_0X7
                        ))
                    },
                    "f3d Loft operand role source",
                )? {
                    return Ok(None);
                }
                let mut sections = Vec::new();
                let mut profiles = profile_groups.iter();
                while let Some(group) =
                    ctx.next_charged(&mut profiles, "f3d Loft section profile source")?
                {
                    let edge_profile =
                        resolved_loft_edge_profile_group(ctx, scope, group, edge_operands)?;
                    let face_profile = if edge_profile.is_none() {
                        resolved_profile_face_group(ctx, scope, group, face_operands)?
                    } else {
                        None
                    };
                    let profile = match edge_profile.or(face_profile) {
                        Some(profile) => profile,
                        None => ProfileRef::Planar(PlanarProfileRef::Native(
                            ctx.copy_retained_text(&group.id, "f3d Loft profile group id")?,
                        )),
                    };
                    ctx.push_vec(
                        &mut sections,
                        LoftSection::Profile(profile),
                        "f3d Loft section",
                    )?;
                }
                let mut guides = Vec::new();
                for group in ctx
                    .admit_iter(&operands, "f3d Loft guide group source")?
                    .filter(|group| group.role() == DesignOperandRole::ROLE_0X5)
                {
                    let path = resolved_loft_path(
                        group,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
                        scope,
                        ctx,
                    )?;
                    ctx.push_vec(&mut guides, path, "f3d Loft guide")?;
                }
                let mut centerline = None;
                for group in ctx
                    .admit_iter(&operands, "f3d Loft centerline group source")?
                    .filter(|group| group.role() == DesignOperandRole::ROLE_0X7)
                {
                    let path = resolved_loft_path(
                        group,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
                        scope,
                        ctx,
                    )?;
                    if centerline.replace(path).is_some() {
                        return Ok(None);
                    }
                }
                if centerline.is_some() && !guides.is_empty() {
                    return Ok(None);
                }
                (sections, guides, centerline)
            } else if *operation == DesignExtrudeOperation::NewBody {
                if profile_groups.len() == 1
                    && ctx.all_by(
                        &operands,
                        |group| {
                            Ok(matches!(
                                group.role(),
                                DesignOperandRole::ROLE_0X43 | DesignOperandRole::ROLE_0X5
                            ))
                        },
                        "f3d Loft point lane role source",
                    )?
                {
                    let point_ordinal = ctx.position_by(
                        &operands,
                        |group| {
                            Ok(group.role() == DesignOperandRole::ROLE_0X5
                                && group.members().len() == 1)
                        },
                        "f3d Loft point section source",
                    )?;
                    let point_ordinal = or_none!(point_ordinal);
                    if !matches!(point_ordinal, 0) && point_ordinal + 1 != operands.len() {
                        return Ok(None);
                    }
                    if ctx.any_by(
                        operands.iter().enumerate(),
                        |(ordinal, group)| {
                            Ok(ordinal != point_ordinal
                                && group.role() == DesignOperandRole::ROLE_0X5
                                && group.members().len() == 1)
                        },
                        "f3d Loft duplicate point source",
                    )? {
                        return Ok(None);
                    }
                    let mut sections = Vec::new();
                    let mut source = operands.iter().enumerate();
                    while let Some((ordinal, group)) =
                        ctx.next_charged(&mut source, "f3d Loft point section output source")?
                    {
                        let id = ctx.copy_retained_text(&group.id, "f3d Loft section group id")?;
                        let section = if ordinal == point_ordinal {
                            LoftSection::Point(LoftPointSection::Native(or_none!(
                                cadmpeg_core::text::NonBlankString::for_decode(
                                    ctx,
                                    id,
                                    "validate nonblank text"
                                )?
                            )))
                        } else {
                            LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(id)))
                        };
                        ctx.push_vec(&mut sections, section, "f3d Loft section")?;
                    }
                    (sections, Vec::new(), None)
                } else if profile_groups.is_empty() {
                    let role = if ctx.all_by(
                        &operands,
                        |group| Ok(group.role() == DesignOperandRole::PROFILE),
                        "f3d Loft homogeneous section role source",
                    )? {
                        DesignOperandRole::PROFILE
                    } else if ctx.all_by(
                        &operands,
                        |group| Ok(group.role() == DesignOperandRole::ROLE_0X5),
                        "f3d Loft homogeneous section role source",
                    )? {
                        DesignOperandRole::ROLE_0X5
                    } else {
                        return Ok(None);
                    };
                    let mut sections = Vec::new();
                    for group in ctx
                        .admit_iter(&operands, "f3d Loft homogeneous section source")?
                        .filter(|group| group.role() == role)
                    {
                        let id = ctx.copy_retained_text(&group.id, "f3d Loft section group id")?;
                        ctx.push_vec(
                            &mut sections,
                            LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(id))),
                            "f3d Loft section",
                        )?;
                    }
                    (sections, Vec::new(), None)
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            };
            if sections.len() < 2
                || sections.len() + guides.len() + usize::from(centerline.is_some()) + body_count
                    != groups.len()
            {
                return Ok(None);
            }
            Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
                sections,
                guidance: if let Some(centerline) = centerline {
                    cadmpeg_ir::features::LoftGuidance::Centerline(centerline)
                } else {
                    cadmpeg_ir::features::LoftGuidance::Guides(guides)
                },
                op: fixed_boolean_operation(*operation),
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            })))
        },
    )?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

fn resolved_loft_path(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    let (selection, selection_storage) =
        ctx.with_scoped_storage("f3d loft edge selection storage", || {
            let (selection_feature_id, _selection_feature_storage) = ctx
                .with_scoped_storage("f3d edge selection feature key", || {
                    crate::design::identity::neutral_feature_id(ctx, scope)
                })?;
            resolved_edge_group(
                group,
                groups,
                operands,
                identity_operands,
                scope.previous_history_state_id(),
                &selection_feature_id,
                ctx,
            )
        })?;
    if matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::All
            | cadmpeg_ir::features::EdgeSelection::Unresolved
            | cadmpeg_ir::features::EdgeSelection::Generated { .. }
            | cadmpeg_ir::features::EdgeSelection::HistoricalPartial { .. }
    ) {
        drop(selection);
        drop(selection_storage);
        return loft_path_from_edge_selection(
            ctx,
            &group.id,
            cadmpeg_ir::features::EdgeSelection::Unresolved,
        );
    }
    selection_storage.commit_value(loft_path_from_edge_selection(ctx, &group.id, selection)?)
}

#[derive(Clone, Copy)]
enum SurfacePatchRecipe {
    Grouped,
    Direct,
}

fn resolved_surface_patch_path(
    groups: &[&DesignConstructionOperandGroup],
    all_groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    recipe: SurfacePatchRecipe,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    let resolve_group = |group: &DesignConstructionOperandGroup| {
        let (selection, selection_storage) =
            ctx.with_scoped_storage("f3d surface patch group selection storage", || {
                let (selection_feature_id, _selection_feature_storage) = ctx
                    .with_scoped_storage("f3d edge selection feature key", || {
                        crate::design::identity::neutral_feature_id(ctx, scope)
                    })?;
                if matches!(recipe, SurfacePatchRecipe::Grouped) {
                    resolved_surface_patch_edge_group(
                        group,
                        all_groups,
                        operands,
                        identity_operands,
                        scope.previous_history_state_id(),
                        &selection_feature_id,
                        ctx,
                    )
                } else {
                    resolved_edge_group(
                        group,
                        all_groups,
                        operands,
                        identity_operands,
                        scope.previous_history_state_id(),
                        &selection_feature_id,
                        ctx,
                    )
                }
            })?;
        if matches!(
            selection,
            cadmpeg_ir::features::EdgeSelection::All
                | cadmpeg_ir::features::EdgeSelection::Unresolved
                | cadmpeg_ir::features::EdgeSelection::Generated { .. }
                | cadmpeg_ir::features::EdgeSelection::HistoricalPartial { .. }
        ) {
            drop(selection);
            drop(selection_storage);
            return loft_path_from_edge_selection(
                ctx,
                &group.id,
                cadmpeg_ir::features::EdgeSelection::Unresolved,
            );
        }
        selection_storage.commit_value(loft_path_from_edge_selection(ctx, &group.id, selection)?)
    };
    if matches!(recipe, SurfacePatchRecipe::Grouped) && groups.len() == 1 {
        let mut source = groups.iter();
        if let Some(group) = ctx.next_charged(&mut source, "f3d surface patch path group source")? {
            return resolve_group(group);
        }
    }
    let (paths, _paths_storage) =
        ctx.with_scoped_storage("f3d surface patch input paths storage", || {
            let mut paths = Vec::new();
            for group in ctx.admit_iter(groups, "f3d surface patch path group source")? {
                ctx.push_vec(&mut paths, resolve_group(group)?, "f3d surface patch path")?;
            }
            Ok::<_, CodecError>(paths)
        })?;
    if paths.is_empty() {
        return Ok(PathRef::Native(
            ctx.copy_retained_text(&scope.id, "f3d surface patch native path")?,
        ));
    }
    if let Some(state) = ctx.find_map(
        &paths,
        |path| {
            Ok(match path {
                PathRef::HistoricalEdges { state, .. } => Some(state),
                _ => None,
            })
        },
        "f3d surface patch historical state source",
    )? {
        if ctx.all_by(
            &paths,
            |path| match path {
                PathRef::HistoricalEdges {
                    state: candidate, ..
                } => ctx.equal(
                    candidate,
                    state,
                    "f3d surface patch historical state comparison",
                ),
                _ => Ok(false),
            },
            "f3d surface patch historical lane source",
        )? {
            let (candidate, candidate_storage) =
                ctx.with_scoped_storage("f3d surface patch accepted historical path", || {
                    let state =
                        (state).try_clone_for_decode(ctx, "f3d surface patch historical state")?;
                    let mut edges = Vec::new();
                    for path in ctx.admit_iter(&paths, "f3d surface patch selected path source")? {
                        if let PathRef::HistoricalEdges {
                            edges: group_edges, ..
                        } = path
                        {
                            for edge in ctx.admit_iter(
                                group_edges.as_slice(),
                                "f3d surface patch selected historical edge source",
                            )? {
                                let edge = (edge).try_clone_for_decode(
                                    ctx,
                                    "f3d surface patch historical edge id",
                                )?;
                                ctx.push_vec(
                                    &mut edges,
                                    edge,
                                    "f3d surface patch historical edge",
                                )?;
                            }
                        }
                    }
                    let members = cadmpeg_ir::features::SelectionMembers::new(
                        edges,
                        ctx,
                        "f3d surface patch historical uniqueness",
                    )?;
                    Ok::<_, CodecError>(members.map(|edges| (state, edges)))
                })?;
            let native =
                ctx.copy_retained_text(&scope.id, "f3d surface patch historical native path")?;
            return Ok(match candidate {
                Ok((state, edges)) => {
                    let native = cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        native,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| CodecError::malformed("surface patch path is blank"))?;
                    let (state, edges) = candidate_storage.commit_value((state, edges))?;
                    PathRef::HistoricalEdges {
                        state,
                        edges,
                        native,
                    }
                }
                Err(_) => {
                    drop(candidate_storage);
                    PathRef::Native(native)
                }
            });
        }
    }
    if ctx.all_by(
        &paths,
        |path| Ok(matches!(path, PathRef::Edges(_))),
        "f3d surface patch direct lane source",
    )? {
        let mut edges = Vec::new();
        for path in ctx.admit_iter(&paths, "f3d surface patch selected path source")? {
            if let PathRef::Edges(group_edges) = path {
                for edge in
                    ctx.admit_iter(group_edges, "f3d surface patch selected direct edge source")?
                {
                    let edge =
                        (edge).try_clone_for_decode(ctx, "f3d surface patch direct edge id")?;
                    ctx.push_vec(&mut edges, edge, "f3d surface patch direct edge")?;
                }
            }
        }
        return Ok(PathRef::Edges(edges));
    }
    Ok(PathRef::Native(ctx.copy_retained_text(
        &scope.id,
        "f3d surface patch native path",
    )?))
}

fn loft_path_from_edge_selection(
    ctx: &DecodeContext<'_>,
    native: &str,
    selection: cadmpeg_ir::features::EdgeSelection,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};

    match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            Ok(PathRef::Edges(edges))
        }
        EdgeSelection::Historical {
            state,
            edges,
            native,
        } => Ok(PathRef::HistoricalEdges {
            state,
            edges,
            native,
        }),
        EdgeSelection::Native(selected)
            if ctx.equal_bytes(
                selected.as_bytes(),
                native.as_bytes(),
                "f3d loft native identity comparison",
            )? =>
        {
            Ok(PathRef::Native(selected))
        }
        EdgeSelection::All
        | EdgeSelection::Unresolved
        | EdgeSelection::Native(_)
        | EdgeSelection::Generated { .. }
        | EdgeSelection::HistoricalPartial { .. } => Ok(PathRef::Native(
            ctx.copy_retained_text(native, "f3d loft native path")?,
        )),
    }
}

pub(super) fn project_fixed_sweep(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PlanarProfileRef, SweepGuideRail,
        SweepOrientation, SweepPathExtent,
    };
    use cadmpeg_ir::scalar::Angle;

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let Some((operation, values, profile, path, paths, guide_surfaces)) =
        (|| -> Result<Option<_>, CodecError> {
            let crate::records::feature::scope::DesignScopePayload::Sweep(Some(
                crate::records::feature::scope::DesignSweepScope {
                    construction:
                        Some(crate::records::feature::path_features::DesignSweepConstruction {
                            operation,
                            values,
                            ..
                        }),
                    ..
                },
            )) = &scope.payload()
            else {
                return Ok(None);
            };
            let values = values.map(cadmpeg_ir::scalar::FiniteReal::get);
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "f3d project_fixed_sweep scope stream"
            )?);
            let mut groups = Vec::new();
            for group in
                ctx.admit_iter(construction_groups, "f3d project_fixed_sweep group source")?
            {
                if !feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d project_fixed_sweep group stream",
                )? || group.scope_record_index != scope.record_index
                {
                    continue;
                }
                scratch_storage
                    .with_storage(|| ctx.push_vec(&mut groups, group, "f3d Sweep scope group"))?;
            }
            let mut profiles = Vec::new();
            let mut paths = Vec::new();
            let mut bodies = Vec::new();
            let mut guide_surfaces = Vec::new();
            for group in ctx.admit_iter(&groups, "f3d Sweep role partition source")? {
                match group.role() {
                    DesignOperandRole::PROFILE => {
                        scratch_storage.with_storage(|| {
                            ctx.push_vec(&mut profiles, *group, "f3d Sweep profile group")
                        })?;
                    }
                    DesignOperandRole::ROLE_0X5 => {
                        scratch_storage.with_storage(|| {
                            ctx.push_vec(&mut paths, *group, "f3d Sweep path group")
                        })?;
                    }
                    DesignOperandRole::BODIES_A => {
                        scratch_storage.with_storage(|| {
                            ctx.push_vec(&mut bodies, *group, "f3d Sweep body group")
                        })?;
                    }
                    DesignOperandRole::FACES => {
                        scratch_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut guide_surfaces,
                                *group,
                                "f3d Sweep guide surface group",
                            )
                        })?;
                    }
                    _ => {}
                }
            }
            ctx.stable_sort_by_key(
                &mut paths[..],
                |value| {
                    let group = value;
                    {
                        group.scope_reference_ordinal
                    }
                },
                Ord::cmp,
                "sort f3d design feature_project 14",
            )?;
            let guide_surface_form = match guide_surfaces.as_slice() {
                [] => false,
                [_] => true,
                _ => return Ok(None),
            };
            let profile = if guide_surface_form {
                let sweep_profile = or_none!(scope.sweep_profile());
                let carrier = |group: &&DesignConstructionOperandGroup| {
                    group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([sweep_profile.record_index])
                };
                let Some(_carrier) = unique_feature_match_for_decode(
                    ctx,
                    profiles.iter().copied(),
                    |group| Ok(carrier(&group)),
                    "f3d Sweep carrier source",
                )?
                else {
                    return Ok(None);
                };
                let Some(selection) = unique_feature_match_for_decode(
                    ctx,
                    profiles.iter().copied(),
                    |group| Ok(!carrier(&group)),
                    "f3d Sweep selection source",
                )?
                else {
                    return Ok(None);
                };
                if selection.members().is_empty()
                    || !ctx.all_by(
                        selection.members(),
                        |member| {
                            ctx.any_by(
                                entity_selection_operands,
                                |operand| {
                                    Ok(feature_stream_matches(
                                        ctx,
                                        &operand.id,
                                        Some(stream),
                                        "f3d Sweep selection operand stream",
                                    )? && operand.scope_record_index == scope.record_index
                                        && operand.group_record_index == selection.record_index
                                        && operand.record_index() == member.value)
                                },
                                "f3d Sweep selection operand source",
                            )
                        },
                        "f3d Sweep selection member source",
                    )?
                {
                    return Ok(None);
                }
                selection
            } else {
                let [profile] = profiles.as_slice() else {
                    return Ok(None);
                };
                *profile
            };
            let ([path] | [path, _]) = paths.as_slice() else {
                return Ok(None);
            };
            let expected_group_count =
                profiles.len() + paths.len() + bodies.len() + guide_surfaces.len();
            if bodies.len() > 1
                || groups.len() != expected_group_count
                || (*operation == DesignExtrudeOperation::NewBody && !bodies.is_empty())
                || (guide_surface_form && paths.len() != 1)
                || values[..4].iter().any(|value| !(0.0..=1.0).contains(value))
                || (paths.len() == 1 && values[2..4] != [1.0; 2])
            {
                return Ok(None);
            }
            Ok(Some((
                operation,
                values,
                profile,
                *path,
                paths,
                guide_surfaces,
            )))
        })()?
    else {
        return Ok(None);
    };
    let path = resolved_loft_path(
        path,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
        ctx,
    )?;
    let guide_extent = SweepPathExtent {
        along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(values[2])),
        against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(values[3])),
    };
    let guide_rail = if let Some(rail) = paths.get(1) {
        Some(SweepGuideRail {
            path: resolved_loft_path(
                rail,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope,
                ctx,
            )?,
            extent: guide_extent,
        })
    } else {
        None
    };
    let orientation = match guide_surfaces.first() {
        Some(group) => Some(SweepOrientation::GuideSurface {
            faces: match resolved_historical_face_group(
                ctx,
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            )? {
                Some(selection) => selection,
                None => FaceSelection::Native(
                    ctx.copy_retained_text(&group.id, "f3d Sweep guide surface id")?,
                ),
            },
        }),
        None => None,
    };
    let twist_angle = or_none!(Angle::new(values[4]));
    let taper_angle = or_none!(Angle::new(values[5]));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::Solid {
                op: if *operation == DesignExtrudeOperation::NewBody {
                    cadmpeg_ir::features::SolidSweepOperation::NewBody
                } else {
                    or_none!(fixed_boolean_operation(*operation).try_into().ok())
                },
                section: cadmpeg_ir::features::SweepSection::Profile(PlanarProfileRef::Native(
                    ctx.copy_retained_text(&profile.id, "f3d Sweep profile id")?,
                )),
                sections: Vec::new(),
            },

            path: Some(path),

            orientation,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: (values[4] != 0.0).then_some(twist_angle),
            path_extent: Some(SweepPathExtent {
                along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(values[0])),
                against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(values[1])),
            }),
            guide_rail,
            taper: (values[5] != 0.0).then_some(taper_angle),
            scale: None,
            allow_multi_profile_faces: None,
        },
    )))
}

fn legacy_pipe_references_complete(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    path_group: &DesignConstructionOperandGroup,
    record_indexes: [u32; 4],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "F3D Pipe reference scratch")?;
    storage.with_storage(|| {
        let mut claimed = BTreeSet::new();
        for record_index in record_indexes {
            if !ctx.insert_btree_set(&mut claimed, record_index, "f3d Pipe claimed record")? {
                return Ok(false);
            }
        }
        if path_group.members().is_empty()
            || !ctx.insert_btree_set(
                &mut claimed,
                path_group.record_index,
                "f3d Pipe claimed record",
            )?
        {
            return Ok(false);
        }
        let mut members = path_group.members().iter();
        while let Some(member) = ctx.next_charged(&mut members, "f3d Pipe path member source")? {
            if !ctx.insert_btree_set(&mut claimed, member.value, "f3d Pipe claimed record")? {
                return Ok(false);
            }
        }
        if scope.reference_members().len() != path_group.members().len() + 6 {
            return Ok(false);
        }
        let mut references = HashSet::new();
        let mut source = scope.reference_members().values();
        while let Some(value) = ctx.next_charged(&mut source, "f3d Pipe reference source")? {
            if !ctx.insert_hash_set(&mut references, *value, "f3d Pipe reference record")? {
                return Ok(false);
            }
        }
        ctx.all_by(
            &claimed,
            |record_index| {
                ctx.contains_hash_set(&references, record_index, "f3d Pipe reference membership")
            },
            "f3d Pipe claimed source",
        )
    })
}

fn project_fixed_pipe(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, GeneratedSweepSection, SweepSection,
    };

    let Some((path_group, section_size, wall_thickness, record_indexes, legacy_reference_layout)) =
        (|| -> Result<Option<_>, CodecError> {
            let crate::records::feature::scope::DesignScopePayload::Pipe(Some(
                crate::records::feature::path_features::DesignPipeConstruction {
                    operation,
                    section_shape,
                    filled,
                    values,
                    record_indexes,
                    ..
                },
            )) = &scope.payload()
            else {
                return Ok(None);
            };
            let values = values.map(cadmpeg_ir::scalar::FiniteReal::get);
            if *operation != DesignExtrudeOperation::NewBody
                || *section_shape
                    != crate::records::feature::surface_ops::DesignPipeSectionShape::Circular
                || values[0..2] != [1.0, 1.0]
                || values[2] <= 0.0
                || values[3] <= 0.0
                || parameters.len() != 4
            {
                return Ok(None);
            }
            let unique = |source_kind: &str| {
                Ok::<_, CodecError>(
                    unique_feature_match_for_decode(
                        ctx,
                        parameters.iter(),
                        |(_, parameter)| {
                            ctx.equal_bytes(
                                parameter.source_kind().as_bytes(),
                                source_kind.as_bytes(),
                                "compare F3D Pipe parameter kind",
                            )
                        },
                        "visit F3D Pipe parameter source",
                    )?
                    .map(|(_, parameter)| *parameter),
                )
            };
            let along = or_none!(unique("AlongDistance")?);
            let against = or_none!(unique("AgainstDistance")?);
            let section_size_parameter = or_none!(unique("SectionSize")?);
            let section_thickness_parameter = or_none!(unique("SectionThickness")?);
            let section_size = or_none!(design_length(section_size_parameter));
            let section_thickness = or_none!(design_positive_length(section_thickness_parameter));
            if along.unit().is_some()
                || against.unit().is_some()
                || along.evaluated_value().get() != values[0]
                || against.evaluated_value().get() != values[1]
                || section_size_parameter.evaluated_value().get() != values[2]
                || section_thickness_parameter.evaluated_value().get() != values[3]
                || section_size.get() <= 0.0
            {
                return Ok(None);
            }
            let wall_thickness = if *filled {
                None
            } else if section_thickness.get() < section_size.get() / 2.0 {
                Some(section_thickness)
            } else {
                return Ok(None);
            };
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "f3d Pipe scope stream"
            )?);
            let matching = |group: &DesignConstructionOperandGroup| -> Result<bool, CodecError> {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d Pipe construction group stream",
                )? && group.scope_record_index == scope.record_index)
            };
            let legacy_reference_layout = matches!(
                (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
                ("405", "259") | ("421", "257") | ("475", "260")
            );
            let path_group = if legacy_reference_layout {
                if ctx.any_by(
                    construction_groups,
                    |group| Ok(matching(group)? && group.role() != DesignOperandRole::ROLE_0X5),
                    "f3d Pipe legacy group role source",
                )? {
                    return Ok(None);
                }
                or_none!(unique_feature_match_for_decode(
                    ctx,
                    construction_groups.iter(),
                    |group| Ok(matching(group)? && group.role() == DesignOperandRole::ROLE_0X5),
                    "f3d Pipe legacy path group source"
                )?)
            } else {
                let path_group = or_none!(unique_feature_match_for_decode(
                    ctx,
                    construction_groups.iter(),
                    matching,
                    "f3d Pipe path group source"
                )?);
                if path_group.role() != DesignOperandRole::ROLE_0X5
                    || path_group.scope_reference_ordinal != 5
                    || scope.reference_members().values().nth(5) != Some(&path_group.record_index)
                    || path_group.members().is_empty()
                    || scope.reference_members().len() != path_group.members().len() + 8
                    || !ctx.all_by(
                        path_group.members().iter().zip(or_none!(scope
                            .reference_members()
                            .values_in(6..scope.reference_members().len() - 2))),
                        |(member, reference)| Ok(member.value == *reference),
                        "f3d Pipe ordered member reference proof",
                    )?
                {
                    return Ok(None);
                }
                path_group
            };
            Ok(Some((
                path_group,
                section_size,
                wall_thickness,
                *record_indexes,
                legacy_reference_layout,
            )))
        })()?
    else {
        return Ok(None);
    };
    if legacy_reference_layout
        && !legacy_pipe_references_complete(ctx, scope, path_group, record_indexes)?
    {
        return Ok(None);
    }
    let path = resolved_loft_path(
        path_group,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
        ctx,
    )?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
                section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
                    region: or_none!(cadmpeg_ir::features::SweepCircularRegion::new(
                        or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                            section_size.get() / 2.0
                        )),
                        wall_thickness,
                    )
                    .ok()),
                }),
                sections: Vec::new(),
            },

            path: Some(path),

            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
    )))
}

fn surface_patch_boundary_continuity(
    continuity: crate::records::feature::surface_ops::DesignPatchContinuity,
) -> Option<cadmpeg_ir::features::SurfaceContinuity> {
    use cadmpeg_ir::features::SurfaceContinuity;

    match continuity {
        crate::records::feature::surface_ops::DesignPatchContinuity::Connected => {
            Some(SurfaceContinuity::Contact)
        }
        crate::records::feature::surface_ops::DesignPatchContinuity::Tangent => {
            Some(SurfaceContinuity::Tangent)
        }
        crate::records::feature::surface_ops::DesignPatchContinuity::Curvature => {
            Some(SurfaceContinuity::Curvature)
        }
        crate::records::feature::surface_ops::DesignPatchContinuity::Unknown(_) => None,
    }
}

/// Map every known boundary-settings continuity in source order.
///
/// A missing or unknown component condition makes the complete per-boundary
/// vector unavailable. The caller can still retain the native scope.
fn surface_patch_boundary_continuities(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
) -> Result<Vec<cadmpeg_ir::features::SurfaceContinuity>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d surface-patch continuity")?;
    let mut conditions = Vec::new();
    let mut boundaries = scope.surface_patch_boundaries().iter();
    while !boundaries.as_slice().is_empty() {
        let Some(boundary) =
            ctx.next_charged(&mut boundaries, "scan F3D surface-patch continuity")?
        else {
            break;
        };
        let Some(condition) = surface_patch_boundary_continuity(boundary.continuity) else {
            return Ok(Vec::new());
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut conditions,
            condition,
            "f3d surface-patch continuity",
        )?;
    }
    storage.commit_value(conditions)
}

fn project_surface_patch(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, SurfaceBoundary,
    };

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
        return Ok(None);
    }
    let Some(stream) = ids::native_stream_charged(ctx, &scope.id, "f3d SurfacePatch scope stream")?
    else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    for group in ctx.admit_iter(
        construction_groups,
        "f3d project_surface_patch group source",
    )? {
        if !feature_stream_matches(
            ctx,
            &group.id,
            Some(stream),
            "f3d project_surface_patch group stream",
        )? || group.scope_record_index != scope.record_index
        {
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut groups, group, "f3d surface-patch groups"))?;
    }
    ctx.stable_sort_by_key(
        &mut groups[..],
        |value| {
            let group = value;
            group.scope_reference_ordinal
        },
        Ord::cmp,
        "sort f3d design feature_project 15",
    )?;

    // The single-group path form stores the group, all of its ordered edge
    // members, and the tool body. It has no per-component settings records.
    let Some(grouped_path_frame_length) = u64::try_from(scope.reference_members().len())
        .ok()
        .and_then(|count| count.checked_mul(11))
        .and_then(|count| count.checked_add(277))
    else {
        return Ok(None);
    };
    if scope.frame_length() == grouped_path_frame_length {
        let [group] = groups.as_slice() else {
            return Ok(None);
        };
        if scope.reference_members().len() < 3 {
            return Ok(None);
        }
        let Some(first_reference) = scope.reference_members().values().next() else {
            return Ok(None);
        };
        let Some(edge_references) = scope
            .reference_members()
            .values_in(1..scope.reference_members().len() - 1)
        else {
            return Ok(None);
        };
        if group.scope_reference_ordinal != 0
            || group.record_index != *first_reference
            || group.role() != DesignOperandRole::BODIES_A
            || group.members().is_empty()
            || !{
                let mut references = edge_references;
                group.members().len() == references.len()
                    && ctx.all_by(
                        group.members(),
                        |member| {
                            Ok(ctx.next_charged(
                                &mut references,
                                "f3d surface-patch edge reference",
                            )? == Some(&member.value))
                        },
                        "f3d surface-patch edge member source",
                    )?
            }
        {
            return Ok(None);
        }
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::FilledSurface {
                boundary: SurfaceBoundary::Path(resolved_surface_patch_path(
                    std::slice::from_ref(group),
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    scope,
                    SurfacePatchRecipe::Grouped,
                    ctx,
                )?),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                    cadmpeg_ir::features::SurfaceContinuity::Contact,
                ),
                merge_result: Some(false),
            },
        )));
    }

    // The reference count separates the two settings-bearing forms: the
    // fixed-path form has `3n + 1` references and the sketch-profile form has
    // three. Frame length does not, because the Design scope envelope has two
    // generations and the later one adds fourteen bytes to both forms.
    let (boundary_count, boundary_role) = if scope.reference_members().len() == 3 {
        (1, DesignOperandRole::PROFILE)
    } else {
        let Some(reference_count) = scope.reference_members().len().checked_sub(1) else {
            return Ok(None);
        };
        let boundary_count = reference_count / 3;
        if boundary_count == 0 || scope.reference_members().len() != boundary_count * 3 + 1 {
            return Ok(None);
        }
        (boundary_count, DesignOperandRole::BODIES_A)
    };
    if groups.len() != boundary_count || scope.surface_patch_boundaries().len() != boundary_count {
        return Ok(None);
    }
    let mut occupied = scratch_storage.with_storage(|| {
        ctx.alloc_filled(
            scope.reference_members().len(),
            false,
            "f3d surface-patch reference occupancy",
        )
    })?;
    let mut boundaries = groups.iter();
    while let Some(boundary) =
        ctx.next_charged(&mut boundaries, "f3d surface-patch boundary source")?
    {
        let Ok(group_ordinal) = usize::try_from(boundary.scope_reference_ordinal) else {
            return Ok(None);
        };
        let Some(member_ordinal) = group_ordinal.checked_add(1) else {
            return Ok(None);
        };
        let Some(settings_ordinal) = group_ordinal.checked_add(2) else {
            return Ok(None);
        };
        let Some(group_reference) = scope.reference_members().values().nth(group_ordinal) else {
            return Ok(None);
        };
        if settings_ordinal >= scope.reference_members().len()
            || boundary.record_index != *group_reference
            || boundary.role() != boundary_role
            || !boundary
                .members()
                .iter()
                .map(|member| member.value)
                .eq(scope
                    .reference_members()
                    .values()
                    .nth(member_ordinal)
                    .into_iter()
                    .copied())
            || occupied[group_ordinal]
            || occupied[member_ordinal]
            || occupied[settings_ordinal]
        {
            return Ok(None);
        }
        let Some(settings) =
            ctx.find_by(
                scope.surface_patch_boundaries(),
                |settings| {
                    Ok(usize::try_from(settings.scope_reference_ordinal).ok()
                        == Some(settings_ordinal))
                },
                "f3d surface-patch settings source",
            )?
        else {
            return Ok(None);
        };
        let Some(settings_reference) = scope.reference_members().values().nth(settings_ordinal)
        else {
            return Ok(None);
        };
        if settings.record_index != *settings_reference
            || settings.model_reference != boundary.record_index
        {
            return Ok(None);
        }
        occupied[group_ordinal] = true;
        occupied[member_ordinal] = true;
        occupied[settings_ordinal] = true;
    }
    let mut unoccupied = Vec::new();
    for (ordinal, occupied) in ctx
        .admit_iter(&occupied, "f3d surface-patch occupancy source")?
        .enumerate()
    {
        if !occupied {
            scratch_storage.with_storage(|| {
                ctx.push_vec(
                    &mut unoccupied,
                    ordinal,
                    "f3d surface-patch unoccupied references",
                )
            })?;
        }
    }
    let endpoint_unoccupied = unoccupied.as_slice() == [0]
        || scope
            .reference_members()
            .len()
            .checked_sub(1)
            .is_some_and(|last| unoccupied.as_slice() == [last]);
    if (scope.reference_members().len() == 3 && !unoccupied.is_empty())
        || (scope.reference_members().len() != 3 && !endpoint_unoccupied)
    {
        return Ok(None);
    }
    let boundary = if let [boundary] = groups.as_slice() {
        resolved_loft_path(
            boundary,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            ctx,
        )?
    } else {
        resolved_surface_patch_path(
            &groups,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            SurfacePatchRecipe::Direct,
            ctx,
        )?
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::FilledSurface {
            boundary: SurfaceBoundary::Path(boundary),
            support_faces: FaceSelection::Faces(Vec::new()),
            continuity: cadmpeg_ir::features::NonEmptyMembers::try_from(
                surface_patch_boundary_continuities(ctx, scope)?,
            )
            .map_or_else(
                |_| cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved(),
                cadmpeg_ir::features::FilledSurfaceContinuityState::per_boundary,
            ),
            merge_result: Some(false),
        },
    )))
}

fn project_boundary_fill(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::BoundaryFill
        || scope.reference_members().len() < 5
    {
        return Ok(None);
    }
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d project_boundary_fill scope stream"
    )?);
    let mut groups = Vec::new();
    for group in ctx.admit_iter(
        construction_groups,
        "f3d project_boundary_fill group source",
    )? {
        if !feature_stream_matches(
            ctx,
            &group.id,
            Some(stream),
            "f3d project_boundary_fill group stream",
        )? || group.scope_record_index != scope.record_index
        {
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut groups, group, "f3d BoundaryFill group"))?;
    }
    ctx.stable_sort_by_key(
        &mut groups[..],
        |value| {
            let group = value;
            group.scope_reference_ordinal
        },
        Ord::cmp,
        "sort f3d design feature_project 16",
    )?;
    let (tools, cells) = or_none!(groups.split_first());
    if tools.scope_reference_ordinal != 0
        || tools.record_index != *or_none!(scope.reference_members().values().next())
        || tools.role() != DesignOperandRole::BODIES_A
        || cells.is_empty()
    {
        return Ok(None);
    }
    let mut source = groups.iter().enumerate();
    while let Some((index, group)) =
        ctx.next_charged(&mut source, "f3d BoundaryFill member group source")?
    {
        let start = or_none!(usize::try_from(group.scope_reference_ordinal).ok());
        let end = groups
            .get(index + 1)
            .and_then(|next| usize::try_from(next.scope_reference_ordinal).ok())
            .unwrap_or(scope.reference_members().len() - 1);
        if start >= end
            || group.record_index != *or_none!(scope.reference_members().values().nth(start))
            || !{
                let mut references = or_none!(scope.reference_members().values_in(start + 1..end));
                group.members().len() == references.len()
                    && ctx.all_by(
                        group.members(),
                        |member| {
                            Ok(ctx.next_charged(
                                &mut references,
                                "f3d BoundaryFill member reference",
                            )? == Some(&member.value))
                        },
                        "f3d BoundaryFill member source",
                    )?
            }
            || (index > 0 && group.role() != DesignOperandRole::ROLE_0X5)
        {
            return Ok(None);
        }
    }
    let mut selected_cells = Vec::new();
    for cell in ctx.admit_iter(cells, "f3d BoundaryFill selected cell source")? {
        ctx.push_vec(
            &mut selected_cells,
            BodySelection::Native(ctx.copy_retained_text(&cell.id, "f3d BoundaryFill cell id")?),
            "f3d BoundaryFill cell",
        )?;
    }
    let Some(selected_cells) = selected_cells.try_into().ok() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::BoundaryFill {
            tools: BodySelection::Native(
                ctx.copy_retained_text(&tools.id, "f3d BoundaryFill tool id")?,
            ),
            cells: selected_cells,
        },
    )))
}

fn project_hole(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        holes::{HoleBottom, HoleKind},
        FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Hole
        || !matches!(parameters.len(), 3 | 5)
    {
        return Ok(None);
    }
    let parameter = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind() == source_kind)
            .map(|(_, parameter)| *parameter);
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let depth = or_none!(design_positive_length(or_none!(parameter("HoleDepth"))));
    let diameter = or_none!(design_positive_length(or_none!(parameter("HoleDiameter"))));
    let tip_angle = or_none!(design_angle(or_none!(parameter("TipAngle"))));
    if tip_angle.get() <= 0.0 || tip_angle.get() > std::f64::consts::PI {
        return Ok(None);
    }
    let counterbore = match parameters.len() {
        3 => None,
        5 => {
            let counterbore_depth =
                or_none!(design_positive_length(or_none!(parameter("CBDepth"))));
            let counterbore_diameter =
                or_none!(design_positive_length(or_none!(parameter("CBDiameter"))));
            if counterbore_depth.get() > depth.get() || counterbore_diameter.get() <= diameter.get()
            {
                return Ok(None);
            }
            Some((counterbore_diameter, counterbore_depth))
        }
        _ => return Ok(None),
    };
    // The tip angle lies in (0, pi]. Pi states a flat bottom; every smaller
    // tip angle is an interior drill-point angle.
    let drill_point_angle = cadmpeg_ir::scalar::InteriorAngle::try_from(tip_angle).ok();
    let (kind, bottom) = match (counterbore, drill_point_angle) {
        (None, None) => (HoleKind::Simple, Some(HoleBottom::Flat)),
        (None, Some(drill_point_angle)) => (HoleKind::SimpleDrilled { drill_point_angle }, None),
        (Some((diameter, depth)), None) => (
            HoleKind::Counterbore { diameter, depth },
            Some(HoleBottom::Flat),
        ),
        (Some((diameter, depth)), Some(drill_point_angle)) => (
            HoleKind::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            },
            None,
        ),
    };
    let placements = if let Some(construction) = scope.hole_construction() {
        Some(vec![cadmpeg_ir::features::holes::HolePlacement::Directed {
            position: or_none!(cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                construction.position[0].get() * 10.0,
                construction.position[1].get() * 10.0,
                construction.position[2].get() * 10.0,
            ))),
            direction: or_none!(cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                construction.direction[0].get(),
                construction.direction[1].get(),
                construction.direction[2].get(),
            ))),
        }])
    } else {
        None
    };
    let face = match resolved_direct_face_selection(ctx, scope, face_operands)? {
        Some(face) => face,
        None => {
            FaceSelection::Native(ctx.copy_retained_text(&scope.id, "f3d Hole fallback face id")?)
        }
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: Some(face),
        direction: None,
        placements,
        shape: or_none!(cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind,
                specification: None,
            },
            None,
            Some(diameter),
        )
        .ok()),

        extent: Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::from(depth),
        }),
        bottom,
        taper_angle: None,
        allow_multi_profile_faces: None,
    })))
}

fn project_replace_face(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(
        "f3d ReplaceFace accepted output",
        || -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
            use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

            if scope.kind() != crate::records::feature::scope::DesignFeatureKind::ReplaceFace
                || scope.class_tag.as_str() != "301"
                || scope.paired_class_tag.as_str() != "258"
                || scope.frame_length() != 290
                || scope.reference_members().len() != 4
            {
                return Ok(None);
            }
            let stream = or_none!(ids::native_stream_charged(
                ctx,
                &scope.id,
                "f3d project_replace_face scope stream"
            )?);
            let mut source = construction_groups.iter();
            let mut groups = || {
                ctx.find_by(
                    &mut source,
                    |group| {
                        Ok(feature_stream_matches(
                            ctx,
                            &group.id,
                            Some(stream),
                            "f3d project_replace_face group stream",
                        )? && group.scope_record_index == scope.record_index)
                    },
                    "f3d project_replace_face group source",
                )
            };
            let (Some(first), Some(second), None) = (groups()?, groups()?, groups()?) else {
                return Ok(None);
            };
            let (replacement_group, target_group) =
                if first.scope_reference_ordinal <= second.scope_reference_ordinal {
                    (first, second)
                } else {
                    (second, first)
                };
            let references = scope.reference_members().values_array::<4>();
            let references = or_none!(references).map(|value| *value);
            if replacement_group.scope_reference_ordinal != 0
                || replacement_group.record_index != references[0]
                || replacement_group.role() != DesignOperandRole::ROLE_0X9
                || !replacement_group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq(references[1..2].iter().copied())
                || target_group.scope_reference_ordinal != 2
                || target_group.record_index != references[2]
                || target_group.role() != DesignOperandRole::ROLE_0X10
                || !target_group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq(references[3..4].iter().copied())
            {
                return Ok(None);
            }
            let replacements = or_none!(resolved_body_recipe_selection(
                ctx,
                scope,
                replacement_group,
                body_recipe_operands
            )?);
            let targets = or_none!(resolved_historical_face_group(
                ctx,
                scope,
                scope.previous_history_state_id(),
                target_group,
                face_operands,
            )?);
            Ok(Some(FeatureDefinition::Operation(
                FeatureOperation::ReplaceFace {
                    operands: or_none!(cadmpeg_ir::features::ReplaceFaceOperands::new(
                        targets,
                        replacements,
                        ctx,
                    )?
                    .ok()),
                },
            )))
        },
    )?;
    match value {
        Some(value) => Ok(Some(storage.commit_value(value)?)),
        None => Ok(None),
    }
}

/// Project the source selections of a `SurfaceTrim` scope.
///
/// Fusion stores the surface target as a role-`0x04` body-recipe group and
/// the trimming path as a role-`0x21` entity-selection group. The cell table
/// that selects the cells to remove is decoded separately and bound to the
/// projected operation after the source selections have been resolved.
fn project_surface_trim(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, TrimRegion};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
        || scope.reference_members().len() != 4
    {
        return Ok(None);
    }
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d project_surface_trim scope stream"
    )?);
    let references = scope.reference_members().values_array::<4>();
    let references = or_none!(references).map(|value| *value);
    let mut source = construction_groups.iter();
    let mut groups = || {
        ctx.find_by(
            &mut source,
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d project_surface_trim group stream",
                )? && group.scope_record_index == scope.record_index)
            },
            "f3d project_surface_trim group source",
        )
    };
    let (Some(first), Some(second), None) = (groups()?, groups()?, groups()?) else {
        return Ok(None);
    };
    let (target_group, tool_group) =
        if first.scope_reference_ordinal <= second.scope_reference_ordinal {
            (first, second)
        } else {
            (second, first)
        };
    if target_group.scope_reference_ordinal != 0
        || target_group.record_index != references[0]
        || target_group.role() != DesignOperandRole::BODIES_A
        || !target_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[1..2].iter().copied())
        || tool_group.scope_reference_ordinal != 2
        || tool_group.record_index != references[2]
        || tool_group.role() != DesignOperandRole::ROLE_0X21
        || !tool_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[3..4].iter().copied())
    {
        return Ok(None);
    }
    let faces = or_none!(resolved_body_recipe_selection(
        ctx,
        scope,
        target_group,
        body_recipe_operands
    )?);
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::TrimSurface {
            faces,
            tool: PathRef::Native(
                ctx.copy_retained_text(&tool_group.id, "f3d SurfaceTrim tool group id")?,
            ),
            keep: TrimRegion::Unresolved,
        },
    )))
}

/// Bind the exact removed-cell set retained by a decoded `SurfaceTrim`.
///
/// A source trim selects cells for removal. That set is independent of the
/// canonical inside/outside shorthand and must remain explicit when it is not
/// reducible to one of those two regions.
pub(crate) fn bind_surface_trim_cell_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    operations: &[DesignSurfaceTrimOperation],
) -> Result<(), CodecError> {
    for feature in ctx.admit_iter(features, "f3d SurfaceTrim feature source")? {
        if !matches!(
            feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface {
                    keep: cadmpeg_ir::features::TrimRegion::Unresolved,
                    ..
                }
            )
        ) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = ctx.find_by(
            scopes,
            |scope| {
                ctx.equal(
                    scope.id.as_str(),
                    native_ref,
                    "f3d SurfaceTrim native scope comparison",
                )
            },
            "f3d SurfaceTrim scope source",
        )?
        else {
            continue;
        };
        let stream =
            ids::native_stream_charged(ctx, &scope.id, "f3d SurfaceTrim bound scope stream")?;
        let Some(operation) = ctx.find_by(
            operations,
            |operation| {
                Ok(operation.scope_record_index == scope.record_index
                    && feature_stream_matches(
                        ctx,
                        &operation.id,
                        stream,
                        "f3d SurfaceTrim operation stream",
                    )?)
            },
            "f3d SurfaceTrim operation source",
        )?
        else {
            continue;
        };
        let mut storage = ctx.reserve_scoped(0, "f3d SurfaceTrim accepted cell selection")?;
        let mut removed = Vec::new();
        for entry in ctx.admit_iter(operation.cell_entries(), "f3d SurfaceTrim cell source")? {
            ctx.push_scoped_vec(
                &mut storage,
                &mut removed,
                entry.ordinal,
                "f3d SurfaceTrim selected cell",
            )?;
        }
        let Some(selection) = cadmpeg_ir::features::TrimCellSelection::new(
            removed,
            u64::from(operation.trailing_value),
        ) else {
            continue;
        };
        let selection = storage.commit_value(selection)?;
        feature.evaluation.edit(|definition, _| {
            if let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface { keep, .. },
            ) = definition
            {
                *keep = cadmpeg_ir::features::TrimRegion::Cells(selection);
            }
        });
    }
    Ok(())
}

pub(super) fn project_split(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Split
        || scope.reference_members().len() < 4
    {
        return Ok(None);
    }
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d Split scope stream"
    )?);
    let mut source = construction_groups.iter();
    let mut groups = || {
        ctx.find_by(
            &mut source,
            |group| {
                Ok(
                    feature_stream_matches(ctx, &group.id, Some(stream), "f3d Split group stream")?
                        && group.scope_record_index == scope.record_index,
                )
            },
            "f3d Split group source",
        )
    };
    let (Some(first), Some(second), None) = (groups()?, groups()?, groups()?) else {
        return Ok(None);
    };
    let parsed = (|| -> Result<Option<_>, CodecError> {
        let (tool_group, targets) =
            if first.scope_reference_ordinal <= second.scope_reference_ordinal {
                (first, second)
            } else {
                (second, first)
            };
        let target_ordinal = or_none!(tool_group.members().len().checked_add(1));
        let tool_members = or_none!(scope.reference_members().values_in(1..target_ordinal));
        let target_record_index = *or_none!(scope.reference_members().values().nth(target_ordinal));
        let target_members = or_none!(scope
            .reference_members()
            .values_in(or_none!(target_ordinal.checked_add(1))..scope.reference_members().len()));
        if tool_group.scope_reference_ordinal != 0
            || tool_group.record_index != *or_none!(scope.reference_members().values().next())
            || tool_group.members().is_empty()
            || !{
                let mut references = tool_members;
                tool_group.members().len() == references.len()
                    && ctx.all_by(
                        tool_group.members(),
                        |member| {
                            Ok(
                                ctx.next_charged(&mut references, "f3d Split member reference")?
                                    == Some(&member.value),
                            )
                        },
                        "f3d Split member source",
                    )?
            }
            || or_none!(usize::try_from(targets.scope_reference_ordinal).ok()) != target_ordinal
            || targets.record_index != target_record_index
            || targets.role() != DesignOperandRole::BODIES_A
            || targets.members().is_empty()
            || !{
                let mut references = target_members;
                targets.members().len() == references.len()
                    && ctx.all_by(
                        targets.members(),
                        |member| {
                            Ok(
                                ctx.next_charged(&mut references, "f3d Split member reference")?
                                    == Some(&member.value),
                            )
                        },
                        "f3d Split member source",
                    )?
            }
        {
            return Ok(None);
        }
        Ok(Some((stream, tool_group, targets)))
    })()?;
    let Some((stream, tool_group, targets)) = parsed else {
        return Ok(None);
    };
    let tools = match tool_group.role() {
        DesignOperandRole::ROLE_0X9 => {
            let [crate::records::identity::Located {
                value: tool_record_index,
                ..
            }] = tool_group.members()
            else {
                return Ok(None);
            };
            let tool = unique_feature_match_for_decode(
                ctx,
                face_operands.iter(),
                |operand| {
                    Ok(feature_stream_matches(
                        ctx,
                        &operand.id,
                        Some(stream),
                        "compare F3D unique feature stream",
                    )? && operand.scope_record_index == scope.record_index
                        && operand.scope_reference_ordinal == 1
                        && operand.record_index() == *tool_record_index
                        && operand.recipe_kind == ConstructionRecipeKind::Face
                        && operand.recipe_program.as_slice() == [0, -1]
                        && operand.recipe_nodes.is_empty())
                },
                "visit F3D unique feature source",
            )?;
            let Some(tool) = tool else {
                return Ok(None);
            };
            let mut tools =
                if let Some(selection) = resolved_historical_face_operand(ctx, scope, tool)? {
                    selection
                } else {
                    direct_face_selection(ctx, scope, face_operands)?
                        .unwrap_or_else(|| FaceSelection::Native(String::new()))
                };
            match &mut tools {
                FaceSelection::Resolved { native, .. } | FaceSelection::Native(native) => {
                    *native = ctx.copy_retained_text(&tool.id, "f3d SplitBody face tool id")?;
                }
                FaceSelection::Historical { native, .. }
                | FaceSelection::HistoricalPartial { native, .. } => {
                    *native = or_none!(cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.copy_retained_text(&tool.id, "f3d SplitBody historical face tool id")?,
                        "validate nonblank text"
                    )?);
                }
                _ => {}
            }
            tools
        }
        DesignOperandRole::ROLE_0X21 => FaceSelection::Native(
            ctx.copy_retained_text(&tool_group.id, "f3d SplitBody path tool group id")?,
        ),
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitBody {
            targets: BodySelection::Native(
                ctx.copy_retained_text(&targets.id, "f3d SplitBody target group id")?,
            ),
            tools,
        },
    )))
}

fn project_split_face(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool};

    let reference_count = scope.reference_members().len();
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || reference_count < 4
    {
        return Ok(None);
    }
    let reference_tail_length = or_none!(11_u64.checked_mul(or_none!(u64::try_from(or_none!(
        reference_count.checked_sub(1)
    ))
    .ok())));
    let frame_base = or_none!(scope.frame_length().checked_sub(reference_tail_length));
    let compact = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("418", "266") | ("277", "258")
    );
    if !(matches!(frame_base, 290 | 291) || compact && frame_base == 286) {
        return Ok(None);
    }
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d project_split_face scope stream"
    )?);
    let mut source = construction_groups.iter();
    let mut groups = || {
        ctx.find_by(
            &mut source,
            |group| {
                Ok(feature_stream_matches(
                    ctx,
                    &group.id,
                    Some(stream),
                    "f3d project_split_face group stream",
                )? && group.scope_record_index == scope.record_index)
            },
            "f3d project_split_face group source",
        )
    };
    let (Some(first), Some(second), None) = (groups()?, groups()?, groups()?) else {
        return Ok(None);
    };
    let (tool, targets) = if first.scope_reference_ordinal <= second.scope_reference_ordinal {
        (first, second)
    } else {
        (second, first)
    };
    let target_ordinal = or_none!(tool.members().len().checked_add(1));
    if tool.scope_reference_ordinal != 0
        || tool.record_index != *or_none!(scope.reference_members().values().next())
        || tool.role() != DesignOperandRole::ROLE_0X21
        || tool.members().is_empty()
        || !{
            let mut references = or_none!(scope.reference_members().values_in(1..target_ordinal));
            tool.members().len() == references.len()
                && ctx.all_by(
                    tool.members(),
                    |member| {
                        Ok(
                            ctx.next_charged(&mut references, "f3d SplitFace tool reference")?
                                == Some(&member.value),
                        )
                    },
                    "f3d SplitFace tool member source",
                )?
        }
        || or_none!(usize::try_from(targets.scope_reference_ordinal).ok()) != target_ordinal
        || targets.record_index != *or_none!(scope.reference_members().values().nth(target_ordinal))
        || targets.role() != DesignOperandRole::ROLE_0X10
        || targets.members().is_empty()
        || !{
            let mut references = scope.reference_members().values().skip(target_ordinal + 1);
            targets.members().len() == references.len()
                && ctx.all_by(
                    targets.members(),
                    |member| {
                        Ok(
                            ctx.next_charged(&mut references, "f3d SplitFace target reference")?
                                == Some(&member.value),
                        )
                    },
                    "f3d SplitFace target member source",
                )?
        }
    {
        return Ok(None);
    }
    let target_selection = project_face_selection(ctx, scope, targets, face_operands, histories)?;
    let mut plane_storage = ctx.reserve_scoped(0, "F3D SplitFace plane rows")?;
    let tool = if let Some(path) =
        resolved_split_face_path(ctx, scope, tool, entity_selection_operands, histories)?
    {
        SplitFaceTool::Path(path)
    } else if let Some(planes) = plane_storage.with_storage(|| {
        selected_work_planes(ctx, scope, tool, entity_selection_operands, scopes)
    })? {
        if let [plane] = planes.as_slice() {
            SplitFaceTool::Plane {
                plane: crate::design::identity::neutral_feature_id(ctx, plane)?,
            }
        } else {
            let mut selected = Vec::new();
            for plane in ctx.admit_iter(planes, "f3d SplitFace selected plane source")? {
                let id = crate::design::identity::neutral_feature_id(ctx, plane)?;
                ctx.push_vec(&mut selected, id, "f3d SplitFace tool plane")?;
            }
            {
                SplitFaceTool::Planes {
                    planes: or_none!(selected.try_into().ok()),
                }
            }
        }
    } else {
        SplitFaceTool::Path(PathRef::Native(
            ctx.copy_retained_text(&tool.id, "f3d SplitFace path tool group id")?,
        ))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitFace {
            targets: target_selection,
            tool,
        },
    )))
}

fn project_delete_face(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let reference_count = scope.reference_members().len();
    let reference_bytes =
        or_none!(11_u64.checked_mul(or_none!(u64::try_from(reference_count).ok())));
    let base_frame_length = or_none!(scope.frame_length().checked_sub(reference_bytes));
    let base_kind_offset = or_none!(scope
        .kind_offset()
        .checked_sub(scope.byte_offset())
        .and_then(|offset| offset.checked_sub(reference_bytes)));
    let heal = match scope.kind_name() {
        "DeleteFace" => match (base_frame_length, base_kind_offset) {
            (236, 139) | (241, 143) => true,
            (232, 135)
                if matches!(
                    (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
                    ("264", "262") | ("383", "263")
                ) =>
            {
                true
            }
            _ => return Ok(None),
        },
        "SurfaceDeleteFace" => {
            let common_layout = matches!(
                (base_frame_length, base_kind_offset),
                (250, 140) | (251, 139)
            );
            let class_layout = matches!(
                (
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                    base_frame_length,
                    base_kind_offset,
                ),
                ("287", "270", 245, 135)
                    | ("287", "270", 256, 146)
                    | ("327" | "545", "257", 250, 139)
                    | ("414", "263", 250, 140)
                    | ("497", "259", 257, 146)
                    | ("545", "257", 246, 135)
                    | ("545", "257", 257, 146)
            );
            if common_layout || class_layout {
                false
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };
    if reference_count < 2 {
        return Ok(None);
    }
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "resolve F3D unique feature stream"
    )?);
    let Some(group) = unique_feature_match_for_decode(
        ctx,
        construction_groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && group.scope_record_index == scope.record_index)
        },
        "visit F3D unique feature source",
    )?
    else {
        return Ok(None);
    };
    if group.scope_reference_ordinal != 0
        || group.record_index != *or_none!(scope.reference_members().values().next())
        || group.role() != DesignOperandRole::ROLE_0X10
        || !{
            let mut references = scope.reference_members().values().skip(1);
            group.members().len() == references.len()
                && ctx.all_by(
                    group.members(),
                    |member| {
                        Ok(
                            ctx.next_charged(&mut references, "f3d DeleteFace member reference")?
                                == Some(&member.value),
                        )
                    },
                    "f3d DeleteFace member source",
                )?
        }
    {
        return Ok(None);
    }
    let faces = resolved_historical_face_group(
        ctx,
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    )?
    .map_or_else(
        || resolved_face_group(ctx, group, face_operands),
        |face| Ok(Some(face)),
    )?;
    let faces = match faces {
        Some(faces) => faces,
        None => FaceSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d DeleteFace fallback group id")?,
        ),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteFace { faces, heal },
    )))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
