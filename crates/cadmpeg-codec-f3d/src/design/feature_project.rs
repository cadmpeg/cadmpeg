// SPDX-License-Identifier: Apache-2.0
//! Project parameter-design features and dispatch per feature family.

use crate::container::ContainerScan;
use crate::design::dimensions::expression_identifiers;
use crate::design::edge_resolve::{
    feature_input_topology_id, project_fixed_fillet, resolved_edge_group,
};
use crate::design::face_resolve::{
    design_angle, resolved_face_group, resolved_profile_face_group, valid_chamfer_spec,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{
    self, native_stream, neutral_feature_id, neutral_parameter_id, neutral_sketch_id,
    neutral_spatial_sketch_id,
};
use crate::records::{
    ConstructionRecipeKind, DesignBodyBinding, DesignCoilExtent, DesignCoilSection,
    DesignCoilSectionPlacement, DesignConstructionOperandGroup, DesignDirectFaceOperation,
    DesignEdgeIdentityOperand, DesignEdgeOperand, DesignExtrudeExtent, DesignExtrudeFaceRole,
    DesignExtrudeOperandRole, DesignExtrudeOperation, DesignExtrudeStart, DesignFaceOperand,
    DesignFilletRadiusGroup, DesignFilletRadiusLaw, DesignParameter, DesignParameterKind,
    DesignParameterOwner, DesignParameterScope, DesignPathFeatureConstruction, DesignRecordHeader,
    DesignSketchPlacement, DesignSolidPrimitive,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{u32_at, u64_at as read_u64};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

/// Design record slices projected together into the neutral construction
/// history: the parameter, owner, and scope tables plus the construction
/// operand, fillet-radius, edge, edge-identity, and face operand records and
/// the sketch placements and body bindings each feature scope resolves against.
pub struct ProjectInputs<'a> {
    pub(crate) native: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(crate) fillet_radius_groups: &'a [DesignFilletRadiusGroup],
    pub(crate) edge_operands: &'a [DesignEdgeOperand],
    pub(crate) edge_identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) face_operands: &'a [DesignFaceOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) body_bindings: &'a [DesignBodyBinding],
}

/// Project parameter scopes and their document- or scope-owned parameters into
/// the neutral construction history.
// Faithful reduced-arg entry point over the same slices as `ProjectInputs`;
// its many test callers pass positional slices, so it defaults the fixed
// edge-identity and body-binding tables and forwards through the bundle.
#[allow(clippy::too_many_arguments)]
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
    project_parameter_design_with_edge_identities(&ProjectInputs {
        native,
        owners,
        scopes,
        construction_groups,
        fillet_radius_groups,
        edge_operands,
        edge_identity_operands: &[],
        face_operands,
        placements,
        body_bindings: &[],
    })
}

