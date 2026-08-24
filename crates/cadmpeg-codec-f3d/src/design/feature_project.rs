// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::items_after_test_module)]
//! Project parameter-design features and dispatch per feature family.

use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::design::decode::operands::entity_selection_matches_curve;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::design::dimensions::expression_identifiers;
use crate::design::edge_resolve::{
    feature_input_topology_id, project_fixed_fillet_with_corners, resolved_edge_flange_group,
    resolved_edge_group, resolved_edge_treatment_group_with_corners,
    resolved_surface_patch_edge_group,
};
use crate::design::face_resolve::{
    design_angle, extrude_omits_zero_side_one_offset, extrude_profile_group_roots,
    resolved_body_recipe_selection, resolved_body_recipe_shape, resolved_direct_face_selection,
    resolved_extrude_profile_face_group, resolved_face_group, resolved_historical_face_group,
    resolved_historical_face_operand,
    resolved_historical_split_face_target_group_with_updated_faces,
    resolved_loft_edge_profile_group, resolved_profile_face_group, valid_chamfer_spec,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{
    self, native_stream, neutral_feature_id, neutral_parameter_id, neutral_sketch_id,
    neutral_spatial_sketch_id,
};
use crate::layout::coil_long_scope_fixed_prologue as coil_long;
use crate::layout::{
    form_class_325_cage_entry as form_325_entry, form_class_325_cage_table as form_325,
    form_class_328_cage_group as form_328_group,
    form_class_328_metadata_group as form_328_metadata,
    form_class_328_reference_entry as form_328_entry, form_class_328_scope,
    form_class_350_member_owner_tail as form_350_tail, form_compact_one_cage_list as form_cage,
    form_legacy_one_cage_owner as legacy_form_cage, form_serializer_frame_132 as form_serializer,
};
use crate::records::{
    ConstructionRecipeKind, DesignBodyBinding, DesignBodyRecipeOperand, DesignCoilExtent,
    DesignCoilSection, DesignCoilSectionPlacement, DesignConstructionOperandGroup,
    DesignDirectFaceOperation, DesignEdgeIdentityOperand, DesignEdgeOperand,
    DesignEdgeTreatmentVertexOperand, DesignExtrudeExtent, DesignExtrudeFaceRole,
    DesignExtrudeOperandRole, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
    DesignFaceOperand, DesignFeatureTimeline, DesignFilletRadiusGroup, DesignFilletRadiusLaw,
    DesignFixedExtrudeDistance, DesignLoftLegacyBodyCarrier, DesignParameter, DesignParameterKind,
    DesignParameterOwner, DesignParameterScope, DesignPathFeatureConstruction,
    DesignSketchPlacement, DesignSolidPrimitive, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport, SketchCurveGeometry, SketchCurveIdentity,
};
use cadmpeg_core::decode::{bounded_len, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

/// Design record slices projected together into the neutral construction
/// history: the parameter, owner, and scope tables plus the construction
/// operand, fillet-radius, edge, edge-identity, face, and whole-body recipe
/// operand records and the sketch placements and body bindings each feature
/// scope resolves against.
pub struct ProjectInputs<'a> {
    pub(crate) native: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) timelines: &'a [DesignFeatureTimeline],
    pub(crate) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(crate) fillet_radius_groups: &'a [DesignFilletRadiusGroup],
    pub(crate) edge_operands: &'a [DesignEdgeOperand],
    pub(crate) edge_identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) edge_treatment_vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) entity_selection_operands: &'a [crate::records::DesignEntitySelectionOperand],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) face_operands: &'a [DesignFaceOperand],
    pub(crate) body_recipe_operands: &'a [DesignBodyRecipeOperand],
    pub(crate) legacy_loft_body_carriers: &'a [DesignLoftLegacyBodyCarrier],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    pub(crate) component_naming_spaces: &'a [crate::records::DesignComponentNamingSpace],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
}

/// Authored construction ordinal of every parameter scope represented by a
/// neutral top-level feature. All input scopes must share one Design stream.
pub(crate) fn authored_scope_ordinals<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let Some(first_scope) = scopes.first() else {
        return Ok(HashMap::new());
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    if scopes
        .iter()
        .any(|scope| native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM) != stream)
    {
        return Err(CodecError::NotImplemented(
            "independent Design scope streams have no shared authored timeline order".into(),
        ));
    }
    authored_scope_ordinals_for_stream(&scopes.iter().collect::<Vec<_>>(), timelines)
}

/// Authored scope ordinals evaluated independently for every Design stream.
pub(crate) fn authored_scope_ordinals_per_stream<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut streams = HashMap::<&str, Vec<&DesignParameterScope>>::new();
    for scope in scopes {
        streams
            .entry(native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM))
            .or_default()
            .push(scope);
    }
    let mut out = HashMap::with_capacity(scopes.len());
    for stream_scopes in streams.into_values() {
        for (key, ordinal) in authored_scope_ordinals_for_stream(&stream_scopes, timelines)? {
            if out.insert(key, ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
    }
    Ok(out)
}

fn authored_scope_ordinals_for_stream<'a>(
    scopes: &[&'a DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut out = HashMap::with_capacity(scopes.len());
    let Some(first_scope) = scopes.first().copied() else {
        return Ok(out);
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let mut scopes_by_record = HashMap::<u32, &DesignParameterScope>::new();
    for scope in scopes {
        if scopes_by_record
            .insert(scope.record_index, *scope)
            .is_some()
        {
            return Err(CodecError::Malformed(
                "Design scope record identity is not unique".into(),
            ));
        }
    }
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index)
        else {
            continue;
        };
        let Some(target) = scopes_by_record.get(&target_record_index) else {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has no JointOrigin target".into(),
            ));
        };
        if scope.kind != "Assemble"
            || target.kind != "JointOrigin"
            || target.joint_origin_transform.is_none()
        {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has an invalid JointOrigin target".into(),
            ));
        }
    }

    let mut stream_timelines = timelines
        .iter()
        .filter(|timeline| native_stream(&timeline.id).unwrap_or(ids::DEFAULT_STREAM) == stream)
        .collect::<Vec<_>>();
    stream_timelines.sort_by_key(|timeline| timeline.source_ordinal);
    if stream_timelines.is_empty() {
        let first_family = design_feature_family(&first_scope.kind);
        let homogeneous = scopes.iter().all(|scope| {
            first_family.map_or_else(
                || scope.kind == first_scope.kind,
                |family| design_feature_family(&scope.kind) == Some(family),
            )
        });
        let mut ordered = scopes.to_vec();
        ordered.sort_by_key(|scope| scope.feature_ordinal);
        let complete_ordinals = ordered.iter().enumerate().all(|(ordinal, scope)| {
            u32::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(1))
                == Some(scope.feature_ordinal)
        });
        if !homogeneous || !complete_ordinals {
            return Err(CodecError::NotImplemented(
                "Design scopes have no complete authored timeline order".into(),
            ));
        }
        for (ordinal, scope) in ordered.into_iter().enumerate() {
            let ordinal = u64::try_from(ordinal)
                .map_err(|_| CodecError::Malformed("Design feature ordinal exceeds u64".into()))?;
            if out
                .insert(
                    (
                        native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM),
                        scope.record_index,
                    ),
                    ordinal,
                )
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
        return Ok(out);
    }

    if !stream_timelines
        .iter()
        .enumerate()
        .all(|(ordinal, timeline)| u32::try_from(ordinal).ok() == Some(timeline.source_ordinal))
    {
        return Err(CodecError::Malformed(
            "Design timeline-record ordinals are not contiguous".into(),
        ));
    }
    if stream_timelines
        .iter()
        .filter(|timeline| !timeline.item_record_indices.is_empty())
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "multiple nonempty Design timelines have no shared authored order".into(),
        ));
    }
    let mut item_ordinals = HashMap::<u64, u64>::new();
    let mut next_ordinal = 0_u64;
    for timeline in stream_timelines {
        for item in &timeline.item_record_indices {
            if *item == 0 || item_ordinals.insert(*item, next_ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design timeline item identity is not unique".into(),
                ));
            }
            next_ordinal = next_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("Design feature ordinal exceeds u64".into())
            })?;
        }
    }
    for scope in scopes {
        if let Some(ordinal) = item_ordinals.get(&u64::from(scope.record_index)).copied() {
            out.insert((stream, scope.record_index), ordinal);
        }
    }
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index)
        else {
            continue;
        };
        let target = scopes_by_record[&target_record_index];
        let source_key = (stream, scope.record_index);
        let Some(source_ordinal) = out.remove(&source_key) else {
            continue;
        };
        let target_key = (stream, target.record_index);
        if item_ordinals.contains_key(&u64::from(target.record_index)) {
            continue;
        }
        if out.insert(target_key, source_ordinal).is_some() {
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
enum ComponentHistoryNamespace {
    Aggregate,
    Component(u64),
}

/// History-state index qualified by component-local Design and ASM history.
pub(crate) struct ScopeHistoryGraph<'a> {
    histories_present: bool,
    bound_histories: HashMap<String, String>,
    component_namespaces: HashMap<String, ComponentHistoryNamespace>,
    scopes_by_state:
        HashMap<(String, ComponentHistoryNamespace, String, i64), Vec<&'a DesignParameterScope>>,
}

impl<'a> ScopeHistoryGraph<'a> {
    pub(crate) fn new(
        scopes: &'a [DesignParameterScope],
        body_bindings: &[DesignBodyBinding],
        body_recipe_operands: &[DesignBodyRecipeOperand],
        component_naming_spaces: &[crate::records::DesignComponentNamingSpace],
        histories: &[crate::history_records::AsmHistory],
    ) -> Self {
        let bound_histories = crate::history::bind_scope_histories(
            scopes,
            body_bindings,
            body_recipe_operands,
            histories,
        );
        let histories_present = !histories.is_empty();
        let component_namespaces = scopes
            .iter()
            .filter_map(|scope| {
                Self::component_namespace(scope, component_naming_spaces)
                    .map(|namespace| (scope.id.clone(), namespace))
            })
            .collect::<HashMap<_, _>>();
        let mut scopes_by_state = HashMap::new();
        for scope in scopes {
            let (Some(stream), Some(state_id)) = (native_stream(&scope.id), scope.history_state_id)
            else {
                continue;
            };
            let history_id = if histories_present {
                let Some(history_id) = bound_histories.get(&scope.id) else {
                    continue;
                };
                history_id.clone()
            } else {
                String::new()
            };
            let Some(component_namespace) = component_namespaces.get(&scope.id) else {
                continue;
            };
            scopes_by_state
                .entry((
                    stream.to_owned(),
                    *component_namespace,
                    history_id,
                    state_id,
                ))
                .or_insert_with(Vec::new)
                .push(scope);
        }
        Self {
            histories_present,
            bound_histories,
            component_namespaces,
            scopes_by_state,
        }
    }

    fn component_namespace(
        scope: &DesignParameterScope,
        component_naming_spaces: &[crate::records::DesignComponentNamingSpace],
    ) -> Option<ComponentHistoryNamespace> {
        let stream = native_stream(&scope.id)?;
        let mut stream_spaces = component_naming_spaces
            .iter()
            .filter(|space| native_stream(&space.id) == Some(stream))
            .peekable();
        if stream_spaces.peek().is_none() {
            return Some(ComponentHistoryNamespace::Aggregate);
        }
        stream_spaces
            .filter(|space| space.component_record_index <= u64::from(scope.record_index))
            .max_by_key(|space| space.component_record_index)
            .map(|space| ComponentHistoryNamespace::Component(space.component_record_index))
    }

    fn history_id(&self, scope: &DesignParameterScope) -> Option<&str> {
        if self.histories_present {
            self.bound_histories.get(&scope.id).map(String::as_str)
        } else {
            Some("")
        }
    }

    fn state_key(
        &self,
        scope: &DesignParameterScope,
        state_id: i64,
    ) -> Option<(String, ComponentHistoryNamespace, String, i64)> {
        Some((
            native_stream(&scope.id)?.to_owned(),
            *self.component_namespaces.get(&scope.id)?,
            self.history_id(scope)?.to_owned(),
            state_id,
        ))
    }

    /// Follow `scope.previous_history_state_id` until a scope accepted by
    /// `projected` is reached. Internal scopes preserve state continuity but
    /// are not themselves authored top-level features.
    pub(crate) fn predecessor<F>(
        &self,
        scope: &DesignParameterScope,
        projected: F,
    ) -> Result<ScopeHistoryPredecessor<'a>, CodecError>
    where
        F: Fn(&DesignParameterScope) -> bool,
    {
        let Some(mut state_id) = scope.previous_history_state_id else {
            return Ok(ScopeHistoryPredecessor::None);
        };
        let Some(stream) = native_stream(&scope.id) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let Some(history_id) = self.history_id(scope) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let mut visited = HashSet::new();
        loop {
            let Some(component_namespace) = self.component_namespaces.get(&scope.id) else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            let Some(candidates) = self.scopes_by_state.get(&(
                stream.to_owned(),
                *component_namespace,
                history_id.to_owned(),
                state_id,
            )) else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            let [candidate] = candidates.as_slice() else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            if candidate.id == scope.id {
                if visited.is_empty() {
                    return Ok(ScopeHistoryPredecessor::None);
                }
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            if projected(candidate) {
                return Ok(ScopeHistoryPredecessor::Scope(candidate));
            }
            if !visited.insert(candidate.id.as_str()) {
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            let Some(previous_state_id) = candidate.previous_history_state_id else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            if candidate.history_state_id == Some(previous_state_id) {
                return Ok(ScopeHistoryPredecessor::None);
            }
            state_id = previous_state_id;
        }
    }
}

fn ensure_feature_dependencies_precede(
    features: &[cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    let ordinals = features
        .iter()
        .map(|feature| (feature.id.clone(), feature.ordinal))
        .collect::<HashMap<_, _>>();
    if ordinals.len() != features.len() {
        return Err(CodecError::Malformed(
            "projected Design feature identity is not unique".into(),
        ));
    }
    let mut unique_ordinals = HashSet::with_capacity(features.len());
    for feature in features {
        if !unique_ordinals.insert(feature.ordinal) {
            return Err(CodecError::Malformed(
                "projected Design feature ordinal is not unique".into(),
            ));
        }
        if let Some((dependency, dependency_ordinal)) =
            feature.dependencies.iter().find_map(|dependency| {
                ordinals
                    .get(dependency)
                    .filter(|ordinal| **ordinal >= feature.ordinal)
                    .map(|ordinal| (dependency, ordinal))
            })
        {
            return Err(CodecError::Malformed(
                format!(
                    "Design feature dependency does not precede its authored timeline position: {dependency} at ordinal {dependency_ordinal} -> {} at ordinal {}",
                    feature.id, feature.ordinal,
                ),
            ));
        }
    }
    Ok(())
}

/// Project parameter scopes and their document- or scope-owned parameters into
/// the neutral construction history.
// Faithful reduced-arg entry point over the same slices as `ProjectInputs`;
// its many test callers pass positional slices, so it defaults the fixed
// edge-identity and body-binding tables and forwards through the bundle.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub fn project_parameter_design(
    native: &[DesignParameter],
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    fillet_radius_groups: &[DesignFilletRadiusGroup],
    edge_operands: &[DesignEdgeOperand],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
) -> (
    Vec<cadmpeg_ir::features::Feature>,
    Vec<cadmpeg_ir::features::DesignParameter>,
) {
    let mut timelines = Vec::<DesignFeatureTimeline>::new();
    for scope in scopes {
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let timeline = timelines
            .iter_mut()
            .find(|timeline| native_stream(&timeline.id) == Some(stream));
        if let Some(timeline) = timeline {
            timeline
                .item_record_indices
                .push(u64::from(scope.record_index));
            timeline.item_record_index_offsets.push(0);
        } else {
            timelines.push(DesignFeatureTimeline {
                id: ids::native_design_feature_timeline_id_in_stream(stream, 0),
                byte_offset: 0,
                class_tag: "256".into(),
                record_index: 1,
                source_ordinal: 0,
                frame_length: 0,
                context_record_index: 1,
                context_record_index_offset: 0,
                item_count_offset: 0,
                item_record_indices: vec![u64::from(scope.record_index)],
                item_record_index_offsets: vec![0],
            });
        }
    }
    project_parameter_design_with_edge_identities(&ProjectInputs {
        native,
        owners,
        scopes,
        timelines: &timelines,
        construction_groups,
        fillet_radius_groups,
        edge_operands,
        edge_identity_operands: &[],
        edge_treatment_vertex_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands,
        body_recipe_operands: &[],
        legacy_loft_body_carriers: &[],
        placements,
        body_bindings: &[],
        component_naming_spaces: &[],
        histories: &[],
    })
    .expect("test projection has a synthetic exact timeline")
}

/// Project Design parameters and feature scopes, including fixed edge identities.
pub fn project_parameter_design_with_edge_identities(
    inputs: &ProjectInputs<'_>,
) -> Result<
    (
        Vec<cadmpeg_ir::features::Feature>,
        Vec<cadmpeg_ir::features::DesignParameter>,
    ),
    CodecError,
> {
    use cadmpeg_ir::features::{
        Angle, DesignParameter as NeutralParameter, DimensionDisplay, Feature, FeatureDefinition,
        Length, ParameterId, ParameterValue, PatternForm, PatternKind, PrimitiveSolid,
    };
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

    let source_ordinals = authored_scope_ordinals(scopes, timelines)?;

    let scope_ids = scopes
        .iter()
        .filter_map(|scope| {
            let stream = native_stream(&scope.id)?;
            source_ordinals
                .contains_key(&(stream, scope.record_index))
                .then(|| ((stream, scope.record_index), neutral_feature_id(scope)))
        })
        .collect::<HashMap<_, _>>();
    let owners_by_index = owners
        .iter()
        .filter_map(|owner| Some(((native_stream(&owner.id)?, owner.record_index), owner)))
        .collect::<HashMap<_, _>>();
    let native_scope_properties = |scope: &DesignParameterScope, native_scope: &str| {
        scope_properties(scope, native_scope, placements)
    };
    let mut features = scopes
        .iter()
        .filter(|scope| {
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            source_ordinals.contains_key(&(stream, scope.record_index))
        })
        .map(|scope| {
            let native_scope = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            let parameters = owners
                .iter()
                .filter(|owner| {
                    native_stream(&owner.id) == Some(native_scope)
                        && owner.scope_record_index == scope.record_index
                })
                .filter_map(|owner| {
                    native
                        .iter()
                        .find(|parameter| {
                            native_stream(&parameter.id) == Some(native_scope)
                                && parameter.record_index == owner.parameter_record_index
                        })
                        .map(|parameter| (owner.local_ordinal, parameter))
                })
                .collect::<Vec<_>>();
            let family = design_feature_family(&scope.kind);
            let definition = match family {
                Some(DesignFeatureFamily::Sketch) => FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Unresolved,
                    sketch: None,
                },
                Some(DesignFeatureFamily::Assemble) => scope
                    .assembly_alignment
                    .as_ref()
                    .filter(|alignment| {
                        alignment.operand_frames.is_some()
                            && ((alignment.legacy_operand_carriers.is_some()
                                && alignment.operand_paths.is_none()
                                && alignment.axial_operand_targets.is_none())
                                || (alignment.legacy_operand_carriers.is_none()
                                    && (alignment.operand_paths.is_some()
                                        ^ alignment.axial_operand_targets.is_some())))
                    })
                    .map_or_else(
                        || FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        },
                        |_| FeatureDefinition::AssemblyJoint {
                            joint: crate::ids::neutral_assembly_joint_id(scope),
                        },
                    ),
                Some(DesignFeatureFamily::Extrude) => project_extrude(
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    placements,
                    body_recipe_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: parameters
                        .iter()
                        .map(|(_, parameter)| {
                            (parameter.name.clone(), parameter.expression.clone())
                        })
                        .collect(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Fillet) => {
                    project_fillet_arm(inputs, scope, parameters.as_slice(), native_scope)
                }
                Some(DesignFeatureFamily::Chamfer) => parameters
                    .is_empty()
                    .then(|| {
                        project_fixed_chamfer(
                            scope,
                            construction_groups,
                            edge_operands,
                            edge_identity_operands,
                            edge_treatment_vertex_operands,
                            histories,
                        )
                    })
                    .flatten()
                    .or_else(|| {
                        project_chamfer(
                            scope,
                            &parameters,
                            construction_groups,
                            edge_operands,
                            edge_identity_operands,
                            edge_treatment_vertex_operands,
                            histories,
                        )
                    })
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter)| {
                                (parameter.name.clone(), parameter.expression.clone())
                            })
                            .collect(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::Combine) => project_combine(scope, native_scope)
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: BTreeMap::new(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::Draft) => project_draft(
                    scope,
                    scopes,
                    construction_groups,
                    entity_selection_operands,
                    face_operands,
                    histories,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::ReplaceFace) => project_replace_face(
                    scope,
                    construction_groups,
                    face_operands,
                    body_recipe_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: parameters
                        .iter()
                        .map(|(_, parameter)| {
                            (parameter.name.clone(), parameter.expression.clone())
                        })
                        .collect(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Revolve) => project_fixed_revolve_with_entities(
                    scope,
                    construction_groups,
                    edge_operands,
                    entity_selection_operands,
                    face_operands,
                    placements,
                    curve_identities,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Loft) => project_fixed_loft(
                    scope,
                    construction_groups,
                    legacy_loft_body_carriers,
                    edge_operands,
                    edge_identity_operands,
                    face_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Sweep) => project_fixed_sweep(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    entity_selection_operands,
                    face_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Pipe) => project_fixed_pipe(
                    scope,
                    &parameters,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: parameters
                        .iter()
                        .map(|(_, parameter)| {
                            (parameter.name.clone(), parameter.expression.clone())
                        })
                        .collect(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::SurfacePatch) => project_surface_patch(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::SurfaceExtend) => {
                    scope.surface_extend_operation.as_ref().map_or_else(
                        || FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        },
                        |operation| {
                            use crate::records::DesignSurfaceExtendMethod;
                            use cadmpeg_ir::features::{FaceSelection, SurfaceExtension};

                            let method = match operation.method {
                                DesignSurfaceExtendMethod::Natural => SurfaceExtension::Natural,
                                DesignSurfaceExtendMethod::Tangent => SurfaceExtension::Linear,
                                DesignSurfaceExtendMethod::Perpendicular => {
                                    SurfaceExtension::Perpendicular
                                }
                            };
                            FeatureDefinition::ExtendSurface {
                                faces: FaceSelection::Native(format!(
                                    "{native_scope}:design-record#{}",
                                    operation.boundary_record_index
                                )),
                                distance: Some(Length(operation.distance * 10.0)),
                                method,
                            }
                        },
                    )
                }
                Some(DesignFeatureFamily::SurfaceOffset) => scope
                    .surface_offset_operation
                    .as_ref()
                    .and_then(|operation| {
                        project_surface_offset(scope, operation, construction_groups, face_operands)
                    })
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: BTreeMap::new(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::SurfaceRuled) => project_ruled_surface(
                    scope,
                    owners,
                    native,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: parameters
                        .iter()
                        .map(|(_, parameter)| {
                            (parameter.name.clone(), parameter.expression.clone())
                        })
                        .collect(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::BoundaryFill) => {
                    project_boundary_fill(scope, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::Hole) => project_hole(scope, &parameters, face_operands)
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter)| {
                                (parameter.name.clone(), parameter.expression.clone())
                            })
                            .collect(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::Split) => {
                    project_split(scope, construction_groups, face_operands).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::CircularPattern) => {
                    project_circular_pattern(scope, construction_groups, face_operands)
                        .unwrap_or_else(|| FeatureDefinition::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::Unresolved {
                                form: Some(PatternForm::Circular),
                            },
                        })
                }
                Some(DesignFeatureFamily::RectangularPattern) => {
                    project_rectangular_pattern_scalars(scope, construction_groups, face_operands)
                        .unwrap_or_else(|| FeatureDefinition::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::Unresolved {
                                form: Some(PatternForm::Linear),
                            },
                        })
                }
                Some(DesignFeatureFamily::Mirror) => {
                    project_mirror(scope, construction_groups, face_operands, scopes)
                        .unwrap_or_else(|| FeatureDefinition::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::Unresolved {
                                form: Some(PatternForm::Mirror),
                            },
                        })
                }
                Some(DesignFeatureFamily::OffsetFaces) => {
                    project_offset_faces(scope, &parameters, face_operands, construction_groups)
                        .unwrap_or_else(|| FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        })
                }
                Some(DesignFeatureFamily::Move) => project_move(scope, construction_groups)
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter)| {
                                (parameter.name.clone(), parameter.expression.clone())
                            })
                            .collect(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::Shell) => {
                    project_shell(scope, face_operands, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::Thicken) => {
                    project_thicken(scope, face_operands, construction_groups).unwrap_or_else(
                        || FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        },
                    )
                }
                Some(DesignFeatureFamily::Coil) => {
                    project_coil(scope, &parameters, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::Scale) => scope.scale_operation.as_ref().map_or_else(
                    || FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: BTreeMap::new(),
                        properties: native_scope_properties(scope, native_scope),
                    },
                    |operation| {
                        let body_group = construction_groups.iter().find(|group| {
                            native_stream(&group.id) == Some(native_scope)
                                && group.scope_record_index == scope.record_index
                                && group.record_index == operation.body_group_record_index
                        });
                        FeatureDefinition::Scale {
                            bodies: body_group.map_or(
                                cadmpeg_ir::features::BodySelection::Unresolved,
                                |group| {
                                    cadmpeg_ir::features::BodySelection::Native(group.id.clone())
                                },
                            ),
                            center: Some(operation.center_position.map_or_else(
                                || {
                                    cadmpeg_ir::features::ScaleCenter::Native(format!(
                                        "{native_scope}:design-record#{}",
                                        operation.center_record_index
                                    ))
                                },
                                |position| {
                                    cadmpeg_ir::features::ScaleCenter::Point(Point3::new(
                                        position[0] * 10.0,
                                        position[1] * 10.0,
                                        position[2] * 10.0,
                                    ))
                                },
                            )),
                            factors: cadmpeg_ir::features::ScaleFactors {
                                uniform: Some(operation.uniform_factor),
                                x: None,
                                y: None,
                                z: None,
                            },
                        }
                    },
                ),
                Some(DesignFeatureFamily::Thread) => {
                    scope.thread_construction.as_ref().map_or_else(
                        || FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        },
                        |construction| {
                            let face = project_thread_face_selection(
                                scope,
                                &construction.face_group_record_indices,
                                construction_groups,
                                face_operands,
                            );
                            let extent = parameters
                                .iter()
                                .find(|(ordinal, _)| *ordinal == 1)
                                .and_then(|(_, parameter)| design_length(parameter))
                                .filter(|length| length.0 > 0.0)
                                .map(|length| cadmpeg_ir::features::CosmeticThreadExtent::Blind {
                                    length,
                                });
                            FeatureDefinition::CosmeticThread {
                                face,
                                diameter: Some(Length(construction.nominal_size)),
                                extent,
                            }
                        },
                    )
                }
                Some(DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    project_edge_flange(scope, inputs).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::SheetMetalHem) => project_hem(scope, inputs)
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter)| {
                                (parameter.name.clone(), parameter.expression.clone())
                            })
                            .collect(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                None => {
                    if let Some(primitive) = scope.solid_primitive.as_ref() {
                        let operation = |operation| match operation {
                            DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
                            DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
                            DesignExtrudeOperation::Intersect => {
                                cadmpeg_ir::features::BooleanOp::Intersect
                            }
                            DesignExtrudeOperation::NewBody => {
                                cadmpeg_ir::features::BooleanOp::NewBody
                            }
                        };
                        match primitive {
                            DesignSolidPrimitive::Box {
                                length,
                                width,
                                height,
                                offset_x,
                                offset_y,
                                operation: result,
                                ..
                            } => {
                                let mut placement = cadmpeg_ir::transform::Transform::identity();
                                placement.rows[0][3] = *offset_x * 10.0;
                                placement.rows[1][3] = *offset_y * 10.0;
                                FeatureDefinition::Block {
                                    dimensions: Some([
                                        Length(*length * 10.0),
                                        Length(*width * 10.0),
                                        Length(*height * 10.0),
                                    ]),
                                    placement: Some(placement),
                                    op: operation(*result),
                                }
                            }
                            DesignSolidPrimitive::Cylinder {
                                height,
                                diameter,
                                operation: result,
                                ..
                            } => FeatureDefinition::Primitive {
                                solid: PrimitiveSolid::Cylinder {
                                    radius: Length(*diameter * 5.0),
                                    height: Length(*height * 10.0),
                                    angle: Angle(std::f64::consts::TAU),
                                },
                                op: operation(*result),
                            },
                            DesignSolidPrimitive::Sphere {
                                transform,
                                diameter,
                                operation: result,
                                ..
                            } => FeatureDefinition::Sphere {
                                center: Point3::new(
                                    transform[0][3] * 10.0,
                                    transform[1][3] * 10.0,
                                    transform[2][3] * 10.0,
                                ),
                                radius: Length(*diameter * 5.0),
                                op: operation(*result),
                            },
                            DesignSolidPrimitive::Torus {
                                transform,
                                major_diameter,
                                minor_diameter,
                                operation: result,
                                ..
                            } => FeatureDefinition::Torus {
                                center: Point3::new(
                                    transform[0][3] * 10.0,
                                    transform[1][3] * 10.0,
                                    transform[2][3] * 10.0,
                                ),
                                axis: Vector3::new(
                                    transform[0][2],
                                    transform[1][2],
                                    transform[2][2],
                                ),
                                major_radius: Length(*major_diameter * 5.0),
                                minor_radius: Length(*minor_diameter * 5.0),
                                op: operation(*result),
                            },
                        }
                    } else if scope.kind == "JointOrigin" {
                        scope.joint_origin_transform.map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: parameters
                                    .iter()
                                    .map(|(_, parameter)| {
                                        (parameter.name.clone(), parameter.expression.clone())
                                    })
                                    .collect(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |transform| FeatureDefinition::DatumCoordinateSystem {
                                origin: Point3::new(
                                    transform[0][3] * 10.0,
                                    transform[1][3] * 10.0,
                                    transform[2][3] * 10.0,
                                ),
                                x_axis: Vector3::new(
                                    transform[0][0],
                                    transform[1][0],
                                    transform[2][0],
                                ),
                                y_axis: Vector3::new(
                                    transform[0][1],
                                    transform[1][1],
                                    transform[2][1],
                                ),
                                z_axis: Vector3::new(
                                    transform[0][2],
                                    transform[1][2],
                                    transform[2][2],
                                ),
                            },
                        )
                    } else if scope.kind == "WorkPlane" {
                        scope.work_plane_transform.map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: parameters
                                    .iter()
                                    .map(|(_, parameter)| {
                                        (parameter.name.clone(), parameter.expression.clone())
                                    })
                                    .collect(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |transform| project_work_plane(scope, transform),
                        )
                    } else if scope.kind == "WorkAxis" {
                        scope
                            .work_axis_construction
                            .as_ref()
                            .and_then(|construction| {
                                let displacement = Vector3::new(
                                    construction.displacement[0],
                                    construction.displacement[1],
                                    construction.displacement[2],
                                );
                                Some((construction, displacement.unit()?))
                            })
                            .map_or_else(
                                || FeatureDefinition::Native {
                                    kind: scope.kind.clone(),
                                    parameters: parameters
                                        .iter()
                                        .map(|(_, parameter)| {
                                            (parameter.name.clone(), parameter.expression.clone())
                                        })
                                        .collect(),
                                    properties: native_scope_properties(scope, native_scope),
                                },
                                |(construction, direction)| FeatureDefinition::DatumAxis {
                                    origin: Point3::new(
                                        construction.origin[0] * 10.0,
                                        construction.origin[1] * 10.0,
                                        construction.origin[2] * 10.0,
                                    ),
                                    direction,
                                },
                            )
                    } else if scope.kind == "WorkPoint" {
                        scope.work_point_construction.as_ref().map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: parameters
                                    .iter()
                                    .map(|(_, parameter)| {
                                        (parameter.name.clone(), parameter.expression.clone())
                                    })
                                    .collect(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |construction| FeatureDefinition::DatumPoint {
                                position: Point3::new(
                                    construction.position[0] * 10.0,
                                    construction.position[1] * 10.0,
                                    construction.position[2] * 10.0,
                                ),
                                construction: project_work_point_construction(
                                    scope,
                                    construction,
                                    &parameters,
                                    edge_operands,
                                    &scope_ids,
                                )
                                .map(Box::new),
                            },
                        )
                    } else if scope.kind == "BaseFlange" {
                        project_base_flange(scope, construction_groups, placements).unwrap_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                        )
                    } else if scope.kind == "RemoveBody" {
                        project_remove_body(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            }
                        })
                    } else if scope.kind == "SurfaceStitch" {
                        project_surface_stitch(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            }
                        })
                    } else if scope.kind == "SplitFace" {
                        project_split_face(
                            scope,
                            scopes,
                            construction_groups,
                            entity_selection_operands,
                            face_operands,
                            histories,
                        )
                        .unwrap_or_else(|| FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        })
                    } else if matches!(scope.kind.as_str(), "DeleteFace" | "SurfaceDeleteFace") {
                        project_delete_face(scope, construction_groups, face_operands)
                            .unwrap_or_else(|| FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            })
                    } else if scope.kind == "CopyPasteBodies" {
                        scope.copy_paste_bodies_operation.as_ref().map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |operation| FeatureDefinition::InsertBodies {
                                bodies: design_body_selection(
                                    scope,
                                    &operation.copied_body_entity_suffixes,
                                    body_bindings,
                                ),
                            },
                        )
                    } else if scope.kind == "CopyPaste" {
                        scope.copy_paste_component_operation.as_ref().map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |operation| FeatureDefinition::InsertComponent {
                                occurrence: crate::ids::neutral_component_occurrence_id(
                                    &operation.copied_occurrence_guid,
                                ),
                            },
                        )
                    } else if scope.kind == "Base Feature" {
                        scope.base_feature_construction.as_ref().map_or_else(
                            || FeatureDefinition::Native {
                                kind: scope.kind.clone(),
                                parameters: BTreeMap::new(),
                                properties: native_scope_properties(scope, native_scope),
                            },
                            |construction| FeatureDefinition::BaseFeature {
                                bodies: design_body_selection(
                                    scope,
                                    construction.body_entity_suffixes(),
                                    body_bindings,
                                ),
                            },
                        )
                    } else {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter)| {
                                    (parameter.name.clone(), parameter.expression.clone())
                                })
                                .collect(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    }
                }
            };
            let outputs = match &definition {
                FeatureDefinition::InsertBodies {
                    bodies: cadmpeg_ir::features::BodySelection::Resolved { bodies, .. },
                } => bodies.clone(),
                _ => Vec::new(),
            };
            Feature {
                id: scope_ids[&(native_scope, scope.record_index)].clone(),
                ordinal: source_ordinals[&(native_scope, scope.record_index)],
                name: Some(format!("{} {}", scope.kind, scope.feature_ordinal)),
                suppressed: Some(
                    matches!(
                        family,
                        Some(
                            DesignFeatureFamily::Extrude
                                | DesignFeatureFamily::Fillet
                                | DesignFeatureFamily::Chamfer
                        )
                    ) && scope.history_state_id.is_none()
                        && scope.previous_history_state_id.is_none(),
                ),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: Some(scope.kind.clone()),
                source_text: None,
                source_content: Vec::new(),
                outputs,
                definition,
                native_ref: Some(scope.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    let scope_history = ScopeHistoryGraph::new(
        scopes,
        body_bindings,
        body_recipe_operands,
        component_naming_spaces,
        histories,
    );
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let ScopeHistoryPredecessor::Scope(predecessor_scope) =
            scope_history.predecessor(scope, |candidate| {
                let stream = native_stream(&candidate.id).unwrap_or(ids::DEFAULT_STREAM);
                scope_ids.contains_key(&(stream, candidate.record_index))
            })?
        else {
            continue;
        };
        let stream = native_stream(&predecessor_scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let Some(predecessor) = scope_ids.get(&(stream, predecessor_scope.record_index)) else {
            continue;
        };
        if predecessor != &feature.id && !feature.dependencies.contains(predecessor) {
            feature.dependencies.push(predecessor.clone());
        }
    }
    for feature in &mut features {
        let FeatureDefinition::Pattern { seeds, .. } = &feature.definition else {
            continue;
        };
        for dependency in seeds.iter().filter_map(|seed| match seed {
            cadmpeg_ir::features::PatternSeed::Feature(feature) => Some(feature),
            _ => None,
        }) {
            if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
    for feature in &mut features {
        let dependencies = match &feature.definition {
            FeatureDefinition::Draft {
                pull_plane: Some(plane),
                ..
            }
            | FeatureDefinition::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Plane { plane },
                ..
            } => vec![plane],
            FeatureDefinition::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
                ..
            } => planes.iter().collect(),
            FeatureDefinition::DatumPoint {
                construction: Some(construction),
                ..
            } => construction.feature_references(),
            FeatureDefinition::DatumThreePointPlane { points, .. } => points
                .iter()
                .filter_map(|point| match point {
                    cadmpeg_ir::features::VertexSelection::Generated { vertex, .. } => {
                        Some(&vertex.feature)
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for dependency in dependencies {
            if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
    let mut history_state_features = HashMap::<
        (String, ComponentHistoryNamespace, String, i64),
        Option<cadmpeg_ir::features::FeatureId>,
    >::new();
    for scope in scopes {
        let Some(state_id) = scope.history_state_id else {
            continue;
        };
        let Some(key) = scope_history.state_key(scope, state_id) else {
            continue;
        };
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let Some(feature_id) = scope_ids.get(&(stream, scope.record_index)) else {
            continue;
        };
        history_state_features
            .entry(key)
            .and_modify(|candidate| *candidate = None)
            .or_insert_with(|| Some(feature_id.clone()));
    }
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let Some(construction) = &scope.work_point_construction else {
            continue;
        };
        for state_id in construction
            .rule
            .inputs()
            .iter()
            .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands))
        {
            let Some(key) = scope_history.state_key(scope, state_id) else {
                continue;
            };
            let Some(Some(dependency)) = history_state_features.get(&key) else {
                continue;
            };
            if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
    features.sort_by(|a, b| a.id.cmp(&b.id));

    let mut parameters = native
        .iter()
        .map(|parameter| {
            let stream = native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM);
            let native_owner = parameter
                .owner_record_index
                .and_then(|record_index| owners_by_index.get(&(stream, record_index)));
            let owner =
                native_owner.and_then(|owner| scope_ids.get(&(stream, owner.scope_record_index)));
            let mut properties = BTreeMap::new();
            if parameter.kind != DesignParameterKind::User {
                properties.insert("source_kind".into(), parameter.source_kind.clone());
            }
            if let (Some(owner_record_index), None) = (parameter.owner_record_index, owner) {
                properties.insert("owner_record_index".into(), owner_record_index.to_string());
            }
            let value = match parameter.unit.as_deref() {
                Some(unit) if design_length_unit(unit) => Some(ParameterValue::Length(Length(
                    parameter.evaluated_value * 10.0,
                ))),
                Some(unit) if design_angle_unit(unit) => {
                    Some(ParameterValue::Angle(Angle(parameter.evaluated_value)))
                }
                None => Some(ParameterValue::Real(parameter.evaluated_value)),
                Some(unit) => {
                    properties.insert("unit".into(), unit.into());
                    properties.insert(
                        "evaluated_scalar".into(),
                        parameter.evaluated_value.to_string(),
                    );
                    None
                }
            };
            NeutralParameter {
                id: neutral_parameter_id(parameter),
                owner: owner.cloned(),
                ordinal: owner
                    .zip(native_owner)
                    .map_or(parameter.source_ordinal, |(_, owner)| owner.local_ordinal),
                name: parameter.name.clone(),
                expression: parameter.expression.clone(),
                display: if parameter.source_kind.contains("Diameter Dimension") {
                    Some(DimensionDisplay::Diameter)
                } else if parameter.source_kind.contains("Radius Dimension") {
                    Some(DimensionDisplay::Radius)
                } else {
                    None
                },
                value,
                dependencies: Vec::new(),
                properties,
                pmi: None,
                native_ref: Some(parameter.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    let parameter_scopes = native
        .iter()
        .filter_map(|parameter| {
            Some((
                neutral_parameter_id(parameter),
                native_stream(&parameter.id)?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut document_aliases = HashMap::<(&str, String), Option<ParameterId>>::new();
    let mut feature_aliases =
        HashMap::<(&str, cadmpeg_ir::features::FeatureId, String), Option<ParameterId>>::new();
    let mut owned_aliases = HashMap::<(&str, String), Vec<ParameterId>>::new();
    for parameter in &parameters {
        let scope = parameter_scopes[&parameter.id];
        if let Some(owner) = &parameter.owner {
            feature_aliases
                .entry((scope, owner.clone(), parameter.name.clone()))
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(parameter.id.clone()));
            owned_aliases
                .entry((scope, parameter.name.clone()))
                .or_default()
                .push(parameter.id.clone());
        } else {
            document_aliases
                .entry((scope, parameter.name.clone()))
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(parameter.id.clone()));
        }
    }
    let parameter_owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    let feature_order = features
        .iter()
        .map(|feature| (feature.id.clone(), feature.ordinal))
        .collect::<HashMap<_, _>>();
    for parameter in &mut parameters {
        let scope = parameter_scopes[&parameter.id];
        let consumer_owner = parameter.owner.clone();
        if parameter.properties.contains_key("owner_record_index") {
            continue;
        }
        let mut seen = HashSet::new();
        parameter.dependencies = expression_identifiers(&parameter.expression)
            .filter_map(|identifier| {
                let preceding_owned = || {
                    let consumer = consumer_owner.as_ref()?;
                    let consumer_order = feature_order.get(consumer)?;
                    let mut candidates = owned_aliases
                        .get(&(scope, identifier.clone()))?
                        .iter()
                        .filter(|candidate| {
                            parameter_owners
                                .get(*candidate)
                                .and_then(Option::as_ref)
                                .and_then(|owner| feature_order.get(owner))
                                .is_some_and(|order| order < consumer_order)
                        });
                    let candidate = candidates.next()?;
                    candidates.next().is_none().then_some(candidate)
                };
                let candidate = if let Some(owner) = &parameter.owner {
                    match feature_aliases.get(&(scope, owner.clone(), identifier.clone())) {
                        Some(None) => return None,
                        Some(Some(local)) => Some(local),
                        None => match document_aliases.get(&(scope, identifier.clone())) {
                            Some(Some(document)) => Some(document),
                            Some(None) => None,
                            None => preceding_owned(),
                        },
                    }
                } else {
                    document_aliases.get(&(scope, identifier))?.as_ref()
                };
                candidate.cloned().filter(|dependency| {
                    let dependency_owner = parameter_owners.get(dependency);
                    match (dependency_owner, &consumer_owner) {
                        (Some(Some(dependency_owner)), Some(consumer_owner))
                            if dependency_owner != consumer_owner =>
                        {
                            feature_order
                                .get(dependency_owner)
                                .zip(feature_order.get(consumer_owner))
                                .is_some_and(|(dependency, consumer)| dependency < consumer)
                        }
                        (Some(Some(_)), None) => false,
                        (Some(_), _) => true,
                        (None, _) => false,
                    }
                })
            })
            .filter(|dependency| dependency != &parameter.id && seen.insert(dependency.clone()))
            .collect();
    }
    normalize_parameter_ordinals(&mut parameters);
    let parameter_owners = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.id.clone(), parameter.owner.clone()?)))
        .collect::<HashMap<_, _>>();
    for feature in &mut features {
        let mut seen = feature.dependencies.iter().cloned().collect::<HashSet<_>>();
        feature.dependencies.extend(
            parameters
                .iter()
                .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
                .flat_map(|parameter| &parameter.dependencies)
                .filter_map(|parameter| parameter_owners.get(parameter))
                .filter(|dependency| {
                    *dependency != &feature.id && seen.insert((*dependency).clone())
                })
                .cloned(),
        );
    }
    ensure_feature_dependencies_precede(&features)?;
    parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((features, parameters))
}

fn work_point_edge_operand<'a>(
    scope: &DesignParameterScope,
    input: &crate::records::DesignWorkPointInput,
    edge_operands: &'a [DesignEdgeOperand],
) -> Option<&'a DesignEdgeOperand> {
    let crate::records::DesignWorkPointInputCarrier::EdgeRecipe { operand_id } =
        input.carrier.as_deref()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut matching = edge_operands.iter().filter(|operand| {
        operand.id == *operand_id
            && native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.record_index == input.record_index
    });
    let operand = matching.next()?;
    matching.next().is_none().then_some(operand)
}

pub(crate) fn work_point_input_history_state_id(
    scope: &DesignParameterScope,
    input: &crate::records::DesignWorkPointInput,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    match input.carrier.as_deref()? {
        crate::records::DesignWorkPointInputCarrier::EdgeRecipe { .. } => {
            work_point_edge_operand(scope, input, edge_operands)?.recipe_state_id
        }
        crate::records::DesignWorkPointInputCarrier::VertexRecipe { recipe } => {
            recipe.resolved_vertex_slot.and(recipe.recipe_state_id)
        }
        crate::records::DesignWorkPointInputCarrier::WorkPlane { .. }
        | crate::records::DesignWorkPointInputCarrier::SketchPoint { .. } => None,
    }
}

pub(crate) fn work_point_recipe_state_id(
    scope: &DesignParameterScope,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    let construction = scope.work_point_construction.as_ref()?;
    let mut states = construction
        .rule
        .inputs()
        .iter()
        .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands));
    let state = states.next()?;
    states.all(|candidate| candidate == state).then_some(state)
}

pub(crate) fn work_plane_recipe_state_id(scope: &DesignParameterScope) -> Option<i64> {
    let crate::records::DesignWorkPlaneConstruction::ThreePoint { inputs, .. } =
        scope.work_plane_construction.as_ref()?;
    let state = inputs[0].recipe_state_id?;
    inputs
        .iter()
        .all(|recipe| {
            recipe.recipe_state_id == Some(state) && recipe.resolved_vertex_slot.is_some()
        })
        .then_some(state)
}

fn project_work_point_construction(
    scope: &DesignParameterScope,
    construction: &crate::records::DesignWorkPointConstruction,
    parameters: &[(u32, &DesignParameter)],
    edge_operands: &[DesignEdgeOperand],
    scope_ids: &HashMap<(&str, u32), cadmpeg_ir::features::FeatureId>,
) -> Option<cadmpeg_ir::features::DatumPointConstruction> {
    use crate::records::{DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRule};
    use cadmpeg_ir::features::{
        DatumPlaneReference, DatumPointConstruction, EdgeSelection, VertexSelection,
    };

    let stream = native_stream(&scope.id)?;
    let edge = |input: &DesignWorkPointInput| {
        let operand = work_point_edge_operand(scope, input, edge_operands)?;
        let Some((state_id, edge_slot)) = operand
            .recipe_state_id
            .zip(crate::design::edge_resolve::resolved_edge_operand(operand))
        else {
            return Some(EdgeSelection::Native(operand.id.clone()));
        };
        let feature_id = neutral_feature_id(scope);
        let feature_key = feature_id
            .0
            .split_once('#')
            .map_or(feature_id.0.as_str(), |(_, key)| key);
        let prefix = ids::history_input_prefix(feature_key, state_id);
        Some(EdgeSelection::Historical {
            state: feature_input_topology_id(&feature_id, state_id),
            edges: vec![ids::history_input_edge_id(&prefix, edge_slot)],
            native: operand.id.clone(),
        })
    };
    let plane = |input: &DesignWorkPointInput| {
        let DesignWorkPointInputCarrier::WorkPlane { selection } = input.carrier.as_deref()? else {
            return None;
        };
        scope_ids
            .get(&(stream, selection.work_plane_scope_record_index))
            .cloned()
            .map(DatumPlaneReference::Feature)
    };

    Some(match &construction.rule {
        DesignWorkPointRule::CircleCenter { input } => {
            DatumPointConstruction::CircleCenter { edge: edge(input)? }
        }
        DesignWorkPointRule::TwoEdgeIntersection { inputs } => {
            DatumPointConstruction::TwoEdgeIntersection {
                edges: [edge(&inputs[0])?, edge(&inputs[1])?],
            }
        }
        DesignWorkPointRule::ThreePlaneIntersection { inputs } => {
            DatumPointConstruction::ThreePlaneIntersection {
                planes: Box::new([plane(&inputs[0])?, plane(&inputs[1])?, plane(&inputs[2])?]),
            }
        }
        DesignWorkPointRule::Vertex { input } => {
            let DesignWorkPointInputCarrier::VertexRecipe { recipe } = input.carrier.as_deref()?
            else {
                return None;
            };
            let vertex = recipe
                .recipe_state_id
                .zip(recipe.resolved_vertex_slot)
                .map_or_else(
                    || VertexSelection::Native(recipe.recipe_id.clone()),
                    |(state_id, vertex_slot)| {
                        let feature_id = neutral_feature_id(scope);
                        let feature_key = feature_id
                            .0
                            .split_once('#')
                            .map_or(feature_id.0.as_str(), |(_, key)| key);
                        let prefix = ids::history_input_prefix(feature_key, state_id);
                        VertexSelection::Historical {
                            state: feature_input_topology_id(&feature_id, state_id),
                            vertex: ids::history_input_vertex_id(&prefix, vertex_slot),
                            native: recipe.recipe_id.clone(),
                        }
                    },
                );
            DatumPointConstruction::Vertex { vertex }
        }
        DesignWorkPointRule::EdgePlaneIntersection { inputs } => {
            DatumPointConstruction::EdgePlaneIntersection {
                edge: edge(&inputs[0])?,
                plane: plane(&inputs[1])?,
            }
        }
        DesignWorkPointRule::DistanceOnEdge { input } => {
            let mut distances = parameters
                .iter()
                .map(|(_, parameter)| *parameter)
                .filter(|parameter| parameter.source_kind == "PathDistance");
            let distance = distances.next()?;
            if distances.next().is_some() || !(0.0..=1.0).contains(&distance.evaluated_value) {
                return None;
            }
            DatumPointConstruction::DistanceOnEdge {
                edge: edge(input)?,
                fraction: distance.evaluated_value,
            }
        }
        DesignWorkPointRule::Native { .. } => return None,
    })
}

fn project_work_plane(
    scope: &DesignParameterScope,
    transform: [[f64; 4]; 4],
) -> cadmpeg_ir::features::FeatureDefinition {
    use crate::records::DesignWorkPlaneConstruction;
    use cadmpeg_ir::features::{FeatureDefinition, VertexSelection};

    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let normal = Vector3::new(transform[0][2], transform[1][2], transform[2][2]);
    let u_axis = Vector3::new(transform[0][0], transform[1][0], transform[2][0]);
    let Some(DesignWorkPlaneConstruction::ThreePoint { inputs, .. }) =
        &scope.work_plane_construction
    else {
        return FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        };
    };
    let Some(state_id) = work_plane_recipe_state_id(scope) else {
        return FeatureDefinition::DatumPlaneUnresolved;
    };
    let feature_id = neutral_feature_id(scope);
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    let prefix = ids::history_input_prefix(feature_key, state_id);
    let points = inputs
        .iter()
        .map(|recipe| {
            Some(VertexSelection::Historical {
                state: feature_input_topology_id(&feature_id, state_id),
                vertex: ids::history_input_vertex_id(&prefix, recipe.resolved_vertex_slot?),
                native: recipe.recipe_id.clone(),
            })
        })
        .collect::<Option<Vec<_>>>();
    let Some(points) = points.and_then(|points| points.try_into().ok()) else {
        return FeatureDefinition::DatumPlaneUnresolved;
    };
    FeatureDefinition::DatumThreePointPlane {
        origin,
        normal,
        u_axis,
        points: Box::new(points),
    }
}

pub(crate) fn project_combine(
    scope: &DesignParameterScope,
    native_scope: &str,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let operation = scope.combine_operation.as_ref()?;
    if operation.tools.is_empty() {
        return None;
    }
    let selection = |record_index| format!("{native_scope}:design-record#{record_index}");
    Some(FeatureDefinition::Combine {
        target: BodySelection::Native(selection(operation.target.record_index)),
        tools: if let [tool] = operation.tools.as_slice() {
            BodySelection::Native(selection(tool.record_index))
        } else {
            BodySelection::NativeSet(
                operation
                    .tools
                    .iter()
                    .map(|tool| selection(tool.record_index))
                    .collect(),
            )
        },
        op: match operation.operation {
            DesignExtrudeOperation::Join => BooleanOp::Join,
            DesignExtrudeOperation::Cut => BooleanOp::Cut,
            DesignExtrudeOperation::Intersect => BooleanOp::Intersect,
            DesignExtrudeOperation::NewBody => return None,
        },
        keep_tools: operation.keep_tools,
    })
}

fn scope_properties(
    scope: &DesignParameterScope,
    native_scope: &str,
    placements: &[DesignSketchPlacement],
) -> std::collections::BTreeMap<String, String> {
    use std::collections::BTreeMap;
    let mut properties = BTreeMap::new();
    for (ordinal, record_index) in scope.reference_members.iter().enumerate() {
        properties.insert(format!("reference:{ordinal}"), record_index.to_string());
    }
    if let Some(profile) = scope
        .extrude_profile
        .as_ref()
        .or(scope.base_flange_profile.as_ref())
    {
        if let Some(placement) = placements.iter().find(|placement| {
            native_stream(&placement.id) == Some(native_scope)
                && placement.entity_id == profile.entity_id
        }) {
            properties.insert("profile".into(), neutral_sketch_id(placement).0);
        }
    }
    properties
}

fn project_fillet_arm(
    inputs: &ProjectInputs<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    native_scope: &str,
) -> cadmpeg_ir::features::FeatureDefinition {
    use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FilletGroup, RadiusSpec};

    if parameters.is_empty() {
        if let Some(definition) = project_full_round_fillet(
            scope,
            inputs.construction_groups,
            inputs.face_operands,
            inputs.owners,
            inputs.fillet_radius_groups,
            inputs.histories,
        ) {
            return definition;
        }
    }

    let &ProjectInputs {
        native,
        construction_groups,
        fillet_radius_groups,
        edge_operands,
        edge_identity_operands,
        edge_treatment_vertex_operands,
        histories,
        placements,
        ..
    } = inputs;

    if let Some(definition) = project_variable_fillet(
        scope,
        parameters,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        edge_treatment_vertex_operands,
        histories,
    ) {
        definition
    } else if let Some(definition) = parameters
        .is_empty()
        .then(|| {
            project_fixed_fillet_with_corners(
                scope,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                edge_treatment_vertex_operands,
                histories,
            )
        })
        .flatten()
    {
        definition
    } else {
        let mut assignments = fillet_radius_groups
            .iter()
            .filter(|assignment| {
                native_stream(&assignment.id) == Some(native_scope)
                    && assignment.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        assignments.sort_by_key(|assignment| assignment.group_ordinal);
        let assigned_parameter_records = assignments
            .iter()
            .flat_map(|assignment| {
                fillet_law_parameter_records(&assignment.law)
                    .into_iter()
                    .chain(assignment.tangency_weight_parameter_record_index)
            })
            .collect::<Vec<_>>();
        let incomplete_assignment = if assignments.is_empty() {
            let radii = parameters
                .iter()
                .filter(|(_, parameter)| parameter.source_kind == "Radius")
                .map(|(_, parameter)| *parameter)
                .collect::<Vec<_>>();
            radii.len() != 1
                || radii
                    .iter()
                    .any(|parameter| design_length(parameter).is_none_or(|value| value.0 <= 0.0))
                || parameters
                    .iter()
                    .any(|(_, parameter)| parameter.source_kind != "Radius")
        } else {
            assigned_parameter_records.len() != parameters.len()
                || parameters.iter().any(|(_, parameter)| {
                    !matches!(
                        parameter.source_kind.as_str(),
                        "Radius" | "ChordLen" | "EdgeOffset1" | "EdgeOffset2" | "TangencyWeight"
                    ) || assigned_parameter_records
                        .iter()
                        .filter(|record_index| **record_index == parameter.record_index)
                        .count()
                        != 1
                })
                || parameters.iter().any(|(_, parameter)| {
                    if matches!(
                        parameter.source_kind.as_str(),
                        "Radius" | "ChordLen" | "EdgeOffset1" | "EdgeOffset2"
                    ) {
                        design_length(parameter).is_none_or(|value| value.0 <= 0.0)
                    } else {
                        !parameter.evaluated_value.is_finite()
                    }
                })
        };
        if incomplete_assignment {
            FeatureDefinition::Native {
                kind: scope.kind.clone(),
                parameters: parameters
                    .iter()
                    .map(|(_, parameter)| (parameter.name.clone(), parameter.expression.clone()))
                    .collect(),
                properties: scope_properties(scope, native_scope, placements),
            }
        } else {
            let groups = assignments
                .into_iter()
                .map(|assignment| {
                    let (radius, edge_radius) = match assignment.law {
                        DesignFilletRadiusLaw::Constant {
                            radius_parameter_record_index,
                        } => {
                            let radius = parameters
                                .iter()
                                .find(|(_, parameter)| {
                                    parameter.record_index == radius_parameter_record_index
                                })
                                .and_then(|(_, parameter)| design_length(parameter))
                                .expect("complete Fillet assignment has a positive radius");
                            (RadiusSpec::Constant { radius }, Some(radius.0))
                        }
                        DesignFilletRadiusLaw::Chordal {
                            chord_length_parameter_record_index,
                        } => {
                            let chord_length = parameters
                                .iter()
                                .find(|(_, parameter)| {
                                    parameter.record_index == chord_length_parameter_record_index
                                })
                                .and_then(|(_, parameter)| design_length(parameter))
                                .expect("complete chordal Fillet has a positive chord length");
                            (RadiusSpec::Chordal { chord_length }, None)
                        }
                        DesignFilletRadiusLaw::Asymmetric {
                            offset_one_parameter_record_index,
                            offset_two_parameter_record_index,
                        } => {
                            let offset = |record_index| {
                                parameters
                                    .iter()
                                    .find(|(_, parameter)| parameter.record_index == record_index)
                                    .and_then(|(_, parameter)| design_length(parameter))
                                    .filter(|offset| offset.0 > 0.0)
                            };
                            let offset_one = offset(offset_one_parameter_record_index)
                                .expect("complete asymmetric Fillet has a positive first offset");
                            let offset_two = offset(offset_two_parameter_record_index)
                                .expect("complete asymmetric Fillet has a positive second offset");
                            (
                                RadiusSpec::Asymmetric {
                                    offset_one,
                                    offset_two,
                                },
                                None,
                            )
                        }
                        DesignFilletRadiusLaw::Variable { .. } => {
                            unreachable!("variable Fillet projected before constants")
                        }
                    };
                    let tangency_weight = assignment
                        .tangency_weight_parameter_record_index
                        .and_then(|record_index| {
                            native.iter().find(|parameter| {
                                native_stream(&parameter.id) == Some(native_scope)
                                    && parameter.record_index == record_index
                            })
                        })
                        .map(|parameter| parameter.evaluated_value)
                        .filter(|weight| weight.is_finite());
                    let edges = construction_groups
                        .iter()
                        .find(|group| {
                            native_stream(&group.id) == Some(native_scope)
                                && group.record_index == assignment.group_record_index
                        })
                        .map_or_else(
                            || EdgeSelection::Native(assignment.id.clone()),
                            |group| {
                                resolved_edge_treatment_group_with_corners(
                                    group,
                                    construction_groups,
                                    edge_operands,
                                    edge_identity_operands,
                                    edge_treatment_vertex_operands,
                                    histories,
                                    scope.previous_history_state_id,
                                    &neutral_feature_id(scope),
                                    edge_radius,
                                )
                            },
                        );
                    FilletGroup {
                        edges,
                        radius,
                        tangency_weight,
                    }
                })
                .collect::<Vec<_>>();
            FeatureDefinition::Fillet {
                groups: if groups.is_empty() {
                    vec![FilletGroup {
                        edges: EdgeSelection::Native(scope.id.clone()),
                        radius: RadiusSpec::Constant {
                            radius: parameters
                                .iter()
                                .filter(|(_, parameter)| parameter.source_kind == "Radius")
                                .find_map(|(_, parameter)| design_length(parameter))
                                .expect("complete ungrouped Fillet has one positive radius"),
                        },
                        tangency_weight: None,
                    }]
                } else {
                    groups
                },
            }
        }
    }
}

fn project_thread_face_selection(
    scope: &DesignParameterScope,
    face_group_record_indices: &[u32],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> cadmpeg_ir::features::FaceSelection {
    use cadmpeg_ir::features::FaceSelection;

    let Some(stream) = native_stream(&scope.id) else {
        return FaceSelection::Unresolved;
    };
    if face_group_record_indices.is_empty() {
        return FaceSelection::Unresolved;
    }
    let mut ordered_groups = Vec::with_capacity(face_group_record_indices.len());
    for record_index in face_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
                && group.role == ROLE_0X10
        });
        let Some(group) = matching.next() else {
            return FaceSelection::Unresolved;
        };
        if matching.next().is_some() {
            return FaceSelection::Unresolved;
        }
        ordered_groups.push(group);
    }
    let native = if let [group] = ordered_groups.as_slice() {
        group.id.clone()
    } else {
        scope.id.clone()
    };
    let mut state = None;
    let mut faces = Vec::new();
    for group in ordered_groups {
        let Some(FaceSelection::Historical {
            state: group_state,
            faces: group_faces,
            ..
        }) = resolved_historical_face_group(scope, group, face_operands)
        else {
            return FaceSelection::Native(native);
        };
        if state
            .as_ref()
            .is_some_and(|expected| expected != &group_state)
        {
            return FaceSelection::Native(native);
        }
        state.get_or_insert(group_state);
        for face in group_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    let Some(state) = state else {
        return FaceSelection::Native(native);
    };
    FaceSelection::Historical {
        state,
        faces,
        native,
    }
}

/// Project Fusion's role-`0x4` full-round face construction.
///
/// This form has no radius parameter. Its one compact member identifies the
/// center face; the trailing true flag requests automatic inference of both
/// side-face sets. A role-`0x4` group with any other shape remains available to
/// the regular feature projectors.
fn project_full_round_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    owners: &[DesignParameterOwner],
    fillet_radius_groups: &[DesignFilletRadiusGroup],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    let stream = native_stream(&scope.id)?;
    if owners.iter().any(|owner| {
        native_stream(&owner.id) == Some(stream) && owner.scope_record_index == scope.record_index
    }) || fillet_radius_groups.iter().any(|assignment| {
        native_stream(&assignment.id) == Some(stream)
            && assignment.scope_record_index == scope.record_index
    }) {
        return None;
    }
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let group = groups.next()?;
    if groups.next().is_some()
        || group.role != ROLE_0X4
        || group.members.len() != 1
        || group.frame.trailing_record_indices.len() != 1
        || group.frame.trailing_flags.len() != 1
        || group.frame.trailing_record_indices[0] != group.frame.trailing_flags[0].record_index
        || !group.frame.trailing_flags[0].value
        || group.frame.variant
    {
        return None;
    }
    let [member] = group.members.as_slice() else {
        return None;
    };
    let mut operands = face_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == Some(group.record_index)
            && operand.group_member_ordinal == Some(0)
            && operand.record_index == *member
    });
    let operand = operands.next()?;
    if operands.next().is_some() {
        return None;
    }
    if operand.recipe_kind != ConstructionRecipeKind::BoundedFace {
        return None;
    }
    if operand.resolved_face_slots.is_empty() {
        return None;
    }
    let center_faces = project_face_selection(scope, group, face_operands, histories);
    if matches!(center_faces, cadmpeg_ir::features::FaceSelection::Native(_)) {
        return None;
    }
    Some(cadmpeg_ir::features::FeatureDefinition::FullRoundFillet {
        groups: vec![cadmpeg_ir::features::FullRoundFilletGroup {
            center_faces,
            side_one_faces: cadmpeg_ir::features::FullRoundSideSelection::Automatic,
            side_two_faces: cadmpeg_ir::features::FullRoundSideSelection::Automatic,
        }],
    })
}