/// Project Design parameters and feature scopes, including fixed edge identities.
pub fn project_parameter_design_with_edge_identities(
    inputs: &ProjectInputs<'_>,
) -> (
    Vec<cadmpeg_ir::features::Feature>,
    Vec<cadmpeg_ir::features::DesignParameter>,
) {
    use cadmpeg_ir::features::{
        Angle, DesignParameter as NeutralParameter, DimensionDisplay, Feature, FeatureDefinition,
        Length, ParameterId, ParameterValue, PatternForm, PatternKind,
    };
    use std::collections::BTreeMap;

    let &ProjectInputs {
        native,
        owners,
        scopes,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        face_operands,
        placements,
        body_bindings,
        ..
    } = inputs;

    let scope_ids = scopes
        .iter()
        .filter_map(|scope| {
            Some((
                (native_stream(&scope.id)?, scope.record_index),
                neutral_feature_id(scope),
            ))
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
                Some(DesignFeatureFamily::Extrude) => project_extrude(
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    placements,
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
                Some(DesignFeatureFamily::Revolve) => {
                    project_fixed_revolve(scope, construction_groups, edge_operands).unwrap_or_else(
                        || FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        },
                    )
                }
                Some(DesignFeatureFamily::Loft) => project_fixed_loft(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    face_operands,
                )
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: BTreeMap::new(),
                    properties: native_scope_properties(scope, native_scope),
                }),
                Some(DesignFeatureFamily::Sweep) => project_fixed_sweep(scope, construction_groups)
                    .unwrap_or_else(|| FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: BTreeMap::new(),
                        properties: native_scope_properties(scope, native_scope),
                    }),
                Some(DesignFeatureFamily::SurfacePatch) => {
                    project_surface_patch(scope, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::BoundaryFill) => {
                    project_boundary_fill(scope, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::Split) => {
                    project_split(scope, construction_groups, face_operands).unwrap_or_else(|| {
                        FeatureDefinition::Native {
                            kind: scope.kind.clone(),
                            parameters: BTreeMap::new(),
                            properties: native_scope_properties(scope, native_scope),
                        }
                    })
                }
                Some(DesignFeatureFamily::CircularPattern | DesignFeatureFamily::Mirror) => {
                    FeatureDefinition::Pattern {
                        seeds: Vec::new(),
                        pattern: PatternKind::Unresolved {
                            form: Some(if family == Some(DesignFeatureFamily::CircularPattern) {
                                PatternForm::Circular
                            } else {
                                PatternForm::Mirror
                            }),
                        },
                    }
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
                            center: Some(cadmpeg_ir::features::ScaleCenter::Native(format!(
                                "{native_scope}:design-record#{}",
                                operation.center_record_index
                            ))),
                            factors: cadmpeg_ir::features::ScaleFactors {
                                uniform: Some(operation.uniform_factor),
                                x: None,
                                y: None,
                                z: None,
                            },
                        }
                    },
                ),
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
                            |transform| FeatureDefinition::DatumPlane {
                                origin: Point3::new(
                                    transform[0][3] * 10.0,
                                    transform[1][3] * 10.0,
                                    transform[2][3] * 10.0,
                                ),
                                normal: Vector3::new(
                                    transform[0][2],
                                    transform[1][2],
                                    transform[2][2],
                                ),
                                u_axis: Vector3::new(
                                    transform[0][0],
                                    transform[1][0],
                                    transform[2][0],
                                ),
                            },
                        )
                    } else if scope.kind == "WorkPoint" {
                        scope.work_point_position.map_or_else(
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
                            |position| FeatureDefinition::DatumPoint {
                                position: Point3::new(
                                    position[0] * 10.0,
                                    position[1] * 10.0,
                                    position[2] * 10.0,
                                ),
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
                                    &construction.body_entity_suffixes,
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
                ordinal: scope.byte_offset,
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
    let mut state_features = HashMap::<(&str, i64), Option<cadmpeg_ir::features::FeatureId>>::new();
    for scope in scopes {
        let (Some(stream), Some(state_id)) = (native_stream(&scope.id), scope.history_state_id)
        else {
            continue;
        };
        state_features
            .entry((stream, state_id))
            .and_modify(|feature| *feature = None)
            .or_insert_with(|| scope_ids.get(&(stream, scope.record_index)).cloned());
    }
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let Some(previous_state_id) = scope.previous_history_state_id else {
            continue;
        };
        if let Some(Some(predecessor)) = state_features.get(&(
            native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM),
            previous_state_id,
        )) {
            if predecessor != &feature.id && !feature.dependencies.contains(predecessor) {
                feature.dependencies.push(predecessor.clone());
            }
        }
    }
    features.sort_by_key(|feature| feature.id.clone());
    assign_feature_ordinals(&mut features);

    let mut parameters = native
        .iter()
        .map(|parameter| {
            let mut properties = BTreeMap::new();
            if parameter.kind != DesignParameterKind::User {
                properties.insert("source_kind".into(), parameter.source_kind.clone());
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
                owner: parameter
                    .owner_record_index
                    .and_then(|owner| {
                        owners_by_index.get(&(
                            native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM),
                            owner,
                        ))
                    })
                    .and_then(|owner| {
                        scope_ids.get(&(
                            native_stream(&owner.id).unwrap_or(ids::DEFAULT_STREAM),
                            owner.scope_record_index,
                        ))
                    })
                    .cloned(),
                ordinal: parameter
                    .owner_record_index
                    .and_then(|owner| {
                        owners_by_index.get(&(
                            native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM),
                            owner,
                        ))
                    })
                    .map_or(parameter.source_ordinal, |owner| owner.local_ordinal),
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
    assign_feature_ordinals(&mut features);
    parameters.sort_by_key(|parameter| parameter.id.clone());
    (features, parameters)
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

    let &ProjectInputs {
        native,
        construction_groups,
        fillet_radius_groups,
        edge_operands,
        edge_identity_operands,
        placements,
        ..
    } = inputs;

    if let Some(definition) = project_variable_fillet(
        scope,
        parameters,
        construction_groups,
        edge_operands,
        edge_identity_operands,
    ) {
        definition
    } else if let Some(definition) = parameters
        .is_empty()
        .then(|| {
            project_fixed_fillet(
                scope,
                construction_groups,
                edge_operands,
                edge_identity_operands,
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
                        "Radius" | "ChordLen" | "TangencyWeight"
                    ) || assigned_parameter_records
                        .iter()
                        .filter(|record_index| **record_index == parameter.record_index)
                        .count()
                        != 1
                })
                || parameters.iter().any(|(_, parameter)| {
                    if matches!(parameter.source_kind.as_str(), "Radius" | "ChordLen") {
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
                                resolved_edge_group(
                                    group,
                                    construction_groups,
                                    edge_operands,
                                    edge_identity_operands,
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
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

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
    for feature in features.iter_mut() {
        if let FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch),
            ..
        } = &feature.definition
        {
            if let Some(dependency) = sketch_features.get(sketch) {
                if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                    feature.dependencies.push(dependency.clone());
                }
            }
        }
    }
}

/// Construction-operand-group role integers used to select a single scoped
/// group. Fusion serializes these as opaque tags; their semantics are
/// unresolved (see `docs/formats/f3d-open-items.md`), so the names are neutral.
const ROLE_0X4: u64 = 0x0000_0004_0000_0000;
const ROLE_0X5: u64 = 0x0000_0005_0000_0000;
const ROLE_0X10: u64 = 0x0000_0010_0000_0000;

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
        let group = single_operand_group(groups, scope, ROLE_0X5)?;
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
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let DesignDirectFaceOperation::Shell {
        thickness, outward, ..
    } = scope.direct_face_operation.as_ref()?
    else {
        return None;
    };
    let removed_faces = direct_face_selection(scope, operands).or_else(|| {
        let group = single_operand_group(groups, scope, ROLE_0X10)?;
        Some(FaceSelection::Native(group.id.clone()))
    })?;
    Some(FeatureDefinition::Shell {
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
/// The Form's cage-list record owns an ordered list of cage-object references.
/// Archive cage order is used only when one Form owns every active cage; a
/// document with multiple Form scopes remains native until the object records
/// can provide an identity join.
pub(crate) fn bind_form_cages(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    features: &mut [cadmpeg_ir::features::Feature],
    cages: &[cadmpeg_ir::subd::SubdSurface],
) -> Result<(), CodecError> {
    let form_scopes = scopes
        .iter()
        .filter(|scope| scope.kind == "Form")
        .collect::<Vec<_>>();
    let [scope] = form_scopes.as_slice() else {
        return Ok(());
    };
    let Some(stream) =
        native_stream(&scope.id).and_then(|stream| stream.strip_prefix(ids::SCHEME_PREFIX))
    else {
        return Ok(());
    };
    let bytes = scan.entry_bytes(stream)?;
    let owned_count = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            headers.iter().find(|header| {
                native_stream(&header.id) == native_stream(&scope.id)
                    && header.record_index == *record_index
            })
        })
        .find_map(|header| {
            form_cage_count(
                bytes,
                header.byte_offset as usize,
                header.record_index,
                scope.record_index,
            )
        });
    if owned_count != Some(cages.len()) || cages.is_empty() {
        return Ok(());
    }
    let feature_id = neutral_feature_id(scope);
    let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
        return Ok(());
    };
    if matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } if kind == "Form"
    ) {
        feature.definition = cadmpeg_ir::features::FeatureDefinition::Form {
            cages: cages.iter().map(|cage| cage.id.clone()).collect(),
        };
    }
    Ok(())
}