fn design_body_selection<T>(
    scope: &DesignParameterScope,
    entity_suffixes: &[T],
    body_bindings: &[DesignBodyBinding],
) -> cadmpeg_ir::features::BodySelection
where
    T: Copy + Into<u64>,
{
    use cadmpeg_ir::features::BodySelection;

    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let bodies = entity_suffixes
        .iter()
        .filter_map(|suffix| {
            let suffix = (*suffix).into();
            let matches = body_bindings
                .iter()
                .filter(|binding| {
                    native_stream(&binding.id) == Some(stream) && binding.entity_suffix == suffix
                })
                .filter_map(|binding| binding.body.clone())
                .collect::<HashSet<_>>();
            (matches.len() == 1)
                .then(|| matches.into_iter().next())
                .flatten()
        })
        .collect::<Vec<_>>();
    if bodies.len() == entity_suffixes.len() {
        BodySelection::Resolved {
            bodies,
            native: scope.id.clone(),
        }
    } else {
        BodySelection::Native(scope.id.clone())
    }
}

/// Bind each Sketch history node to geometry in exactly one neutral sketch arena.
pub fn bind_sketch_feature_geometry(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, LoftSection, PathRef, ProfileRef,
        SketchPointSelection,
    };

    for feature in features.iter_mut() {
        if !matches!(
            feature.definition,
            FeatureDefinition::Sketch { .. } | FeatureDefinition::SpatialSketch { .. }
        ) {
            continue;
        }
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let stream = native_stream(&scope.id);
        let matching = placements
            .iter()
            .filter(|placement| {
                native_stream(&placement.id) == stream
                    && placement.scope_record_index == Some(scope.record_index)
            })
            .collect::<Vec<_>>();
        let [placement] = matching.as_slice() else {
            continue;
        };
        let planar = neutral_sketch_id(placement);
        let spatial = neutral_spatial_sketch_id(placement);
        let has_planar = sketches.iter().any(|sketch| sketch.id == planar);
        let has_spatial = spatial_sketches.iter().any(|sketch| sketch.id == spatial);
        feature.definition = match (has_planar, has_spatial) {
            (true, false) => FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(planar),
            },
            (false, true) => FeatureDefinition::SpatialSketch {
                sketch: Some(spatial),
            },
            _ => FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Unresolved,
                sketch: None,
            },
        };
    }
    for feature in features.iter_mut() {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let FeatureDefinition::Extrude { profile, .. } = &mut feature.definition else {
            continue;
        };
        let ProfileRef::Sketch(sketch) = profile else {
            continue;
        };
        let planar_id = sketch.clone();
        if sketches.iter().any(|candidate| candidate.id == planar_id) {
            continue;
        }
        let matching = placements
            .iter()
            .filter(|placement| neutral_sketch_id(placement) == planar_id)
            .filter_map(|placement| {
                let spatial_id = neutral_spatial_sketch_id(placement);
                spatial_sketches
                    .iter()
                    .find(|candidate| candidate.id == spatial_id)
            })
            .collect::<Vec<_>>();
        let [spatial] = matching.as_slice() else {
            continue;
        };
        if spatial.profiles.is_empty() {
            let Some(profile_operand) = scope.extrude_profile.as_ref() else {
                continue;
            };
            let Some(stream) = native_stream(&scope.id) else {
                continue;
            };
            // The spatial carrier has no closed loop that can be represented
            // by a profile index. Keep the exact profile frame as a native
            // selection instead of retaining the provisional planar ID.
            *profile = ProfileRef::SpatialSketchSelection {
                sketch: spatial.id.clone(),
                selections: vec![format!(
                    "{stream}:design-record-header#{}",
                    profile_operand.byte_offset
                )],
            };
            continue;
        }
        let Ok(profile_count) = u32::try_from(spatial.profiles.len()) else {
            continue;
        };
        *profile = ProfileRef::SpatialSketchProfiles {
            sketch: spatial.id.clone(),
            profiles: (0..profile_count).collect(),
        };
    }
    let sketch_features = features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
            } => Some((sketch.clone(), feature.id.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let spatial_sketch_features = features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::SpatialSketch {
                sketch: Some(sketch),
            } => Some((sketch.clone(), feature.id.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let profile_dependency = |profile: &ProfileRef| match profile {
        ProfileRef::Sketch(sketch)
        | ProfileRef::SketchProfiles { sketch, .. }
        | ProfileRef::SketchRegions { sketch, .. }
        | ProfileRef::SketchEntities { sketch, .. }
        | ProfileRef::SketchSelection { sketch, .. } => sketch_features.get(sketch).cloned(),
        ProfileRef::SpatialSketchProfiles { sketch, .. }
        | ProfileRef::SpatialSketchSelection { sketch, .. } => {
            spatial_sketch_features.get(sketch).cloned()
        }
        _ => None,
    };
    let path_dependency = |path: &PathRef| match path {
        PathRef::Sketch(sketch) | PathRef::SketchCurves { sketch, .. } => {
            sketch_features.get(sketch).cloned()
        }
        PathRef::SpatialSketchSelection { sketch, .. }
        | PathRef::SpatialSketchCurves { sketch, .. } => {
            spatial_sketch_features.get(sketch).cloned()
        }
        _ => None,
    };
    let sketch_point_dependency = |point: &SketchPointSelection| match point {
        SketchPointSelection::Planar { sketch, .. } => sketch_features.get(sketch).cloned(),
        SketchPointSelection::Spatial { sketch, .. } => {
            spatial_sketch_features.get(sketch).cloned()
        }
        SketchPointSelection::Unresolved | SketchPointSelection::Native(_) => None,
    };
    for feature in features.iter_mut() {
        let mut dependencies = Vec::new();
        match &feature.definition {
            FeatureDefinition::Extrude { profile, .. } => {
                dependencies.extend(profile_dependency(profile));
            }
            FeatureDefinition::SheetMetalBaseFlange { profile, .. } => {
                dependencies.extend(profile_dependency(profile));
            }
            FeatureDefinition::Revolve { construction, .. } => {
                dependencies.extend(construction.profile.as_ref().and_then(profile_dependency));
                dependencies.extend(
                    construction
                        .axis_reference
                        .as_ref()
                        .and_then(path_dependency),
                );
            }
            FeatureDefinition::Sweep {
                section,
                sections,
                path,
                guide_rail,
                ..
            } => {
                dependencies.extend(
                    std::iter::once(section)
                        .chain(sections)
                        .filter_map(|section| section.referenced_profile())
                        .filter_map(profile_dependency),
                );
                dependencies.extend(path.as_ref().and_then(path_dependency));
                dependencies.extend(
                    guide_rail
                        .as_ref()
                        .and_then(|guide| path_dependency(&guide.path)),
                );
            }
            FeatureDefinition::Loft {
                sections,
                guides,
                centerline,
                ..
            } => {
                dependencies.extend(sections.iter().filter_map(|section| match section {
                    LoftSection::Profile(profile) => profile_dependency(profile),
                    LoftSection::Point(_) => None,
                }));
                dependencies.extend(guides.iter().filter_map(path_dependency));
                dependencies.extend(centerline.as_ref().and_then(path_dependency));
            }
            FeatureDefinition::DatumPoint {
                construction: Some(construction),
                ..
            } => {
                if let DatumPointConstruction::SketchPoint { point } = construction.as_ref() {
                    dependencies.extend(sketch_point_dependency(point));
                }
            }
            _ => {}
        }
        for dependency in dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

/// Bind `WorkPoint` inputs that select a sketch point after the sketch arenas
/// have been projected. The Design selection identifies a native point record;
/// the neutral point identity depends on whether that record belongs to a
/// planar or model-space sketch.
pub fn bind_work_point_sketch_point_constructions(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    spatial_sketch_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) {
    use cadmpeg_ir::features::{DatumPointConstruction, FeatureDefinition, SketchPointSelection};

    for feature in features.iter_mut() {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let FeatureDefinition::DatumPoint { construction, .. } = &mut feature.definition else {
            continue;
        };
        if construction.is_some() {
            continue;
        }
        let Some(crate::records::DesignWorkPointConstruction {
            rule:
                crate::records::DesignWorkPointRule::Vertex {
                    input:
                        crate::records::DesignWorkPointInput {
                            carrier: Some(carrier),
                            record_index,
                            ..
                        },
                },
            ..
        }) = scope.work_point_construction.as_ref()
        else {
            continue;
        };
        let crate::records::DesignWorkPointInputCarrier::SketchPoint { selection } =
            carrier.as_ref()
        else {
            continue;
        };
        let native = format!(
            "{}:design-record#{record_index}",
            native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM)
        );
        if let Some(entity) = sketch_entities
            .iter()
            .find(|entity| entity.native_ref.as_deref() == Some(selection.point_native_id.as_str()))
        {
            *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                point: SketchPointSelection::Planar {
                    sketch: entity.sketch.clone(),
                    point: entity.id.clone(),
                    native,
                },
            }));
        } else if let Some(entity) = spatial_sketch_entities
            .iter()
            .find(|entity| entity.native_ref.as_deref() == Some(selection.point_native_id.as_str()))
        {
            *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                point: SketchPointSelection::Spatial {
                    sketch: entity.sketch.clone(),
                    point: entity.id.clone(),
                    native,
                },
            }));
        }
    }
}

/// Construction-operand-group role integers used to select a scoped group.
/// Fusion serializes these as opaque tags; each role is interpreted only in
/// the feature family that owns the group.
const ROLE_0X4: u64 = 0x0000_0004_0000_0000;
const ROLE_0X8: u64 = 0x0000_0008_0000_0000;
const ROLE_0X5: u64 = 0x0000_0005_0000_0000;
const ROLE_0X9: u64 = 0x0000_0009_0000_0000;
const ROLE_0X10: u64 = 0x0000_0010_0000_0000;
const ROLE_0X21: u64 = 0x0000_0021_0000_0000;
const ROLE_0X12: u64 = 0x0000_0012_0000_0000;
const ROLE_0X41: u64 = 0x0000_0041_0000_0000;

fn project_surface_offset(
    scope: &DesignParameterScope,
    operation: &DesignSurfaceOffsetOperation,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let stream = native_stream(&scope.id)?;
    let DesignSurfaceOffsetSupport::FaceGroups {
        group_record_indices,
    } = &operation.support
    else {
        let DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index,
            ..
        } = &operation.support
        else {
            return None;
        };
        return Some(FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(format!("{stream}:design-record#{boundary_record_index}")),
            distance: Some(Length(operation.distance * 10.0)),
        });
    };
    let mut faces = Vec::new();
    for group_record_index in group_record_indices {
        let mut matching_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *group_record_index
                && group.role == ROLE_0X41
                && !group.members.is_empty()
        });
        let group = matching_groups.next()?;
        if matching_groups.next().is_some() {
            return None;
        }
        let FaceSelection::Resolved {
            faces: group_faces, ..
        } = resolved_face_group(group, face_operands)?
        else {
            return None;
        };
        for face in group_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (!faces.is_empty()).then(|| FeatureDefinition::OffsetSurface {
        faces: FaceSelection::Resolved {
            faces,
            native: scope.id.clone(),
        },
        distance: Some(Length(operation.distance * 10.0)),
    })
}

/// Derive the neutral material-side flag from F3D's signed Draft angle.
///
/// F3D does not store a second outward bit. Keeping this rule in one helper
/// makes every Draft projection branch use the same convention.
pub(crate) const fn draft_outward(angle: f64) -> bool {
    angle < 0.0
}

fn project_draft(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{Angle, FeatureDefinition};

    let construction = scope.draft_operation.as_ref()?;
    let faces = single_operand_group(groups, scope, ROLE_0X10)?;
    let role_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && group.role == ROLE_0X21
                && !group.members.is_empty()
        })
        .collect::<Vec<_>>();
    let member_of_scope = |group: &DesignConstructionOperandGroup| {
        group
            .members
            .iter()
            .all(|member| scope.reference_members.contains(member))
    };
    if !scope.reference_members.contains(&faces.record_index) || !member_of_scope(faces) {
        return None;
    }
    match role_groups.as_slice() {
        [neutral_plane]
            if member_of_scope(neutral_plane)
                && group_has_entity_selection(scope, neutral_plane, entity_selection_operands) =>
        {
            if let Some(neutral_plane) =
                selected_work_plane(scope, neutral_plane, entity_selection_operands, scopes)
            {
                let transform = neutral_plane.work_plane_transform?;
                let pull_direction =
                    Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()?;
                return Some(FeatureDefinition::Draft {
                    faces: project_draft_face_selection(scope, faces, face_operands, histories),
                    neutral_plane: cadmpeg_ir::features::FaceSelection::Native(
                        neutral_feature_id(neutral_plane).0,
                    ),
                    parting_tool: None,
                    pull_direction: Some(pull_direction),
                    pull_plane: Some(neutral_feature_id(neutral_plane)),
                    angle: Some(Angle(construction.angle)),
                    outward: Some(draft_outward(construction.angle)),
                });
            }
            let neutral_plane = selected_historical_face_selection(
                scope,
                neutral_plane,
                entity_selection_operands,
                histories,
            )?;
            Some(FeatureDefinition::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories),
                neutral_plane,
                parting_tool: None,
                pull_direction: None,
                pull_plane: None,
                angle: Some(Angle(construction.angle)),
                outward: Some(draft_outward(construction.angle)),
            })
        }
        [neutral_plane] if member_of_scope(neutral_plane) => Some(FeatureDefinition::Draft {
            faces: project_draft_face_selection(scope, faces, face_operands, histories),
            neutral_plane: project_draft_face_selection(
                scope,
                neutral_plane,
                face_operands,
                histories,
            ),
            parting_tool: None,
            pull_direction: None,
            pull_plane: None,
            angle: Some(Angle(construction.angle)),
            outward: Some(draft_outward(construction.angle)),
        }),
        [first, second] if member_of_scope(first) && member_of_scope(second) => {
            let first_plane = selected_work_plane(scope, first, entity_selection_operands, scopes);
            let second_plane =
                selected_work_plane(scope, second, entity_selection_operands, scopes);
            let (parting_tool, pull_plane) = match (first_plane, second_plane) {
                (Some(plane), None)
                    if !group_has_entity_selection(scope, second, entity_selection_operands) =>
                {
                    (second, plane)
                }
                (None, Some(plane))
                    if !group_has_entity_selection(scope, first, entity_selection_operands) =>
                {
                    (first, plane)
                }
                _ => return None,
            };
            let transform = pull_plane.work_plane_transform?;
            let pull_direction =
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()?;
            Some(FeatureDefinition::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories),
                neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                parting_tool: Some(project_draft_face_selection(
                    scope,
                    parting_tool,
                    face_operands,
                    histories,
                )),
                pull_direction: Some(pull_direction),
                pull_plane: Some(neutral_feature_id(pull_plane)),
                angle: Some(Angle(construction.angle)),
                outward: Some(draft_outward(construction.angle)),
            })
        }
        _ => None,
    }
}

fn selected_historical_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let [member] = group.members.as_slice() else {
        return None;
    };
    let selections = entity_selection_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index == *member
        })
        .collect::<Vec<_>>();
    let [selection] = selections.as_slice() else {
        return None;
    };
    if selection.secondary_identity.is_some() || selection.curve_secondary_identity.is_some() {
        return None;
    }
    let mut face_slots = selection
        .historical_face_candidates
        .iter()
        .filter(|candidate| candidate.historical_state_ids.contains(&previous_state_id))
        .map(|candidate| candidate.face_slot);
    let face_slot = face_slots.next()?;
    if face_slots.any(|candidate| candidate != face_slot) {
        return None;
    }
    let feature = neutral_feature_id(scope);
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    let prefix = ids::history_input_prefix(feature_key, previous_state_id);
    Some(cadmpeg_ir::features::FaceSelection::Historical {
        state: feature_input_topology_id(&feature, previous_state_id),
        faces: vec![ids::history_input_face_id(&prefix, face_slot)],
        native: group.id.clone(),
    })
}

fn project_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> cadmpeg_ir::features::FaceSelection {
    let historical = crate::history::effective_scope_previous_history_state_id(scope, histories)
        .and_then(|previous_state_id| {
            let mut effective_scope = scope.clone();
            if scope.previous_history_state_id != Some(previous_state_id) {
                effective_scope.previous_history_state_id = Some(previous_state_id);
            }
            let updated_face_slots = scope
                .history_state_id
                .and_then(|state_id| {
                    crate::history::unique_history_state_pair(
                        histories,
                        state_id,
                        previous_state_id,
                    )
                })
                .and_then(|(_, state, _)| state.transition.as_ref())
                .map_or(&[][..], |transition| {
                    transition.topology.faces.updated.as_slice()
                });
            resolved_historical_face_group(&effective_scope, group, face_operands).or_else(|| {
                resolved_historical_split_face_target_group_with_updated_faces(
                    &effective_scope,
                    group,
                    face_operands,
                    updated_face_slots,
                )
            })
        });
    historical
        .or_else(|| resolved_face_group(group, face_operands))
        .unwrap_or_else(|| cadmpeg_ir::features::FaceSelection::Native(group.id.clone()))
}

fn project_draft_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> cadmpeg_ir::features::FaceSelection {
    let selection = project_face_selection(scope, group, face_operands, histories);
    if matches!(&selection, cadmpeg_ir::features::FaceSelection::Native(_)) {
        crate::design::face_resolve::resolved_explicit_bounded_face_group(group, face_operands)
            .unwrap_or(selection)
    } else {
        selection
    }
}

fn group_has_entity_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
) -> bool {
    let Some(stream) = native_stream(&scope.id) else {
        return false;
    };
    group
        .members
        .iter()
        .enumerate()
        .any(|(ordinal, record_index)| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            entity_selection_operands.iter().any(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index == *record_index
            })
        })
}

fn selected_work_plane<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<&'a DesignParameterScope> {
    let planes = selected_work_planes(scope, group, entity_selection_operands, scopes)?;
    let [plane] = planes.as_slice() else {
        return None;
    };
    Some(*plane)
}

fn selected_work_planes<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<Vec<&'a DesignParameterScope>> {
    let stream = native_stream(&scope.id)?;
    if group.members.is_empty() {
        return None;
    }
    let mut planes = Vec::with_capacity(group.members.len());
    let mut target_record_indices = HashSet::with_capacity(group.members.len());
    for (ordinal, member) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let selections = entity_selection_operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index == *member
            })
            .collect::<Vec<_>>();
        let [selection] = selections.as_slice() else {
            return None;
        };
        if selection.secondary_identity.is_some() || selection.curve_secondary_identity.is_some() {
            return None;
        }
        let target_record_index = u32::try_from(selection.primary_identity)
            .ok()?
            .checked_add(1)?;
        if !target_record_indices.insert(target_record_index) {
            return None;
        }
        let mut target_scopes = scopes.iter().filter(|candidate| {
            native_stream(&candidate.id) == Some(stream)
                && candidate.record_index == target_record_index
                && candidate.kind == "WorkPlane"
                && candidate.work_plane_transform.is_some()
        });
        let target = target_scopes.next()?;
        if target_scopes.next().is_some() {
            return None;
        }
        planes.push(target);
    }
    Some(planes)
}

fn resolved_split_face_path(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::PathRef> {
    use cadmpeg_ir::features::PathRef;

    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let feature = neutral_feature_id(scope);
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    let prefix = ids::history_input_prefix(feature_key, previous_state_id);
    let mut edge_slots = Vec::with_capacity(group.members.len());
    for (ordinal, member) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut selections = entity_selection_operands.iter().filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == scope.record_index
                && selection.group_record_index == group.record_index
                && selection.group_member_ordinal == ordinal
                && selection.record_index == *member
        });
        let selection = selections.next()?;
        if selections.next().is_some()
            || selection.secondary_identity.is_some()
            || selection.curve_secondary_identity.is_some()
        {
            return None;
        }
        let edge_slot = selection.resolved_edge_slot?;
        if edge_slots.contains(&edge_slot) {
            return None;
        }
        edge_slots.push(edge_slot);
    }
    (!edge_slots.is_empty()).then(|| PathRef::HistoricalEdges {
        state: feature_input_topology_id(&feature, previous_state_id),
        edges: edge_slots
            .into_iter()
            .map(|edge_slot| ids::history_input_edge_id(&prefix, edge_slot))
            .collect(),
        native: group.id.clone(),
    })
}

/// Return the unique non-empty construction operand group in `scope` carrying
/// `role`. Yields `None` unless exactly one such group exists.
fn single_operand_group<'a>(
    groups: &'a [DesignConstructionOperandGroup],
    scope: &DesignParameterScope,
    role: u64,
) -> Option<&'a DesignConstructionOperandGroup> {
    let matching = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && group.role == role
                && !group.members.is_empty()
        })
        .collect::<Vec<_>>();
    let [group] = matching.as_slice() else {
        return None;
    };
    Some(*group)
}

pub(crate) fn project_offset_faces(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceMotion, FeatureDefinition, Length};

    let parameter_distance = match parameters {
        [] => None,
        [(_, distance)] if distance.source_kind == "distance" => Some(design_length(distance)?),
        _ => return None,
    };
    let fixed_distance = match &scope.direct_face_operation {
        Some(DesignDirectFaceOperation::OffsetFaces { distance, .. }) => {
            Some(Length(*distance * 10.0))
        }
        None => None,
        Some(_) => return None,
    };
    let distance = match (parameter_distance, fixed_distance) {
        (Some(parameter), Some(fixed)) if (parameter.0 - fixed.0).abs() <= 1.0e-9 => parameter,
        (Some(distance), None) | (None, Some(distance)) => distance,
        _ => return None,
    };
    let faces = direct_face_selection(scope, operands).or_else(|| {
        let group = single_operand_group(groups, scope, ROLE_0X10)?;
        Some(cadmpeg_ir::features::FaceSelection::Native(
            group.id.clone(),
        ))
    })?;
    Some(FeatureDefinition::MoveFace {
        faces,
        motion: FaceMotion::Offset { distance },
    })
}

pub(crate) fn project_thicken(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};

    let DesignDirectFaceOperation::Thicken {
        signed_thickness, ..
    } = scope.direct_face_operation.as_ref()?
    else {
        return None;
    };
    let faces = direct_face_selection(scope, operands).or_else(|| {
        let mut candidates = groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && matches!(group.role, ROLE_0X5 | ROLE_0X12)
                && !group.members.is_empty()
        });
        let group = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(FaceSelection::Native(group.id.clone()))
    })?;
    Some(FeatureDefinition::Thicken {
        faces,
        thickness: Some(Length(signed_thickness.abs() * 10.0)),
        side: Some(if *signed_thickness > 0.0 {
            ThickenSide::Forward
        } else {
            ThickenSide::Reverse
        }),
    })
}

pub(crate) fn project_shell(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, Length};

    let DesignDirectFaceOperation::Shell {
        thickness, outward, ..
    } = scope.direct_face_operation.as_ref()?
    else {
        return None;
    };
    let bodies = single_operand_group(groups, scope, ROLE_0X4)
        .map(|group| BodySelection::Native(group.id.clone()));
    let removed_faces = direct_face_selection(scope, operands)
        .or_else(|| {
            let group = single_operand_group(groups, scope, ROLE_0X10)?;
            Some(FaceSelection::Native(group.id.clone()))
        })
        .or_else(|| bodies.is_some().then(|| FaceSelection::Faces(Vec::new())))?;
    Some(FeatureDefinition::Shell {
        bodies,
        removed_faces,
        thickness: Some(Length(*thickness * 10.0)),
        outward: Some(*outward),
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    })
}

fn project_move(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    let operation = scope.move_operation.as_ref()?;
    let group = single_operand_group(groups, scope, ROLE_0X4)?;
    Some(FeatureDefinition::MoveBody {
        bodies: BodySelection::Native(group.id.clone()),
        translation: Vector3::new(
            operation.transform[0][3] * 10.0,
            operation.transform[1][3] * 10.0,
            operation.transform[2][3] * 10.0,
        ),
        rotation: matrix_axis_angle(&operation.transform),
        copies: 0,
    })
}

pub(crate) fn project_remove_body(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};

    let group = single_operand_group(groups, scope, ROLE_0X4)?;
    Some(FeatureDefinition::DeleteBody {
        bodies: BodySelection::Native(group.id.clone()),
        mode: BodyRetentionMode::DeleteSelected,
    })
}

fn project_base_flange(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    placements: &[DesignSketchPlacement],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ProfileRef, SheetMetalThicknessSide};

    let operation = scope.base_flange_operation.as_ref()?;
    let matching = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let [profile_group] = matching.as_slice() else {
        return None;
    };
    if profile_group.scope_reference_ordinal != 0
        || profile_group.record_index != operation.profile_group_record_index
        || profile_group.role != 0x0000_0041_0000_0000
        || profile_group.members != [operation.profile_record_index]
    {
        return None;
    }
    let profile = scope.base_flange_profile.as_ref()?;
    if profile.scope_reference_ordinal != 1
        || profile.record_index != operation.profile_record_index
    {
        return None;
    }
    let placement = placements.iter().find(|placement| {
        native_stream(&placement.id) == native_stream(&scope.id)
            && placement.entity_id == profile.entity_id
    })?;
    Some(FeatureDefinition::SheetMetalBaseFlange {
        profile: ProfileRef::Sketch(neutral_sketch_id(placement)),
        thickness: Length(operation.thickness * 10.0),
        side: SheetMetalThicknessSide::Forward,
    })
}

/// Project a sheet-metal `EdgeFlange` scope onto its neutral operation.
///
/// The typed operation supplies the bend position, height datum, and inside
/// radius. Owner parameters supply the height, angle, and width. A to-object
/// height resolves its target entity to a known neutral construction feature;
/// otherwise the source selection remains explicit in the neutral height law.
pub(crate) fn project_edge_flange(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeWidthParameterSource,
        DesignEdgeWidthMode, DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, Length, SheetMetalBendPosition, SheetMetalFlangeHeight,
        SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
        SheetMetalHeightDatum,
    };

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
    let operation = scope.edge_flange_operation.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let parameter = |owner_record_index, source_kind: &str| {
        let mut matching = owners.iter().filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && owner.record_index == owner_record_index
        });
        let owner = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        parameters.iter().find(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index
                && parameter.source_kind == source_kind
        })
    };

    let height = match &operation.height_extent {
        DesignEdgeFlangeHeightExtent::Distance => SheetMetalFlangeHeight::Distance(design_length(
            parameter(operation.height_owner_record_index, "FlangeHeight")?,
        )?),
        DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index,
            target_operand_record_index,
            offset_owner_record_index,
            ..
        } => {
            let target_group = groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == scope.record_index
                        && group.record_index == *target_group_record_index
                        && group.role == 0x0000_0021_0000_0000
                        && group.members == [*target_operand_record_index]
                })
                .collect::<Vec<_>>();
            let [target_group] = target_group.as_slice() else {
                return None;
            };
            let target_selections = entity_selection_operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == target_group.record_index
                        && operand.group_member_ordinal == 0
                        && operand.record_index == *target_operand_record_index
                })
                .collect::<Vec<_>>();
            let [target_selection] = target_selections.as_slice() else {
                return None;
            };
            let target_record_index = u32::try_from(target_selection.primary_identity)
                .ok()?
                .checked_add(1)?;
            let target_scopes = scopes
                .iter()
                .filter(|candidate| {
                    native_stream(&candidate.id) == Some(stream)
                        && candidate.record_index == target_record_index
                        && matches!(candidate.kind.as_str(), "WorkPlane" | "WorkPoint")
                })
                .collect::<Vec<_>>();
            let target = match target_scopes.as_slice() {
                [target_scope] => {
                    SheetMetalFlangeHeightTarget::Feature(neutral_feature_id(target_scope))
                }
                [] => SheetMetalFlangeHeightTarget::Native(target_selection.id.clone()),
                _ => return None,
            };
            let offset = design_length(parameter(*offset_owner_record_index, "ToObjectOffset")?)?;
            SheetMetalFlangeHeight::ToObject { target, offset }
        }
    };
    let angle = design_angle(parameter(
        operation.angle_owner_record_index,
        "FlangeAngle",
    )?)?;

    let width_parameter = |owner_record_index, kind| match (operation.width_parameter_source, kind)
    {
        (DesignEdgeFlangeWidthParameterSource::EdgeOffset, "EdgeWidth_1") => {
            parameter(owner_record_index, "EdgeOffset_1")
        }
        (DesignEdgeFlangeWidthParameterSource::EdgeOffset, "EdgeWidth_2") => {
            parameter(owner_record_index, "EdgeOffset_2")
        }
        (DesignEdgeFlangeWidthParameterSource::EdgeWidth, kind) => {
            parameter(owner_record_index, kind)
        }
        _ => None,
    };
    let width_length = |owner_record_index, kind| {
        let length = design_length(width_parameter(owner_record_index, kind)?)?;
        Some(match operation.width_parameter_source {
            DesignEdgeFlangeWidthParameterSource::EdgeWidth => length,
            DesignEdgeFlangeWidthParameterSource::EdgeOffset => Length(length.0.abs()),
        })
    };

    // The width owners are ordered, and their parameter kinds name the mode
    // independently of the owner count, so both must agree.
    let width = match operation.edge_width_mode() {
        DesignEdgeWidthMode::FullEdge
            if operation.width_distance_owner_record_indices.is_empty() =>
        {
            SheetMetalFlangeWidth::FullEdge
        }
        DesignEdgeWidthMode::Symmetric
            if let [owner] = operation.width_distance_owner_record_indices.as_slice() =>
        {
            SheetMetalFlangeWidth::Symmetric {
                width: design_length(parameter(*owner, "EdgeWidth")?)?,
            }
        }
        DesignEdgeWidthMode::SymmetricPerEdge
            if operation.width_distance_owner_record_indices.len()
                == operation.edge_group_record_indices.len() =>
        {
            let widths = operation
                .width_distance_owner_record_indices
                .iter()
                .map(|owner| design_length(parameter(*owner, "EdgeWidth")?))
                .collect::<Option<Vec<_>>>()?;
            let [first, rest @ ..] = widths.as_slice() else {
                return None;
            };
            if rest.iter().any(|width| width != first) {
                return None;
            }
            SheetMetalFlangeWidth::Symmetric { width: *first }
        }
        DesignEdgeWidthMode::TwoSidesPerEdge
            if operation.width_distance_owner_record_indices_by_edge.len()
                == operation.edge_group_record_indices.len() =>
        {
            let flattened_owner_indices = operation
                .width_distance_owner_record_indices_by_edge
                .iter()
                .flat_map(|[first, second]| [*first, *second])
                .collect::<Vec<_>>();
            if flattened_owner_indices != operation.width_distance_owner_record_indices {
                return None;
            }
            let widths = operation
                .width_distance_owner_record_indices_by_edge
                .iter()
                .map(|[first_owner, second_owner]| {
                    Some(SheetMetalFlangeTwoSidedWidth {
                        first: width_length(*first_owner, "EdgeWidth_1")?,
                        second: width_length(*second_owner, "EdgeWidth_2")?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            SheetMetalFlangeWidth::TwoSidesPerEdge { widths }
        }
        DesignEdgeWidthMode::TwoSides => {
            let [first, second] = operation.width_distance_owner_record_indices.as_slice() else {
                return None;
            };
            SheetMetalFlangeWidth::TwoSides {
                first: width_length(*first, "EdgeWidth_1")?,
                second: width_length(*second, "EdgeWidth_2")?,
            }
        }
        _ => return None,
    };

    let height_datum = match operation.height_datum {
        DesignSheetMetalHeightDatum::InnerFaces => SheetMetalHeightDatum::InnerFaces,
        DesignSheetMetalHeightDatum::OuterFaces => SheetMetalHeightDatum::OuterFaces,
        DesignSheetMetalHeightDatum::Unknown(_) => return None,
    };
    let bend_position = match operation.bend_position {
        DesignBendPosition::Outside => SheetMetalBendPosition::Outside,
        DesignBendPosition::Inside => SheetMetalBendPosition::Inside,
        DesignBendPosition::Adjacent => SheetMetalBendPosition::Adjacent,
        DesignBendPosition::TangentToSide => SheetMetalBendPosition::TangentToSide,
        DesignBendPosition::Unknown(_) => return None,
    };

    // Each role-`0x08` group carries one selected edge. The aggregate role-`0x43`
    // group repeats them, so it contributes no separate selection.
    if operation.edge_group_record_indices.is_empty() {
        return None;
    }
    let selections = operation
        .edge_group_record_indices
        .iter()
        .map(|edge_group_record_index| {
            let mut matching = groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && group.record_index == *edge_group_record_index
            });
            let edge_group = matching.next()?;
            if matching.next().is_some()
                || edge_group.role != 0x0000_0008_0000_0000
                || edge_group.members.len() != 1
            {
                return None;
            }
            Some(resolved_edge_flange_group(
                edge_group,
                groups,
                edge_operands,
                edge_identity_operands,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let edges = if selections.len() == 1 {
        selections.into_iter().next()?
    } else {
        merge_edge_selections(scope, selections)
    };

    Some(FeatureDefinition::SheetMetalEdgeFlange {
        edges,
        height,
        angle,
        height_datum,
        bend_position,
        width,
        bend_radius: Length(operation.bend_radius * 10.0),
    })
}

/// Project a sheet-metal `Hem` scope onto its neutral operation.
///
/// The owner layout distinguishes the rolled and teardrop forms from the
/// shared gap-and-length layout. Fold direction is recovered from the signed
/// placement of the inserted bend carriers against the preceding source face;
/// an incomplete transition keeps it unresolved.
pub(crate) fn project_hem(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use crate::records::DesignHemParameterOwners;
    use cadmpeg_ir::features::{
        FeatureDefinition, Length, SheetMetalHemDirection, SheetMetalHemForm,
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
    let operation = scope.hem_operation.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let parameter = |owner_record_index: u32, source_kind: &str| {
        let mut matching_owners = owners.iter().filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && owner.record_index == owner_record_index
        });
        let owner = matching_owners.next()?;
        if matching_owners.next().is_some() {
            return None;
        }
        let mut matching_parameters = parameters.iter().filter(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index
                && parameter.source_kind == source_kind
        });
        let parameter = matching_parameters.next()?;
        matching_parameters.next().is_none().then_some(parameter)
    };

    let form = match &operation.parameter_owners {
        DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        } => SheetMetalHemForm::GapLength {
            gap: design_length(parameter(*gap_owner_record_index, "HemGap")?)?,
            length: design_length(parameter(*length_owner_record_index, "HemLength")?)?,
        },
        DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        } => SheetMetalHemForm::Rolled {
            radius: design_length(parameter(*radius_owner_record_index, "HemRadius")?)?,
            angle: design_angle(parameter(*angle_owner_record_index, "HemAngle")?)?,
        },
        DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        } => SheetMetalHemForm::Teardrop {
            gap: design_length(parameter(*gap_owner_record_index, "HemGap")?)?,
            length: design_length(parameter(*length_owner_record_index, "HemLength")?)?,
            radius: design_length(parameter(*radius_owner_record_index, "HemRadius")?)?,
        },
    };

    let mut edge_groups = groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == operation.edge_group_record_index
    });
    let edge_group = edge_groups.next()?;
    let edge_has_extra = edge_groups.next().is_some();
    let edge_role_ok = edge_group.role == 0x0000_0008_0000_0000;
    let edge_members_ok = edge_group.members == [operation.edge_operand_record_index];
    if edge_has_extra || !edge_role_ok || !edge_members_ok {
        return None;
    }

    let mut aggregate_groups = groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == operation.aggregate_group_record_index
    });
    let aggregate_group = aggregate_groups.next()?;
    let aggregate_has_extra = aggregate_groups.next().is_some();
    let aggregate_role_ok = aggregate_group.role == 0x0000_0043_0000_0000;
    let aggregate_members_ok =
        aggregate_group.members == [operation.aggregate_operand_record_index];
    if aggregate_has_extra || !aggregate_role_ok || !aggregate_members_ok {
        return None;
    }

    let edges = crate::design::edge_resolve::resolved_hem_edge_group(
        edge_group,
        groups,
        edge_operands,
        edge_identity_operands,
        crate::history::effective_scope_previous_history_state_id(scope, histories),
        &neutral_feature_id(scope),
    );

    let edge_slot = edge_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == native_stream(&edge_group.id)
                && operand.scope_record_index == edge_group.scope_record_index
                && operand.record_index == operation.edge_operand_record_index
        })
        .collect::<Vec<_>>();
    let edge_slot = match edge_slot.as_slice() {
        [operand] => crate::design::edge_resolve::resolved_hem_edge_slot(
            operand,
            crate::history::effective_scope_previous_history_state_id(scope, histories),
        ),
        _ => None,
    };
    let semantics = edge_slot
        .map(|edge_slot| crate::history::hem_geometry_semantics(scope, edge_slot, histories));
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

    Some(FeatureDefinition::SheetMetalHem {
        edges,
        form,
        direction,
        bend_radius: Length(operation.bend_radius * 10.0),
    })
}

pub(crate) fn project_surface_stitch(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let operation = scope.surface_stitch_operation.as_ref()?;
    let input_references = scope
        .reference_members
        .get(..scope.reference_members.len() - 2)?;
    let mut matching = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    matching.sort_by_key(|group| group.scope_reference_ordinal);
    if matching.len().checked_mul(2)? != input_references.len()
        || matching.iter().enumerate().any(|(ordinal, group)| {
            u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                || group.record_index != input_references[ordinal * 2]
                || group.members.as_slice() != [input_references[ordinal * 2 + 1]]
                || group.role != ROLE_0X5
        })
    {
        return None;
    }
    Some(FeatureDefinition::KnitSurface {
        faces: FaceSelection::Native(scope.id.clone()),
        merge_entities: Some(true),
        create_solid: Some(true),
        gap_tolerance: Some(Length(operation.gap_tolerance * 10.0)),
    })
}

pub(crate) fn project_ruled_surface(
    scope: &DesignParameterScope,
    owners: &[crate::records::DesignParameterOwner],
    parameters: &[DesignParameter],
    groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use crate::records::{DesignRuledSurfaceCorner, DesignRuledSurfaceMethod};
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, RuledSurfaceCorner, RuledSurfaceMode,
    };

    let operation = scope.ruled_surface_operation.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let parameter = |owner_record_index, source_kind: &str| {
        let mut matching = owners.iter().filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && owner.record_index == owner_record_index
        });
        let owner = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        parameters.iter().find(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index
                && parameter.source_kind == source_kind
        })
    };
    let distance = design_length(parameter(
        operation.distance_owner_record_index,
        "ruledDistance",
    )?)?;
    if distance.0 <= 0.0 {
        return None;
    }
    let angle = design_angle(parameter(operation.angle_owner_record_index, "ruledAngle")?)?;
    let mode = match operation.method {
        DesignRuledSurfaceMethod::Tangent => RuledSurfaceMode::Tangent { distance },
        DesignRuledSurfaceMethod::Normal => RuledSurfaceMode::Normal { distance },
        DesignRuledSurfaceMethod::Direction => return None,
    };
    let mut ordered_groups = Vec::with_capacity(operation.edge_group_record_indices.len());
    for record_index in &operation.edge_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
        });
        let group = matching.next()?;
        if matching.next().is_some()
            || group.role != 0x0000_0008_0000_0000
            || group.members.len() != 1
        {
            return None;
        }
        let reference_ordinal = usize::try_from(group.scope_reference_ordinal).ok()?;
        if scope.reference_members.get(reference_ordinal) != Some(record_index)
            || scope.reference_members.get(reference_ordinal + 1) != group.members.first()
        {
            return None;
        }
        ordered_groups.push(group);
    }
    let selections = ordered_groups
        .iter()
        .map(|group| {
            resolved_edge_group(
                group,
                groups,
                edge_operands,
                edge_identity_operands,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
            )
        })
        .collect::<Vec<_>>();
    let edges = merge_edge_selections(scope, selections);
    Some(FeatureDefinition::RuledSurface {
        edges,
        support_faces: FaceSelection::Native(scope.id.clone()),
        mode,
        angle: Some(angle),
        alternate_face: Some(operation.alternate_face),
        corner: Some(match operation.corner {
            DesignRuledSurfaceCorner::Rounded => RuledSurfaceCorner::Rounded,
            DesignRuledSurfaceCorner::Mitered => RuledSurfaceCorner::Mitered,
        }),
    })
}

fn merge_edge_selections(
    scope: &DesignParameterScope,
    selections: Vec<cadmpeg_ir::features::EdgeSelection>,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    if selections.iter().all(|selection| {
        matches!(
            selection,
            EdgeSelection::Edges(_) | EdgeSelection::Resolved { .. }
        )
    }) {
        let mut resolved = Vec::new();
        for selection in selections {
            let (EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. }) = selection
            else {
                unreachable!("filtered resolved ruled-surface edge selection");
            };
            for edge in edges {
                if resolved.contains(&edge) {
                    return EdgeSelection::Native(scope.id.clone());
                }
                resolved.push(edge);
            }
        }
        return EdgeSelection::Resolved {
            edges: resolved,
            native: scope.id.clone(),
        };
    }
    let state = selections.first().and_then(|selection| match selection {
        EdgeSelection::Historical { state, .. } => Some(state.clone()),
        _ => None,
    });
    if let Some(state) = state {
        if selections.iter().all(|selection| {
            matches!(selection, EdgeSelection::Historical { state: candidate, .. } if candidate == &state)
        }) {
            let mut resolved = Vec::new();
            for selection in selections {
                let EdgeSelection::Historical { edges, .. } = selection else {
                    unreachable!("filtered historical ruled-surface edge selection");
                };
                for edge in edges {
                    if resolved.contains(&edge) {
                        return EdgeSelection::Native(scope.id.clone());
                    }
                    resolved.push(edge);
                }
            }
            return EdgeSelection::Historical {
                state,
                edges: resolved,
                native: scope.id.clone(),
            };
        }
    }
    EdgeSelection::Native(scope.id.clone())
}

pub(crate) fn matrix_axis_angle(
    transform: &[[f64; 4]; 4],
) -> Option<cadmpeg_ir::features::AxisAngle> {
    use cadmpeg_ir::features::{Angle, AxisAngle};

    let trace = transform[0][0] + transform[1][1] + transform[2][2];
    let angle = ((trace - 1.0) * 0.5).clamp(-1.0, 1.0).acos();
    if angle.abs() <= 1.0e-12 {
        return None;
    }
    let (x, y, z) = if (std::f64::consts::PI - angle).abs() <= 1.0e-8 {
        let x = ((transform[0][0] + 1.0) * 0.5).max(0.0).sqrt();
        let y = ((transform[1][1] + 1.0) * 0.5).max(0.0).sqrt()
            * (transform[0][1] + transform[1][0]).signum();
        let z = ((transform[2][2] + 1.0) * 0.5).max(0.0).sqrt()
            * (transform[0][2] + transform[2][0]).signum();
        (x, y, z)
    } else {
        let scale = 2.0 * angle.sin();
        (
            (transform[2][1] - transform[1][2]) / scale,
            (transform[0][2] - transform[2][0]) / scale,
            (transform[1][0] - transform[0][1]) / scale,
        )
    };
    let norm = x.hypot(y).hypot(z);
    (norm > 1.0e-12).then_some(AxisAngle {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(x / norm, y / norm, z / norm),
        angle: Angle(angle),
    })
}

pub(crate) fn direct_face_selection(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    let mut matching = operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == native_stream(&scope.id)
                && operand.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    matching.sort_by_key(|operand| operand.scope_reference_ordinal);
    if matching.is_empty() {
        return None;
    }
    let members = matching
        .iter()
        .map(|operand| (operand.id.as_str(), operand.resolved_face_slots.as_slice()))
        .collect::<Vec<_>>();
    let feature_id = neutral_feature_id(scope);
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    let historical_face = |previous_state_id, slot| {
        ids::history_input_face_id(
            &ids::history_input_prefix(feature_key, previous_state_id),
            slot,
        )
    };
    let faces = match scope.previous_history_state_id {
        Some(previous_state_id) if members.iter().all(|(_, faces)| !faces.is_empty()) => {
            let mut resolved = Vec::new();
            for slot in members.iter().flat_map(|(_, faces)| faces.iter().copied()) {
                let face = historical_face(previous_state_id, slot);
                if !resolved.contains(&face) {
                    resolved.push(face);
                }
            }
            FaceSelection::Historical {
                state: feature_input_topology_id(&feature_id, previous_state_id),
                faces: resolved,
                native: scope.id.clone(),
            }
        }
        Some(previous_state_id) if members.iter().any(|(_, faces)| !faces.is_empty()) => {
            let mut faces = Vec::new();
            let mut unresolved = Vec::new();
            for (identity, slots) in &members {
                if slots.is_empty() {
                    unresolved.push((*identity).to_owned());
                } else {
                    for slot in *slots {
                        let face = historical_face(previous_state_id, *slot);
                        if !faces.contains(&face) {
                            faces.push(face);
                        }
                    }
                }
            }
            FaceSelection::HistoricalPartial {
                state: feature_input_topology_id(&feature_id, previous_state_id),
                faces,
                unresolved,
                native: scope.id.clone(),
            }
        }
        _ => FaceSelection::Native(scope.id.clone()),
    };
    Some(faces)
}