fn form_cage_count(
    bytes: &[u8],
    offset: usize,
    expected_record_index: u32,
    scope_record_index: u32,
) -> Option<usize> {
    let class_length = u32_at(bytes, offset)? as usize;
    let class_start = offset.checked_add(4)?;
    let class_end = class_start.checked_add(class_length)?;
    bytes.get(class_start..class_end)?;
    let record_index = read_u64(bytes, class_end)?;
    let prefix = bytes.get(class_end + 8..class_end + 14)?;
    if record_index != expected_record_index as u64
        || prefix != [0; 6]
        || bytes.get(class_end + 14) != Some(&1)
        || read_u64(bytes, class_end + 15)? != scope_record_index as u64
        || bytes.get(class_end + 23..class_end + 25)? != [0, 0]
    {
        return None;
    }
    let count = u32_at(bytes, class_end + 25)? as usize;
    let mut cursor = class_end.checked_add(29)?;
    for _ in 0..count {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        read_u64(bytes, cursor + 1)?;
        if bytes.get(cursor + 9..cursor + 11)? != [0, 0] {
            return None;
        }
        cursor = cursor.checked_add(11)?;
    }
    Some(count)
}

#[cfg(test)]
mod form_tests {
    #[test]
    fn reads_owned_cage_count() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"402");
        bytes.extend_from_slice(&2196u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.push(1);
        bytes.extend_from_slice(&2190u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for reference in [8300u64, 8303] {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 2]);
        }
        assert_eq!(super::form_cage_count(&bytes, 0, 2196, 2190), Some(2));
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
            ready.sort_by_key(|index| (parameters[*index].ordinal, parameters[*index].id.clone()));
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