/// Replace a resolved Form scope's native definition with its committed cages.
///
/// The Form's cage-list record owns ordered cage-object references. Each object
/// reaches a surface record, and the serializer naming that surface supplies
/// the archive entry identity of the neutral cage.
pub(crate) fn bind_form_cages(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    features: &mut [cadmpeg_ir::features::Feature],
    cages: &[cadmpeg_ir::subd::SubdSurface],
) -> Result<(), CodecError> {
    for scope in scopes.iter().filter(|scope| scope.kind == "Form") {
        let Some(stream) =
            native_stream(&scope.id).and_then(|stream| stream.strip_prefix(ids::SCHEME_PREFIX))
        else {
            continue;
        };
        let bytes = scan.entry_bytes(stream)?;
        let records = IndexedRecordOffsets::build(bytes);
        let cage_lists = scope
            .reference_members
            .iter()
            .filter_map(|record_index| {
                form_cage_objects(bytes, &records, *record_index, scope.record_index)
            })
            .collect::<Vec<_>>();
        let cage_counts = scope
            .reference_members
            .iter()
            .filter_map(|record_index| {
                form_cage_objects(bytes, &records, *record_index, scope.record_index)
                    .map(|objects| objects.len())
                    .or_else(|| {
                        legacy_form_cage_count(bytes, &records, *record_index, scope.record_index)
                    })
            })
            .collect::<Vec<_>>();
        if scope.class_tag == "325" {
            if let Some(cage_objects) = form_class_325_cage_objects(
                bytes,
                &records,
                scope.record_index,
                &scope.reference_members,
            ) {
                let serializers = form_cage_serializers(bytes, &records);
                let mut resolved = Vec::new();
                let mut valid = true;
                for object in cage_objects {
                    let Some(surface) = form_class_325_cage_surface(bytes, &records, object) else {
                        valid = false;
                        break;
                    };
                    let Some(Some(entry_name)) = serializers.by_surface.get(&surface) else {
                        valid = false;
                        break;
                    };
                    let mut matches = cages.iter().filter(|cage| {
                        cage.source_object
                            .as_ref()
                            .and_then(|source| source.object_id.rsplit('/').next())
                            == Some(entry_name.as_str())
                    });
                    let Some(cage) = matches.next() else {
                        continue;
                    };
                    if matches.next().is_some() {
                        valid = false;
                        break;
                    }
                    resolved.push(cage.id.clone());
                }
                if valid
                    && !resolved.is_empty()
                    && resolved.iter().collect::<HashSet<_>>().len() == resolved.len()
                {
                    let feature_id = neutral_feature_id(scope);
                    if let Some(feature) =
                        features.iter_mut().find(|feature| feature.id == feature_id)
                    {
                        if matches!(
                            &feature.definition,
                            cadmpeg_ir::features::FeatureDefinition::Native { .. }
                        ) {
                            feature.definition =
                                cadmpeg_ir::features::FeatureDefinition::Form { cages: resolved };
                        }
                    }
                }
                continue;
            }
        }
        if scope.class_tag == "328"
            && scopes
                .iter()
                .filter(|candidate| candidate.kind == "Form")
                .count()
                == 1
            && form_class_328_envelope(bytes, &records, scope)
        {
            let serializers = form_cage_serializers(bytes, &records);
            let mut resolved = Vec::with_capacity(serializers.ordered.len());
            let mut valid = true;
            for (_, entry_name) in &serializers.ordered {
                let Some(entry_name) = entry_name else {
                    valid = false;
                    break;
                };
                let mut matches = cages.iter().filter(|cage| {
                    cage.source_object
                        .as_ref()
                        .and_then(|source| source.object_id.rsplit('/').next())
                        == Some(entry_name.as_str())
                });
                let Some(cage) = matches.next() else {
                    continue;
                };
                if matches.next().is_some() {
                    valid = false;
                    break;
                }
                resolved.push(cage.id.clone());
            }
            if valid
                && !resolved.is_empty()
                && resolved.len() == cages.len()
                && resolved.iter().collect::<HashSet<_>>().len() == resolved.len()
            {
                let feature_id = neutral_feature_id(scope);
                if let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id)
                {
                    if matches!(
                        &feature.definition,
                        cadmpeg_ir::features::FeatureDefinition::Native { .. }
                    ) {
                        feature.definition =
                            cadmpeg_ir::features::FeatureDefinition::Form { cages: resolved };
                    }
                }
            }
            continue;
        }
        if scopes
            .iter()
            .filter(|candidate| candidate.kind == "Form")
            .count()
            == 1
            && cages.len() == 1
            && cage_counts.as_slice() == [1]
        {
            let feature_id = neutral_feature_id(scope);
            if let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) {
                if matches!(
                    &feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Native { .. }
                ) {
                    feature.definition = cadmpeg_ir::features::FeatureDefinition::Form {
                        cages: vec![cages[0].id.clone()],
                    };
                }
            }
            continue;
        }
        let [cage_objects] = cage_lists.as_slice() else {
            continue;
        };
        let surfaces = cage_objects
            .iter()
            .map(|object| form_cage_surface(bytes, &records, *object, scope.record_index))
            .collect::<Option<Vec<_>>>();
        let Some(surfaces) = surfaces else {
            continue;
        };
        let serializers = form_cage_serializers(bytes, &records);
        let resolved = surfaces
            .iter()
            .map(|surface| {
                let entry_name = serializers.by_surface.get(surface)?.as_ref()?;
                let mut matches = cages.iter().filter(|cage| {
                    cage.source_object
                        .as_ref()
                        .and_then(|source| source.object_id.rsplit('/').next())
                        == Some(entry_name.as_str())
                });
                let cage = matches.next()?;
                matches.next().is_none().then(|| cage.id.clone())
            })
            .collect::<Option<Vec<_>>>();
        let Some(resolved) = resolved else {
            continue;
        };
        if resolved.iter().collect::<HashSet<_>>().len() != resolved.len() {
            continue;
        }
        let feature_id = neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        if matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Native { .. }
        ) {
            feature.definition = cadmpeg_ir::features::FeatureDefinition::Form { cages: resolved };
        }
    }
    Ok(())
}

/// Read the one-cage envelopes used by legacy Form record generations.
///
/// The compact envelope identifies the sole cage object directly. The older
/// owner envelope identifies one nested cage-object wrapper; its companion
/// record carries generation-specific scalar data and is not a cage count.
fn legacy_form_cage_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<usize> {
    if let Some(count) = legacy_form_owner_count(bytes, records, record_index, scope_record_index) {
        return Some(count);
    }
    for (start, paired) in records.frames(record_index) {
        if paired.checked_sub(start)? != form_cage::LEN
            || bytes.get(start + form_cage::ZERO_RUN_10..start + form_cage::OWNER_MARKER)?
                != [0; 10]
            || bytes.get(start + form_cage::OWNER_MARKER) != Some(&1)
            || View::u64_le_at(bytes, start + form_cage::OWNER_SCOPE_RECORD_INDEX)?
                != u64::from(scope_record_index)
            || bytes.get(start + form_cage::ZERO_RUN_2..start + form_cage::CAGE_COUNT)? != [0; 2]
        {
            continue;
        }
        let count = usize::try_from(View::u32_le_at(bytes, start + form_cage::CAGE_COUNT)?).ok()?;
        if count != 1 {
            continue;
        }
        let member = start + form_cage::MEMBER_MARKER;
        if bytes.get(member) != Some(&1)
            || bytes.get(start + form_cage::MEMBER_ZERO..start + form_cage::MEMBER_FLAGS + 2)?
                != [0, 0, 0xfc, 0]
        {
            continue;
        }
        let object = u32::try_from(View::u64_le_at(
            bytes,
            start + form_cage::CAGE_OBJECT_RECORD_INDEX,
        )?)
        .ok()?;
        if records.offsets(object).is_empty() {
            continue;
        }
        return Some(count);
    }
    None
}

fn legacy_form_owner_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<usize> {
    let frames = records.frames(record_index).collect::<Vec<_>>();
    let [(start, paired)] = frames.as_slice() else {
        return None;
    };
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
    if paired.checked_sub(*start)? != legacy_form_cage::LEN
        || View::u64_le_at(bytes, start + 7)? != u64::from(record_index)
        || bytes
            .get(start + legacy_form_cage::ZERO_RUN_14..start + legacy_form_cage::OWNER_MARKER)?
            != [0; 14]
        || bytes.get(start + legacy_form_cage::OWNER_MARKER) != Some(&1)
        || View::u64_le_at(bytes, start + legacy_form_cage::OWNER_SCOPE_RECORD_INDEX)?
            != u64::from(scope_record_index)
        || bytes
            .get(start + legacy_form_cage::ZERO_RUN_24..start + legacy_form_cage::NESTED_MARKER)?
            != [0; 24]
        || bytes.get(start + legacy_form_cage::NESTED_MARKER) != Some(&1)
        || bytes.get(
            start + legacy_form_cage::NESTED_ZERO_RUN
                ..start + legacy_form_cage::OWNER_REPEAT_MARKER,
        )? != [0; 3]
        || bytes.get(start + legacy_form_cage::OWNER_REPEAT_MARKER) != Some(&1)
        || View::u64_le_at(bytes, start + legacy_form_cage::OWNER_REPEAT_SCOPE)?
            != u64::from(scope_record_index)
        || bytes.get(start + legacy_form_cage::TAIL_ZERO_RUN..start + legacy_form_cage::LEN)?
            != [0; 2]
    {
        return None;
    }
    let nested_record = u32::try_from(View::u64_le_at(
        bytes,
        start + legacy_form_cage::NESTED_RECORD_INDEX,
    )?)
    .ok()?;
    let [nested_at] = records.offsets(nested_record) else {
        return None;
    };
    (bytes.get(nested_at + 4..nested_at + 7) == Some(nested_class)).then_some(1)
}

fn form_class_328_envelope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> bool {
    let Some((scope_start, scope_paired)) = one_indexed_frame(records, scope.record_index) else {
        return false;
    };
    if scope_paired.checked_sub(scope_start) != Some(form_class_328_scope::LEN)
        || bytes.get(scope_start + 4..scope_start + 7) != Some(b"328")
        || bytes.get(scope_paired + 4..scope_paired + 7) != Some(b"267")
    {
        return false;
    }
    if scope.reference_members.len() != 2 {
        return false;
    }
    let Some(group_record) = scope
        .reference_members
        .iter()
        .copied()
        .find(|record_index| {
            records.frames(*record_index).any(|(start, paired)| {
                bytes.get(start + 4..start + 7) == Some(b"417")
                    && bytes.get(paired + 4..paired + 7) == Some(b"267")
            })
        })
    else {
        return false;
    };
    let Some(metadata_record) = scope
        .reference_members
        .iter()
        .copied()
        .find(|record_index| {
            *record_index != group_record
                && records.frames(*record_index).any(|(start, paired)| {
                    bytes.get(start + 4..start + 7) == Some(b"341")
                        && bytes.get(paired + 4..paired + 7) == Some(b"267")
                })
        })
    else {
        return false;
    };
    let Some((group_start, group_paired)) = records.frames(group_record).find(|(start, paired)| {
        bytes.get(*start + 4..*start + 7) == Some(b"417")
            && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
    }) else {
        return false;
    };
    if group_paired.checked_sub(group_start) != Some(form_328_group::LEN)
        || bytes.get(
            group_start + form_328_group::ZERO_RUN_14..group_start + form_328_group::OWNER_MARKER,
        ) != Some(&[0; 14])
        || bytes.get(group_start + form_328_group::OWNER_MARKER)
            != Some(&form_328_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_328_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_328_group::ZERO_RUN_14_AFTER_OWNER
                ..group_start + form_328_group::MEMBER_COUNT,
        ) != Some(&[0; 14])
        || View::u32_le_at(bytes, group_start + form_328_group::MEMBER_COUNT)
            != Some(form_328_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(bytes, group_start + form_328_group::TERMINAL_U32)
            != Some(form_328_group::TERMINAL_U32_VALUE)
        || bytes.get(group_start + form_328_group::FIRST_TAIL_MARKER)
            != Some(&form_328_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_328_group::FIRST_TAIL_ZERO_RUN
                ..group_start + form_328_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(group_start + form_328_group::SECOND_TAIL_MARKER)
            != Some(&form_328_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_328_group::SECOND_TAIL_ZERO_RUN
                ..group_start + form_328_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(group_start + form_328_group::FINAL_SCOPE_MARKER)
            != Some(&form_328_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_328_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes
            .get(group_start + form_328_group::PAIRED_ZERO_TAIL..group_start + form_328_group::LEN)
            != Some(&[0; 2])
    {
        return false;
    }
    let Some(group_class_272) =
        View::u64_le_at(bytes, group_start + form_328_group::FIRST_TAIL_RECORD_INDEX)
            .and_then(|value| u32::try_from(value).ok())
    else {
        return false;
    };
    let Some(group_class_404) = View::u64_le_at(
        bytes,
        group_start + form_328_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, group_class_272, b"272")
        || !unique_record_has_class(bytes, records, group_class_404, b"404")
    {
        return false;
    }
    let mut members = HashSet::new();
    for ordinal in 0..form_328_group::MEMBER_COUNT_VALUE as usize {
        let at = group_start + form_328_group::MEMBER_ENTRIES + ordinal * form_328_entry::LEN;
        if bytes.get(at) != Some(&form_328_entry::MARKER_VALUE)
            || bytes.get(at + form_328_entry::ZERO_TAIL..at + form_328_entry::LEN) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) = View::u64_le_at(bytes, at + form_328_entry::RECORD_INDEX)
            .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if !members.insert(member) {
            return false;
        }
        let Some((member_start, member_paired)) = one_indexed_frame(records, member) else {
            return false;
        };
        if bytes.get(member_start + 4..member_start + 7) != Some(b"350")
            || bytes.get(member_paired + 4..member_paired + 7) != Some(b"351")
            || member_paired < member_start + form_350_tail::LEN
            || bytes.get(member_paired - form_350_tail::LEN + form_350_tail::OWNER_MARKER)
                != Some(&form_350_tail::OWNER_MARKER_VALUE)
            || View::u64_le_at(
                bytes,
                member_paired - form_350_tail::LEN + form_350_tail::OWNER_GROUP_RECORD_INDEX,
            ) != Some(u64::from(group_record))
            || bytes.get(
                member_paired - form_350_tail::LEN + form_350_tail::ZERO_RUN_3
                    ..member_paired - form_350_tail::LEN + form_350_tail::PAIRED_MARKER,
            ) != Some(&[0; 3])
            || bytes.get(member_paired - form_350_tail::LEN + form_350_tail::PAIRED_MARKER)
                != Some(&form_350_tail::PAIRED_MARKER_VALUE)
        {
            return false;
        }
    }
    if members.len() != form_328_group::MEMBER_COUNT_VALUE as usize {
        return false;
    }
    let Some((metadata_start, metadata_paired)) =
        records.frames(metadata_record).find(|(start, paired)| {
            bytes.get(*start + 4..*start + 7) == Some(b"341")
                && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
        })
    else {
        return false;
    };
    if metadata_paired.checked_sub(metadata_start) != Some(form_328_metadata::LEN)
        || bytes.get(
            metadata_start + form_328_metadata::ZERO_RUN_10
                ..metadata_start + form_328_metadata::OWNER_MARKER,
        ) != Some(&[0; 10])
        || bytes.get(metadata_start + form_328_metadata::OWNER_MARKER)
            != Some(&form_328_metadata::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_328_metadata::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_328_metadata::ZERO_RUN_2
                ..metadata_start + form_328_metadata::MEMBER_COUNT,
        ) != Some(&[0; 2])
        || View::u32_le_at(bytes, metadata_start + form_328_metadata::MEMBER_COUNT)
            != Some(form_328_metadata::MEMBER_COUNT_VALUE)
        || View::u32_le_at(bytes, metadata_start + form_328_metadata::TAIL_U32)
            != Some(form_328_metadata::TAIL_U32_VALUE)
        || View::f64_le_at(bytes, metadata_start + form_328_metadata::TAIL_SCALAR)
            .is_none_or(|value| !value.is_finite())
        || bytes.get(metadata_start + form_328_metadata::FIRST_TAIL_MARKER)
            != Some(&form_328_metadata::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_328_metadata::FIRST_TAIL_ZERO_RUN
                ..metadata_start + form_328_metadata::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(metadata_start + form_328_metadata::SECOND_TAIL_MARKER)
            != Some(&form_328_metadata::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_328_metadata::SECOND_TAIL_ZERO_RUN
                ..metadata_start + form_328_metadata::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(metadata_start + form_328_metadata::FINAL_SCOPE_MARKER)
            != Some(&form_328_metadata::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_328_metadata::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_328_metadata::PAIRED_ZERO_TAIL
                ..metadata_start + form_328_metadata::LEN,
        ) != Some(&[0; 2])
    {
        return false;
    }
    let Some(metadata_class_259) = View::u64_le_at(
        bytes,
        metadata_start + form_328_metadata::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Some(metadata_class_404) = View::u64_le_at(
        bytes,
        metadata_start + form_328_metadata::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, metadata_class_259, b"259")
        || !unique_record_has_class(bytes, records, metadata_class_404, b"404")
    {
        return false;
    }
    let mut metadata_members = HashSet::new();
    for ordinal in 0..form_328_metadata::MEMBER_COUNT_VALUE as usize {
        let at = metadata_start + form_328_metadata::MEMBER_ENTRIES + ordinal * form_328_entry::LEN;
        if bytes.get(at) != Some(&form_328_entry::MARKER_VALUE)
            || bytes.get(at + form_328_entry::ZERO_TAIL..at + form_328_entry::LEN) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) = View::u64_le_at(bytes, at + form_328_entry::RECORD_INDEX)
            .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if !metadata_members.insert(member)
            || records.offsets(member).len() != 1
            || records
                .offsets(member)
                .first()
                .and_then(|offset| bytes.get(*offset + 4..*offset + 7))
                != Some(b"320")
        {
            return false;
        }
    }
    metadata_members.len() == form_328_metadata::MEMBER_COUNT_VALUE as usize
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

fn one_indexed_frame(records: &IndexedRecordOffsets, record_index: u32) -> Option<(usize, usize)> {
    let frames = records.frames(record_index).collect::<Vec<_>>();
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

fn form_class_325_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner_record_indices: &[u32],
) -> Option<Vec<u32>> {
    const CAGE_COUNT: usize = 32;
    const TYPE_DISCRIMINATOR_FIRST: u32 = 307;

    let frames = records.frames(scope_record_index).collect::<Vec<_>>();
    let [(start, paired)] = frames.as_slice() else {
        return None;
    };
    let start = *start;
    if bytes.get(start + 4..start + 7) != Some(b"325")
        || bytes.get(*paired + 4..*paired + 7) != Some(b"258")
        || paired.checked_sub(start)? != form_325::LEN
        || bytes.get(start + form_325::ZERO_RUN_9..start + form_325::LIST_MARKER)? != [0; 9]
        || bytes.get(start + form_325::LIST_MARKER) != Some(&1)
        || bytes.get(start + form_325::ZERO_RUN_5..start + form_325::OWNER_MARKER)? != [0; 5]
        || bytes.get(start + form_325::OWNER_MARKER) != Some(&1)
        || bytes.get(start + form_325::ZERO_RUN_2..start + form_325::CAGE_COUNT)? != [0; 2]
    {
        return None;
    }
    let owner_record = u32::try_from(View::u64_le_at(
        bytes,
        start + form_325::OWNER_RESULT_RECORD_INDEX,
    )?)
    .ok()?;
    let [owner_at, ..] = records.offsets(owner_record) else {
        return None;
    };
    if !owner_record_indices.contains(&owner_record)
        || bytes.get(*owner_at + 4..*owner_at + 7) != Some(b"407")
    {
        return None;
    }
    let count = bounded_len(
        u64::from(View::u32_le_at(bytes, start + form_325::CAGE_COUNT)?),
        form_325_entry::LEN,
        paired.checked_sub(start + form_325::CAGE_ENTRIES)?,
    )?;
    if count != CAGE_COUNT {
        return None;
    }
    let mut objects = Vec::with_capacity(count);
    let mut seen_type_discriminators = [false; CAGE_COUNT];
    for ordinal in 0..count {
        let entry = start
            .checked_add(form_325::CAGE_ENTRIES)?
            .checked_add(form_325_entry::LEN.checked_mul(ordinal)?)?;
        if bytes.get(entry + form_325_entry::CAGE_OBJECT_MARKER) != Some(&1)
            || bytes.get(
                entry + form_325_entry::CAGE_OBJECT_ZERO
                    ..entry + form_325_entry::TYPE_DISCRIMINATOR,
            )? != [0, 0]
            || bytes.get(entry + form_325_entry::COMPANION_MARKER) != Some(&1)
            || bytes.get(entry + form_325_entry::COMPANION_ZERO..entry + form_325_entry::LEN)?
                != [0, 0]
        {
            return None;
        }
        let type_discriminator = u32::try_from(View::u64_le_at(
            bytes,
            entry + form_325_entry::TYPE_DISCRIMINATOR,
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
            entry + form_325_entry::CAGE_OBJECT_RECORD_INDEX,
        )?)
        .ok()?;
        let companion = u32::try_from(View::u64_le_at(
            bytes,
            entry + form_325_entry::COMPANION_RECORD_INDEX,
        )?)
        .ok()?;
        let object_frames = records
            .frames(object)
            .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"))
            .collect::<Vec<_>>();
        let [(object_at, _)] = object_frames.as_slice() else {
            return None;
        };
        let [companion_at, ..] = records.offsets(companion) else {
            return None;
        };
        if bytes.get(*object_at + 4..*object_at + 7) != Some(b"289")
            || bytes.get(*companion_at + 4..*companion_at + 7) != Some(b"273")
        {
            return None;
        }
        objects.push(object);
    }
    Some(objects)
}

fn form_class_325_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
) -> Option<u32> {
    let frames = records
        .frames(object_record)
        .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"))
        .collect::<Vec<_>>();
    let [(start, paired)] = frames.as_slice() else {
        return None;
    };
    let start = *start;
    if bytes.get(start + 4..start + 7) != Some(b"289") {
        return None;
    }
    let mut surfaces = Vec::new();
    for at in start.checked_add(11)?..*paired {
        if bytes.get(at) != Some(&1) {
            continue;
        }
        let target = View::u32_le_at(bytes, at + 1)?;
        let [target_at] = records.offsets(target) else {
            continue;
        };
        if bytes.get(target_at + 4..target_at + 7) == Some(b"310") {
            surfaces.push(target);
        }
    }
    let [surface] = surfaces.as_slice() else {
        return None;
    };
    Some(*surface)
}

fn form_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<Vec<u32>> {
    let frames = records
        .frames(record_index)
        .filter(|(_, paired)| matches!(bytes.get(paired + 4..paired + 7), Some(b"258" | b"264")))
        .collect::<Vec<_>>();
    let [(offset, paired)] = frames.as_slice() else {
        return None;
    };
    if View::u64_le_at(bytes, offset + 7)? != record_index as u64
        || bytes.get(offset + 15..offset + 21)? != [0; 6]
        || bytes.get(offset + 21) != Some(&1)
        || View::u64_le_at(bytes, offset + 22)? != scope_record_index as u64
        || bytes.get(offset + 30..offset + 32)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(bytes, offset + 32)?).ok()?;
    if paired.checked_sub(*offset)? != 88usize.checked_add(11usize.checked_mul(count)?)? {
        return None;
    }
    let mut cursor = offset.checked_add(36)?;
    let mut objects = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        objects.push(u32::try_from(View::u64_le_at(bytes, cursor + 1)?).ok()?);
        if bytes.get(cursor + 9..cursor + 11)? != [0, 0] {
            return None;
        }
        cursor = cursor.checked_add(11)?;
    }
    Some(objects)
}

fn form_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
    scope_record: u32,
) -> Option<u32> {
    let [object_at] = records.offsets(object_record) else {
        return None;
    };
    if bytes.get(object_at + 4..object_at + 7) != Some(b"301")
        || next_indexed_record_offset(bytes, object_at + 1)? != object_at + 200
        || bytes.get(object_at + 189) != Some(&1)
    {
        return None;
    }
    let first_wrapper = u32::try_from(View::u64_le_at(bytes, object_at + 190)?).ok()?;
    let [first_at] = records.offsets(first_wrapper) else {
        return None;
    };
    if bytes.get(first_at + 4..first_at + 7) != Some(b"373")
        || next_indexed_record_offset(bytes, first_at + 1)? != first_at + 33
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
        || next_indexed_record_offset(bytes, second_at + 1)? != second_at + 29
        || bytes.get(second_at + 11..second_at + 21)? != [0; 10]
    {
        return None;
    }
    let carrier = u32::try_from(View::u64_le_at(bytes, second_at + 21)?).ok()?;
    let carrier_frames = records.frames(carrier).collect::<Vec<_>>();
    let [(carrier_at, carrier_paired)] = carrier_frames.as_slice() else {
        return None;
    };
    if carrier_paired.checked_sub(*carrier_at)? != 665
        || bytes.get(carrier_at + 4..carrier_at + 7) != Some(b"457")
        || bytes.get(carrier_paired + 4..carrier_paired + 7) != Some(b"264")
        || bytes.get(carrier_at + 317) != Some(&1)
        || View::u64_le_at(bytes, carrier_at + 318)? != scope_record as u64
        || bytes.get(carrier_at + 339) != Some(&1)
    {
        return None;
    }
    u32::try_from(View::u64_le_at(bytes, carrier_at + 340)?).ok()
}

struct FormCageSerializers {
    by_surface: HashMap<u32, Option<String>>,
    ordered: Vec<(u32, Option<String>)>,
}

fn form_cage_serializers(bytes: &[u8], records: &IndexedRecordOffsets) -> FormCageSerializers {
    let mut offsets = records
        .records()
        .flat_map(|(_, offsets)| offsets.iter().copied())
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    let mut by_surface = HashMap::<u32, Option<String>>::new();
    let mut ordered = Vec::<(u32, Option<String>)>::new();
    let mut ordered_positions = HashMap::<u32, usize>::new();
    for offset in offsets {
        let is_class_335 = bytes.get(offset + 4..offset + 7) == Some(b"335");
        if !matches!(
            bytes.get(offset + 4..offset + 7),
            Some(b"315" | b"335" | b"349" | b"360" | b"431" | b"446")
        ) || bytes
            .get(offset + form_serializer::ZERO_RUN_10..offset + form_serializer::ENTRY_NAME_LENGTH)
            != Some(&[0; 10])
        {
            continue;
        }
        let Some(next) = next_indexed_record_offset(bytes, offset + 1) else {
            continue;
        };
        if next != offset + form_serializer::LEN
            || (is_class_335 && bytes.get(next + 4..next + 7) != Some(b"331"))
        {
            continue;
        }
        let Some((entry_name, after_name)) =
            lp_utf16_bounded(bytes, offset + form_serializer::ENTRY_NAME_LENGTH, 1..=256)
        else {
            continue;
        };
        if !entry_name.starts_with("TSpline.")
            || !std::path::Path::new(&entry_name)
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
                .get(after_name + 11..offset + form_serializer::LEN)
                .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
                || !records.offsets(surface).iter().any(|surface_offset| {
                    bytes.get(*surface_offset + 4..*surface_offset + 7) == Some(b"358")
                }))
        {
            continue;
        }
        if !is_class_335 && after_name + 11 != offset + form_serializer::LEN {
            continue;
        }
        if let Some(candidate) = by_surface.get_mut(&surface) {
            *candidate = None;
            if let Some(position) = ordered_positions.get(&surface).copied() {
                ordered[position].1 = None;
            }
        } else {
            ordered_positions.insert(surface, ordered.len());
            by_surface.insert(surface, Some(entry_name.clone()));
            ordered.push((surface, Some(entry_name)));
        }
    }
    FormCageSerializers {
        by_surface,
        ordered,
    }
}

fn normalize_parameter_ordinals(parameters: &mut [cadmpeg_ir::features::DesignParameter]) {
    use cadmpeg_ir::features::{FeatureId, ParameterId};

    let owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    let mut groups = HashMap::<Option<FeatureId>, Vec<usize>>::new();
    for (index, parameter) in parameters.iter().enumerate() {
        groups
            .entry(parameter.owner.clone())
            .or_default()
            .push(index);
    }
    for (owner, indices) in groups {
        let mut ordinals = indices
            .iter()
            .map(|index| parameters[*index].ordinal)
            .collect::<Vec<_>>();
        ordinals.sort_unstable();
        let mut unresolved = indices.into_iter().collect::<HashSet<_>>();
        let mut resolved = HashSet::<ParameterId>::new();
        let mut order = Vec::with_capacity(unresolved.len());
        while !unresolved.is_empty() {
            let mut ready = unresolved
                .iter()
                .copied()
                .filter(|index| {
                    parameters[*index].dependencies.iter().all(|dependency| {
                        owners.get(dependency) != Some(&owner) || resolved.contains(dependency)
                    })
                })
                .collect::<Vec<_>>();
            ready.sort_by(|a, b| {
                let pa = &parameters[*a];
                let pb = &parameters[*b];
                pa.ordinal.cmp(&pb.ordinal).then_with(|| pa.id.cmp(&pb.id))
            });
            if ready.is_empty() {
                let breaker = *unresolved
                    .iter()
                    .min_by_key(|index| {
                        (parameters[**index].ordinal, parameters[**index].id.clone())
                    })
                    .expect("nonempty unresolved parameter group");
                parameters[breaker].dependencies.retain(|dependency| {
                    owners.get(dependency) != Some(&owner) || resolved.contains(dependency)
                });
                ready.push(breaker);
            }
            for index in ready {
                if unresolved.remove(&index) {
                    resolved.insert(parameters[index].id.clone());
                    order.push(index);
                }
            }
        }
        for (index, ordinal) in order.into_iter().zip(ordinals) {
            parameters[index].ordinal = ordinal;
        }
    }
}

pub(crate) fn design_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::features::Length> {
    (parameter.unit.as_deref().is_some_and(design_length_unit)
        && parameter.evaluated_value.is_finite())
    .then_some(cadmpeg_ir::features::Length(
        parameter.evaluated_value * 10.0,
    ))
}

pub(crate) fn design_length_unit(unit: &str) -> bool {
    matches!(unit, "mm" | "cm" | "m" | "in" | "ft")
}

pub(crate) fn design_angle_unit(unit: &str) -> bool {
    matches!(unit, "deg" | "rad")
}

pub(crate) fn design_dimension_unit(parameter: &DesignParameter) -> bool {
    let unit = parameter.unit.as_deref();
    if parameter.source_kind.starts_with("Linear Dimension")
        || parameter.source_kind.starts_with("Radius Dimension")
        || parameter.source_kind.starts_with("Radial Dimension")
        || parameter.source_kind.starts_with("Diameter Dimension")
    {
        return unit.is_some_and(design_length_unit);
    }
    if parameter.source_kind.starts_with("Angular Dimension") {
        return unit.is_some_and(design_angle_unit);
    }
    if parameter.source_kind.starts_with("Tangent Dimension") {
        return unit.is_some_and(design_length_unit);
    }
    false
}

fn project_variable_fillet(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, FilletGroup, RadiusSpec};

    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let [group] = groups.as_slice() else {
        return None;
    };
    let (points, tangency_weight) = variable_fillet_law(parameters)?;
    Some(FeatureDefinition::Fillet {
        groups: vec![FilletGroup {
            edges: resolved_edge_treatment_group_with_corners(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                edge_treatment_vertex_operands,
                histories,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
                None,
            ),
            radius: RadiusSpec::Variable { points },
            tangency_weight,
        }],
    })
}

pub(crate) fn variable_fillet_law(
    parameters: &[(u32, &DesignParameter)],
) -> Option<(Vec<cadmpeg_ir::features::VariableRadius>, Option<f64>)> {
    use cadmpeg_ir::features::VariableRadius;

    let unique_parameter = |kind: &str| {
        let mut matches = parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind == kind).then_some(*parameter));
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let start = design_length(unique_parameter("StartRadius")?)?;
    let end = design_length(unique_parameter("EndRadius")?)?;
    if start.0 < 0.0 || end.0 < 0.0 {
        return None;
    }
    let tangency_weight = {
        let mut matches = parameters.iter().filter_map(|(_, parameter)| {
            (parameter.source_kind == "TangencyWeight").then_some(*parameter)
        });
        match (matches.next(), matches.next()) {
            (None, None) => None,
            (Some(parameter), None) => {
                if parameter.evaluated_value.is_finite() {
                    Some(parameter.evaluated_value)
                } else {
                    return None;
                }
            }
            (None, Some(_)) => return None,
            (Some(_), Some(_)) => return None,
        }
    };
    let mut middle_radii = parameters
        .iter()
        .filter_map(|(ordinal, parameter)| {
            (parameter.source_kind == "MidRadius").then_some((*ordinal, *parameter))
        })
        .collect::<Vec<_>>();
    let mut middle_parameters = parameters
        .iter()
        .filter_map(|(ordinal, parameter)| {
            (parameter.source_kind == "MidParams").then_some((*ordinal, *parameter))
        })
        .collect::<Vec<_>>();
    middle_radii.sort_by_key(|(ordinal, _)| *ordinal);
    middle_parameters.sort_by_key(|(ordinal, _)| *ordinal);
    if middle_radii.len() != middle_parameters.len()
        || parameters.iter().any(|(_, parameter)| {
            !matches!(
                parameter.source_kind.as_str(),
                "StartRadius" | "EndRadius" | "MidRadius" | "MidParams" | "TangencyWeight"
            )
        })
    {
        return None;
    }
    let mut points = Vec::with_capacity(middle_radii.len() + 2);
    points.push(VariableRadius {
        parameter: 0.0,
        radius: start,
    });
    for ((_, radius), (_, parameter)) in middle_radii.into_iter().zip(middle_parameters) {
        let radius = design_length(radius)?;
        let parameter = parameter.evaluated_value;
        if radius.0 < 0.0 || !parameter.is_finite() || !(0.0..1.0).contains(&parameter) {
            return None;
        }
        points.push(VariableRadius { parameter, radius });
    }
    points.push(VariableRadius {
        parameter: 1.0,
        radius: end,
    });
    if !points
        .windows(2)
        .all(|pair| pair[0].parameter < pair[1].parameter)
        || !points.iter().any(|point| point.radius.0 > 0.0)
    {
        return None;
    }
    Some((points, tangency_weight))
}

fn fillet_law_parameter_records(law: &DesignFilletRadiusLaw) -> Vec<u32> {
    match law {
        DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index,
        } => vec![*radius_parameter_record_index],
        DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index,
        } => vec![*chord_length_parameter_record_index],
        DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index,
            offset_two_parameter_record_index,
        } => vec![
            *offset_one_parameter_record_index,
            *offset_two_parameter_record_index,
        ],
        DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index,
            end_radius_parameter_record_index,
            middle_radius_parameter_record_indices,
            middle_parameter_record_indices,
        } => std::iter::once(*start_radius_parameter_record_index)
            .chain(std::iter::once(*end_radius_parameter_record_index))
            .chain(middle_radius_parameter_record_indices.iter().copied())
            .chain(middle_parameter_record_indices.iter().copied())
            .collect(),
    }
}

/// Count parameters whose unit token has no settled neutral quantity kind.
pub(crate) fn untyped_parameter_unit_count(parameters: &[DesignParameter]) -> usize {
    parameters
        .iter()
        .filter(|parameter| {
            parameter
                .unit
                .as_deref()
                .is_some_and(|unit| !design_length_unit(unit) && !design_angle_unit(unit))
        })
        .count()
}

fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{ChamferGroup, ChamferSpec, EdgeSelection, FeatureDefinition};

    let native_scope = native_stream(&scope.id);
    let mut edge_groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_scope
                && group.scope_record_index == scope.record_index
                && group.extrude_role.is_none()
        })
        .collect::<Vec<_>>();
    edge_groups.sort_by_key(|group| group.scope_reference_ordinal);
    let group_count = edge_groups.len().max(1);

    let ordered_parameters = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind == source_kind)
            .copied()
            .collect::<Vec<_>>();
        matches.sort_by_key(|(local_ordinal, _)| *local_ordinal);
        matches
            .into_iter()
            .map(|(_, parameter)| parameter)
            .collect::<Vec<_>>()
    };
    let distances = ordered_parameters("Distance");
    let first_distances = ordered_parameters("Distance 1");
    let second_distances = ordered_parameters("Distance 2");
    let left_distances = ordered_parameters("leftDistance");
    let right_distances = ordered_parameters("rightDistance");
    let mut angles = parameters
        .iter()
        .filter(|(_, parameter)| {
            matches!(
                parameter.source_kind.as_str(),
                "Angle" | "Rotate Angle" | "rotateAngle"
            )
        })
        .copied()
        .collect::<Vec<_>>();
    angles.sort_by_key(|(local_ordinal, _)| *local_ordinal);
    let angles = angles
        .into_iter()
        .map(|(_, parameter)| parameter)
        .collect::<Vec<_>>();

    if !parameters.iter().all(|(_, parameter)| {
        matches!(
            parameter.source_kind.as_str(),
            "Distance"
                | "Distance 1"
                | "Distance 2"
                | "leftDistance"
                | "rightDistance"
                | "Angle"
                | "Rotate Angle"
                | "rotateAngle"
        )
    }) {
        return None;
    }

    let candidates = if !left_distances.is_empty() || !right_distances.is_empty() {
        if !distances.is_empty() || !first_distances.is_empty() || !second_distances.is_empty() {
            return None;
        }
        if right_distances.is_empty() && !angles.is_empty() {
            (left_distances.len() == group_count && angles.len() == group_count).then(|| {
                left_distances
                    .iter()
                    .zip(&angles)
                    .map(|(distance, angle)| {
                        design_length(distance)
                            .zip(design_angle(angle))
                            .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle })
                    })
                    .collect::<Vec<_>>()
            })
        } else if right_distances.is_empty() {
            if !angles.is_empty() {
                return None;
            }
            (left_distances.len() == group_count).then(|| {
                left_distances
                    .iter()
                    .map(|distance| {
                        design_length(distance).map(|distance| ChamferSpec::Distance { distance })
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            if !angles.is_empty() {
                return None;
            }
            (left_distances.len() == group_count && right_distances.len() == group_count).then(
                || {
                    left_distances
                        .iter()
                        .zip(&right_distances)
                        .map(|(first, second)| {
                            design_length(first)
                                .zip(design_length(second))
                                .map(|(first, second)| ChamferSpec::TwoDistances { first, second })
                        })
                        .collect::<Vec<_>>()
                },
            )
        }
    } else if !first_distances.is_empty() || !second_distances.is_empty() {
        if !distances.is_empty() || !angles.is_empty() {
            return None;
        }
        let candidates = (first_distances.len() == group_count
            && second_distances.len() == group_count)
            .then(|| {
                first_distances
                    .iter()
                    .zip(&second_distances)
                    .map(|(first, second)| {
                        design_length(first)
                            .zip(design_length(second))
                            .map(|(first, second)| ChamferSpec::TwoDistances { first, second })
                    })
                    .collect::<Vec<_>>()
            });
        candidates
    } else if !angles.is_empty() {
        if distances.len() != group_count || angles.len() != group_count {
            return None;
        }
        let candidates = Some({
            distances
                .iter()
                .zip(&angles)
                .map(|(distance, angle)| {
                    design_length(distance)
                        .zip(design_angle(angle))
                        .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle })
                })
                .collect::<Vec<_>>()
        });
        candidates
    } else if !distances.is_empty() {
        if distances.len() != group_count {
            return None;
        }
        let candidates = Some({
            distances
                .iter()
                .map(|distance| {
                    design_length(distance).map(|distance| ChamferSpec::Distance { distance })
                })
                .collect::<Vec<_>>()
        });
        candidates
    } else {
        None
    };
    let candidates = candidates?.into_iter().collect::<Option<Vec<_>>>()?;
    if !candidates.iter().all(valid_chamfer_spec) {
        return None;
    }

    let groups = candidates
        .into_iter()
        .enumerate()
        .map(|(index, spec)| {
            let edge_group = edge_groups.get(index).copied();
            ChamferGroup {
                edges: match edge_group {
                    Some(group) => resolved_edge_treatment_group_with_corners(
                        group,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
                        edge_treatment_vertex_operands,
                        histories,
                        scope.previous_history_state_id,
                        &neutral_feature_id(scope),
                        None,
                    ),
                    None => EdgeSelection::Native(scope.id.clone()),
                },
                spec,
            }
        })
        .collect();
    Some(FeatureDefinition::Chamfer {
        groups,
        flip_direction: false,
    })
}

fn project_fixed_chamfer(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{ChamferGroup, ChamferSpec, FeatureDefinition, Length};

    let fixed = scope.fixed_chamfer_parameters.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let [group] = groups.as_slice() else {
        return None;
    };
    let spec = match fixed {
        crate::records::DesignFixedChamferParameters::EqualDistance { distance } => {
            ChamferSpec::Distance {
                distance: Length(distance.value * 10.0),
            }
        }
        crate::records::DesignFixedChamferParameters::TwoDistances { first, second } => {
            ChamferSpec::TwoDistances {
                first: Length(first.value * 10.0),
                second: Length(second.value * 10.0),
            }
        }
    };
    Some(FeatureDefinition::Chamfer {
        groups: vec![ChamferGroup {
            edges: resolved_edge_treatment_group_with_corners(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                edge_treatment_vertex_operands,
                histories,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
                None,
            ),
            spec,
        }],
        flip_direction: false,
    })
}

fn fixed_boolean_operation(operation: DesignExtrudeOperation) -> cadmpeg_ir::features::BooleanOp {
    match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    }
}

pub(crate) fn project_fixed_revolve_with_entities(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    face_operands: &[crate::records::DesignFaceOperand],
    placements: &[DesignSketchPlacement],
    curve_identities: &[SketchCurveIdentity],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        Angle, FeatureDefinition, ProfileRef, RevolutionAxis, RevolutionConstruction,
        RevolveExtent, Termination,
    };

    let DesignPathFeatureConstruction::Revolve {
        operation, angle, ..
    } = scope.path_feature_construction.as_ref()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let profiles = groups
        .iter()
        .filter(|group| group.role == 0x41_0000_0000)
        .collect::<Vec<_>>();
    let axes = groups
        .iter()
        .filter(|group| group.role == 0x21_0000_0000)
        .collect::<Vec<_>>();
    let bodies = groups
        .iter()
        .filter(|group| matches!(group.role, 0x04_0000_0000 | 0x08_0000_0000))
        .collect::<Vec<_>>();
    let ([profile], [axis_group]) = (profiles.as_slice(), axes.as_slice()) else {
        return None;
    };
    let expected_body_groups = usize::from(*operation != DesignExtrudeOperation::NewBody);
    if bodies.len() != expected_body_groups || groups.len() != 2 + expected_body_groups {
        return None;
    }
    let [_profile_member] = profile.members.as_slice() else {
        return None;
    };
    let [axis_member] = axis_group.members.as_slice() else {
        return None;
    };
    let matches = edge_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.record_index == *axis_member
        })
        .collect::<Vec<_>>();
    let axis = if let [axis_operand] = matches.as_slice() {
        Some(RevolutionAxis {
            origin: axis_operand.resolved_axis_origin?,
            direction: axis_operand.resolved_axis_direction?,
        })
    } else if matches.is_empty() {
        let resolved = resolve_sketch_axis_selection(
            scope,
            axis_group,
            *axis_member,
            entity_selection_operands,
            placements,
            curve_identities,
        );
        if resolved.is_none()
            && !unresolved_historical_face_axis_selection(
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
            )
            && revolve_face_axis_operand(scope, axis_group, *axis_member, face_operands).is_none()
        {
            return None;
        }
        resolved
    } else {
        return None;
    };
    Some(FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: Some(ProfileRef::Native(profile.id.clone())),
            axis,
            extent: Some(RevolveExtent::OneSided {
                termination: Termination::Angle {
                    angle: Angle(*angle),
                },
            }),
            axis_reference: None,
            solid: None,
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op: fixed_boolean_operation(*operation),
    })
}

fn unresolved_historical_face_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
) -> bool {
    let stream = native_stream(&scope.id);
    let selections = entity_selection_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == axis_group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index == axis_member
        })
        .collect::<Vec<_>>();
    matches!(selections.as_slice(), [selection] if !selection.historical_face_candidates.is_empty())
}

fn revolve_face_axis_operand<'a>(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    face_operands: &'a [crate::records::DesignFaceOperand],
) -> Option<&'a crate::records::DesignFaceOperand> {
    let stream = native_stream(&scope.id);
    let operands = face_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == Some(axis_group.record_index)
                && operand.group_member_ordinal == Some(0)
                && operand.record_index == axis_member
        })
        .collect::<Vec<_>>();
    let [operand] = operands.as_slice() else {
        return None;
    };
    (!crate::design::face_resolve::historical_face_operand_candidates(operand).is_empty())
        .then_some(*operand)
}

/// Resolve Revolve axes selected through history-qualified analytic faces.
pub(crate) fn bind_revolve_face_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    face_operands: &[crate::records::DesignFaceOperand],
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) {
    use cadmpeg_ir::features::FeatureDefinition;

    for feature in features {
        let FeatureDefinition::Revolve { construction, .. } = &mut feature.definition else {
            continue;
        };
        if construction.axis.is_some() {
            continue;
        }
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let groups = construction_groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && group.role == 0x21_0000_0000
            })
            .collect::<Vec<_>>();
        let [group] = groups.as_slice() else {
            continue;
        };
        let [member] = group.members.as_slice() else {
            continue;
        };
        let selections = entity_selection_operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index == *member
            })
            .collect::<Vec<_>>();
        let entity_face_slot = match selections.as_slice() {
            [selection] => selection
                .historical_face_candidates
                .first()
                .map(|candidate| candidate.face_slot)
                .filter(|slot| {
                    selection
                        .historical_face_candidates
                        .iter()
                        .all(|candidate| candidate.face_slot == *slot)
                }),
            _ => None,
        };
        let entity_axis = entity_face_slot.and_then(|face_slot| {
            analytic_axis_for_face(
                &cadmpeg_ir::ids::FaceId(ids::brep_entity_id(face_slot)),
                faces,
                surfaces,
            )
        });
        let recipe_axis =
            revolve_face_axis_operand(scope, group, *member, face_operands).and_then(|operand| {
                let candidates =
                    crate::design::face_resolve::historical_face_operand_candidates(operand);
                let mut axes = candidates
                    .iter()
                    .map(|face_id| analytic_axis_for_face(face_id, faces, surfaces));
                let first = axes.next().flatten()?;
                axes.all(|axis| {
                    axis.is_some_and(|axis| {
                        crate::history::same_axis_line(
                            (first.origin, first.direction),
                            (axis.origin, axis.direction),
                        )
                    })
                })
                .then_some(first)
            });
        construction.axis = match (entity_axis, recipe_axis) {
            (Some(entity), Some(recipe))
                if crate::history::same_axis_line(
                    (entity.origin, entity.direction),
                    (recipe.origin, recipe.direction),
                ) =>
            {
                Some(entity)
            }
            (Some(axis), None) | (None, Some(axis)) => Some(axis),
            _ => None,
        };
    }
}

fn analytic_axis_for_face(
    face_id: &cadmpeg_ir::ids::FaceId,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let faces = faces
        .iter()
        .filter(|face| &face.id == face_id)
        .collect::<Vec<_>>();
    let [face] = faces.as_slice() else {
        return None;
    };
    let surfaces = surfaces
        .iter()
        .filter(|surface| surface.id == face.surface)
        .collect::<Vec<_>>();
    let [surface] = surfaces.as_slice() else {
        return None;
    };
    analytic_surface_axis(&surface.geometry)
}

fn analytic_surface_axis(
    geometry: &cadmpeg_ir::geometry::SurfaceGeometry,
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let (origin, direction) = match geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => (*origin, *normal),
        SurfaceGeometry::Cylinder { origin, axis, .. }
        | SurfaceGeometry::Cone { origin, axis, .. } => (*origin, *axis),
        SurfaceGeometry::Torus { center, axis, .. } => (*center, *axis),
        _ => return None,
    };
    let length = direction.norm();
    (origin.x.is_finite()
        && origin.y.is_finite()
        && origin.z.is_finite()
        && length.is_finite()
        && length > 0.0)
        .then_some(cadmpeg_ir::features::RevolutionAxis {
            origin,
            direction: direction.scale(1.0 / length),
        })
}

fn resolve_sketch_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
    curve_identities: &[SketchCurveIdentity],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let stream = native_stream(&scope.id)?;
    let selections = entity_selection_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == axis_group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index == axis_member
        })
        .collect::<Vec<_>>();
    let [selection] = selections.as_slice() else {
        return None;
    };
    let owner_reference = u32::try_from(selection.primary_identity).ok()?;
    let placements = placements
        .iter()
        .filter(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_suffix == selection.primary_identity
        })
        .collect::<Vec<_>>();
    let [placement] = placements.as_slice() else {
        return None;
    };
    let curves = curve_identities
        .iter()
        .filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && entity_selection_matches_curve(selection, curve)
        })
        .collect::<Vec<_>>();
    let [curve] = curves.as_slice() else {
        return None;
    };
    let SketchCurveGeometry::Line {
        start, direction, ..
    } = curve.geometry.as_ref()?
    else {
        return None;
    };
    let origin_scale = crate::design::face_resolve::placement_origin_scale(placement);
    let origin = Point3::new(
        placement.transform[0][0] * start.x
            + placement.transform[0][1] * start.y
            + placement.transform[0][2] * start.z
            + placement.transform[0][3] * origin_scale,
        placement.transform[1][0] * start.x
            + placement.transform[1][1] * start.y
            + placement.transform[1][2] * start.z
            + placement.transform[1][3] * origin_scale,
        placement.transform[2][0] * start.x
            + placement.transform[2][1] * start.y
            + placement.transform[2][2] * start.z
            + placement.transform[2][3] * origin_scale,
    );
    let direction = Vector3::new(
        placement.transform[0][0] * direction.x
            + placement.transform[0][1] * direction.y
            + placement.transform[0][2] * direction.z,
        placement.transform[1][0] * direction.x
            + placement.transform[1][1] * direction.y
            + placement.transform[1][2] * direction.z,
        placement.transform[2][0] * direction.x
            + placement.transform[2][1] * direction.y
            + placement.transform[2][2] * direction.z,
    );
    let length = direction.norm();
    (origin.x.is_finite()
        && origin.y.is_finite()
        && origin.z.is_finite()
        && length.is_finite()
        && length > 0.0)
        .then_some(cadmpeg_ir::features::RevolutionAxis {
            origin,
            direction: direction.scale(1.0 / length),
        })
}

pub(crate) fn project_fixed_loft(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    legacy_body_carriers: &[DesignLoftLegacyBodyCarrier],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, LoftPointSection, LoftSection, ProfileRef};

    let DesignPathFeatureConstruction::Loft { operation, .. } =
        scope.path_feature_construction.as_ref()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let matching_legacy_carriers = legacy_body_carriers
        .iter()
        .filter(|carrier| {
            native_stream(&carrier.id) == Some(stream)
                && carrier.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let legacy_body_group_identity = match matching_legacy_carriers.as_slice() {
        [] => None,
        [carrier] => {
            if carrier.scope_reference_ordinal != 0
                || groups.iter().any(|group| group.role == ROLE_0X4)
            {
                return None;
            }
            let mut body_groups = groups
                .iter()
                .filter(|group| group.role == ROLE_0X8 && group.scope_reference_ordinal == 1);
            let body_group = body_groups.next()?;
            if body_groups.next().is_some() {
                return None;
            }
            Some((body_group.record_index, body_group.scope_reference_ordinal))
        }
        _ => return None,
    };
    let is_body_group = |group: &DesignConstructionOperandGroup| {
        group.role == ROLE_0X4
            || legacy_body_group_identity
                == Some((group.record_index, group.scope_reference_ordinal))
    };
    let body_count = groups.iter().filter(|group| is_body_group(group)).count();
    let expected_body_count = usize::from(*operation != DesignExtrudeOperation::NewBody);
    if body_count != expected_body_count {
        return None;
    }
    let operands = groups
        .iter()
        .filter(|group| !is_body_group(group))
        .copied()
        .collect::<Vec<_>>();
    let profile_groups = operands
        .iter()
        .filter(|group| matches!(group.role, 0x41_0000_0000 | 0x43_0000_0000))
        .copied()
        .collect::<Vec<_>>();
    let (sections, guides, centerline) = if profile_groups.len() >= 2 {
        if operands.iter().any(|group| {
            !matches!(
                group.role,
                0x41_0000_0000 | 0x43_0000_0000 | ROLE_0X5 | 0x7_0000_0000
            )
        }) {
            return None;
        }
        let sections = profile_groups
            .iter()
            .map(|group| {
                LoftSection::Profile(
                    resolved_loft_edge_profile_group(scope, group, edge_operands)
                        .or_else(|| resolved_profile_face_group(scope, group, face_operands))
                        .unwrap_or_else(|| ProfileRef::Native(group.id.clone())),
                )
            })
            .collect::<Vec<_>>();
        let guides = operands
            .iter()
            .filter(|group| group.role == ROLE_0X5)
            .map(|group| {
                resolved_loft_path(
                    group,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    scope,
                )
            })
            .collect::<Vec<_>>();
        let centerlines = operands
            .iter()
            .filter(|group| group.role == 0x7_0000_0000)
            .map(|group| {
                resolved_loft_path(
                    group,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    scope,
                )
            })
            .collect::<Vec<_>>();
        let centerline = match centerlines.as_slice() {
            [] => None,
            [centerline] if guides.is_empty() => Some(centerline.clone()),
            _ => return None,
        };
        (sections, guides, centerline)
    } else if *operation == DesignExtrudeOperation::NewBody {
        if profile_groups.len() == 1
            && operands
                .iter()
                .all(|group| matches!(group.role, 0x43_0000_0000 | ROLE_0X5))
        {
            let point_ordinal = operands
                .iter()
                .position(|group| group.role == ROLE_0X5 && group.members.len() == 1)?;
            if !matches!(point_ordinal, 0) && point_ordinal + 1 != operands.len() {
                return None;
            }
            if operands.iter().enumerate().any(|(ordinal, group)| {
                ordinal != point_ordinal && group.role == ROLE_0X5 && group.members.len() == 1
            }) {
                return None;
            }
            (
                operands
                    .iter()
                    .enumerate()
                    .map(|(ordinal, group)| {
                        if ordinal == point_ordinal {
                            LoftSection::Point(LoftPointSection::Native(group.id.clone()))
                        } else {
                            LoftSection::Profile(ProfileRef::Native(group.id.clone()))
                        }
                    })
                    .collect(),
                Vec::new(),
                None,
            )
        } else if profile_groups.is_empty() {
            let role = if operands.iter().all(|group| group.role == 0x41_0000_0000) {
                0x41_0000_0000
            } else if operands.iter().all(|group| group.role == ROLE_0X5) {
                ROLE_0X5
            } else {
                return None;
            };
            (
                operands
                    .iter()
                    .filter(|group| group.role == role)
                    .map(|group| LoftSection::Profile(ProfileRef::Native(group.id.clone())))
                    .collect::<Vec<_>>(),
                Vec::new(),
                None,
            )
        } else {
            return None;
        }
    } else {
        return None;
    };
    if sections.len() < 2
        || sections.len() + guides.len() + usize::from(centerline.is_some()) + body_count
            != groups.len()
    {
        return None;
    }
    Some(FeatureDefinition::Loft {
        sections,
        guides,
        centerline,
        op: fixed_boolean_operation(*operation),
        closed: false,
        solid: true,
        ruled: false,
        max_degree: None,
        check_compatibility: None,
        allow_multi_profile_faces: None,
    })
}

fn resolved_loft_path(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
) -> cadmpeg_ir::features::PathRef {
    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        scope.previous_history_state_id,
        &neutral_feature_id(scope),
    );
    loft_path_from_edge_selection(&group.id, selection)
}

fn resolved_surface_patch_path(
    groups: &[&DesignConstructionOperandGroup],
    all_groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    grouped_recipe: bool,
) -> cadmpeg_ir::features::PathRef {
    use cadmpeg_ir::features::PathRef;

    let paths = groups
        .iter()
        .map(|group| {
            let selection = if grouped_recipe {
                resolved_surface_patch_edge_group(
                    group,
                    all_groups,
                    operands,
                    identity_operands,
                    scope.previous_history_state_id,
                    &neutral_feature_id(scope),
                )
            } else {
                resolved_edge_group(
                    group,
                    all_groups,
                    operands,
                    identity_operands,
                    scope.previous_history_state_id,
                    &neutral_feature_id(scope),
                )
            };
            loft_path_from_edge_selection(&group.id, selection)
        })
        .collect::<Vec<_>>();
    if grouped_recipe {
        if let [path] = paths.as_slice() {
            return path.clone();
        }
    }
    if paths.is_empty() {
        return PathRef::Native(scope.id.clone());
    }
    if let Some(state) = paths.iter().find_map(|path| {
        let PathRef::HistoricalEdges { state, .. } = path else {
            return None;
        };
        Some(state.clone())
    }) {
        if paths.iter().all(|path| {
            matches!(
                path,
                PathRef::HistoricalEdges {
                    state: candidate,
                    ..
                } if *candidate == state
            )
        }) {
            let edges = paths
                .into_iter()
                .flat_map(|path| match path {
                    PathRef::HistoricalEdges { edges, .. } => edges,
                    _ => unreachable!("validated historical SurfacePatch paths"),
                })
                .collect();
            return PathRef::HistoricalEdges {
                state,
                edges,
                native: scope.id.clone(),
            };
        }
    }
    if paths.iter().all(|path| matches!(path, PathRef::Edges(_))) {
        return PathRef::Edges(
            paths
                .into_iter()
                .flat_map(|path| match path {
                    PathRef::Edges(edges) => edges,
                    _ => unreachable!("validated direct SurfacePatch paths"),
                })
                .collect(),
        );
    }
    PathRef::Native(scope.id.clone())
}