fn assign_feature_ordinals(features: &mut [cadmpeg_ir::features::Feature]) {
    let indices = features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut assigned = HashSet::new();
    let mut order = Vec::with_capacity(features.len());
    while order.len() < features.len() {
        let candidate = features
            .iter()
            .enumerate()
            .filter(|(_, feature)| !assigned.contains(&feature.id))
            .filter(|(_, feature)| {
                feature.dependencies.iter().all(|dependency| {
                    !indices.contains_key(dependency) || assigned.contains(dependency)
                })
            })
            .min_by_key(|(_, feature)| (feature.ordinal, feature.id.clone()))
            .map(|(index, feature)| (index, feature.id.clone()));
        let Some((index, id)) = candidate else {
            return;
        };
        assigned.insert(id);
        order.push(index);
    }
    for (ordinal, index) in order.into_iter().enumerate() {
        features[index].ordinal = ordinal as u64;
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
        || parameter.source_kind.starts_with("Diameter Dimension")
    {
        return unit.is_some_and(design_length_unit);
    }
    if parameter.source_kind.starts_with("Angular Dimension") {
        return unit.is_some_and(design_angle_unit);
    }
    false
}

fn project_variable_fillet(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
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
            edges: resolved_edge_group(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
                None,
            ),
            radius: RadiusSpec::Variable { points },
            tangency_weight: Some(tangency_weight),
        }],
    })
}