pub(crate) fn loft_path_from_edge_selection(
    native: &str,
    selection: cadmpeg_ir::features::EdgeSelection,
) -> cadmpeg_ir::features::PathRef {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};

    match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            PathRef::Edges(edges)
        }
        EdgeSelection::Historical {
            state,
            edges,
            native,
        } => PathRef::HistoricalEdges {
            state,
            edges,
            native,
        },
        EdgeSelection::HistoricalPartial {
            state,
            edges,
            unresolved,
            native,
        } if unresolved.is_empty() && !edges.is_empty() => PathRef::HistoricalEdges {
            state,
            edges,
            native,
        },
        EdgeSelection::All
        | EdgeSelection::Unresolved
        | EdgeSelection::Native(_)
        | EdgeSelection::Generated { .. }
        | EdgeSelection::HistoricalPartial { .. } => PathRef::Native(native.to_owned()),
    }
}

pub(crate) fn project_circular_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, PatternKind, PatternSeed};

    let construction = scope.circular_pattern_construction.as_ref()?;
    let (axis_origin, axis_dir) = circular_pattern_axis(&construction.axis)?;
    let stream = native_stream(&scope.id)?;
    let matching_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0008_0000_0000)
                && !group.members.is_empty()
        })
        .collect::<Vec<_>>();
    let [group] = matching_groups.as_slice() else {
        return None;
    };
    let seed = if group.role == 0x0000_0004_0000_0000 {
        PatternSeed::Faces(
            resolved_historical_face_group(scope, group, face_operands)
                .unwrap_or_else(|| cadmpeg_ir::features::FaceSelection::Native(group.id.clone())),
        )
    } else {
        PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
            group.id.clone(),
        ))
    };
    Some(FeatureDefinition::Pattern {
        seeds: vec![seed],
        pattern: PatternKind::Circular {
            axis_origin,
            axis_dir,
            angle: Angle(construction.angle),
            count: construction.count,
        },
    })
}

fn circular_pattern_axis(
    axis: &crate::records::DesignCircularPatternAxis,
) -> Option<(Point3, Vector3)> {
    use crate::records::DesignCircularPatternAxis;

    match axis {
        DesignCircularPatternAxis::Inline {
            origin, direction, ..
        } => Some((
            Point3::new(origin[0] * 10.0, origin[1] * 10.0, origin[2] * 10.0),
            Vector3::new(direction[0], direction[1], direction[2]),
        )),
        DesignCircularPatternAxis::HistoricalEdge {
            resolved_origin: Some(origin),
            resolved_direction: Some(direction),
            ..
        } => Some((*origin, *direction)),
        DesignCircularPatternAxis::HistoricalEdge { .. } => None,
    }
}

fn project_rectangular_pattern_scalars(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, Length, PatternKind, PatternSeed};

    let construction = scope.rectangular_pattern_construction.as_ref()?;
    let active = [
        (
            construction.u_count,
            construction.u_extent,
            construction.v_count,
        ),
        (
            construction.v_count,
            construction.v_extent,
            construction.u_count,
        ),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 1)
    .collect::<Vec<_>>();
    let [(count, extent, inactive_count)] = active.as_slice() else {
        return None;
    };
    if *inactive_count != 1 {
        return None;
    }
    let direction = construction.instances.as_ref().and_then(|instances| {
        if instances.transforms.len() != usize::try_from(*count).ok()? {
            return None;
        }
        let first = instances.transforms.first()?;
        let last = instances.transforms.last()?;
        let delta = Vector3::new(
            last[0][3] - first[0][3],
            last[1][3] - first[1][3],
            last[2][3] - first[2][3],
        );
        let norm = delta.norm();
        (norm > 0.0).then_some(delta.scale(1.0 / norm))
    });
    let component_seed = construction
        .instances
        .as_ref()
        .and_then(|instances| instances.component_occurrences.as_ref())
        .map(|occurrences| {
            PatternSeed::Occurrences(vec![crate::ids::neutral_component_occurrence_id(
                &occurrences.seed_occurrence_guid,
            )])
        });
    let group_seed = native_stream(&scope.id).and_then(|stream| {
        let matching_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0008_0000_0000)
                    && !group.members.is_empty()
            })
            .collect::<Vec<_>>();
        let [group] = matching_groups.as_slice() else {
            return None;
        };
        Some(if group.role == 0x0000_0004_0000_0000 {
            PatternSeed::Faces(
                resolved_historical_face_group(scope, group, face_operands).unwrap_or_else(|| {
                    cadmpeg_ir::features::FaceSelection::Native(group.id.clone())
                }),
            )
        } else {
            PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
                group.id.clone(),
            ))
        })
    });
    let seeds = component_seed.or(group_seed).into_iter().collect();
    Some(FeatureDefinition::Pattern {
        seeds,
        pattern: PatternKind::Linear {
            direction,
            spacing: Length(extent.abs() * 10.0 / f64::from(count.saturating_sub(1))),
            count: *count,
            second: None,
        },
    })
}

pub(crate) fn project_mirror(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    scopes: &[DesignParameterScope],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, PatternKind, PatternSeed};

    let construction = scope.mirror_construction.as_ref()?;
    if construction.count != 2 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let matching_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let seed_groups = matching_groups
        .iter()
        .copied()
        .filter(|group| {
            group.record_index == construction.seed_group_record_index
                && matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0008_0000_0000)
                && !group.members.is_empty()
        })
        .collect::<Vec<_>>();
    let plane_groups = matching_groups
        .iter()
        .copied()
        .filter(|group| {
            group.record_index == construction.plane_group_record_index
                && group.role == 0x0000_0005_0000_0000
                && group.members.len() == 1
        })
        .collect::<Vec<_>>();
    let ([seed_group], [_plane_group]) = (seed_groups.as_slice(), plane_groups.as_slice()) else {
        return None;
    };
    let seed = if let Some(record_index) = construction.seed_feature_scope_record_index {
        let matching_scopes = scopes
            .iter()
            .filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == record_index
            })
            .collect::<Vec<_>>();
        let [seed_scope] = matching_scopes.as_slice() else {
            return None;
        };
        PatternSeed::Feature(neutral_feature_id(seed_scope))
    } else if seed_group.role == 0x0000_0008_0000_0000 {
        PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
            seed_group.id.clone(),
        ))
    } else {
        PatternSeed::Faces(
            resolved_historical_face_group(scope, seed_group, face_operands).unwrap_or_else(|| {
                cadmpeg_ir::features::FaceSelection::Native(seed_group.id.clone())
            }),
        )
    };
    let (plane_origin, plane_normal, scale_origin) =
        match (construction.plane_origin, construction.plane_normal) {
            (Some(origin), Some(normal)) => (origin, normal, false),
            (None, None) => {
                let plane_scope_record_index = construction.plane_scope_record_index?;
                let matching_planes = scopes
                    .iter()
                    .filter(|candidate| {
                        native_stream(&candidate.id) == Some(stream)
                            && candidate.record_index == plane_scope_record_index
                            && candidate.kind == "WorkPlane"
                            && candidate.work_plane_transform.is_some()
                    })
                    .collect::<Vec<_>>();
                let [plane] = matching_planes.as_slice() else {
                    return None;
                };
                let transform = plane.work_plane_transform?;
                (
                    Point3::new(transform[0][3], transform[1][3], transform[2][3]),
                    Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                    true,
                )
            }
            _ => return None,
        };
    let origin_scale = if scale_origin { 10.0 } else { 1.0 };
    Some(FeatureDefinition::Pattern {
        seeds: vec![seed],
        pattern: PatternKind::Mirror {
            plane_origin: Point3::new(
                plane_origin.x * origin_scale,
                plane_origin.y * origin_scale,
                plane_origin.z * origin_scale,
            ),
            plane_normal,
        },
    })
}

pub(crate) fn project_fixed_sweep(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        Angle, FaceSelection, FeatureDefinition, ProfileRef, SweepGuideRail, SweepMode,
        SweepOrientation, SweepPathExtent,
    };

    let DesignPathFeatureConstruction::Sweep {
        operation, values, ..
    } = scope.path_feature_construction.as_ref()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let profiles = groups
        .iter()
        .filter(|group| group.role == 0x41_0000_0000)
        .collect::<Vec<_>>();
    let mut paths = groups
        .iter()
        .filter(|group| group.role == ROLE_0X5)
        .collect::<Vec<_>>();
    paths.sort_by_key(|group| group.scope_reference_ordinal);
    let bodies = groups
        .iter()
        .filter(|group| group.role == ROLE_0X4)
        .collect::<Vec<_>>();
    let guide_surfaces = groups
        .iter()
        .filter(|group| group.role == 0x11_0000_0000)
        .collect::<Vec<_>>();
    let guide_surface_form = match guide_surfaces.as_slice() {
        [] => false,
        [_] => true,
        _ => return None,
    };
    let profile = if guide_surface_form {
        let sweep_profile = scope.sweep_profile.as_ref()?;
        let carriers = profiles
            .iter()
            .filter(|group| group.members.as_slice() == [sweep_profile.record_index])
            .collect::<Vec<_>>();
        let selections = profiles
            .iter()
            .filter(|group| group.members.as_slice() != [sweep_profile.record_index])
            .collect::<Vec<_>>();
        let ([_carrier], [selection]) = (carriers.as_slice(), selections.as_slice()) else {
            return None;
        };
        if selection.members.is_empty()
            || !selection.members.iter().all(|member| {
                entity_selection_operands.iter().any(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == selection.record_index
                        && operand.record_index == *member
                })
            })
        {
            return None;
        }
        *selection
    } else {
        let [profile] = profiles.as_slice() else {
            return None;
        };
        *profile
    };
    let ([path] | [path, _]) = paths.as_slice() else {
        return None;
    };
    let expected_group_count = profiles.len() + paths.len() + bodies.len() + guide_surfaces.len();
    if bodies.len() > 1
        || groups.len() != expected_group_count
        || (*operation == DesignExtrudeOperation::NewBody && !bodies.is_empty())
        || (guide_surface_form && paths.len() != 1)
        || values[..4].iter().any(|value| !(0.0..=1.0).contains(value))
        || (paths.len() == 1 && values[2..4] != [1.0; 2])
        || !values[4].is_finite()
        || !values[5].is_finite()
    {
        return None;
    }
    let path = resolved_loft_path(
        path,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
    );
    let guide_rail = paths.get(1).map(|rail| SweepGuideRail {
        path: resolved_loft_path(
            rail,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
        ),
        extent: SweepPathExtent {
            along_fraction: values[2],
            against_fraction: values[3],
        },
    });
    let orientation = guide_surfaces
        .first()
        .map(|group| SweepOrientation::GuideSurface {
            faces: resolved_historical_face_group(scope, group, face_operands)
                .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
        });
    Some(FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Native(
            profile.id.clone(),
        )),
        sections: Vec::new(),
        path: Some(path),
        mode: if *operation == DesignExtrudeOperation::NewBody {
            SweepMode::Unresolved
        } else {
            SweepMode::Solid {
                op: fixed_boolean_operation(*operation),
            }
        },
        orientation,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: (values[4] != 0.0).then_some(Angle(values[4])),
        path_extent: Some(SweepPathExtent {
            along_fraction: values[0],
            against_fraction: values[1],
        }),
        guide_rail,
        taper: (values[5] != 0.0).then_some(Angle(values[5])),
        scale: None,
        allow_multi_profile_faces: None,
    })
}

fn project_fixed_pipe(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        BooleanOp, FeatureDefinition, GeneratedSweepSection, SweepMode, SweepSection,
    };

    let DesignPathFeatureConstruction::Pipe {
        operation,
        section_shape,
        filled,
        values,
        record_indexes,
        ..
    } = scope.path_feature_construction.as_ref()?
    else {
        return None;
    };
    if scope.kind != "Pipe"
        || *operation != DesignExtrudeOperation::NewBody
        || *section_shape != 1
        || values[0..2] != [1.0, 1.0]
        || values[2] <= 0.0
        || values[3] <= 0.0
        || parameters.len() != 4
    {
        return None;
    }
    let unique = |source_kind: &str| {
        let matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind == source_kind)
            .map(|(_, parameter)| *parameter)
            .collect::<Vec<_>>();
        let [parameter] = matches.as_slice() else {
            return None;
        };
        Some(*parameter)
    };
    let along = unique("AlongDistance")?;
    let against = unique("AgainstDistance")?;
    let section_size_parameter = unique("SectionSize")?;
    let section_thickness_parameter = unique("SectionThickness")?;
    let section_size = design_length(section_size_parameter)?;
    let section_thickness = design_length(section_thickness_parameter)?;
    if along.unit.is_some()
        || against.unit.is_some()
        || along.evaluated_value != values[0]
        || against.evaluated_value != values[1]
        || section_size_parameter.evaluated_value != values[2]
        || section_thickness_parameter.evaluated_value != values[3]
        || section_size.0 <= 0.0
        || section_thickness.0 <= 0.0
    {
        return None;
    }
    let wall_thickness = if *filled {
        None
    } else if section_thickness.0 < section_size.0 / 2.0 {
        Some(section_thickness)
    } else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let legacy_reference_layout = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("405", "259") | ("421", "257") | ("475", "260")
    );
    let path_group = if legacy_reference_layout {
        if groups.iter().any(|group| group.role != ROLE_0X5) {
            return None;
        }
        let mut legacy_paths = groups.iter().filter(|group| group.role == ROLE_0X5);
        let path_group = legacy_paths.next()?;
        if legacy_paths.next().is_some() {
            return None;
        }
        let mut claimed = record_indexes.iter().copied().collect::<HashSet<_>>();
        if claimed.len() != record_indexes.len()
            || path_group.members.is_empty()
            || !claimed.insert(path_group.record_index)
            || path_group
                .members
                .iter()
                .any(|record_index| !claimed.insert(*record_index))
            || scope.reference_members.len() != path_group.members.len() + 6
            || scope.reference_members.iter().collect::<HashSet<_>>().len()
                != scope.reference_members.len()
            || claimed
                .iter()
                .any(|record_index| !scope.reference_members.contains(record_index))
        {
            return None;
        }
        path_group
    } else {
        let [path_group] = groups.as_slice() else {
            return None;
        };
        if path_group.role != ROLE_0X5
            || path_group.scope_reference_ordinal != 5
            || scope.reference_members.get(5) != Some(&path_group.record_index)
            || path_group.members.is_empty()
            || scope.reference_members.len() != path_group.members.len() + 8
            || path_group.members.as_slice()
                != &scope.reference_members[6..scope.reference_members.len() - 2]
        {
            return None;
        }
        path_group
    };
    let path = resolved_loft_path(
        path_group,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
    );
    Some(FeatureDefinition::Sweep {
        section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
            outer_radius: cadmpeg_ir::features::Length(section_size.0 / 2.0),
            wall_thickness,
        }),
        sections: Vec::new(),
        path: Some(path),
        mode: SweepMode::Solid {
            op: BooleanOp::NewBody,
        },
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
    })
}

fn surface_patch_boundary_continuity(
    continuity: crate::records::DesignPatchContinuity,
) -> Option<cadmpeg_ir::features::SurfaceContinuity> {
    use cadmpeg_ir::features::SurfaceContinuity;

    match continuity {
        crate::records::DesignPatchContinuity::Connected => Some(SurfaceContinuity::Contact),
        crate::records::DesignPatchContinuity::Tangent => Some(SurfaceContinuity::Tangent),
        crate::records::DesignPatchContinuity::Curvature => Some(SurfaceContinuity::Curvature),
        crate::records::DesignPatchContinuity::Unknown(_) => None,
    }
}

/// Map every known boundary-settings continuity in source order.
///
/// A missing or unknown component condition makes the complete per-boundary
/// vector unavailable. The caller can still retain the native scope.
pub(crate) fn surface_patch_boundary_continuities(
    scope: &DesignParameterScope,
) -> Vec<cadmpeg_ir::features::SurfaceContinuity> {
    scope
        .surface_patch_boundaries
        .iter()
        .map(|boundary| surface_patch_boundary_continuity(boundary.continuity))
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

/// Map the boundary-settings continuity of a `SurfacePatch` scope onto one
/// neutral continuity when every boundary uses the same condition.
pub(crate) fn surface_patch_continuity(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::SurfaceContinuity> {
    let boundaries = surface_patch_boundary_continuities(scope);
    let (first, rest) = boundaries.split_first()?;
    if rest.iter().any(|other| other != first) {
        return None;
    }
    Some(*first)
}

pub(crate) fn project_surface_patch(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, SurfaceBoundary};

    if scope.kind != "SurfacePatch" {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);

    // The single-group path form stores the group, all of its ordered edge
    // members, and the tool body. It has no per-component settings records.
    let grouped_path_frame_length = u64::try_from(scope.reference_members.len())
        .ok()?
        .checked_mul(11)?
        .checked_add(277)?;
    if scope.frame_length == grouped_path_frame_length {
        let [group] = groups.as_slice() else {
            return None;
        };
        if scope.reference_members.len() < 3
            || group.scope_reference_ordinal != 0
            || group.record_index != scope.reference_members[0]
            || group.role != ROLE_0X4
            || group.members.is_empty()
            || group.members.as_slice()
                != &scope.reference_members[1..scope.reference_members.len() - 1]
        {
            return None;
        }
        return Some(FeatureDefinition::FilledSurface {
            boundary: SurfaceBoundary::Path(resolved_surface_patch_path(
                std::slice::from_ref(group),
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope,
                true,
            )),
            support_faces: FaceSelection::Faces(Vec::new()),
            continuity: Some(cadmpeg_ir::features::SurfaceContinuity::Contact),
            boundary_continuities: Vec::new(),
            merge_result: Some(false),
        });
    }

    // The reference count separates the two settings-bearing forms: the
    // fixed-path form has `3n + 1` references and the sketch-profile form has
    // three. Frame length does not, because the Design scope envelope has two
    // generations and the later one adds fourteen bytes to both forms.
    let (boundary_count, boundary_role) = if scope.reference_members.len() == 3 {
        (1, 0x0000_0041_0000_0000)
    } else {
        let boundary_count = scope.reference_members.len().checked_sub(1)? / 3;
        if boundary_count == 0 || scope.reference_members.len() != boundary_count * 3 + 1 {
            return None;
        }
        (boundary_count, ROLE_0X4)
    };
    if groups.len() != boundary_count || scope.surface_patch_boundaries.len() != boundary_count {
        return None;
    }
    let mut occupied = vec![false; scope.reference_members.len()];
    for boundary in &groups {
        let group_ordinal = usize::try_from(boundary.scope_reference_ordinal).ok()?;
        let member_ordinal = group_ordinal.checked_add(1)?;
        let settings_ordinal = group_ordinal.checked_add(2)?;
        if settings_ordinal >= scope.reference_members.len()
            || boundary.record_index != scope.reference_members[group_ordinal]
            || boundary.role != boundary_role
            || boundary.members.as_slice()
                != &scope.reference_members[member_ordinal..=member_ordinal]
            || occupied[group_ordinal]
            || occupied[member_ordinal]
            || occupied[settings_ordinal]
        {
            return None;
        }
        let settings = scope.surface_patch_boundaries.iter().find(|settings| {
            usize::try_from(settings.scope_reference_ordinal).ok() == Some(settings_ordinal)
        })?;
        if settings.record_index != scope.reference_members[settings_ordinal]
            || settings.model_reference != boundary.record_index
        {
            return None;
        }
        occupied[group_ordinal] = true;
        occupied[member_ordinal] = true;
        occupied[settings_ordinal] = true;
    }
    let unoccupied = occupied
        .iter()
        .enumerate()
        .filter_map(|(ordinal, occupied)| (!occupied).then_some(ordinal))
        .collect::<Vec<_>>();
    let endpoint_unoccupied = unoccupied.as_slice() == [0]
        || unoccupied.as_slice() == [scope.reference_members.len().saturating_sub(1)];
    if (scope.reference_members.len() == 3 && !unoccupied.is_empty())
        || (scope.reference_members.len() != 3 && !endpoint_unoccupied)
    {
        return None;
    }
    let boundary = if let [boundary] = groups.as_slice() {
        resolved_loft_path(
            boundary,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
        )
    } else {
        resolved_surface_patch_path(
            &groups,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            false,
        )
    };
    Some(FeatureDefinition::FilledSurface {
        boundary: SurfaceBoundary::Path(boundary),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: surface_patch_continuity(scope),
        boundary_continuities: surface_patch_boundary_continuities(scope),
        merge_result: Some(false),
    })
}

pub(crate) fn project_boundary_fill(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    if scope.kind != "BoundaryFill" || scope.reference_members.len() < 5 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let (tools, cells) = groups.split_first()?;
    if tools.scope_reference_ordinal != 0
        || tools.record_index != scope.reference_members[0]
        || tools.role != ROLE_0X4
        || cells.is_empty()
    {
        return None;
    }
    for (index, group) in groups.iter().enumerate() {
        let start = usize::try_from(group.scope_reference_ordinal).ok()?;
        let end = groups
            .get(index + 1)
            .and_then(|next| usize::try_from(next.scope_reference_ordinal).ok())
            .unwrap_or(scope.reference_members.len() - 1);
        if start >= end
            || group.record_index != scope.reference_members[start]
            || group.members.as_slice() != &scope.reference_members[start + 1..end]
            || (index > 0 && group.role != ROLE_0X5)
        {
            return None;
        }
    }
    Some(FeatureDefinition::BoundaryFill {
        tools: BodySelection::Native(tools.id.clone()),
        cells: cells
            .iter()
            .map(|cell| BodySelection::Native(cell.id.clone()))
            .collect(),
    })
}

fn project_hole(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, HoleBottom, HoleKind, Termination,
    };

    if scope.kind != "Hole" || !matches!(parameters.len(), 3 | 5) {
        return None;
    }
    let parameter = |source_kind: &str| {
        let matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind == source_kind)
            .map(|(_, parameter)| *parameter)
            .collect::<Vec<_>>();
        let [parameter] = matches.as_slice() else {
            return None;
        };
        Some(*parameter)
    };
    let depth = design_length(parameter("HoleDepth")?)?;
    let diameter = design_length(parameter("HoleDiameter")?)?;
    let tip_angle = design_angle(parameter("TipAngle")?)?;
    if depth.0 <= 0.0
        || diameter.0 <= 0.0
        || tip_angle.0 <= 0.0
        || tip_angle.0 > std::f64::consts::PI
    {
        return None;
    }
    let counterbore = match parameters.len() {
        3 => None,
        5 => {
            let counterbore_depth = design_length(parameter("CBDepth")?)?;
            let counterbore_diameter = design_length(parameter("CBDiameter")?)?;
            if counterbore_depth.0 <= 0.0
                || counterbore_depth.0 > depth.0
                || counterbore_diameter.0 <= diameter.0
            {
                return None;
            }
            Some((counterbore_diameter, counterbore_depth))
        }
        _ => return None,
    };
    let (kind, bottom) = match (counterbore, tip_angle.0 == std::f64::consts::PI) {
        (None, true) => (HoleKind::Simple, Some(HoleBottom::Flat)),
        (None, false) => (
            HoleKind::SimpleDrilled {
                drill_point_angle: tip_angle,
            },
            None,
        ),
        (Some((diameter, depth)), true) => (
            HoleKind::Counterbore { diameter, depth },
            Some(HoleBottom::Flat),
        ),
        (Some((diameter, depth)), false) => (
            HoleKind::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle: tip_angle,
            },
            None,
        ),
    };
    let face = resolved_direct_face_selection(scope, face_operands)
        .unwrap_or_else(|| FaceSelection::Native(scope.id.clone()));
    let (position, direction) = scope
        .hole_construction
        .as_ref()
        .map(|construction| {
            (
                Point3::new(
                    construction.position[0] * 10.0,
                    construction.position[1] * 10.0,
                    construction.position[2] * 10.0,
                ),
                Vector3::new(
                    construction.direction[0],
                    construction.direction[1],
                    construction.direction[2],
                ),
            )
        })
        .unzip();
    Some(FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: Some(face),
        position,
        direction,
        placements: Vec::new(),
        kind,
        exit_kind: None,
        diameter: Some(diameter),
        extent: Some(Termination::Blind { length: depth }),
        bottom,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    })
}

fn project_replace_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::FeatureDefinition;

    if scope.kind != "ReplaceFace"
        || scope.class_tag != "301"
        || scope.paired_class_tag != "258"
        || scope.frame_length != 290
        || scope.reference_members.len() != 4
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let [replacement_group, target_group] = groups.as_slice() else {
        return None;
    };
    let references = scope.reference_members.as_slice();
    if replacement_group.scope_reference_ordinal != 0
        || replacement_group.record_index != references[0]
        || replacement_group.role != ROLE_0X9
        || replacement_group.members.as_slice() != &references[1..2]
        || target_group.scope_reference_ordinal != 2
        || target_group.record_index != references[2]
        || target_group.role != ROLE_0X10
        || target_group.members.as_slice() != &references[3..4]
    {
        return None;
    }
    let replacements =
        resolved_body_recipe_selection(scope, replacement_group, body_recipe_operands)?;
    let targets = resolved_historical_face_group(scope, target_group, face_operands)?;
    Some(FeatureDefinition::ReplaceFace {
        targets,
        replacements,
    })
}

pub(crate) fn project_split(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};

    if scope.kind != "Split" || scope.reference_members.len() < 4 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let [tool_group, targets] = groups.as_slice() else {
        return None;
    };
    let target_ordinal = tool_group.members.len().checked_add(1)?;
    let tool_members = scope.reference_members.get(1..target_ordinal)?;
    let target_record_index = *scope.reference_members.get(target_ordinal)?;
    let target_members = scope
        .reference_members
        .get(target_ordinal.checked_add(1)?..)?;
    if tool_group.scope_reference_ordinal != 0
        || tool_group.record_index != scope.reference_members[0]
        || tool_group.members.is_empty()
        || tool_group.members.as_slice() != tool_members
        || usize::try_from(targets.scope_reference_ordinal).ok()? != target_ordinal
        || targets.record_index != target_record_index
        || targets.role != ROLE_0X4
        || targets.members.is_empty()
        || targets.members.as_slice() != target_members
    {
        return None;
    }
    let tools = match tool_group.role {
        ROLE_0X9 => {
            let [tool_record_index] = tool_group.members.as_slice() else {
                return None;
            };
            let matching_tools = face_operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.scope_reference_ordinal == 1
                        && operand.record_index == *tool_record_index
                        && operand.recipe_kind == ConstructionRecipeKind::Face
                        && operand.recipe_program.as_slice() == [0, -1]
                        && operand.recipe_nodes.is_empty()
                })
                .collect::<Vec<_>>();
            let [tool] = matching_tools.as_slice() else {
                return None;
            };
            let mut tools = resolved_historical_face_operand(scope, tool)
                .or_else(|| direct_face_selection(scope, face_operands))
                .unwrap_or_else(|| FaceSelection::Native(tool.id.clone()));
            match &mut tools {
                FaceSelection::Resolved { native, .. }
                | FaceSelection::Historical { native, .. }
                | FaceSelection::HistoricalPartial { native, .. } => native.clone_from(&tool.id),
                FaceSelection::Native(native) => native.clone_from(&tool.id),
                _ => {}
            }
            tools
        }
        0x0000_0021_0000_0000 => FaceSelection::Native(tool_group.id.clone()),
        _ => return None,
    };
    Some(FeatureDefinition::SplitBody {
        targets: BodySelection::Native(targets.id.clone()),
        tools,
    })
}

fn project_split_face(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef, SplitFaceTool};

    let reference_count = scope.reference_members.len();
    if scope.kind != "SplitFace" || reference_count < 4 {
        return None;
    }
    let reference_tail_length =
        11_u64.checked_mul(u64::try_from(reference_count.checked_sub(1)?).ok()?)?;
    let frame_base = scope.frame_length.checked_sub(reference_tail_length)?;
    let compact = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("418", "266") | ("277", "258")
    );
    if !(matches!(frame_base, 290 | 291) || compact && frame_base == 286) {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    let [tool, targets] = groups.as_slice() else {
        return None;
    };
    let target_ordinal = tool.members.len().checked_add(1)?;
    if tool.scope_reference_ordinal != 0
        || tool.record_index != scope.reference_members[0]
        || tool.role != ROLE_0X21
        || tool.members.is_empty()
        || tool.members.as_slice() != &scope.reference_members[1..target_ordinal]
        || usize::try_from(targets.scope_reference_ordinal).ok()? != target_ordinal
        || targets.record_index != scope.reference_members[target_ordinal]
        || targets.role != 0x0000_0010_0000_0000
        || targets.members.is_empty()
        || targets.members.as_slice() != &scope.reference_members[target_ordinal + 1..]
    {
        return None;
    }
    let target_selection = project_face_selection(scope, targets, face_operands, histories);
    let tool = resolved_split_face_path(scope, tool, entity_selection_operands, histories)
        .map_or_else(
            || {
                selected_work_planes(scope, tool, entity_selection_operands, scopes).map_or_else(
                    || SplitFaceTool::Path(PathRef::Native(tool.id.clone())),
                    |planes| {
                        let planes = planes
                            .into_iter()
                            .map(neutral_feature_id)
                            .collect::<Vec<_>>();
                        match planes.as_slice() {
                            [plane] => SplitFaceTool::Plane {
                                plane: plane.clone(),
                            },
                            _ => SplitFaceTool::Planes { planes },
                        }
                    },
                )
            },
            SplitFaceTool::Path,
        );
    Some(FeatureDefinition::SplitFace {
        targets: if matches!(target_selection, FaceSelection::Native(_)) {
            FaceSelection::Native(targets.id.clone())
        } else {
            target_selection
        },
        tool,
    })
}

fn project_delete_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let reference_count = scope.reference_members.len();
    let reference_bytes = 11_u64.checked_mul(u64::try_from(reference_count).ok()?)?;
    let base_frame_length = scope.frame_length.checked_sub(reference_bytes)?;
    let base_kind_offset = scope
        .kind_offset
        .checked_sub(scope.byte_offset)?
        .checked_sub(reference_bytes)?;
    let heal = match scope.kind.as_str() {
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
            _ => return None,
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
                return None;
            }
        }
        _ => return None,
    };
    if reference_count < 2 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let matching = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let [group] = matching.as_slice() else {
        return None;
    };
    if group.scope_reference_ordinal != 0
        || group.record_index != scope.reference_members[0]
        || group.role != ROLE_0X10
        || group.members.as_slice() != &scope.reference_members[1..]
    {
        return None;
    }
    let faces = resolved_historical_face_group(scope, group, face_operands)
        .or_else(|| resolved_face_group(group, face_operands))
        .unwrap_or_else(|| FaceSelection::Native(group.id.clone()));
    Some(FeatureDefinition::DeleteFace { faces, heal })
}

pub(crate) fn project_extrude(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
        FaceSelection, FeatureDefinition, Length, ProfileRef, Termination,
    };

    // Per-side terminations without side-local modifiers; drafts and offsets
    // are attached below once they are resolved.
    enum ExtentShape {
        OneSided(Termination),
        Symmetric(Termination),
        TwoSided {
            first: Termination,
            second: Termination,
        },
    }

    #[derive(Clone, Copy)]
    enum AlongDirection {
        SignedDistance,
        PrologueReversal,
    }

    let supported_parameter = |source_kind: &str| {
        matches!(
            source_kind,
            "AlongDistance"
                | "AgainstDistance"
                | "ProfileOffset"
                | "Side1Offset"
                | "Side2Offset"
                | "TaperAngle"
                | "Side2TaperAngle"
        )
    };
    if parameters
        .iter()
        .any(|(_, parameter)| !supported_parameter(&parameter.source_kind))
    {
        return None;
    }
    let scope_groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let profile_groups = extrude_profile_group_roots(scope, construction_groups)?;
    let prologue = scope.extrude_prologue?;
    let profile_ref = match scope.extrude_profile.as_ref() {
        Some(profile) => {
            let placement = placements.iter().find(|placement| {
                native_stream(&placement.id) == native_stream(&scope.id)
                    && placement.entity_id == profile.entity_id
            })?;
            ProfileRef::Sketch(neutral_sketch_id(placement))
        }
        None => {
            let [first, rest @ ..] = profile_groups.as_slice() else {
                return None;
            };
            if rest.is_empty() {
                resolved_extrude_profile_face_group(
                    scope,
                    first,
                    construction_groups,
                    face_operands,
                )
                .unwrap_or_else(|| ProfileRef::Native(first.id.clone()))
            } else {
                let resolved = profile_groups
                    .iter()
                    .map(|group| {
                        resolved_extrude_profile_face_group(
                            scope,
                            group,
                            construction_groups,
                            face_operands,
                        )
                    })
                    .collect::<Option<Vec<_>>>();
                match resolved {
                    Some(selections) => {
                        let mut state = None;
                        let mut faces = Vec::new();
                        let mut native = Vec::new();
                        let complete = selections.into_iter().all(|selection| {
                            let ProfileRef::HistoricalFaces {
                                state: selected_state,
                                faces: selected_faces,
                                native: selected_native,
                            } = selection
                            else {
                                return false;
                            };
                            if state.as_ref().is_some_and(|state| state != &selected_state) {
                                return false;
                            }
                            state = Some(selected_state);
                            for face in selected_faces {
                                if !faces.contains(&face) {
                                    faces.push(face);
                                }
                            }
                            native.extend(selected_native);
                            true
                        });
                        match (complete, state) {
                            (true, Some(state)) if !faces.is_empty() => {
                                ProfileRef::HistoricalFaces {
                                    state,
                                    faces,
                                    native,
                                }
                            }
                            _ => ProfileRef::Native(scope.id.clone()),
                        }
                    }
                    None => ProfileRef::Native(scope.id.clone()),
                }
            }
        }
    };
    let face_groups = scope_groups
        .iter()
        .filter(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Faces))
        .copied()
        .collect::<Vec<_>>();
    let unique = |source_kind: &str| {
        let matches = parameters
            .iter()
            .map(|(_, parameter)| *parameter)
            .filter(|parameter| parameter.source_kind == source_kind)
            .collect::<Vec<_>>();
        (matches.len() <= 1).then(|| matches.first().copied())
    };
    let parameter_along = match unique("AlongDistance")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    let fixed_along = scope
        .fixed_extrude_parameters
        .as_ref()
        .and_then(|fixed| fixed.along_distance.as_ref())
        .map(|fixed| match fixed {
            DesignFixedExtrudeDistance::FixedScalar(scalar) => {
                (Length(scalar.value * 10.0), AlongDirection::SignedDistance)
            }
            DesignFixedExtrudeDistance::DistanceConstruction(scalar) => (
                Length(scalar.value * 10.0),
                AlongDirection::PrologueReversal,
            ),
        });
    let along = match (parameter_along, fixed_along) {
        (Some(parameter), Some((fixed, AlongDirection::SignedDistance)))
            if (parameter.0 - fixed.0).abs() <= 1.0e-9 =>
        {
            Some((parameter, AlongDirection::SignedDistance))
        }
        (Some(distance), None) => Some((distance, AlongDirection::SignedDistance)),
        (None, Some(distance)) => Some(distance),
        (None, None) => None,
        _ => return None,
    };
    let against = match unique("AgainstDistance")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    let profile_offset = match unique("ProfileOffset")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    let side_one_offset = match unique("Side1Offset")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    let side_one_offset_count = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind == "Side1Offset")
        .count();
    let omitted_zero_side_one_offset = side_one_offset.is_none()
        && extrude_omits_zero_side_one_offset(scope, &prologue, side_one_offset_count);
    let face_side_one_offset =
        side_one_offset.or_else(|| omitted_zero_side_one_offset.then_some(Length(0.0)));
    let effective_side_one_offset = side_one_offset.filter(|offset| offset.0 != 0.0);
    let side_two_offset = match unique("Side2Offset")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    let effective_side_two_offset = side_two_offset.filter(|offset| offset.0 != 0.0);
    if effective_side_two_offset.is_some()
        && !matches!(
            scope
                .extrude_prologue
                .and_then(DesignExtrudePrologue::extent),
            Some(
                DesignExtrudeExtent::TwoSidedToFaces | DesignExtrudeExtent::TwoSidedDistanceToFace,
            )
        )
    {
        return None;
    }
    let side_two_draft = match unique("Side2TaperAngle")? {
        Some(parameter) => Some(design_angle(parameter)?),
        None => None,
    };
    let start_groups = face_groups
        .iter()
        .filter(|group| group.extrude_face_role == Some(DesignExtrudeFaceRole::Start))
        .copied()
        .collect::<Vec<_>>();
    let termination_groups = face_groups
        .iter()
        .filter(|group| group.extrude_face_role == Some(DesignExtrudeFaceRole::Termination))
        .copied()
        .collect::<Vec<_>>();
    let first_side_target_ordinal = match prologue {
        DesignExtrudePrologue::ReferenceAware {
            first_side_target_ordinal,
            ..
        } => first_side_target_ordinal.map(|target| target.scope_reference_ordinal),
        DesignExtrudePrologue::LegacyDistance { .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { .. }
        | DesignExtrudePrologue::LegacyShifted { .. } => None,
    };
    let target_shape_groups = scope_groups
        .iter()
        .filter(|group| {
            group.role == 0x0000_0005_0000_0000
                && group.extrude_role.is_none()
                && group.extrude_face_role.is_none()
                && first_side_target_ordinal
                    .is_none_or(|ordinal| group.scope_reference_ordinal == ordinal)
        })
        .copied()
        .collect::<Vec<_>>();
    if start_groups.len() + termination_groups.len() != face_groups.len() {
        return None;
    }
    let start = match prologue.start() {
        DesignExtrudeStart::ProfilePlane if start_groups.is_empty() => {
            if profile_offset.is_some() {
                return None;
            }
            ExtrudeStart::ProfilePlane
        }
        DesignExtrudeStart::OffsetProfilePlane if start_groups.is_empty() => {
            ExtrudeStart::OffsetProfilePlane {
                offset: profile_offset?,
            }
        }
        DesignExtrudeStart::FromFace => {
            let [start] = start_groups.as_slice() else {
                return None;
            };
            let offset = profile_offset?;
            ExtrudeStart::FromFace {
                face: resolved_historical_face_group(scope, start, face_operands)
                    .or_else(|| resolved_face_group(start, face_operands))
                    .unwrap_or_else(|| FaceSelection::Native(start.id.clone())),
                offset: (offset.0 != 0.0).then_some(offset),
            }
        }
        _ => return None,
    };
    let (shape, reverse_direction) = match (prologue.extent()?, along, against) {
        (DesignExtrudeExtent::OneSidedDistance, Some((along, along_direction)), None)
            if along.0 != 0.0
                && (matches!(along_direction, AlongDirection::PrologueReversal)
                    || !prologue.direction_reversed())
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(Termination::Blind {
                    length: Length(along.0.abs()),
                }),
                match along_direction {
                    AlongDirection::SignedDistance => along.0 < 0.0,
                    AlongDirection::PrologueReversal => prologue.direction_reversed(),
                },
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistance,
            Some((along, AlongDirection::SignedDistance)),
            Some(against),
        ) if along.0 != 0.0
            && against.0 != 0.0
            && !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::TwoSided {
                    first: Termination::Blind {
                        length: Length(along.0.abs()),
                    },
                    second: Termination::Blind {
                        length: Length(against.0.abs()),
                    },
                },
                along.0 < 0.0,
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistanceToFace,
            Some((along, AlongDirection::SignedDistance)),
            None,
        ) if along.0 != 0.0
            && !prologue.direction_reversed()
            && termination_groups.len() == 1
            && target_shape_groups.is_empty()
            && effective_side_one_offset.is_none()
            && side_two_offset.is_some() =>
        {
            let [termination] = termination_groups.as_slice() else {
                return None;
            };
            (
                ExtentShape::TwoSided {
                    first: Termination::Blind {
                        length: Length(along.0.abs()),
                    },
                    second: Termination::ToFace {
                        face: resolved_historical_face_group(scope, termination, face_operands)
                            .or_else(|| resolved_face_group(termination, face_operands))
                            .unwrap_or_else(|| FaceSelection::Native(termination.id.clone())),
                        offset: side_two_offset.filter(|offset| offset.0 != 0.0),
                    },
                },
                along.0 < 0.0,
            )
        }
        (DesignExtrudeExtent::TwoSidedToFaces, None, None)
            if termination_groups.len() == 2
                && target_shape_groups.is_empty()
                && side_one_offset.is_some()
                && side_two_offset.is_some() =>
        {
            let [first, second] = termination_groups.as_slice() else {
                return None;
            };
            (
                ExtentShape::TwoSided {
                    first: Termination::ToFace {
                        face: resolved_historical_face_group(scope, first, face_operands)
                            .or_else(|| resolved_face_group(first, face_operands))
                            .unwrap_or_else(|| FaceSelection::Native(first.id.clone())),
                        offset: side_one_offset.filter(|offset| offset.0 != 0.0),
                    },
                    second: Termination::ToFace {
                        face: resolved_historical_face_group(scope, second, face_operands)
                            .or_else(|| resolved_face_group(second, face_operands))
                            .unwrap_or_else(|| FaceSelection::Native(second.id.clone())),
                        offset: side_two_offset.filter(|offset| offset.0 != 0.0),
                    },
                },
                prologue.direction_reversed(),
            )
        }
        (
            DesignExtrudeExtent::SymmetricDistance,
            Some((along, AlongDirection::SignedDistance)),
            None,
        ) if along.0 != 0.0
            && !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::Symmetric(Termination::Blind {
                    length: Length(along.0.abs()),
                }),
                along.0 < 0.0,
            )
        }
        (DesignExtrudeExtent::SymmetricThroughAll, None, None)
            if !prologue.direction_reversed()
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (ExtentShape::Symmetric(Termination::ThroughAll), false)
        }
        (DesignExtrudeExtent::OneSidedToFace, None, None) => {
            match (
                termination_groups.as_slice(),
                target_shape_groups.as_slice(),
            ) {
                ([termination], []) => {
                    let offset = face_side_one_offset?;
                    (
                        ExtentShape::OneSided(Termination::ToFace {
                            face: resolved_historical_face_group(scope, termination, face_operands)
                                .or_else(|| resolved_face_group(termination, face_operands))
                                .unwrap_or_else(|| FaceSelection::Native(termination.id.clone())),
                            offset: (offset.0 != 0.0).then_some(offset),
                        }),
                        prologue.direction_reversed(),
                    )
                }
                ([], [target]) if effective_side_one_offset.is_none() => (
                    ExtentShape::OneSided(Termination::ToShape {
                        target: resolved_body_recipe_shape(scope, target, body_recipe_operands)
                            .unwrap_or_else(|| FaceSelection::Native(target.id.clone())),
                    }),
                    prologue.direction_reversed(),
                ),
                _ => return None,
            }
        }
        (DesignExtrudeExtent::OneSidedThroughNext, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(Termination::ThroughNext),
                prologue.direction_reversed(),
            )
        }
        (DesignExtrudeExtent::OneSidedThroughAll, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(Termination::ThroughAll),
                prologue.direction_reversed(),
            )
        }
        _ => return None,
    };
    let direction = if reverse_direction {
        ExtrudeDirection::ReversedProfileNormal
    } else {
        ExtrudeDirection::ProfileNormal
    };
    let parameter_draft = match unique("TaperAngle")? {
        Some(parameter) => {
            let angle = design_angle(parameter)?;
            Some(angle)
        }
        None => None,
    };
    let fixed_draft = scope
        .fixed_extrude_parameters
        .as_ref()
        .and_then(|fixed| fixed.taper_angle.as_ref())
        .map(|fixed| Angle(fixed.value));
    let draft = match (parameter_draft, fixed_draft) {
        (Some(parameter), Some(fixed)) if (parameter.0 - fixed.0).abs() <= 1.0e-12 => {
            Some(parameter)
        }
        (Some(angle), None) | (None, Some(angle)) => Some(angle),
        (None, None) => None,
        _ => return None,
    }
    .filter(|angle| angle.0 != 0.0);
    let second_draft = side_two_draft.filter(|angle| angle.0 != 0.0);
    // A side-two draft requires a two-sided extent. Other extents have no neutral
    // field for it, so return None.
    if second_draft.is_some() && !matches!(shape, ExtentShape::TwoSided { .. }) {
        return None;
    }
    let extent = match shape {
        ExtentShape::OneSided(termination) => ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination,
                draft,
                offset: None,
            },
        },
        ExtentShape::Symmetric(termination) => ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination,
                draft,
                offset: None,
            },
        },
        ExtentShape::TwoSided { first, second } => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: first,
                draft,
                offset: None,
            },
            second: ExtrudeSide {
                termination: second,
                draft: second_draft,
                offset: None,
            },
        },
    };
    let has_body_operands = scope_groups
        .iter()
        .any(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Bodies));
    let op = match (prologue.operation(), has_body_operands) {
        (DesignExtrudeOperation::Join, true) => BooleanOp::Join,
        (DesignExtrudeOperation::Cut, true) => BooleanOp::Cut,
        (DesignExtrudeOperation::Intersect, true) => BooleanOp::Intersect,
        (DesignExtrudeOperation::NewBody, false) => BooleanOp::NewBody,
        _ => return None,
    };
    Some(FeatureDefinition::Extrude {
        profile: profile_ref,
        direction,
        start,
        extent,
        op,
        direction_source: None,
        solid: Some(prologue.solid_operation()),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn spatial_sketch_entity_endpoints(
    entity: &cadmpeg_ir::sketches::SpatialSketchEntity,
) -> Option<[Point3; 2]> {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;

    match &entity.geometry {
        SpatialSketchGeometry::Line { start, end } => Some([*start, *end]),
        SpatialSketchGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let transverse = normal.cross(*reference_direction);
            let at = |angle: f64| {
                center.translated(
                    reference_direction.scale(angle.cos()) + transverse.scale(angle.sin()),
                    radius.0,
                )
            };
            Some([at(start_angle.0), at(end_angle.0)])
        }
        SpatialSketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => {
            let degree_index = usize::try_from(*degree).ok()?;
            let start = *knots.get(degree_index)?;
            let end = *knots.get(knots.len().checked_sub(degree_index + 1)?)?;
            Some([
                cadmpeg_ir::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    start,
                )?,
                cadmpeg_ir::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    end,
                )?,
            ])
        }
        _ => None,
    }
}

pub(crate) fn closed_spatial_sketch_profiles(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SpatialSketchProfile> {
    use cadmpeg_ir::sketches::{
        SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchProfile,
    };

    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Vec::new();
    }
    let mut profiles = entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && !entity.construction)
        .filter_map(|entity| match &entity.geometry {
            SpatialSketchGeometry::Circle {
                center,
                normal,
                reference_direction,
                ..
            } => Some(SpatialSketchProfile {
                origin: *center,
                normal: *normal,
                u_axis: *reference_direction,
                boundary: vec![SpatialSketchEntityUse {
                    entity: entity.id.clone(),
                    reversed: false,
                }],
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let edges = entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && !entity.construction)
        .filter_map(|entity| spatial_sketch_entity_endpoints(entity).map(|ends| (entity, ends)))
        .collect::<Vec<_>>();
    let close = |a: Point3, b: Point3| (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z) <= tolerance;
    let mut unused = (0..edges.len()).collect::<HashSet<_>>();
    while let Some(&first) = unused
        .iter()
        .min_by_key(|index| edges[**index].0.id.clone())
    {
        unused.remove(&first);
        let mut uses = vec![(first, false)];
        let start = edges[first].1[0];
        let mut end = edges[first].1[1];
        while !close(end, start) {
            let candidates = unused
                .iter()
                .filter_map(|index| {
                    let [candidate_start, candidate_end] = edges[*index].1;
                    if close(end, candidate_start) {
                        Some((*index, false))
                    } else if close(end, candidate_end) {
                        Some((*index, true))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let [next] = candidates.as_slice() else {
                break;
            };
            unused.remove(&next.0);
            uses.push(*next);
            end = if next.1 {
                edges[next.0].1[0]
            } else {
                edges[next.0].1[1]
            };
        }
        let start_degree = edges
            .iter()
            .filter(|(_, [edge_start, edge_end])| {
                close(*edge_start, start) || close(*edge_end, start)
            })
            .count();
        if !close(end, start) || uses.len() < 3 || start_degree != 2 {
            continue;
        }
        let points = uses
            .iter()
            .map(|(index, reversed)| edges[*index].1[usize::from(*reversed)])
            .collect::<Vec<_>>();
        let origin = points[0];
        let mut normal = Vector3::new(0.0, 0.0, 0.0);
        for pair in points[1..].windows(2) {
            let a = pair[0].vector_from(origin);
            let b = pair[1].vector_from(origin);
            normal = normal + a.cross(b);
        }
        let normal_length = normal.norm();
        let first_end = edges[uses[0].0].1[1];
        let u = first_end.vector_from(origin);
        let u_length = u.norm();
        if normal_length <= tolerance || u_length <= tolerance {
            continue;
        }
        normal = normal.scale(1.0 / normal_length);
        let u_axis = u.scale(1.0 / u_length);
        if points
            .iter()
            .any(|point| point.vector_from(origin).dot(normal).abs() > tolerance)
        {
            continue;
        }
        profiles.push(SpatialSketchProfile {
            origin,
            normal,
            u_axis,
            boundary: uses
                .into_iter()
                .map(|(index, reversed)| SpatialSketchEntityUse {
                    entity: edges[index].0.id.clone(),
                    reversed,
                })
                .collect(),
        });
    }
    profiles.sort_by(|a, b| a.boundary[0].entity.cmp(&b.boundary[0].entity));
    profiles
}

fn project_coil(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        BodySelection, BooleanOp, CoilConstruction, CoilExtent, CoilPlacement, CoilResult,
        CoilSection, CoilSectionPlacement, FeatureDefinition,
    };

    let unique = |kind: &str| {
        let mut matches = parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind == kind).then_some(*parameter));
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let diameter = design_length(unique("Diameter")?)?;
    let section_size = design_length(unique("SectionSize")?)?;
    if diameter.0 <= 0.0 || section_size.0 <= 0.0 {
        return None;
    }
    let dimensionless = |kind: &str| {
        let parameter = unique(kind)?;
        (parameter.unit.is_none() && parameter.evaluated_value.is_finite())
            .then_some(parameter.evaluated_value)
    };
    let (extent, taper, expected_parameter_kinds): (_, _, &[&str]) = match scope.coil_extent? {
        DesignCoilExtent::RevolutionsHeight => (
            CoilExtent::RevolutionsHeight {
                revolutions: dimensionless("Revolutions")?,
                height: design_length(unique("Height")?)?,
            },
            design_angle(unique("TaperAngle")?)?,
            &[
                "Diameter",
                "SectionSize",
                "TaperAngle",
                "Revolutions",
                "Height",
            ],
        ),
        DesignCoilExtent::RevolutionsPitch => (
            CoilExtent::RevolutionsPitch {
                revolutions: dimensionless("Revolutions")?,
                pitch: design_length(unique("Pitch")?)?,
            },
            design_angle(unique("TaperAngle")?)?,
            &[
                "Diameter",
                "SectionSize",
                "TaperAngle",
                "Revolutions",
                "Pitch",
            ],
        ),
        DesignCoilExtent::HeightPitch => (
            CoilExtent::HeightPitch {
                height: design_length(unique("Height")?)?,
                pitch: design_length(unique("Pitch")?)?,
            },
            design_angle(unique("TaperAngle")?)?,
            &["Diameter", "SectionSize", "TaperAngle", "Height", "Pitch"],
        ),
        DesignCoilExtent::Spiral => (
            CoilExtent::Spiral {
                revolutions: dimensionless("Revolutions")?,
                radial_pitch: design_length(unique("Pitch")?)?,
            },
            cadmpeg_ir::features::Angle(0.0),
            &["Diameter", "SectionSize", "Revolutions", "Pitch"],
        ),
    };
    if parameters.len() != expected_parameter_kinds.len()
        || parameters.iter().any(|(_, parameter)| {
            !expected_parameter_kinds.contains(&parameter.source_kind.as_str())
        })
    {
        return None;
    }
    let section = match scope.coil_section? {
        DesignCoilSection::Circular => CoilSection::Circular {
            diameter: section_size,
        },
        DesignCoilSection::Square => CoilSection::Square { size: section_size },
        DesignCoilSection::ExternalTriangle => CoilSection::ExternalTriangle { size: section_size },
        DesignCoilSection::InternalTriangle => CoilSection::InternalTriangle { size: section_size },
    };
    let section_placement = match scope.coil_section_placement? {
        DesignCoilSectionPlacement::Inside => CoilSectionPlacement::Inside,
        DesignCoilSectionPlacement::Center => CoilSectionPlacement::Center,
        DesignCoilSectionPlacement::Outside => CoilSectionPlacement::Outside,
    };
    let operation = scope.coil_operation?;
    let stream = native_stream(&scope.id)?;
    let first_body_group = if operation == DesignExtrudeOperation::NewBody {
        // The long Coil form carries one role-4 construction group for its
        // generated body even when the result is a new body. It is not a
        // Boolean target and must not suppress the typed result.
        None
    } else {
        let expected_role = if scope.coil_operation_offset
            == scope.byte_offset.checked_add(coil_long::OPERATION as u64)
        {
            0x0000_0004_0000_0000
        } else {
            0x0000_0008_0000_0000
        };
        let mut body_groups = construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.role == expected_role
        });
        let first_body_group = body_groups.next();
        if body_groups.next().is_some() {
            return None;
        }
        first_body_group
    };
    let result = match (operation, first_body_group) {
        (DesignExtrudeOperation::NewBody, None) => CoilResult::NewBody,
        (operation, Some(group)) => CoilResult::Boolean {
            operation: match operation {
                DesignExtrudeOperation::Join => BooleanOp::Join,
                DesignExtrudeOperation::Cut => BooleanOp::Cut,
                DesignExtrudeOperation::Intersect => BooleanOp::Intersect,
                DesignExtrudeOperation::NewBody => return None,
            },
            targets: BodySelection::Native(group.id.clone()),
        },
        _ => return None,
    };
    let placement = scope
        .coil_placement
        .as_ref()
        .map(|placement| &placement.transform)
        .or_else(|| {
            scope
                .coil_transform
                .as_ref()
                .map(|transform| &transform.transform)
        })
        .map_or_else(
            || CoilPlacement::Native {
                native_ref: scope.id.clone(),
            },
            |transform| CoilPlacement::Explicit {
                origin: Point3::new(
                    transform[0][3] * 10.0,
                    transform[1][3] * 10.0,
                    transform[2][3] * 10.0,
                ),
                axis: Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                radial: Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
            },
        );
    Some(FeatureDefinition::Coil {
        construction: CoilConstruction {
            placement,
            diameter,
            extent,
            section,
            section_placement,
            clockwise: scope.coil_clockwise?,
            taper,
        },
        result,
    })
}

#[cfg(test)]
mod tests;