pub(crate) fn variable_fillet_law(
    parameters: &[(u32, &DesignParameter)],
) -> Option<(Vec<cadmpeg_ir::features::VariableRadius>, f64)> {
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
    let tangency_weight = unique_parameter("TangencyWeight")?.evaluated_value;
    if !tangency_weight.is_finite() {
        return None;
    }
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
    let angles = ordered_parameters("Angle");

    if !parameters.iter().all(|(_, parameter)| {
        matches!(
            parameter.source_kind.as_str(),
            "Distance" | "Distance 1" | "Distance 2" | "Angle"
        )
    }) {
        return None;
    }

    let candidates = if !first_distances.is_empty() || !second_distances.is_empty() {
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
                    Some(group) => resolved_edge_group(
                        group,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
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
    Some(FeatureDefinition::Chamfer {
        groups: vec![ChamferGroup {
            edges: resolved_edge_group(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
                None,
            ),
            spec: ChamferSpec::Distance {
                distance: Length(fixed.distance * 10.0),
            },
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

pub(crate) fn project_fixed_revolve(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        Angle, Extent, FeatureDefinition, ProfileRef, RevolutionAxis, RevolutionConstruction,
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
    let ([profile], [axis_group]) = (profiles.as_slice(), axes.as_slice()) else {
        return None;
    };
    if groups.len() != 2 {
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
    let [axis_operand] = matches.as_slice() else {
        return None;
    };
    let axis = RevolutionAxis {
        origin: axis_operand.resolved_axis_origin?,
        direction: axis_operand.resolved_axis_direction?,
    };
    Some(FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: Some(ProfileRef::Native(profile.id.clone())),
            axis: Some(axis),
            extent: Some(Extent::Angle {
                angle: Angle(*angle),
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

pub(crate) fn project_fixed_loft(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
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
    let body_count = groups.iter().filter(|group| group.role == ROLE_0X4).count();
    let (sections, guides, centerline) = match operation {
        DesignExtrudeOperation::Join if body_count == 1 => (
            groups
                .iter()
                .filter(|group| group.role == 0x41_0000_0000)
                .map(|group| LoftSection::Profile(ProfileRef::Native(group.id.clone())))
                .collect::<Vec<_>>(),
            Vec::new(),
            None,
        ),
        DesignExtrudeOperation::NewBody if body_count == 0 => {
            let guided_profiles = groups
                .iter()
                .filter(|group| group.role == 0x43_0000_0000)
                .map(|group| {
                    LoftSection::Profile(
                        resolved_profile_face_group(scope, group, face_operands)
                            .unwrap_or_else(|| ProfileRef::Native(group.id.clone())),
                    )
                })
                .collect::<Vec<_>>();
            if guided_profiles.len() == 2 {
                let guides = groups
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
                let centerlines = groups
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
                (guided_profiles, guides, centerline)
            } else if guided_profiles.is_empty() {
                let role = if groups.iter().all(|group| group.role == 0x41_0000_0000) {
                    0x41_0000_0000
                } else if groups.iter().all(|group| group.role == ROLE_0X5) {
                    ROLE_0X5
                } else {
                    return None;
                };
                (
                    groups
                        .iter()
                        .filter(|group| group.role == role)
                        .map(|group| LoftSection::Profile(ProfileRef::Native(group.id.clone())))
                        .collect::<Vec<_>>(),
                    Vec::new(),
                    None,
                )
            } else if guided_profiles.len() == 1
                && groups
                    .iter()
                    .all(|group| matches!(group.role, 0x43_0000_0000 | ROLE_0X5))
            {
                let point_ordinal = groups
                    .iter()
                    .position(|group| group.role == ROLE_0X5 && group.members.len() == 1)?;
                if !matches!(point_ordinal, 0) && point_ordinal + 1 != groups.len() {
                    return None;
                }
                if groups.iter().enumerate().any(|(ordinal, group)| {
                    ordinal != point_ordinal && group.role == ROLE_0X5 && group.members.len() == 1
                }) {
                    return None;
                }
                (
                    groups
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
            } else {
                return None;
            }
        }
        _ => return None,
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
        None,
    );
    loft_path_from_edge_selection(&group.id, selection)
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

fn project_fixed_sweep(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, PathRef, ProfileRef, SweepMode};

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
    let profile = groups
        .iter()
        .filter(|group| group.role == 0x41_0000_0000)
        .collect::<Vec<_>>();
    let path = groups
        .iter()
        .filter(|group| group.role == ROLE_0X5)
        .collect::<Vec<_>>();
    let ([profile], [path]) = (profile.as_slice(), path.as_slice()) else {
        return None;
    };
    if groups.len() != 2 || values[5] != 0.0 {
        return None;
    }
    Some(FeatureDefinition::Sweep {
        profile: Some(ProfileRef::Native(profile.id.clone())),
        sections: Vec::new(),
        path: Some(PathRef::Native(path.id.clone())),
        mode: SweepMode::Solid {
            op: fixed_boolean_operation(*operation),
        },
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: (values[4] != 0.0).then_some(Angle(values[4])),
        scale: None,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn project_surface_patch(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef, SurfaceBoundary};

    if scope.kind != "SurfacePatch" {
        return None;
    }
    let (boundary_count, boundary_role) =
        if scope.frame_length == 339 && scope.reference_members.len() == 3 {
            (1, 0x0000_0041_0000_0000)
        } else {
            let boundary_count = scope.reference_members.len().checked_sub(1)? / 3;
            if boundary_count == 0
                || scope.reference_members.len() != boundary_count * 3 + 1
                || scope.frame_length != 354 + 44 * u64::try_from(boundary_count - 1).ok()?
            {
                return None;
            }
            (boundary_count, ROLE_0X4)
        };
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    if groups.len() != boundary_count {
        return None;
    }
    let mut groups = groups;
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    for (ordinal, boundary) in groups.iter().enumerate() {
        let reference_ordinal = ordinal * 3;
        if boundary.scope_reference_ordinal != u32::try_from(reference_ordinal).ok()?
            || boundary.record_index != scope.reference_members[reference_ordinal]
            || boundary.role != boundary_role
            || boundary.members.as_slice()
                != &scope.reference_members[reference_ordinal + 1..reference_ordinal + 2]
        {
            return None;
        }
    }
    let boundary = if let [boundary] = groups.as_slice() {
        boundary.id.clone()
    } else {
        scope.id.clone()
    };
    Some(FeatureDefinition::FilledSurface {
        boundary: SurfaceBoundary::Path(PathRef::Native(boundary)),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: None,
        merge_result: None,
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

fn project_split(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};

    if scope.kind != "Split" || scope.frame_length != 325 || scope.reference_members.len() != 4 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let [targets] = groups.as_slice() else {
        return None;
    };
    if targets.scope_reference_ordinal != 2
        || targets.record_index != scope.reference_members[2]
        || targets.role != ROLE_0X4
        || targets.members.as_slice() != &scope.reference_members[3..4]
    {
        return None;
    }
    let matching_tools = face_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.scope_reference_ordinal == 1
                && operand.record_index == scope.reference_members[1]
                && operand.recipe_kind == ConstructionRecipeKind::Face
        })
        .collect::<Vec<_>>();
    let [tool] = matching_tools.as_slice() else {
        return None;
    };
    let mut tools = direct_face_selection(scope, face_operands)
        .unwrap_or_else(|| FaceSelection::Native(tool.id.clone()));
    match &mut tools {
        FaceSelection::Resolved { native, .. }
        | FaceSelection::Historical { native, .. }
        | FaceSelection::HistoricalPartial { native, .. } => native.clone_from(&tool.id),
        _ => {}
    }
    Some(FeatureDefinition::SplitBody {
        targets: BodySelection::Native(targets.id.clone()),
        tools,
    })
}

pub(crate) fn project_extrude(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, Extent, ExtrudeDirection, ExtrudeStart, FaceSelection, FeatureDefinition,
        Length, ProfileRef,
    };

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
    let profile_groups = scope_groups
        .iter()
        .filter(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Profile))
        .copied()
        .collect::<Vec<_>>();
    let profile_ref = match scope.extrude_profile.as_ref() {
        Some(profile) => {
            if !profile_groups.is_empty()
                && !matches!(
                    profile_groups.as_slice(),
                    [group] if group.members.first() == Some(&profile.record_index)
                )
            {
                return None;
            }
            let placement = placements.iter().find(|placement| {
                native_stream(&placement.id) == native_stream(&scope.id)
                    && placement.entity_id == profile.entity_id
            })?;
            ProfileRef::Sketch(neutral_sketch_id(placement))
        }
        None => {
            let [group] = profile_groups.as_slice() else {
                return None;
            };
            resolved_profile_face_group(scope, group, face_operands)
                .unwrap_or_else(|| ProfileRef::Native(group.id.clone()))
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
        .map(|fixed| Length(fixed.along_distance * 10.0));
    let along = match (parameter_along, fixed_along) {
        (Some(parameter), Some(fixed)) if (parameter.0 - fixed.0).abs() <= 1.0e-9 => {
            Some(parameter)
        }
        (Some(distance), None) | (None, Some(distance)) => Some(distance),
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
    let side_two_offset = match unique("Side2Offset")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
    };
    if side_two_offset.is_some() {
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
    if start_groups.len() + termination_groups.len() != face_groups.len() {
        return None;
    }
    let start = match scope.extrude_start? {
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
                face: resolved_face_group(start, face_operands)
                    .unwrap_or_else(|| FaceSelection::Native(start.id.clone())),
                offset: (offset.0 != 0.0).then_some(offset),
            }
        }
        _ => return None,
    };
    let (extent, reverse_direction) = match (scope.extrude_extent?, along, against) {
        (DesignExtrudeExtent::OneSidedDistance, Some(along), None)
            if along.0 != 0.0
                && scope.extrude_direction_reversed == Some(false)
                && termination_groups.is_empty()
                && side_one_offset.is_none() =>
        {
            (
                Extent::Blind {
                    length: Length(along.0.abs()),
                },
                along.0 < 0.0,
            )
        }
        (DesignExtrudeExtent::TwoSidedDistance, Some(along), Some(against))
            if along.0 != 0.0
                && against.0 != 0.0
                && scope.extrude_direction_reversed == Some(false)
                && termination_groups.is_empty()
                && side_one_offset.is_none() =>
        {
            (
                Extent::TwoSided {
                    first: Length(along.0.abs()),
                    second: Length(against.0.abs()),
                },
                along.0 < 0.0,
            )
        }
        (DesignExtrudeExtent::OneSidedToFace, None, None) => {
            let offset = side_one_offset?;
            let [termination] = termination_groups.as_slice() else {
                return None;
            };
            (
                Extent::ToFace {
                    face: resolved_face_group(termination, face_operands)
                        .unwrap_or_else(|| FaceSelection::Native(termination.id.clone())),
                    offset: (offset.0 != 0.0).then_some(offset),
                },
                scope.extrude_direction_reversed?,
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
        .map(|fixed| Angle(fixed.taper_angle));
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
    if second_draft.is_some() && !matches!(extent, Extent::TwoSided { .. }) {
        return None;
    }
    let has_body_operands = scope_groups
        .iter()
        .any(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Bodies));
    let op = match (scope.extrude_operation?, has_body_operands) {
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
        draft,
        second_draft,
        direction_source: None,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        first_offset: None,
        second_offset: None,
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
            let transverse = Vector3::new(
                normal.y * reference_direction.z - normal.z * reference_direction.y,
                normal.z * reference_direction.x - normal.x * reference_direction.z,
                normal.x * reference_direction.y - normal.y * reference_direction.x,
            );
            let at = |angle: f64| {
                Point3::new(
                    center.x
                        + radius.0
                            * (reference_direction.x * angle.cos() + transverse.x * angle.sin()),
                    center.y
                        + radius.0
                            * (reference_direction.y * angle.cos() + transverse.y * angle.sin()),
                    center.z
                        + radius.0
                            * (reference_direction.z * angle.cos() + transverse.z * angle.sin()),
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
            let a = Vector3::new(
                pair[0].x - origin.x,
                pair[0].y - origin.y,
                pair[0].z - origin.z,
            );
            let b = Vector3::new(
                pair[1].x - origin.x,
                pair[1].y - origin.y,
                pair[1].z - origin.z,
            );
            normal.x += a.y * b.z - a.z * b.y;
            normal.y += a.z * b.x - a.x * b.z;
            normal.z += a.x * b.y - a.y * b.x;
        }
        let normal_length = normal.norm();
        let first_end = edges[uses[0].0].1[1];
        let u = Vector3::new(
            first_end.x - origin.x,
            first_end.y - origin.y,
            first_end.z - origin.z,
        );
        let u_length = u.norm();
        if normal_length <= tolerance || u_length <= tolerance {
            continue;
        }
        normal = Vector3::new(
            normal.x / normal_length,
            normal.y / normal_length,
            normal.z / normal_length,
        );
        let u_axis = Vector3::new(u.x / u_length, u.y / u_length, u.z / u_length);
        if points.iter().any(|point| {
            ((point.x - origin.x) * normal.x
                + (point.y - origin.y) * normal.y
                + (point.z - origin.z) * normal.z)
                .abs()
                > tolerance
        }) {
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
    profiles.sort_by_key(|profile| profile.boundary[0].entity.clone());
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
    };
    let stream = native_stream(&scope.id)?;
    let mut body_groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.role == 0x0000_0008_0000_0000
    });
    let first_body_group = body_groups.next();
    if body_groups.next().is_some() {
        return None;
    }
    let result = match (scope.coil_operation?, first_body_group) {
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
    Some(FeatureDefinition::Coil {
        construction: CoilConstruction {
            placement: CoilPlacement::Native {
                native_ref: scope.id.clone(),
            },
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
