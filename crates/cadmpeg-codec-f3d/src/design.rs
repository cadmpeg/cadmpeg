// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::records::{
    BodyNativeKey, ConstructionRecipe, ConstructionRecipeKind, DesignBodyBinding, DesignBodyBounds,
    DesignBodyMember, DesignConfiguration, DesignConfigurationKind, DesignConstructionOperandGroup,
    DesignConstructionOperandIdentity, DesignConstructionPersistentIdentity, DesignDimensionLocus,
    DesignDimensionLocusGroup, DesignDimensionLocusPair, DesignDimensionNullLocusPair,
    DesignDimensionRecipeRecord, DesignEdgeOperand, DesignEntityHeader, DesignExtrudeExtent,
    DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignExtrudeOperation,
    DesignExtrudeProfileOperand, DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember,
    DesignExtrudeStart, DesignFaceOperand, DesignFilletRadiusGroup, DesignObject, DesignObjectKind,
    DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    DesignParameterScope, DesignRecordHeader, DesignSketchPlacement, DesignTopologyRecipeEntry,
    DesignTopologyRecipeSide, DesignTopologyRecipeTriplet, LostEdgeReference, PersistentReference,
    PersistentReferenceKind, PersistentSubentityTag, SketchConstraintKind, SketchCurveGeometry,
    SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationOperand,
};
use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::le::{
    f64_at, f64s_at, lp_u32_bytes_at, u32_at, u32_at as read_u32, u64_at as read_u64, utf16le_at,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::container::{role, ContainerScan};

const RECIPES: &[(&[u8], ConstructionRecipeKind)] = &[
    (b"body_recipe_data", ConstructionRecipeKind::Body),
    (b"face_recipe_data", ConstructionRecipeKind::Face),
    (
        b"bounded_face_recipe_data",
        ConstructionRecipeKind::BoundedFace,
    ),
    (b"edge_recipe_data", ConstructionRecipeKind::Edge),
    (b"vertex_recipe_data", ConstructionRecipeKind::Vertex),
];

pub(crate) const fn construction_recipe_family_name_len(kind: ConstructionRecipeKind) -> usize {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data".len(),
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        ConstructionRecipeKind::Edge => b"edge_recipe_data".len(),
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data".len(),
    }
}

/// Decode every JSON design-configuration table and rule entry.
pub fn decode_configurations(scan: &ContainerScan) -> Result<Vec<DesignConfiguration>, CodecError> {
    let configurations = scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::DESIGN_CONFIG)
        .map(|entry| {
            let bytes = scan.entry_bytes(&entry.name)?;
            let payload: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                CodecError::Malformed(format!(
                    "invalid F3D configuration JSON {}: {error}",
                    entry.name
                ))
            })?;
            if !payload.is_object() {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration JSON must be an object: {}",
                    entry.name
                )));
            }
            let kind = if entry.name.ends_with(".dsgcfgrule") {
                DesignConfigurationKind::Rule
            } else {
                DesignConfigurationKind::Table
            };
            validate_configuration_payload(&entry.name, kind, &payload)?;
            Ok(DesignConfiguration {
                id: format!("f3d:configuration:entry#{}", entry.name),
                entry_name: entry.name.clone(),
                kind,
                payload,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    for configuration in &configurations {
        if !names.insert(configuration.entry_name.as_str())
            || !ids.insert(configuration.id.as_str())
        {
            return Err(CodecError::Malformed(format!(
                "duplicate F3D configuration identity: {}",
                configuration.entry_name
            )));
        }
    }
    Ok(configurations)
}

/// Validate the typed fields of one configuration document while permitting
/// unrecognized object members for forward-compatible native retention.
pub(crate) fn validate_configuration_payload(
    entry_name: &str,
    kind: DesignConfigurationKind,
    payload: &serde_json::Value,
) -> Result<(), CodecError> {
    let object = payload.as_object().ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D configuration JSON must be an object: {entry_name}"
        ))
    })?;
    if kind == DesignConfigurationKind::Rule {
        let condition = object.get("when");
        let target = object.get("activate");
        if condition.is_some() || target.is_some() {
            if !condition.is_some_and(serde_json::Value::is_string)
                || !target.is_some_and(serde_json::Value::is_string)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration rule `when` and `activate` must be paired strings: {entry_name}"
                )));
            }
        }
        return Ok(());
    }
    let configurations = match object.get("configurations") {
        Some(value) => Some(value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `configurations` must be an object: {entry_name}"
            ))
        })?),
        None => None,
    };
    if let Some(active) = object.get("active") {
        let active = active.as_str().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `active` must be a string: {entry_name}"
            ))
        })?;
        if !configurations.is_some_and(|variants| variants.contains_key(active)) {
            return Err(CodecError::Malformed(format!(
                "F3D active configuration `{active}` is not a named variant: {entry_name}"
            )));
        }
    }
    for (name, value) in configurations.into_iter().flatten() {
        let definition = value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            ))
        })?;
        if definition
            .get("parameters")
            .is_some_and(|value| !value.is_object())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` parameters must be an object: {entry_name}"
            )));
        }
        if let Some(suppressed) = definition.get("suppressed") {
            let valid = suppressed
                .as_array()
                .is_some_and(|values| values.iter().all(serde_json::Value::is_string));
            if !valid {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
                )));
            }
        }
        if definition
            .get("material")
            .is_some_and(|value| !value.is_string())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` material must be a string: {entry_name}"
            )));
        }
    }
    Ok(())
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub fn project_configurations(
    native: &[DesignConfiguration],
) -> Vec<cadmpeg_ir::features::DesignConfiguration> {
    use cadmpeg_ir::features::DesignConfiguration as NeutralConfiguration;
    use std::collections::BTreeMap;

    let mut projected = Vec::new();
    for table in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Table)
    {
        let active = table
            .payload
            .get("active")
            .and_then(serde_json::Value::as_str);
        let Some(configurations) = table
            .payload
            .get("configurations")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (name, definition) in configurations {
            let mut properties = BTreeMap::new();
            let definition = definition.as_object();
            if let Some(parameters) = definition
                .and_then(|value| value.get("parameters"))
                .and_then(serde_json::Value::as_object)
            {
                for (parameter, value) in parameters {
                    properties.insert(format!("parameter:{parameter}"), json_scalar_text(value));
                }
            }
            if let Some(suppressed) = definition
                .and_then(|value| value.get("suppressed"))
                .and_then(serde_json::Value::as_array)
            {
                for feature in suppressed.iter().filter_map(serde_json::Value::as_str) {
                    properties.insert(format!("suppressed:{feature}"), "true".into());
                }
            }
            let material = definition
                .and_then(|value| value.get("material"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let ordinal = u32::try_from(projected.len()).unwrap_or(u32::MAX);
            projected.push(NeutralConfiguration {
                id: neutral_configuration_id(&table.entry_name, name),
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: name.clone(),
                material,
                properties,
                bodies: Vec::new(),
                native_ref: Some(table.id.clone()),
            });
        }
    }
    for rule in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Rule)
    {
        let Some(condition) = rule.payload.get("when").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = rule
            .payload
            .get("activate")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let mut matches = projected
            .iter_mut()
            .filter(|configuration| configuration.name == target);
        let Some(configuration) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        configuration.properties.insert(
            format!("activation_rule:{}", rule.entry_name),
            condition.to_owned(),
        );
    }
    projected
}

pub(crate) fn unresolved_configuration_rule_count(
    native: &[DesignConfiguration],
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    native
        .iter()
        .filter(|rule| {
            rule.kind == DesignConfigurationKind::Rule
                && rule
                    .payload
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
        })
        .filter(|rule| {
            !projected.iter().any(|configuration| {
                configuration
                    .properties
                    .contains_key(&format!("activation_rule:{}", rule.entry_name))
            })
        })
        .count()
}

fn neutral_configuration_id(
    entry_name: &str,
    variant_name: &str,
) -> cadmpeg_ir::features::ConfigurationId {
    use cadmpeg_ir::features::ConfigurationId;

    ConfigurationId(format!(
        "f3d:configuration:variant#{}:{}{}:{}",
        entry_name.len(),
        entry_name,
        variant_name.len(),
        variant_name,
    ))
}

/// Project parameter scopes and their document- or scope-owned parameters into
/// the neutral construction history.
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
    use cadmpeg_ir::features::{
        Angle, DesignParameter as NeutralParameter, DimensionDisplay, EdgeSelection, Feature,
        FeatureDefinition, FilletGroup, Length, ParameterId, ParameterValue, ProfileRef,
        RadiusForm, RadiusSpec, SketchSpace,
    };
    use std::collections::BTreeMap;

    let scope_ids = scopes
        .iter()
        .filter_map(|scope| {
            Some((
                (native_stream(&scope.id)?, scope.record_index),
                neutral_feature_id(scope),
            ))
        })
        .collect::<HashMap<_, _>>();
    let sketches_by_scope = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (native_stream(&placement.id)?, placement.scope_record_index),
                neutral_sketch_id(placement),
            ))
        })
        .collect::<HashMap<_, _>>();
    let owners_by_index = owners
        .iter()
        .filter_map(|owner| Some(((native_stream(&owner.id)?, owner.record_index), owner)))
        .collect::<HashMap<_, _>>();
    let mut features = scopes
        .iter()
        .map(|scope| {
            let native_scope = native_stream(&scope.id).unwrap_or("f3d:design");
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
            let definition = if scope.kind == "Sketch" {
                FeatureDefinition::Sketch {
                    space: SketchSpace::Planar,
                    sketch: sketches_by_scope
                        .get(&(native_scope, scope.record_index))
                        .cloned(),
                }
            } else if scope.kind == "Extrude" {
                project_extrude(
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    placements,
                )
                .unwrap_or_else(|| {
                    let mut properties = BTreeMap::new();
                    if let Some(profile) = scope.extrude_profile.as_ref() {
                        if let Some(placement) = placements.iter().find(|placement| {
                            native_stream(&placement.id) == Some(native_scope)
                                && placement.entity_id == profile.entity_id
                        }) {
                            properties.insert("profile".into(), neutral_sketch_id(placement).0);
                        }
                    }
                    FeatureDefinition::Native {
                        kind: scope.kind.clone(),
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter)| {
                                (parameter.name.clone(), parameter.expression.clone())
                            })
                            .collect(),
                        properties,
                    }
                })
            } else if scope.kind == "Fillet" {
                let mut assignments = fillet_radius_groups
                    .iter()
                    .filter(|assignment| {
                        native_stream(&assignment.id) == Some(native_scope)
                            && assignment.scope_record_index == scope.record_index
                    })
                    .collect::<Vec<_>>();
                assignments.sort_by_key(|assignment| assignment.group_ordinal);
                let groups = assignments
                    .into_iter()
                    .map(|assignment| {
                        let radius = native
                            .iter()
                            .find(|parameter| {
                                native_stream(&parameter.id) == Some(native_scope)
                                    && parameter.record_index
                                        == assignment.radius_parameter_record_index
                            })
                            .and_then(design_length)
                            .filter(|radius| radius.0 > 0.0)
                            .map_or(
                                RadiusSpec::Unresolved {
                                    form: Some(RadiusForm::Constant),
                                },
                                |radius| RadiusSpec::Constant { radius },
                            );
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
                                        scope.previous_history_state_id,
                                        &neutral_feature_id(scope),
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
                            radius: match parameters
                                .iter()
                                .filter(|(_, parameter)| parameter.source_kind == "Radius")
                                .filter_map(|(_, parameter)| design_length(parameter))
                                .collect::<Vec<_>>()
                                .as_slice()
                            {
                                [radius] if radius.0 > 0.0 => {
                                    RadiusSpec::Constant { radius: *radius }
                                }
                                [_] => RadiusSpec::Unresolved {
                                    form: Some(RadiusForm::Constant),
                                },
                                _ => RadiusSpec::Unresolved { form: None },
                            },
                            tangency_weight: None,
                        }]
                    } else {
                        groups
                    },
                }
            } else if scope.kind == "Chamfer" {
                project_chamfer(scope, &parameters, construction_groups, edge_operands)
            } else {
                let mut properties = BTreeMap::new();
                if let Some(profile) = scope.extrude_profile.as_ref() {
                    if let Some(placement) = placements.iter().find(|placement| {
                        native_stream(&placement.id) == Some(native_scope)
                            && placement.entity_id == profile.entity_id
                    }) {
                        properties.insert("profile".into(), neutral_sketch_id(placement).0);
                    }
                }
                FeatureDefinition::Native {
                    kind: scope.kind.clone(),
                    parameters: parameters
                        .iter()
                        .map(|(_, parameter)| {
                            (parameter.name.clone(), parameter.expression.clone())
                        })
                        .collect(),
                    properties,
                }
            };
            Feature {
                id: scope_ids[&(native_scope, scope.record_index)].clone(),
                ordinal: scope.byte_offset,
                name: None,
                suppressed: matches!(scope.kind.as_str(), "Extrude" | "Fillet" | "Chamfer")
                    && scope.history_state_id.is_none()
                    && scope.previous_history_state_id.is_none(),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: Some(scope.kind.clone()),
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
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
            native_stream(&scope.id).unwrap_or("f3d:design"),
            previous_state_id,
        )) {
            if predecessor != &feature.id && !feature.dependencies.contains(predecessor) {
                feature.dependencies.push(predecessor.clone());
            }
        }
    }
    let sketch_features = features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } => Some((sketch.0.clone(), feature.id.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for feature in &mut features {
        if let FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch),
            ..
        } = &feature.definition
        {
            if let Some(dependency) = sketch_features.get(&sketch.0) {
                if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                    feature.dependencies.push(dependency.clone());
                }
            }
        }
    }
    features.sort_by_key(|feature| feature.id.clone());

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
                    None
                }
            };
            NeutralParameter {
                id: neutral_parameter_id(parameter),
                owner: parameter
                    .owner_record_index
                    .and_then(|owner| {
                        owners_by_index
                            .get(&(native_stream(&parameter.id).unwrap_or("f3d:design"), owner))
                    })
                    .and_then(|owner| {
                        scope_ids.get(&(
                            native_stream(&owner.id).unwrap_or("f3d:design"),
                            owner.scope_record_index,
                        ))
                    })
                    .cloned(),
                ordinal: parameter
                    .owner_record_index
                    .and_then(|owner| {
                        owners_by_index
                            .get(&(native_stream(&parameter.id).unwrap_or("f3d:design"), owner))
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
    let mut owned_aliases = HashMap::<(&str, String), Option<ParameterId>>::new();
    for parameter in &parameters {
        let scope = parameter_scopes[&parameter.id];
        if let Some(owner) = &parameter.owner {
            feature_aliases
                .entry((scope, owner.clone(), parameter.name.clone()))
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(parameter.id.clone()));
            owned_aliases
                .entry((scope, parameter.name.clone()))
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(parameter.id.clone()));
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
                let candidate = if let Some(owner) = &parameter.owner {
                    match feature_aliases.get(&(scope, owner.clone(), identifier.clone())) {
                        Some(None) => return None,
                        Some(Some(local)) => Some(local),
                        None => document_aliases
                            .get(&(scope, identifier.clone()))
                            .or_else(|| owned_aliases.get(&(scope, identifier)))?
                            .as_ref(),
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

fn design_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::features::Length> {
    (parameter.unit.as_deref().is_some_and(design_length_unit)
        && parameter.evaluated_value.is_finite())
    .then_some(cadmpeg_ir::features::Length(
        parameter.evaluated_value * 10.0,
    ))
}

fn design_length_unit(unit: &str) -> bool {
    matches!(unit, "mm" | "cm" | "m" | "in" | "ft")
}

fn design_angle_unit(unit: &str) -> bool {
    matches!(unit, "deg" | "rad")
}

fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
) -> cadmpeg_ir::features::FeatureDefinition {
    use cadmpeg_ir::features::{
        ChamferForm, ChamferGroup, ChamferSpec, EdgeSelection, FeatureDefinition,
    };

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

    let (form, candidates) = if !first_distances.is_empty() || !second_distances.is_empty() {
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
        (Some(ChamferForm::TwoDistances), candidates)
    } else if !angles.is_empty() {
        let candidates =
            (distances.len() == group_count && angles.len() == group_count).then(|| {
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
        (Some(ChamferForm::DistanceAngle), candidates)
    } else if !distances.is_empty() {
        let candidates = (distances.len() == group_count).then(|| {
            distances
                .iter()
                .map(|distance| {
                    design_length(distance).map(|distance| ChamferSpec::Distance { distance })
                })
                .collect::<Vec<_>>()
        });
        (Some(ChamferForm::Distance), candidates)
    } else {
        (None, None)
    };
    let candidates = candidates.unwrap_or_else(|| vec![None; group_count]);

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
                        scope.previous_history_state_id,
                        &neutral_feature_id(scope),
                    ),
                    None => EdgeSelection::Native(scope.id.clone()),
                },
                spec: spec
                    .filter(valid_chamfer_spec)
                    .unwrap_or(ChamferSpec::Unresolved { form }),
            }
        })
        .collect();
    FeatureDefinition::Chamfer { groups }
}

fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;
    use cadmpeg_ir::ids::HistoricalEdgeId;

    if !group.lost_edge_references.is_empty() {
        return EdgeSelection::Unresolved;
    }
    let stream = native_stream(&group.id);
    let mut matched_operands = Vec::with_capacity(group.members.len());
    let mut member_identities = HashSet::new();
    for member in &group.members {
        if !member_identities.insert(*member) {
            return EdgeSelection::Native(group.id.clone());
        }
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *member
        });
        let Some(operand) = matches.next() else {
            return EdgeSelection::Native(group.id.clone());
        };
        if matches.next().is_some() {
            return EdgeSelection::Native(group.id.clone());
        }
        matched_operands.push(operand);
    }
    let resolved_slots = matched_operands
        .iter()
        .map(|operand| resolved_edge_operand(operand))
        .collect::<Option<Vec<_>>>()
        .or_else(|| unique_edge_group_assignment(&matched_operands))
        .or_else(|| {
            context_only_edge_group_candidates(matched_operands.iter().map(|operand| {
                (
                    resolved_edge_operand(operand),
                    operand.changed_boundary_edge_slots.as_slice(),
                )
            }))
        })
        .or_else(|| {
            common_deleted_edge_group_candidates(
                matched_operands
                    .iter()
                    .map(|operand| operand.deleted_boundary_edge_slots.as_slice()),
                matched_operands.len(),
            )
        })
        .or_else(|| scope_partition_edge_group_candidates(group, groups, operands));
    let Some(resolved_slots) = resolved_slots else {
        return EdgeSelection::Native(group.id.clone());
    };
    let Some(previous_state_id) = previous_state_id else {
        return EdgeSelection::Native(group.id.clone());
    };
    let state = feature_input_topology_id(feature_id, previous_state_id);
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    let mut edges = Vec::new();
    for edge_slot in resolved_slots {
        let edge = HistoricalEdgeId(format!(
            "f3d:history-input:edge#{}:{}:{previous_state_id}:{edge_slot}",
            feature_key.len(),
            feature_key
        ));
        if !edges.contains(&edge) {
            edges.push(edge);
        }
    }
    if edges.is_empty() {
        EdgeSelection::Native(group.id.clone())
    } else {
        EdgeSelection::Historical {
            state,
            edges,
            native: group.id.clone(),
        }
    }
}

fn context_only_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (Option<i64>, &'a [i64])>,
) -> Option<Vec<i64>> {
    let mut edges = Vec::new();
    for (resolved, changed_candidates) in members {
        match resolved {
            Some(edge) => {
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            None if changed_candidates.is_empty() => {}
            None => return None,
        }
    }
    (!edges.is_empty()).then_some(edges)
}

pub(crate) fn feature_input_topology_id(
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    cadmpeg_ir::ids::FeatureInputTopologyId(format!(
        "f3d:history-input:state#{}:{}:{previous_state_id}",
        feature_key.len(),
        feature_key
    ))
}

fn unique_edge_group_assignment(operands: &[&DesignEdgeOperand]) -> Option<Vec<i64>> {
    if operands.is_empty() {
        return None;
    }
    let candidate_sets = operands
        .iter()
        .map(|operand| {
            if let Some(edge) = resolved_edge_operand(operand) {
                Some(Some(vec![edge]))
            } else if operand.changed_boundary_edge_slots.is_empty() {
                Some(None)
            } else {
                Some(Some(edge_assignment_candidates(
                    &operand.recipe_selectors,
                    operand
                        .recipe_reference_contexts
                        .iter()
                        .map(|context| context.changed_reference_edge_slots.as_slice()),
                )?))
            }
        })
        .collect::<Option<Vec<_>>>()?;
    unique_edge_assignment_with_context(&candidate_sets)
}

fn unique_edge_assignment_with_context(candidate_sets: &[Option<Vec<i64>>]) -> Option<Vec<i64>> {
    let edge_candidate_sets = candidate_sets
        .iter()
        .filter_map(|candidates| candidates.clone())
        .collect::<Vec<_>>();
    unique_bipartite_assignment(&edge_candidate_sets)
}

fn edge_assignment_candidates<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<Vec<i64>> {
    let shared_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    if !selector_contexts.is_empty()
        && selector_contexts
            .iter()
            .all(|selector| !selector.incidence_matching_edge_slots.is_empty())
    {
        corroborated_edge_candidates(selector_contexts, shared_edge_sets.iter().copied(), false)
    } else {
        corroborated_edge_candidates(selector_contexts, shared_edge_sets.iter().copied(), true)
    }
}

fn unique_bipartite_assignment(candidate_sets: &[Vec<i64>]) -> Option<Vec<i64>> {
    if candidate_sets.is_empty() {
        return None;
    }
    let mut normalized = candidate_sets.to_vec();
    for candidates in &mut normalized {
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            return None;
        }
    }
    let assignment = bipartite_assignment(&normalized, None)?;
    for (member, edge) in assignment.iter().copied().enumerate() {
        if bipartite_assignment(&normalized, Some((member, edge))).is_some() {
            return None;
        }
    }
    Some(assignment)
}

fn bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    forbidden: Option<(usize, i64)>,
) -> Option<Vec<i64>> {
    fn augment(
        member: usize,
        candidate_sets: &[Vec<i64>],
        forbidden: Option<(usize, i64)>,
        visited: &mut HashSet<i64>,
        edge_members: &mut HashMap<i64, usize>,
    ) -> bool {
        for edge in &candidate_sets[member] {
            if forbidden == Some((member, *edge)) || !visited.insert(*edge) {
                continue;
            }
            let displaced = edge_members.get(edge).copied();
            if displaced.is_none_or(|displaced| {
                augment(displaced, candidate_sets, forbidden, visited, edge_members)
            }) {
                edge_members.insert(*edge, member);
                return true;
            }
        }
        false
    }

    let mut edge_members = HashMap::new();
    for member in 0..candidate_sets.len() {
        if !augment(
            member,
            candidate_sets,
            forbidden,
            &mut HashSet::new(),
            &mut edge_members,
        ) {
            return None;
        }
    }
    let mut assignment = vec![0; candidate_sets.len()];
    for (edge, member) in edge_members {
        assignment[member] = edge;
    }
    Some(assignment)
}

fn scope_partition_edge_group_candidates(
    target: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let stream = native_stream(&target.id)?;
    let mut scope_groups = Vec::new();
    let mut target_ordinal = None;
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == target.scope_record_index
            && group.lost_edge_references.is_empty()
            && !group.members.is_empty()
    }) {
        let mut members = Vec::with_capacity(group.members.len());
        let mut complete = true;
        for member in &group.members {
            let matches = operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index == *member
                })
                .collect::<Vec<_>>();
            let [operand] = matches.as_slice() else {
                complete = false;
                break;
            };
            members.push((
                operand.record_index,
                resolved_edge_operand(operand),
                operand.deleted_boundary_edge_slots.clone(),
            ));
        }
        if !complete {
            continue;
        }
        if group.id == target.id {
            target_ordinal = Some(scope_groups.len());
        }
        scope_groups.push(members);
    }
    partition_unique_incomplete_edge_group(target_ordinal?, &scope_groups)
}

fn partition_unique_incomplete_edge_group(
    target_ordinal: usize,
    groups: &[Vec<(u32, Option<i64>, Vec<i64>)>],
) -> Option<Vec<i64>> {
    if groups.len() < 2 || target_ordinal >= groups.len() {
        return None;
    }
    let mut identities = HashSet::new();
    let mut universe = None::<Vec<i64>>;
    for (identity, _, deleted) in groups.iter().flatten() {
        if !identities.insert(*identity) {
            return None;
        }
        let mut deleted = deleted.clone();
        deleted.sort_unstable();
        deleted.dedup();
        if deleted.is_empty()
            || universe
                .as_ref()
                .is_some_and(|universe| *universe != deleted)
        {
            return None;
        }
        universe.get_or_insert(deleted);
    }
    let universe = universe?;
    if identities.len() != universe.len() {
        return None;
    }
    let incomplete = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.iter().any(|(_, resolved, _)| resolved.is_none()))
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    if incomplete.as_slice() != [target_ordinal] {
        return None;
    }
    let mut reserved = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        if ordinal == target_ordinal {
            continue;
        }
        for (_, resolved, _) in group {
            let resolved = resolved.as_ref()?;
            if !universe.contains(resolved) || reserved.contains(resolved) {
                return None;
            }
            reserved.push(*resolved);
        }
    }
    let target = universe
        .into_iter()
        .filter(|candidate| !reserved.contains(candidate))
        .collect::<Vec<_>>();
    if target.len() != groups[target_ordinal].len()
        || groups[target_ordinal]
            .iter()
            .filter_map(|(_, resolved, _)| *resolved)
            .any(|resolved| !target.contains(&resolved))
    {
        return None;
    }
    Some(target)
}

fn common_deleted_edge_group_candidates<'a>(
    candidate_sets: impl IntoIterator<Item = &'a [i64]>,
    member_count: usize,
) -> Option<Vec<i64>> {
    if member_count == 0 {
        return None;
    }
    let mut candidate_sets = candidate_sets.into_iter();
    let mut candidates = candidate_sets.next()?.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    if candidates.len() != member_count {
        return None;
    }
    for candidate_set in candidate_sets {
        let mut normalized = candidate_set.to_vec();
        normalized.sort_unstable();
        normalized.dedup();
        if normalized != candidates {
            return None;
        }
    }
    Some(candidates)
}

fn resolved_edge_operand(operand: &DesignEdgeOperand) -> Option<i64> {
    operand
        .resolved_edge_slot
        .or_else(|| resolve_edge_operand_candidates(operand))
}

pub(crate) fn resolve_edge_operand_candidates(operand: &DesignEdgeOperand) -> Option<i64> {
    resolved_edge_candidate_intersection(
        &operand.recipe_selectors,
        operand
            .recipe_reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice()),
    )
}

fn resolved_edge_candidate_intersection<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    let ordered_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    let shared_edge_sets = ordered_edge_sets
        .iter()
        .copied()
        .filter(|edges| !edges.is_empty())
        .collect::<Vec<_>>();
    if shared_edge_sets.is_empty() {
        return None;
    }
    let reference = ordered_edge_sets.get(..2).and_then(|sets| {
        sets.iter()
            .all(|set| !set.is_empty())
            .then(|| unique_edge_set_intersection(sets))
            .flatten()
    });
    let incidence = corroborated_edge_intersection(selector_contexts, &shared_edge_sets, false);
    let boundary_count = corroborated_edge_intersection(selector_contexts, &shared_edge_sets, true);
    let common_triplet =
        corroborated_common_triplet_intersection(selector_contexts, &shared_edge_sets);
    let proofs = [reference, incidence, boundary_count, common_triplet]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let edge = *proofs.first()?;
    proofs.iter().all(|proof| *proof == edge).then_some(edge)
}

fn corroborated_common_triplet_intersection(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let mut edge_sets = selector_contexts.iter().flat_map(|selector| {
        selector
            .clause_entries
            .iter()
            .zip(&selector.clause_triplet_edge_slots)
            .filter_map(|(entry, triplet_edges)| {
                entry.as_ref()?.common_incident_edge_ordinal?;
                let [first, second] = triplet_edges.as_ref()?;
                let mut common = first.clone();
                common.retain(|edge| second.contains(edge));
                common.sort_unstable();
                common.dedup();
                (!common.is_empty()).then_some(common)
            })
    });
    let mut candidates = edge_sets.next()?;
    for edges in edge_sets {
        candidates.retain(|candidate| edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    for edges in shared_edge_sets {
        candidates.retain(|candidate| edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    (candidates.len() == 1).then_some(candidates[0])
}

fn unique_edge_set_intersection(edge_sets: &[&[i64]]) -> Option<i64> {
    let mut sets = edge_sets.iter();
    let mut candidates = sets.next()?.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for edge_set in sets {
        candidates.retain(|candidate| edge_set.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    (candidates.len() == 1).then_some(candidates[0])
}

fn corroborated_edge_intersection(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
    boundary_counts_only: bool,
) -> Option<i64> {
    let candidates = corroborated_edge_candidates(
        selector_contexts,
        shared_edge_sets.iter().copied(),
        boundary_counts_only,
    )?;
    (candidates.len() == 1).then_some(candidates[0])
}

fn corroborated_edge_candidates<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    boundary_counts_only: bool,
) -> Option<Vec<i64>> {
    let mut selectors = selector_contexts.iter();
    let first = selector_candidate_edges(selectors.next()?, boundary_counts_only);
    if first.is_empty() {
        return None;
    }
    let mut candidates = first.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for selector in selectors {
        let selector_edges = selector_candidate_edges(selector, boundary_counts_only);
        if selector_edges.is_empty() {
            return None;
        }
        candidates.retain(|candidate| selector_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    for shared_edges in shared_edge_sets {
        candidates.retain(|candidate| shared_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    Some(candidates)
}

fn selector_candidate_edges(
    selector: &crate::records::DesignEdgeRecipeSelectorContext,
    boundary_counts_only: bool,
) -> &[i64] {
    if boundary_counts_only {
        &selector.boundary_count_matching_edge_slots
    } else {
        &selector.incidence_matching_edge_slots
    }
}

fn project_extrude(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        BooleanOp, Extent, ExtrudeStart, FaceSelection, FeatureDefinition, Length, ProfileRef,
    };

    let profile = scope.extrude_profile.as_ref()?;
    let profile_placement = placements.iter().find(|placement| {
        native_stream(&placement.id) == native_stream(&scope.id)
            && placement.entity_id == profile.entity_id
    })?;
    let scope_groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
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
    let along = match unique("AlongDistance")? {
        Some(parameter) => Some(design_length(parameter)?),
        None => None,
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
    if side_two_draft.is_some_and(|angle| angle.0 != 0.0) {
        return None;
    }
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
            if along.0 != 0.0 && termination_groups.is_empty() && side_one_offset.is_none() =>
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
                && start_groups.is_empty()
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
        Some(Vector3::new(
            -profile_placement.transform[0][2],
            -profile_placement.transform[1][2],
            -profile_placement.transform[2][2],
        ))
    } else {
        None
    };
    let draft = match unique("TaperAngle")? {
        Some(parameter) => design_angle(parameter).filter(|angle| angle.0 != 0.0),
        None => None,
    };
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
        profile: ProfileRef::Sketch(neutral_sketch_id(profile_placement)),
        direction,
        start,
        extent,
        op,
        draft,
    })
}

fn resolved_face_group(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let Some(operand_faces) = resolved_face_operand(operand) else {
            return None;
        };
        for face in operand_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (!faces.is_empty()).then(|| cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native: group.id.clone(),
    })
}

fn resolved_face_operand(operand: &DesignFaceOperand) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if let Some(slot) = operand.resolved_face_slot {
        return Some(vec![cadmpeg_ir::ids::FaceId(format!(
            "f3d:brep:entity#{slot}"
        ))]);
    }
    let candidates = face_operand_candidates(operand);
    if !operand.alternate_selector_candidate_faces.is_empty() {
        return Some(candidates.to_vec());
    }
    let [face] = candidates else { return None };
    Some(vec![face.clone()])
}

pub(crate) fn resolve_face_operand_history_candidates(operand: &DesignFaceOperand) -> Option<i64> {
    let candidate = match operand.preceding_candidate_faces.as_slice() {
        [face] => face,
        _ => {
            let [face] = operand.changed_candidate_faces.as_slice() else {
                return None;
            };
            face
        }
    };
    if !face_operand_candidates(operand).contains(candidate) {
        return None;
    }
    candidate.0.rsplit_once('#')?.1.parse().ok()
}

pub(crate) fn face_operand_candidates(operand: &DesignFaceOperand) -> &[cadmpeg_ir::ids::FaceId] {
    if !operand.alternate_selector_candidate_faces.is_empty() {
        &operand.alternate_selector_candidate_faces
    } else if operand.unreferenced_candidate_faces.is_empty() {
        &operand.candidate_faces
    } else {
        &operand.unreferenced_candidate_faces
    }
}

/// Resolve selected-face Extrude starts from exact sketch-plane coincidence.
pub(crate) struct ExtrudeStartPlaneResolution<'a> {
    pub faces: &'a [cadmpeg_ir::topology::Face],
    pub surfaces: &'a [cadmpeg_ir::geometry::Surface],
    pub groups: &'a [DesignConstructionOperandGroup],
    pub operands: &'a mut [DesignFaceOperand],
    pub linear_tolerance: f64,
    pub angular_tolerance: f64,
}

pub(crate) fn bind_extrude_start_planes(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: ExtrudeStartPlaneResolution<'_>,
) {
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, ProfileRef};

    for feature in features {
        let FeatureDefinition::Extrude { profile, start, .. } = &mut feature.definition else {
            continue;
        };
        let sketch_id = match profile {
            ProfileRef::Sketch(sketch)
            | ProfileRef::SketchProfiles { sketch, .. }
            | ProfileRef::SketchRegions { sketch, .. }
            | ProfileRef::SketchSelection { sketch, .. } => sketch,
            ProfileRef::Native(_) | ProfileRef::Faces(_) => continue,
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        let ExtrudeStart::FromFace {
            face: FaceSelection::Native(native),
            offset,
        } = start
        else {
            continue;
        };
        let retained_offset = *offset;
        let mut matching_groups = resolution.groups.iter().filter(|group| group.id == *native);
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some()
            || group.extrude_face_role != Some(DesignExtrudeFaceRole::Start)
        {
            continue;
        }
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut candidates = Vec::new();
        for record_index in &group.members {
            let mut matching_operands = resolution.operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *record_index
            });
            let Some(operand) = matching_operands.next() else {
                candidates.clear();
                break;
            };
            if matching_operands.next().is_some() {
                candidates.clear();
                break;
            }
            candidates.extend(face_operand_candidates(operand).iter().cloned());
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        candidates.dedup();
        let coincident = candidates
            .into_iter()
            .filter(|candidate| {
                face_coincident_with_sketch(
                    candidate,
                    sketch,
                    resolution.faces,
                    resolution.surfaces,
                    resolution.linear_tolerance,
                    resolution.angular_tolerance,
                )
            })
            .collect::<Vec<_>>();
        if let [face] = coincident.as_slice() {
            retain_face_operand_resolution(group, resolution.operands, face);
            *start = ExtrudeStart::FromFace {
                face: FaceSelection::Resolved {
                    faces: vec![face.clone()],
                    native: native.clone(),
                },
                offset: retained_offset,
            };
        }
    }
}

fn retain_face_operand_resolution(
    group: &DesignConstructionOperandGroup,
    operands: &mut [DesignFaceOperand],
    face: &cadmpeg_ir::ids::FaceId,
) -> bool {
    let Some(stream) = native_stream(&group.id) else {
        return false;
    };
    let Some(slot) = face
        .0
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
    else {
        return false;
    };
    let mut matches = operands.iter_mut().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && group.members.contains(&operand.record_index)
            && face_operand_candidates(operand).contains(face)
            && operand
                .resolved_face_slot
                .is_none_or(|resolved| resolved == slot)
    });
    let Some(operand) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    operand.resolved_face_slot = Some(slot);
    true
}

fn face_coincident_with_sketch(
    candidate: &cadmpeg_ir::ids::FaceId,
    sketch: &cadmpeg_ir::sketches::Sketch,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let Some(face) = faces.iter().find(|face| face.id == *candidate) else {
        return false;
    };
    let Some(surface) = surfaces.iter().find(|surface| surface.id == face.surface) else {
        return false;
    };
    let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry else {
        return false;
    };
    parallel_vectors(*normal, sketch.normal, angular_tolerance)
        && point_plane_distance(*origin, sketch.origin, sketch.normal) <= linear_tolerance
}

fn parallel_vectors(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    let cross = Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    );
    let left_length = (left.x * left.x + left.y * left.y + left.z * left.z).sqrt();
    let right_length = (right.x * right.x + right.y * right.y + right.z * right.z).sqrt();
    let cross_length = (cross.x * cross.x + cross.y * cross.y + cross.z * cross.z).sqrt();
    left_length > 0.0
        && right_length > 0.0
        && cross_length <= tolerance * left_length * right_length
}

fn point_plane_distance(point: Point3, origin: Point3, normal: Vector3) -> f64 {
    let normal_length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    if normal_length == 0.0 {
        return f64::INFINITY;
    }
    ((point.x - origin.x) * normal.x
        + (point.y - origin.y) * normal.y
        + (point.z - origin.z) * normal.z)
        .abs()
        / normal_length
}

fn design_angle(parameter: &DesignParameter) -> Option<cadmpeg_ir::features::Angle> {
    (parameter.unit.as_deref().is_some_and(design_angle_unit)
        && parameter.evaluated_value.is_finite())
    .then_some(cadmpeg_ir::features::Angle(parameter.evaluated_value))
}

fn valid_chamfer_spec(spec: &cadmpeg_ir::features::ChamferSpec) -> bool {
    use cadmpeg_ir::features::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } => distance.0 > 0.0,
        ChamferSpec::TwoDistances { first, second } => first.0 > 0.0 && second.0 > 0.0,
        ChamferSpec::DistanceAngle { distance, angle } => {
            distance.0 > 0.0 && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
        }
        ChamferSpec::Unresolved { .. } => false,
    }
}

fn neutral_feature_id(scope: &DesignParameterScope) -> cadmpeg_ir::features::FeatureId {
    neutral_feature_id_parts(
        native_stream(&scope.id).unwrap_or("f3d:design"),
        &scope.kind,
        scope.feature_ordinal,
    )
}

fn neutral_feature_id_parts(
    stream: &str,
    kind: &str,
    feature_ordinal: u32,
) -> cadmpeg_ir::features::FeatureId {
    cadmpeg_ir::features::FeatureId(format!(
        "f3d:model:feature#{}:{}{}:{}{}",
        stream.len(),
        stream,
        kind.len(),
        kind,
        feature_ordinal,
    ))
}

fn neutral_parameter_id(parameter: &DesignParameter) -> cadmpeg_ir::features::ParameterId {
    neutral_parameter_id_parts(
        native_stream(&parameter.id).unwrap_or("f3d:design"),
        parameter.source_ordinal,
    )
}

fn neutral_parameter_id_parts(
    stream: &str,
    source_ordinal: u32,
) -> cadmpeg_ir::features::ParameterId {
    cadmpeg_ir::features::ParameterId(format!(
        "f3d:model:parameter#{}:{}{}",
        stream.len(),
        stream,
        source_ordinal,
    ))
}

pub(crate) fn neutral_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId(format!(
        "f3d:model:sketch#{}@{}",
        native_stream(&placement.id).unwrap_or("f3d:design"),
        placement.entity_suffix
    ))
}

pub(crate) fn neutral_sketch_point_id(
    native_ref: &str,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    let stream = native_stream(native_ref).unwrap_or("f3d:design");
    cadmpeg_ir::sketches::SketchEntityId(format!(
        "f3d:model:sketch-entity#{}:{}p{persistent_id}",
        stream.len(),
        stream,
    ))
}

pub(crate) fn neutral_sketch_curve_id(
    native_ref: &str,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    let stream = native_stream(native_ref).unwrap_or("f3d:design");
    cadmpeg_ir::sketches::SketchEntityId(format!(
        "f3d:model:sketch-entity#{}:{}c{primary_id}:{secondary_id}",
        stream.len(),
        stream,
    ))
}

pub(crate) fn neutral_sketch_constraint_id(
    native_ref: &str,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    cadmpeg_ir::sketches::SketchConstraintId(format!(
        "f3d:model:sketch-constraint#{}@{record_index}",
        native_stream(native_ref).unwrap_or("f3d:design")
    ))
}

pub(crate) fn neutral_dimension_constraint_id(
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let parameter_key = parameter
        .0
        .split_once('#')
        .map_or(parameter.0.as_str(), |(_, key)| key);
    cadmpeg_ir::sketches::SketchConstraintId(format!(
        "f3d:model:sketch-constraint#dimension:{}:{}{}:{}",
        parameter_key.len(),
        parameter_key,
        form.len(),
        form,
    ))
}

/// Project placed Design sketches and their exact planar point/curve records.
pub fn project_sketch_design(
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    linear_tolerance: f64,
) -> (
    Vec<cadmpeg_ir::sketches::Sketch>,
    Vec<cadmpeg_ir::sketches::SketchEntity>,
) {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry};

    let placements_by_suffix = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                placement,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut sketches = placements
        .iter()
        .map(|placement| Sketch {
            id: neutral_sketch_id(placement),
            name: Some(placement.entity_id.clone()),
            configuration: None,
            origin: Point3::new(
                placement.transform[0][3],
                placement.transform[1][3],
                placement.transform[2][3],
            ),
            normal: Vector3::new(
                placement.transform[0][2],
                placement.transform[1][2],
                placement.transform[2][2],
            ),
            u_axis: Vector3::new(
                placement.transform[0][0],
                placement.transform[1][0],
                placement.transform[2][0],
            ),
            profiles: Vec::new(),
            native_ref: Some(placement.id.clone()),
        })
        .collect::<Vec<_>>();
    sketches.sort_by_key(|sketch| sketch.id.clone());

    let mut entities = points
        .iter()
        .filter_map(|point| {
            let placement =
                placements_by_suffix.get(&(native_stream(&point.id)?, point.owner_reference?))?;
            Some(SketchEntity {
                id: neutral_sketch_point_id(&point.id, point.persistent_id),
                sketch: neutral_sketch_id(placement),
                construction: false,
                native_ref: Some(point.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: point.coordinates,
                },
            })
        })
        .collect::<Vec<_>>();
    entities.extend(curves.iter().filter_map(|curve| {
        let placement =
            placements_by_suffix.get(&(native_stream(&curve.id)?, curve.owner_reference?))?;
        let geometry = match curve.geometry.as_ref()? {
            SketchCurveGeometry::Line {
                start, end, normal, ..
            } if planar_point(start) && planar_point(end) && positive_sketch_normal(normal) => {
                SketchGeometry::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                }
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if planar_point(center)
                && positive_sketch_normal(normal)
                && reference_direction.z.abs() <= 1.0e-9
                && *radius > 0.0 =>
            {
                let phase = reference_direction.y.atan2(reference_direction.x);
                let start_angle = phase + start_angle;
                let end_angle = phase + end_angle;
                if (end_angle - start_angle).abs() >= std::f64::consts::TAU - 1.0e-9 {
                    SketchGeometry::Circle {
                        center: Point2::new(center.x, center.y),
                        radius: Length(*radius),
                    }
                } else {
                    SketchGeometry::Arc {
                        center: Point2::new(center.x, center.y),
                        radius: Length(*radius),
                        start_angle: Angle(start_angle),
                        end_angle: Angle(end_angle),
                    }
                }
            }
            SketchCurveGeometry::Nurbs {
                degree,
                knots,
                weights,
                control_points,
                ..
            } if control_points.iter().all(planar_point) && clamped_nurbs(*degree, knots) => {
                SketchGeometry::Nurbs {
                    degree: *degree,
                    knots: knots.clone(),
                    control_points: control_points
                        .iter()
                        .map(|point| Point2::new(point.x, point.y))
                        .collect(),
                    weights: (!weights.is_empty()).then(|| weights.clone()),
                    periodic: false,
                }
            }
            _ => return None,
        };
        Some(SketchEntity {
            id: neutral_sketch_curve_id(&curve.id, curve.primary_id, curve.secondary_id),
            sketch: neutral_sketch_id(placement),
            construction: false,
            native_ref: Some(curve.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        })
    }));
    entities.sort_by_key(|entity| entity.id.clone());
    for sketch in &mut sketches {
        sketch.profiles = closed_sketch_profiles(&sketch.id, &entities, linear_tolerance);
    }
    (sketches, entities)
}

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub histories: &'a [crate::history_records::AsmHistory],
    pub linear_tolerance: f64,
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: ExtrudeProfileResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    for feature in features {
        let Some(scope) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|candidate| candidate.id == scope) else {
            continue;
        };
        let mut matching_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        matching_groups.sort_by_key(|group| group.scope_reference_ordinal);
        if matching_groups.is_empty() {
            continue;
        }
        let FeatureDefinition::Extrude { profile, .. } = &mut feature.definition else {
            continue;
        };
        let ProfileRef::Sketch(sketch_id) = profile else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        *profile = if let [group] = matching_groups.as_slice() {
            resolved_extrude_profile_selection(
                sketch_id,
                group,
                members,
                sketch,
                resolution,
                scope.history_state_id,
                scope.previous_history_state_id,
            )
        } else {
            ProfileRef::SketchSelection {
                sketch: sketch_id.clone(),
                selections: matching_groups
                    .iter()
                    .map(|group| group.id.clone())
                    .collect(),
            }
        };
    }
}

#[derive(Debug, PartialEq)]
enum ResolvedProfileSelection {
    Loops(Vec<u32>),
    Regions(Vec<cadmpeg_ir::features::SketchProfileRegion>),
}

fn resolved_extrude_profile_selection(
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> cadmpeg_ir::features::ProfileRef {
    use cadmpeg_ir::features::ProfileRef;

    let mut selection_members = members
        .iter()
        .filter(|member| {
            native_stream(&member.id) == native_stream(&group.id)
                && member.group_record_index == group.record_index
        })
        .collect::<Vec<_>>();
    selection_members.sort_by_key(|member| member.group_member_ordinal);
    let exact_member_run = selection_members.len() == group.members.len()
        && selection_members
            .iter()
            .zip(&group.members)
            .all(|(member, record_index)| member.record_index == *record_index);
    let resolved_profiles = exact_member_run.then(|| {
        let mut selected = Vec::new();
        for member in &selection_members {
            let SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            } = member.resolved_geometry.as_ref()?
            else {
                return None;
            };
            let entity = neutral_sketch_curve_id(&member.id, *primary_id, *secondary_id);
            let matches = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity))
                .map(|(index, _)| u32::try_from(index).ok())
                .collect::<Option<Vec<_>>>()?;
            let [profile_index] = matches.as_slice() else {
                return None;
            };
            if !selected.contains(profile_index) {
                selected.push(*profile_index);
            }
        }
        (!selected.is_empty()).then_some(ResolvedProfileSelection::Loops(selected))
    });
    let resolved_profiles = resolved_profiles
        .flatten()
        .or_else(|| {
            exact_member_run.then(|| {
                historical_selection_regions(
                    &selection_members,
                    sketch,
                    resolution.entities,
                    resolution.histories,
                    resolution.linear_tolerance,
                )
            })?
        })
        .or_else(|| {
            transition_profile_selection(
                sketch,
                resolution.entities,
                resolution.histories,
                history_state_id?,
                previous_history_state_id?,
                resolution.linear_tolerance,
            )
        });
    match resolved_profiles {
        Some(ResolvedProfileSelection::Loops(profiles)) => ProfileRef::SketchProfiles {
            sketch: sketch_id.clone(),
            profiles,
        },
        Some(ResolvedProfileSelection::Regions(regions)) => ProfileRef::SketchRegions {
            sketch: sketch_id.clone(),
            regions,
        },
        None => ProfileRef::SketchSelection {
            sketch: sketch_id.clone(),
            selections: vec![group.id.clone()],
        },
    }
}

fn transition_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
) -> Option<ResolvedProfileSelection> {
    let mut states = histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
    {
        return None;
    }
    let topology = state.topology.as_ref()?;
    let inserted_faces = &state.transition.as_ref()?.topology.faces.inserted;
    let tolerance = linear_tolerance.max(1.0e-7);
    let mut profiles = Vec::new();
    for face in inserted_faces {
        let Some(points) = historical_face_points(*face, topology) else {
            continue;
        };
        let Some(selection) = selection_containing_points(sketch, entities, &points, tolerance)
        else {
            continue;
        };
        if !profiles.contains(&selection) {
            profiles.push(selection);
        }
    }
    (profiles.len() == 1).then(|| profiles.remove(0))
}

fn historical_face_points(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    let loops = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)?;
    let mut positions = Vec::new();
    for loop_ref in &loops.member_refs {
        let coedges = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)?;
        for coedge_ref in &coedges.member_refs {
            let coedge = topology
                .coedge_topology
                .iter()
                .find(|coedge| coedge.coedge == *coedge_ref)?;
            let edge = topology
                .edge_vertices
                .iter()
                .find(|edge| edge.edge == coedge.edge)?;
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                let point_ref = topology
                    .vertex_points
                    .iter()
                    .find(|binding| binding.entity == vertex_ref)?
                    .carrier;
                let position = topology
                    .point_positions
                    .iter()
                    .find(|point| point.point == point_ref)?
                    .position;
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
    }
    (positions.len() >= 3).then_some(positions)
}

fn historical_selection_regions(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    linear_tolerance: f64,
) -> Option<ResolvedProfileSelection> {
    let member_points = members
        .iter()
        .map(|member| historical_member_points(member, histories))
        .collect::<Option<Vec<_>>>()?;
    if member_points.iter().any(Vec::is_empty) {
        return None;
    }
    let tolerance = linear_tolerance.max(1.0e-7);
    let all_points = member_points.iter().flatten().copied().collect::<Vec<_>>();
    if let Some(selection) = selection_containing_points(sketch, entities, &all_points, tolerance) {
        return Some(selection);
    }
    ordered_unique_profile_selections(
        member_points
            .iter()
            .map(|points| selection_containing_points(sketch, entities, points, tolerance)),
    )
}

fn ordered_unique_profile_selections(
    matches: impl IntoIterator<Item = Option<ResolvedProfileSelection>>,
) -> Option<ResolvedProfileSelection> {
    let mut loops = Vec::new();
    let mut regions = Vec::new();
    for selection in matches {
        match selection? {
            ResolvedProfileSelection::Loops(selected) if regions.is_empty() => {
                for loop_index in selected {
                    if !loops.contains(&loop_index) {
                        loops.push(loop_index);
                    }
                }
            }
            ResolvedProfileSelection::Regions(selected) if loops.is_empty() => {
                for region in selected {
                    if !regions.contains(&region) {
                        regions.push(region);
                    }
                }
            }
            _ => return None,
        }
    }
    if !loops.is_empty() {
        Some(ResolvedProfileSelection::Loops(loops))
    } else if !regions.is_empty() {
        Some(ResolvedProfileSelection::Regions(regions))
    } else {
        None
    }
}

fn selection_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<ResolvedProfileSelection> {
    let projected = points
        .iter()
        .map(|point| project_to_sketch(sketch, *point))
        .collect::<Vec<_>>();
    let boundaries = sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| {
            projected.iter().all(|point| {
                profile.iter().any(|use_| {
                    entities
                        .iter()
                        .find(|entity| entity.id == use_.entity)
                        .is_some_and(|entity| point_on_sketch_entity(*point, entity, tolerance))
                })
            })
        })
        .map(|(index, _)| u32::try_from(index).ok())
        .collect::<Option<Vec<_>>>()?;
    if let [profile] = boundaries.as_slice() {
        return Some(ResolvedProfileSelection::Loops(vec![*profile]));
    }
    if !boundaries.is_empty() {
        return None;
    }
    region_containing_points(sketch, entities, points, tolerance)
        .map(|region| ResolvedProfileSelection::Regions(vec![region]))
}

fn region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<cadmpeg_ir::features::SketchProfileRegion> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let polygons = sketch
        .profiles
        .iter()
        .map(|profile| line_profile_vertices(profile, entities, tolerance))
        .collect::<Vec<_>>();
    let projected = points
        .iter()
        .map(|point| project_to_sketch(sketch, *point))
        .collect::<Vec<_>>();
    if projected.iter().any(|point| {
        sketch.profiles.iter().any(|profile| {
            profile.iter().any(|use_| {
                entities
                    .iter()
                    .find(|entity| entity.id == use_.entity)
                    .is_some_and(|entity| point_on_sketch_entity(*point, entity, tolerance))
            })
        })
    }) {
        return None;
    }
    let containing = polygons
        .iter()
        .enumerate()
        .filter_map(|(index, polygon)| Some((index, polygon.as_ref()?)))
        .filter(|(_, polygon)| {
            projected
                .iter()
                .all(|point| point_in_polygon(*point, polygon))
        })
        .map(|(index, polygon)| (index, polygon, polygon_area(polygon)))
        .collect::<Vec<_>>();
    if containing.iter().enumerate().any(|(left_index, left)| {
        containing.iter().skip(left_index + 1).any(|right| {
            !polygon_strictly_contains(left.1, right.1)
                && !polygon_strictly_contains(right.1, left.1)
        })
    }) {
        return None;
    }
    let &(outer, outer_polygon, _) = containing
        .iter()
        .min_by(|left, right| left.2.total_cmp(&right.2))?;
    let mut holes = Vec::new();
    for (candidate, polygon) in polygons
        .iter()
        .enumerate()
        .filter_map(|(index, polygon)| Some((index, polygon.as_ref()?)))
    {
        if candidate == outer || !polygon_strictly_contains(outer_polygon, polygon) {
            continue;
        }
        let candidate_area = polygon_area(polygon);
        let parent = polygons
            .iter()
            .enumerate()
            .filter_map(|(index, parent)| Some((index, parent.as_ref()?)))
            .filter(|(index, parent)| {
                *index != candidate
                    && polygon_area(parent) > candidate_area
                    && polygon_strictly_contains(parent, polygon)
            })
            .min_by(|left, right| polygon_area(left.1).total_cmp(&polygon_area(right.1)))
            .map(|(index, _)| index);
        if parent == Some(outer) {
            holes.push(u32::try_from(candidate).ok()?);
        }
    }
    Some(SketchProfileRegion {
        outer: u32::try_from(outer).ok()?,
        holes,
    })
}

fn line_profile_vertices(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<Point2>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut vertices = Vec::with_capacity(profile.len());
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
        let SketchGeometry::Line { start, end } = entity.geometry else {
            return None;
        };
        let [start, end] = if use_.reversed {
            [end, start]
        } else {
            [start, end]
        };
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return None;
        }
        vertices.push(start);
        previous_end = Some(end);
    }
    if vertices.len() < 3
        || previous_end.is_none_or(|end| point_distance(end, vertices[0]) > tolerance)
    {
        return None;
    }
    Some(vertices)
}

fn point_in_polygon(point: Point2, vertices: &[Point2]) -> bool {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
        .filter(|(start, end)| {
            (start.v > point.v) != (end.v > point.v)
                && point.u < start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
        })
        .count()
        % 2
        == 1
}

fn polygon_area(vertices: &[Point2]) -> f64 {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
        .map(|(start, end)| start.u * end.v - end.u * start.v)
        .sum::<f64>()
        .abs()
        * 0.5
}

fn polygon_strictly_contains(outer: &[Point2], inner: &[Point2]) -> bool {
    inner.iter().all(|point| point_in_polygon(*point, outer))
        && !polygon_edges(outer).any(|outer_edge| {
            polygon_edges(inner).any(|inner_edge| segments_intersect(outer_edge, inner_edge))
        })
}

fn polygon_edges(vertices: &[Point2]) -> impl Iterator<Item = (Point2, Point2)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

fn segments_intersect(left: (Point2, Point2), right: (Point2, Point2)) -> bool {
    fn side(line: (Point2, Point2), point: Point2) -> f64 {
        (line.1.u - line.0.u) * (point.v - line.0.v) - (line.1.v - line.0.v) * (point.u - line.0.u)
    }

    let left_start = side(left, right.0);
    let left_end = side(left, right.1);
    let right_start = side(right, left.0);
    let right_end = side(right, left.1);
    if left_start == 0.0 && left_end == 0.0 && right_start == 0.0 && right_end == 0.0 {
        let overlaps = |a0: f64, a1: f64, b0: f64, b1: f64| {
            a0.min(a1) <= b0.max(b1) && b0.min(b1) <= a0.max(a1)
        };
        return overlaps(left.0.u, left.1.u, right.0.u, right.1.u)
            && overlaps(left.0.v, left.1.v, right.0.v, right.1.v);
    }
    left_start * left_end <= 0.0 && right_start * right_end <= 0.0
}

fn historical_member_points(
    member: &DesignExtrudeSelectionMember,
    histories: &[crate::history_records::AsmHistory],
) -> Option<Vec<Point3>> {
    let kind = member.historical_entity_kind?;
    let entity_ref = member
        .historical_entity_ref
        .or_else(|| i64::try_from(member.local_id).ok())?;
    let mut positions = Vec::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        if !member.historical_state_ids.contains(&state.state_id) {
            continue;
        }
        let Some(topology) = state.topology.as_ref() else {
            continue;
        };
        if let Some(mut state_positions) = historical_entity_positions(kind, entity_ref, topology) {
            positions.append(&mut state_positions);
        }
    }
    positions.dedup_by(|a, b| a == b);
    (!positions.is_empty()).then_some(positions)
}

fn historical_entity_positions(
    kind: crate::records::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    use crate::records::AsmHistoricalEntityKind;

    let mut positions = Vec::new();
    let edge_refs = match kind {
        AsmHistoricalEntityKind::Coedge => topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == local_id)
            .map(|coedge| coedge.edge)
            .collect::<Vec<_>>(),
        AsmHistoricalEntityKind::Edge => vec![local_id],
        AsmHistoricalEntityKind::Curve => topology
            .edge_curves
            .iter()
            .filter(|binding| binding.carrier == Some(local_id))
            .map(|binding| binding.entity)
            .collect(),
        AsmHistoricalEntityKind::Loop => topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.owner_loop == local_id)
            .map(|coedge| coedge.edge)
            .collect(),
        AsmHistoricalEntityKind::Pcurve => topology
            .coedge_pcurves
            .iter()
            .filter(|binding| binding.carrier == Some(local_id))
            .filter_map(|binding| {
                topology
                    .coedge_topology
                    .iter()
                    .find(|coedge| coedge.coedge == binding.entity)
                    .map(|coedge| coedge.edge)
            })
            .collect(),
        AsmHistoricalEntityKind::Vertex => {
            positions.extend(historical_vertex_positions(topology, local_id));
            Vec::new()
        }
        AsmHistoricalEntityKind::Point => {
            positions.extend(
                topology
                    .point_positions
                    .iter()
                    .filter(|point| point.point == local_id)
                    .map(|point| point.position),
            );
            Vec::new()
        }
        _ => return None,
    };
    for edge_ref in edge_refs {
        let edge = topology
            .edge_vertices
            .iter()
            .find(|edge| edge.edge == edge_ref)?;
        let start = historical_vertex_positions(topology, edge.start_vertex).collect::<Vec<_>>();
        let end = historical_vertex_positions(topology, edge.end_vertex).collect::<Vec<_>>();
        if start.is_empty() || end.is_empty() {
            return None;
        }
        positions.extend(start);
        positions.extend(end);
    }
    (!positions.is_empty()).then_some(positions)
}

fn historical_vertex_positions(
    topology: &crate::history_records::AsmHistoricalTopology,
    vertex_ref: i64,
) -> impl Iterator<Item = Point3> + '_ {
    topology
        .vertex_points
        .iter()
        .filter(move |binding| binding.entity == vertex_ref)
        .filter_map(|binding| {
            topology
                .point_positions
                .iter()
                .find(|point| point.point == binding.carrier)
                .map(|point| point.position)
        })
}

fn project_to_sketch(sketch: &cadmpeg_ir::sketches::Sketch, point: Point3) -> Point2 {
    let x = point.x - sketch.origin.x;
    let y = point.y - sketch.origin.y;
    let z = point.z - sketch.origin.z;
    let v_axis = Vector3::new(
        sketch.normal.y * sketch.u_axis.z - sketch.normal.z * sketch.u_axis.y,
        sketch.normal.z * sketch.u_axis.x - sketch.normal.x * sketch.u_axis.z,
        sketch.normal.x * sketch.u_axis.y - sketch.normal.y * sketch.u_axis.x,
    );
    Point2::new(
        x * sketch.u_axis.x + y * sketch.u_axis.y + z * sketch.u_axis.z,
        x * v_axis.x + y * v_axis.y + z * v_axis.z,
    )
}

fn point_on_sketch_entity(
    point: Point2,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    match &entity.geometry {
        SketchGeometry::Line { start, end } => {
            let dx = end.u - start.u;
            let dy = end.v - start.v;
            let length_squared = dx * dx + dy * dy;
            if length_squared == 0.0 {
                return point_distance(point, *start) <= tolerance;
            }
            let t = ((point.u - start.u) * dx + (point.v - start.v) * dy) / length_squared;
            if !(-tolerance..=1.0 + tolerance).contains(&t) {
                return false;
            }
            point_distance(point, Point2::new(start.u + t * dx, start.v + t * dy)) <= tolerance
        }
        SketchGeometry::Circle { center, radius } => {
            (point_distance(point, *center) - radius.0).abs() <= tolerance
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radial_error = (point_distance(point, *center) - radius.0).abs();
            if radial_error > tolerance {
                return false;
            }
            let angle = (point.v - center.v).atan2(point.u - center.u);
            angle_in_sweep(angle, start_angle.0, end_angle.0, tolerance / radius.0)
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } if major_radius.0 > 0.0 && minor_radius.0 > 0.0 => {
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let cosine = major_angle.0.cos();
            let sine = major_angle.0.sin();
            let local_u = du * cosine + dv * sine;
            let local_v = -du * sine + dv * cosine;
            let parameter = (local_v / minor_radius.0).atan2(local_u / major_radius.0);
            let boundary = Point2::new(
                center.u + major_radius.0 * parameter.cos() * cosine
                    - minor_radius.0 * parameter.sin() * sine,
                center.v
                    + major_radius.0 * parameter.cos() * sine
                    + minor_radius.0 * parameter.sin() * cosine,
            );
            if point_distance(point, boundary) > tolerance {
                return false;
            }
            match (start_angle, end_angle) {
                (None, None) => true,
                (Some(start), Some(end)) => angle_in_sweep(
                    parameter,
                    start.0,
                    end.0,
                    tolerance / major_radius.0.min(minor_radius.0),
                ),
                _ => false,
            }
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } if !periodic
            && usize::try_from(*degree).ok().is_some_and(|degree| {
                knots.len() > control_points.len() + degree
                    && knots[..=degree].iter().all(|knot| *knot == knots[degree])
                    && knots[control_points.len()..]
                        .iter()
                        .all(|knot| *knot == knots[control_points.len()])
            }) =>
        {
            cadmpeg_ir::eval::nurbs_pcurve_contains_point(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                point,
                tolerance,
            )
            .unwrap_or(false)
        }
        _ => false,
    }
}

fn angle_in_sweep(angle: f64, start: f64, end: f64, tolerance: f64) -> bool {
    let sweep = end - start;
    if sweep.abs() >= std::f64::consts::TAU - tolerance {
        return true;
    }
    if sweep >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep + tolerance
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU) <= -sweep + tolerance
    }
}

fn point_distance(a: Point2, b: Point2) -> f64 {
    ((a.u - b.u).powi(2) + (a.v - b.v).powi(2)).sqrt()
}

fn closed_sketch_profiles(
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry};

    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return Vec::new();
    }
    let mut profiles = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
        .filter(|entity| {
            matches!(
                entity.geometry,
                SketchGeometry::Circle { .. }
                    | SketchGeometry::Ellipse {
                        start_angle: None,
                        end_angle: None,
                        ..
                    }
            )
        })
        .map(|entity| {
            vec![SketchEntityUse {
                entity: entity.id.clone(),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let edges = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
        .filter_map(|entity| sketch_entity_endpoints(entity).map(|ends| (entity, ends)))
        .collect::<Vec<_>>();
    if edges.is_empty() {
        profiles.sort_by_key(|profile| profile[0].entity.clone());
        return profiles;
    }

    let endpoints = edges
        .iter()
        .flat_map(|(_, [start, end])| [*start, *end])
        .collect::<Vec<_>>();
    let mut parents = (0..endpoints.len()).collect::<Vec<_>>();
    let mut endpoint_cells = HashMap::<(i64, i64), Vec<usize>>::new();
    for (endpoint, point) in endpoints.iter().copied().enumerate() {
        let cell = (
            (point.u / linear_tolerance).floor() as i64,
            (point.v / linear_tolerance).floor() as i64,
        );
        for u_offset in -1..=1 {
            for v_offset in -1..=1 {
                let adjacent = (
                    cell.0.saturating_add(u_offset),
                    cell.1.saturating_add(v_offset),
                );
                for candidate in endpoint_cells.get(&adjacent).into_iter().flatten() {
                    if sketch_endpoints_close(point, endpoints[*candidate], linear_tolerance) {
                        union_endpoint_nodes(&mut parents, endpoint, *candidate);
                    }
                }
            }
        }
        endpoint_cells.entry(cell).or_default().push(endpoint);
    }
    let edge_nodes = (0..edges.len())
        .map(|edge| {
            [
                endpoint_root(&mut parents, edge * 2),
                endpoint_root(&mut parents, edge * 2 + 1),
            ]
        })
        .collect::<Vec<_>>();
    let mut adjacency = HashMap::<usize, Vec<usize>>::new();
    for (edge, [start, end]) in edge_nodes.iter().copied().enumerate() {
        adjacency.entry(start).or_default().push(edge);
        adjacency.entry(end).or_default().push(edge);
    }
    for incident in adjacency.values_mut() {
        incident.sort_by_key(|edge| edges[*edge].0.id.clone());
    }

    let mut visited = vec![false; edges.len()];
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by_key(|edge| edges[*edge].0.id.clone());
    for first_edge in order {
        if visited[first_edge] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![first_edge];
        let mut component_seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !component_seen.insert(edge) {
                continue;
            }
            component.push(edge);
            for node in edge_nodes[edge] {
                pending.extend(adjacency[&node].iter().copied());
            }
        }
        let component_nodes = component
            .iter()
            .flat_map(|edge| edge_nodes[*edge])
            .collect::<HashSet<_>>();
        if component_nodes
            .iter()
            .any(|node| adjacency[node].len() != 2)
        {
            if component
                .iter()
                .all(|edge| matches!(edges[*edge].0.geometry, SketchGeometry::Line { .. }))
            {
                profiles.extend(branched_line_profiles(
                    &component,
                    &edges,
                    &edge_nodes,
                    &adjacency,
                    linear_tolerance,
                ));
            }
            for edge in component {
                visited[edge] = true;
            }
            continue;
        }

        component.sort_by_key(|edge| edges[*edge].0.id.clone());
        let first_edge = component[0];
        let start_node = edge_nodes[first_edge][0];
        let mut current_node = edge_nodes[first_edge][1];
        let mut profile = vec![SketchEntityUse {
            entity: edges[first_edge].0.id.clone(),
            reversed: false,
        }];
        visited[first_edge] = true;
        while current_node != start_node {
            let Some(next_edge) = adjacency[&current_node]
                .iter()
                .copied()
                .find(|edge| !visited[*edge])
            else {
                profile.clear();
                break;
            };
            let [stored_start, stored_end] = edge_nodes[next_edge];
            let reversed = stored_end == current_node;
            current_node = if reversed { stored_start } else { stored_end };
            visited[next_edge] = true;
            profile.push(SketchEntityUse {
                entity: edges[next_edge].0.id.clone(),
                reversed,
            });
        }
        if !profile.is_empty() && component.iter().all(|edge| visited[*edge]) {
            profiles.push(profile);
        }
    }
    profiles.sort_by_key(|profile| profile[0].entity.clone());
    profiles
}

fn branched_line_profiles(
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    adjacency: &HashMap<usize, Vec<usize>>,
    linear_tolerance: f64,
) -> Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::SketchEntityUse;

    let component = component.iter().copied().collect::<HashSet<_>>();
    let mut outgoing = HashMap::<usize, Vec<usize>>::new();
    for edge in &component {
        outgoing
            .entry(edge_nodes[*edge][0])
            .or_default()
            .push(edge * 2);
        outgoing
            .entry(edge_nodes[*edge][1])
            .or_default()
            .push(edge * 2 + 1);
    }
    for half_edges in outgoing.values_mut() {
        half_edges.sort_by(|first, second| {
            let angle = |half_edge: usize| {
                let edge = half_edge / 2;
                let [start, end] = edges[edge].1;
                let (from, to) = if half_edge.is_multiple_of(2) {
                    (start, end)
                } else {
                    (end, start)
                };
                (to.v - from.v).atan2(to.u - from.u)
            };
            angle(*first)
                .total_cmp(&angle(*second))
                .then_with(|| edges[*first / 2].0.id.cmp(&edges[*second / 2].0.id))
                .then_with(|| first.cmp(second))
        });
    }

    let mut next = HashMap::new();
    for edge in &component {
        for half_edge in [edge * 2, edge * 2 + 1] {
            let destination = edge_nodes[*edge][usize::from(half_edge.is_multiple_of(2))];
            let around = &outgoing[&destination];
            let twin = half_edge ^ 1;
            let twin_position = around
                .iter()
                .position(|candidate| *candidate == twin)
                .expect("each line half-edge has a twin at its destination");
            next.insert(
                half_edge,
                around[(twin_position + around.len() - 1) % around.len()],
            );
        }
    }

    let mut profiles = Vec::new();
    let mut visited = HashSet::new();
    let mut starts = component
        .iter()
        .flat_map(|edge| [edge * 2, edge * 2 + 1])
        .collect::<Vec<_>>();
    starts.sort_by_key(|half_edge| (edges[*half_edge / 2].0.id.clone(), half_edge % 2));
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut current = start;
        let mut profile = Vec::new();
        let mut twice_area = 0.0;
        loop {
            if !visited.insert(current) {
                if current != start {
                    profile.clear();
                }
                break;
            }
            let edge = current / 2;
            let [stored_start, stored_end] = edges[edge].1;
            let reversed = !current.is_multiple_of(2);
            let (from, to) = if reversed {
                (stored_end, stored_start)
            } else {
                (stored_start, stored_end)
            };
            twice_area += from.u * to.v - from.v * to.u;
            profile.push(SketchEntityUse {
                entity: edges[edge].0.id.clone(),
                reversed,
            });
            current = next[&current];
        }
        if !profile.is_empty() && twice_area > 2.0 * linear_tolerance * linear_tolerance {
            profiles.push(profile);
        }
    }

    debug_assert!(component.iter().all(|edge| {
        edge_nodes[*edge]
            .iter()
            .all(|node| adjacency[node].contains(edge))
    }));
    profiles
}

fn sketch_entity_endpoints(entity: &cadmpeg_ir::sketches::SketchEntity) -> Option<[Point2; 2]> {
    use cadmpeg_ir::sketches::SketchGeometry;

    match &entity.geometry {
        SketchGeometry::Line { start, end } => Some([*start, *end]),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            Point2::new(
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ),
            Point2::new(
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start_angle),
            end_angle: Some(end_angle),
        } => {
            let point_at = |parameter: f64| {
                let x = major_radius.0 * parameter.cos();
                let y = minor_radius.0 * parameter.sin();
                Point2::new(
                    center.u + x * major_angle.0.cos() - y * major_angle.0.sin(),
                    center.v + x * major_angle.0.sin() + y * major_angle.0.cos(),
                )
            };
            Some([point_at(start_angle.0), point_at(end_angle.0)])
        }
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } => Some([*control_points.first()?, *control_points.last()?]),
        _ => None,
    }
}

fn sketch_endpoints_close(first: Point2, second: Point2, tolerance: f64) -> bool {
    (first.u - second.u).hypot(first.v - second.v) <= tolerance
}

fn endpoint_root(parents: &mut [usize], node: usize) -> usize {
    if parents[node] != node {
        parents[node] = endpoint_root(parents, parents[node]);
    }
    parents[node]
}

fn union_endpoint_nodes(parents: &mut [usize], first: usize, second: usize) {
    let first = endpoint_root(parents, first);
    let second = endpoint_root(parents, second);
    if first != second {
        parents[second] = first;
    }
}

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its member roles do not determine neutral loci.
pub fn project_sketch_constraints(
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    relations: &[SketchRelation],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition as Definition, SketchNativeOperand,
    };

    let sketches = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                neutral_sketch_id(placement),
            ))
        })
        .collect::<HashMap<_, _>>();
    let record_keys_by_native_ref = points
        .iter()
        .filter_map(|point| {
            Some((
                point.id.as_str(),
                (native_stream(&point.id)?, point.record_index),
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                curve.id.as_str(),
                (native_stream(&curve.id)?, curve.record_index),
            ))
        }))
        .collect::<HashMap<_, _>>();
    let projected = entities
        .iter()
        .filter_map(|entity| {
            entity
                .native_ref
                .as_deref()
                .and_then(|native_ref| record_keys_by_native_ref.get(native_ref).copied())
                .map(|key| (key, entity))
        })
        .collect::<HashMap<_, _>>();
    let point_native_refs = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                point.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let curve_native_refs = curves
        .iter()
        .filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                curve.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();

    let mut constraints = relations
        .iter()
        .filter_map(|relation| {
            let scope = native_stream(&relation.id)?;
            let sketch = sketches.get(&(scope, relation.owner_reference))?.clone();
            let member_entities = relation
                .members
                .iter()
                .filter_map(|record_index| projected.get(&(scope, *record_index)).copied())
                .collect::<Vec<_>>();
            let exact = relation.unknown_constraint_bits == 0
                && relation.constraint_kinds.len() == 1
                && member_entities.len() == relation.members.len();
            let definition = exact
                .then(|| exact_atomic_constraint(relation.constraint_kinds[0], &member_entities))
                .flatten()
                .or_else(|| exact_offset_constraint(relation, scope, &projected))
                .unwrap_or_else(|| Definition::Native {
                    native_kind: relation_kind_name(relation),
                    entities: member_entities
                        .iter()
                        .map(|entity| entity.id.clone())
                        .collect(),
                    parameter: None,
                    operands: relation
                        .resolved_members
                        .iter()
                        .filter_map(|operand| {
                            let record_index = relation_operand_index(operand);
                            (!projected.contains_key(&(scope, record_index))).then(|| {
                                SketchNativeOperand {
                                    native_kind: relation_operand_kind(operand).into(),
                                    object_index: record_index,
                                    native_ref: point_native_refs
                                        .get(&(scope, record_index))
                                        .copied()
                                        .or_else(|| {
                                            curve_native_refs.get(&(scope, record_index)).copied()
                                        })
                                        .map(str::to_owned),
                                }
                            })
                        })
                        .collect(),
                });
            Some(SketchConstraint {
                id: neutral_sketch_constraint_id(&relation.id, relation.record_index),
                sketch,
                definition,
                native_ref: Some(relation.id.clone()),
            })
        })
        .collect::<Vec<_>>();
    constraints.sort_by_key(|constraint| constraint.id.clone());
    constraints
}

/// Project dimensional parameter companions into parameter-backed sketch
/// constraints. Two-locus dimensions have neutral semantics; aggregate and
/// role-dependent forms remain explicit native constraints.
#[allow(clippy::too_many_arguments)]
pub fn project_dimension_constraints(
    placements: &[DesignSketchPlacement],
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionNullLocusPair],
    companions: &[DesignParameterCompanion],
    recipe_records: &[DesignDimensionRecipeRecord],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition as Definition, SketchGeometry,
        SketchNativeOperand,
    };

    let sketches = placements
        .iter()
        .filter_map(|placement| {
            let scope = native_stream(&placement.id)?;
            u32::try_from(placement.entity_suffix)
                .ok()
                .map(|suffix| ((scope, suffix), neutral_sketch_id(placement)))
        })
        .collect::<HashMap<_, _>>();
    let sketches_by_scope = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (native_stream(&placement.id)?, placement.scope_record_index),
                neutral_sketch_id(placement),
            ))
        })
        .collect::<HashMap<_, _>>();
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let parameter_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (native_stream(&owner.id)?, owner.companion_record_index),
                owner.parameter_record_index,
            ))
        })
        .collect::<HashMap<_, _>>();
    let native_geometry = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                ("point", point.owner_reference, point.id.as_str()),
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                ("curve", curve.owner_reference, curve.id.as_str()),
            ))
        }))
        .collect::<HashMap<_, _>>();
    let record_indices_by_native_ref = native_geometry
        .iter()
        .map(|(key, (_, _, native_ref))| (*native_ref, *key))
        .collect::<HashMap<_, _>>();
    let projected = entities
        .iter()
        .filter_map(|entity| {
            let native_ref = entity.native_ref.as_deref()?;
            record_indices_by_native_ref
                .get(native_ref)
                .map(|key| (*key, entity))
        })
        .collect::<HashMap<_, _>>();

    let parameter_for = |scope: &str, companion_record_index: u32| {
        let record_index = *parameter_by_companion.get(&(scope, companion_record_index))?;
        let parameter = *parameters.get(&(scope, record_index))?;
        Some((parameter, neutral_parameter_id(parameter)))
    };
    let sketch_for_geometry = |scope: &str, indices: &[u32]| {
        let mut owners = indices
            .iter()
            .filter_map(|record_index| native_geometry.get(&(scope, *record_index))?.1)
            .collect::<HashSet<_>>();
        (owners.len() == 1)
            .then(|| owners.drain().next())
            .flatten()
            .and_then(|owner| sketches.get(&(scope, owner)).cloned())
    };
    let native_operands = |scope: &str, indices: &[u32]| {
        indices
            .iter()
            .filter(|record_index| !projected.contains_key(&(scope, **record_index)))
            .map(|record_index| {
                let (native_kind, _, native_ref) = native_geometry
                    .get(&(scope, *record_index))
                    .copied()
                    .unwrap_or(("record", None, ""));
                SketchNativeOperand {
                    native_kind: native_kind.into(),
                    object_index: *record_index,
                    native_ref: (!native_ref.is_empty()).then(|| native_ref.to_owned()),
                }
            })
            .collect::<Vec<_>>()
    };
    let native_definition =
        |scope: &str, source_kind: &str, indices: &[u32], parameter| Definition::Native {
            native_kind: source_kind.to_owned(),
            entities: indices
                .iter()
                .filter_map(|record_index| {
                    projected
                        .get(&(scope, *record_index))
                        .map(|entity| entity.id.clone())
                })
                .collect(),
            parameter: Some(parameter),
            operands: native_operands(scope, indices),
        };
    let exact_definition = |scope: &str,
                            source_kind: &str,
                            indices: &[u32],
                            evaluated_value: f64,
                            parameter: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        let entities = indices
            .iter()
            .map(|record_index| projected.get(&(scope, *record_index)).copied())
            .collect::<Option<Vec<_>>>()?;
        if let [entity] = entities.as_slice() {
            if let Some(definition) =
                radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
            {
                return Some(definition);
            }
        }
        if source_kind.starts_with("Linear Dimension") && entities.len() == 2 {
            return Some(Definition::Distance {
                entities: entities.iter().map(|entity| entity.id.clone()).collect(),
                parameter,
            });
        }
        if source_kind.starts_with("Angular Dimension")
            && entities.len() == 2
            && entities
                .iter()
                .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        {
            return Some(Definition::Angle {
                first: entities[0].id.clone(),
                second: entities[1].id.clone(),
                parameter,
            });
        }
        if source_kind.starts_with("Angular Dimension") && entities.len() == 2 {
            let (first, second) =
                indirect_angular_lines(scope, &entities, evaluated_value, &projected)?;
            return Some(Definition::Angle {
                first,
                second,
                parameter,
            });
        }
        None
    };
    let exact_group_definition = |scope: &str,
                                  group: &DesignDimensionLocusGroup,
                                  parameter: &DesignParameter,
                                  parameter_id: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        let entities = group
            .loci
            .iter()
            .map(|locus| {
                projected
                    .get(&(scope, locus.geometry_record_index))
                    .copied()
            })
            .collect::<Option<Vec<_>>>()?;
        if let [entity] = entities.as_slice() {
            if let Some(definition) = radial_dimension_definition(
                entity,
                &parameter.source_kind,
                parameter.evaluated_value,
                parameter_id.clone(),
            ) {
                return Some(definition);
            }
        }
        if parameter.source_kind.starts_with("Angular Dimension") {
            let indices = group
                .loci
                .iter()
                .map(|locus| locus.geometry_record_index)
                .collect::<Vec<_>>();
            return exact_definition(
                scope,
                &parameter.source_kind,
                &indices,
                parameter.evaluated_value,
                parameter_id,
            );
        }
        if parameter.source_kind.starts_with("Linear Dimension") {
            if group.state == 0x20 && group.unknown_constraint_bits == 0 {
                let loci = group
                    .loci
                    .iter()
                    .map(|locus| (locus.geometry_record_index, locus.role))
                    .collect::<Vec<_>>();
                let entities_by_record = group
                    .loci
                    .iter()
                    .zip(&entities)
                    .map(|(locus, entity)| (locus.geometry_record_index, *entity))
                    .collect::<HashMap<_, _>>();
                return exact_counted_offset(&loci, &group.return_members, &entities_by_record);
            }
            if let Some(definition) = directional_point_dimension(
                &entities,
                parameter.evaluated_value * 10.0,
                parameter_id.clone(),
            ) {
                return Some(definition);
            }
            if group.state == 0 && group.unknown_constraint_bits == 0 {
                if let Some(definition) = exact_counted_dimension_relation(&entities) {
                    return Some(definition);
                }
                return two_locus_distance_dimension(&entities, parameter_id);
            }
        }
        None
    };
    let exact_pair_companions = pairs
        .iter()
        .filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            let (parameter, parameter_id) = parameter_for(scope, pair.companion_record_index)?;
            let indices = [
                pair.first_geometry_record_index,
                pair.second_geometry_record_index,
            ];
            exact_definition(
                scope,
                &parameter.source_kind,
                &indices,
                parameter.evaluated_value,
                parameter_id,
            )
            .map(|_| (scope.to_owned(), pair.companion_record_index))
        })
        .collect::<HashSet<_>>();
    let locus_companions = pairs
        .iter()
        .filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        })
        .chain(groups.iter().filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.companion_record_index,
            ))
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        }))
        .collect::<HashSet<_>>();

    let mut constraints = pairs
        .iter()
        .filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            let (parameter, parameter_id) = parameter_for(scope, pair.companion_record_index)?;
            let indices = [
                pair.first_geometry_record_index,
                pair.second_geometry_record_index,
            ];
            let sketch = sketch_for_geometry(scope, &indices)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "pair");
            let definition = exact_definition(
                scope,
                &parameter.source_kind,
                &indices,
                parameter.evaluated_value,
                parameter_id.clone(),
            )
            .unwrap_or_else(|| {
                native_definition(scope, &parameter.source_kind, &indices, parameter_id)
            });
            Some(SketchConstraint {
                id: constraint_id,
                sketch,
                definition,
                native_ref: Some(pair.id.clone()),
            })
        })
        .chain(groups.iter().filter_map(|group| {
            let scope = native_stream(&group.id)?;
            if exact_pair_companions.contains(&(scope.to_owned(), group.companion_record_index)) {
                return None;
            }
            let (parameter, parameter_id) = parameter_for(scope, group.companion_record_index)?;
            let indices = group
                .loci
                .iter()
                .map(|locus| locus.geometry_record_index)
                .collect::<Vec<_>>();
            let sketch = sketches.get(&(scope, group.owner_reference))?.clone();
            let definition = exact_group_definition(scope, group, parameter, parameter_id.clone())
                .unwrap_or_else(|| {
                    native_definition(scope, &parameter.source_kind, &indices, parameter_id)
                });
            Some(SketchConstraint {
                id: neutral_sketch_constraint_id(&group.id, group.record_index),
                sketch,
                definition,
                native_ref: Some(group.id.clone()),
            })
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            let (parameter, parameter_id) = parameter_for(scope, pair.companion_record_index)?;
            let indices = [pair.geometry_record_index];
            let sketch = sketch_for_geometry(scope, &indices)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "null-pair");
            if let Some(entity) = projected.get(&(scope, pair.geometry_record_index)) {
                if let Some(definition) = null_locus_dimension_definition(
                    pair,
                    entity,
                    &parameter.source_kind,
                    parameter.evaluated_value,
                    parameter_id.clone(),
                ) {
                    return Some(SketchConstraint {
                        id: constraint_id,
                        sketch,
                        definition,
                        native_ref: Some(pair.id.clone()),
                    });
                }
            }
            let mut operands = vec![SketchNativeOperand {
                native_kind: "null_locus".into(),
                object_index: 0,
                native_ref: None,
            }];
            operands.extend(native_operands(scope, &indices));
            Some(SketchConstraint {
                id: constraint_id,
                sketch,
                definition: Definition::Native {
                    native_kind: parameter.source_kind.clone(),
                    entities: indices
                        .iter()
                        .filter_map(|record_index| {
                            projected
                                .get(&(scope, *record_index))
                                .map(|entity| entity.id.clone())
                        })
                        .collect(),
                    parameter: Some(parameter_id),
                    operands,
                },
                native_ref: Some(pair.id.clone()),
            })
        }))
        .collect::<Vec<_>>();
    let companions_by_key = companions
        .iter()
        .filter_map(|companion| {
            Some((
                (
                    native_stream(&companion.id)?.to_owned(),
                    companion.record_index,
                ),
                companion,
            ))
        })
        .collect::<HashMap<_, _>>();
    let owners_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (
                    native_stream(&owner.id)?.to_owned(),
                    owner.companion_record_index,
                ),
                owner,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut recipes_by_companion = BTreeMap::<(String, u32), Vec<_>>::new();
    for record in recipe_records {
        let Some(scope) = native_stream(&record.id) else {
            continue;
        };
        let key = (scope.to_owned(), record.companion_record_index);
        if !locus_companions.contains(&key) {
            recipes_by_companion.entry(key).or_default().push(record);
        }
    }
    for records in recipes_by_companion.values_mut() {
        records.sort_by_key(|record| record.recipe_ordinal);
    }
    constraints.extend(recipes_by_companion.into_iter().filter_map(
        |((scope, companion_record_index), records)| {
            let companion = companions_by_key.get(&(scope.clone(), companion_record_index))?;
            let owner = owners_by_companion.get(&(scope.clone(), companion_record_index))?;
            let (parameter, parameter_id) = parameter_for(&scope, companion_record_index)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "recipe-group");
            let sketch = sketches_by_scope
                .get(&(scope.as_str(), owner.scope_record_index))?
                .clone();
            let linear_candidates = parameter
                .source_kind
                .starts_with("Linear Dimension")
                .then(|| {
                    recipe_linear_dimension_candidates(
                        entities,
                        &sketch,
                        parameter.evaluated_value * 10.0,
                        &parameter_id,
                    )
                })
                .unwrap_or_default();
            let repeated = repeated_linear_dimension(&linear_candidates, parameter_id.clone());
            let definition = match (linear_candidates.as_slice(), repeated) {
                ([definition], _) => definition.clone(),
                (_, Some(definition)) => definition,
                _ => Definition::Native {
                    native_kind: parameter.source_kind.clone(),
                    entities: recipe_dimension_candidate_entities(&linear_candidates),
                    parameter: Some(parameter_id),
                    operands: records
                        .into_iter()
                        .map(|record| SketchNativeOperand {
                            native_kind: "construction_recipe".into(),
                            object_index: record.record_index,
                            native_ref: Some(record.id.clone()),
                        })
                        .collect(),
                },
            };
            Some(SketchConstraint {
                id: constraint_id,
                sketch,
                definition,
                native_ref: Some(companion.id.clone()),
            })
        },
    ));
    constraints.sort_by_key(|constraint| constraint.id.clone());
    constraints
}

fn repeated_linear_dimension(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinition],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchDistanceMeasurement as Measurement,
        SketchLocus,
    };

    if candidates.len() < 2 {
        return None;
    }
    let mut entities = HashSet::new();
    let mut measurements = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let (first, second, measurement) = match candidate {
            Definition::Distance { entities: pair, .. } => {
                let [first, second] = pair.as_slice() else {
                    return None;
                };
                (
                    first,
                    second,
                    Measurement::Distance {
                        first: SketchLocus::Entity(first.clone()),
                        second: SketchLocus::Entity(second.clone()),
                    },
                )
            }
            Definition::HorizontalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Horizontal {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            Definition::VerticalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Vertical {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            _ => return None,
        };
        if first == second || !entities.insert(first.clone()) || !entities.insert(second.clone()) {
            return None;
        }
        measurements.push(measurement);
    }
    Some(Definition::RepeatedDistance {
        measurements,
        parameter,
    })
}

fn locus_entity_id(
    locus: &cadmpeg_ir::sketches::SketchLocus,
) -> &cadmpeg_ir::sketches::SketchEntityId {
    use cadmpeg_ir::sketches::SketchLocus;
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn null_locus_dimension_definition(
    pair: &DesignDimensionNullLocusPair,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinition as Definition, SketchGeometry,
    };

    if let Some(definition) =
        radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
    {
        return Some(definition);
    }
    (source_kind == "Angular Dimension-2"
        && pair.null_role == 14
        && pair.geometry_role == 3
        && matches!(entity.geometry, SketchGeometry::Line { .. }))
    .then(|| Definition::AngleToAxis {
        entity: entity.id.clone(),
        axis: SketchAxis::Horizontal,
        parameter,
    })
}

fn radial_dimension_definition(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry,
    };

    let radius = match &entity.geometry {
        Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => radius.0,
        _ => return None,
    };
    let measured = if source_kind.starts_with("Radius Dimension") {
        radius
    } else if source_kind.starts_with("Diameter Dimension") {
        2.0 * radius
    } else {
        return None;
    };
    let evaluated = evaluated_value * 10.0;
    let scale = 1.0 + measured.abs().max(evaluated.abs());
    if !evaluated.is_finite() || (measured - evaluated).abs() > 1.0e-9 * scale {
        return None;
    }
    Some(if source_kind.starts_with("Radius Dimension") {
        Definition::Radius {
            entity: entity.id.clone(),
            parameter,
        }
    } else {
        Definition::Diameter {
            entity: entity.id.clone(),
            parameter,
        }
    })
}

/// Remove generic relation parses whose exact stream position is owned by a
/// typed dimension frame.
pub fn remove_dimension_frame_relations(
    relations: &mut Vec<SketchRelation>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionNullLocusPair],
) {
    let dimension_frames =
        pairs
            .iter()
            .filter_map(|pair| Some((native_stream(&pair.id)?.to_owned(), pair.byte_offset)))
            .chain(groups.iter().filter_map(|group| {
                Some((native_stream(&group.id)?.to_owned(), group.byte_offset))
            }))
            .chain(
                null_pairs.iter().filter_map(|pair| {
                    Some((native_stream(&pair.id)?.to_owned(), pair.byte_offset))
                }),
            )
            .collect::<HashSet<_>>();
    relations.retain(|relation| {
        native_stream(&relation.id).is_none_or(|scope| {
            !dimension_frames.contains(&(scope.to_owned(), relation.byte_offset))
        })
    });
}

/// Bind geometry referenced only by dimensional companions to the sketch
/// reached through the parameter scope or the counted frame's explicit owner.
pub fn bind_dimension_loci(
    placements: &[DesignSketchPlacement],
    owners: &[DesignParameterOwner],
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionNullLocusPair],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
) -> Result<(), CodecError> {
    let placements_by_scope = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (native_stream(&placement.id)?, placement.scope_record_index),
                u32::try_from(placement.entity_suffix).ok()?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let scopes_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (native_stream(&owner.id)?, owner.companion_record_index),
                owner.scope_record_index,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut bindings = HashMap::<(String, u32), u32>::new();
    for pair in pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(
            &mut bindings,
            scope,
            pair.first_geometry_record_index,
            owner,
        )?;
        insert_dimension_binding(
            &mut bindings,
            scope,
            pair.second_geometry_record_index,
            owner,
        )?;
    }
    for group in groups {
        let Some(scope) = native_stream(&group.id) else {
            continue;
        };
        for locus in &group.loci {
            insert_dimension_binding(
                &mut bindings,
                scope,
                locus.geometry_record_index,
                group.owner_reference,
            )?;
        }
    }
    for pair in null_pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(&mut bindings, scope, pair.geometry_record_index, owner)?;
    }
    for point in points {
        let Some(scope) = native_stream(&point.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(&(scope.to_owned(), point.record_index))
            .copied()
        else {
            continue;
        };
        if point
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(CodecError::Malformed(format!(
                "Fusion sketch point {} has conflicting relation and dimension owners",
                point.record_index
            )));
        }
    }
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(&(scope.to_owned(), curve.record_index))
            .copied()
        else {
            continue;
        };
        if curve
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(CodecError::Malformed(format!(
                "Fusion sketch curve {} has conflicting relation and dimension owners",
                curve.record_index
            )));
        }
    }
    Ok(())
}

fn insert_dimension_binding(
    bindings: &mut HashMap<(String, u32), u32>,
    scope: &str,
    record_index: u32,
    owner: u32,
) -> Result<(), CodecError> {
    if bindings
        .insert((scope.to_owned(), record_index), owner)
        .is_some_and(|existing| existing != owner)
    {
        return Err(CodecError::Malformed(format!(
            "Fusion dimensional geometry record {record_index} belongs to multiple sketches"
        )));
    }
    Ok(())
}

pub(crate) fn native_stream(id: &str) -> Option<&str> {
    id.rsplit_once(':').map(|(stream, _)| stream)
}

fn exact_atomic_constraint(
    kind: SketchConstraintKind,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry,
    };

    let lines = || {
        (entities.len() == 2
            && entities
                .iter()
                .all(|entity| matches!(entity.geometry, Geometry::Line { .. })))
        .then(|| (entities[0].id.clone(), entities[1].id.clone()))
    };
    match kind {
        SketchConstraintKind::Coincident if entities.len() >= 2 => Some(Definition::Coincident {
            entities: entities.iter().map(|entity| entity.id.clone()).collect(),
        }),
        SketchConstraintKind::Colinear => {
            lines().map(|(first, second)| Definition::Collinear { first, second })
        }
        SketchConstraintKind::Concentric => {
            if entities.len() == 2
                && entities.iter().all(|entity| {
                    matches!(
                        entity.geometry,
                        Geometry::Circle { .. } | Geometry::Arc { .. } | Geometry::Ellipse { .. }
                    )
                })
            {
                return Some(Definition::Concentric {
                    first: entities[0].id.clone(),
                    second: entities[1].id.clone(),
                });
            }
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(first.id.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(second.id.clone()),
                axis: axis.id.clone(),
            })
        }
        SketchConstraintKind::Symmetry => {
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(first.id.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(second.id.clone()),
                axis: axis.id.clone(),
            })
        }
        SketchConstraintKind::EqualLength => {
            lines().map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Parallel => lines()
            .map(|(first, second)| Definition::Parallel { first, second })
            .or_else(|| midpoint_constraint(entities)),
        SketchConstraintKind::Perpendicular => {
            lines().map(|(first, second)| Definition::Perpendicular { first, second })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 1 && matches!(entities[0].geometry, Geometry::Line { .. }) =>
        {
            Some(Definition::Horizontal {
                entity: entities[0].id.clone(),
            })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 1 && matches!(entities[0].geometry, Geometry::Line { .. }) =>
        {
            Some(Definition::Vertical {
                entity: entities[0].id.clone(),
            })
        }
        SketchConstraintKind::Tangent if entities.len() == 2 => Some(Definition::Tangent {
            first: entities[0].id.clone(),
            second: entities[1].id.clone(),
        }),
        SketchConstraintKind::Curvature if entities.len() == 2 => Some(Definition::Curvature {
            first: entities[0].id.clone(),
            second: entities[1].id.clone(),
        }),
        SketchConstraintKind::Midpoint => midpoint_constraint(entities),
        SketchConstraintKind::Equal if entities.len() == 2 => Some(Definition::Equal {
            first: entities[0].id.clone(),
            second: entities[1].id.clone(),
        }),
        _ => None,
    }
}

fn midpoint_constraint(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry, SketchLocus,
    };

    let (line, point) = match entities {
        [line, point]
            if matches!(line.geometry, Geometry::Line { .. })
                && matches!(point.geometry, Geometry::Point { .. }) =>
        {
            (*line, *point)
        }
        [point, line]
            if matches!(line.geometry, Geometry::Line { .. })
                && matches!(point.geometry, Geometry::Point { .. }) =>
        {
            (*line, *point)
        }
        _ => return None,
    };
    let Geometry::Line { start, end } = &line.geometry else {
        unreachable!("line operand matched above")
    };
    let Geometry::Point { position } = &point.geometry else {
        unreachable!("point operand matched above")
    };
    let midpoint = Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5);
    ((position.u - midpoint.u).abs() <= 1.0e-9 && (position.v - midpoint.v).abs() <= 1.0e-9).then(
        || Definition::Midpoint {
            point: SketchLocus::Entity(point.id.clone()),
            entity: line.id.clone(),
        },
    )
}

fn indirect_angular_lines(
    scope: &str,
    operands: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_value: f64,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<(
    cadmpeg_ir::sketches::SketchEntityId,
    cadmpeg_ir::sketches::SketchEntityId,
)> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let (point_ordinal, point, explicit_line) = match operands {
        [point, line]
            if matches!(point.geometry, SketchGeometry::Point { .. })
                && matches!(line.geometry, SketchGeometry::Line { .. }) =>
        {
            (0, *point, *line)
        }
        [line, point]
            if matches!(line.geometry, SketchGeometry::Line { .. })
                && matches!(point.geometry, SketchGeometry::Point { .. }) =>
        {
            (1, *point, *line)
        }
        _ => return None,
    };
    let SketchGeometry::Point { position } = &point.geometry else {
        unreachable!("point operand matched above")
    };
    if !evaluated_value.is_finite() || !(0.0..=std::f64::consts::PI).contains(&evaluated_value) {
        return None;
    }
    let mut candidates = projected
        .iter()
        .filter(|((candidate_scope, _), candidate)| {
            *candidate_scope == scope
                && candidate.sketch == explicit_line.sketch
                && candidate.id != explicit_line.id
        })
        .filter_map(|(_, candidate)| {
            let SketchGeometry::Line { start, end } = &candidate.geometry else {
                return None;
            };
            (sketch_points_close(*position, *start) || sketch_points_close(*position, *end))
                .then_some(*candidate)
        })
        .filter(|candidate| {
            line_angle_matches(
                &explicit_line.geometry,
                &candidate.geometry,
                evaluated_value,
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    candidates.dedup_by(|left, right| left.id == right.id);
    let candidate = (candidates.len() == 1).then(|| candidates.remove(0))?;
    Some(if point_ordinal == 0 {
        (candidate.id.clone(), explicit_line.id.clone())
    } else {
        (explicit_line.id.clone(), candidate.id.clone())
    })
}

fn directional_point_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_mm: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry, SketchLocus,
    };

    let [first, second] = entities else {
        return None;
    };
    let SketchGeometry::Point {
        position: first_position,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Point {
        position: second_position,
    } = &second.geometry
    else {
        return None;
    };
    let expected = evaluated_mm.abs();
    let scale = 1.0 + expected;
    let first_locus = SketchLocus::Entity(first.id.clone());
    let second_locus = SketchLocus::Entity(second.id.clone());
    if (first_position.v - second_position.v).abs() <= scale * 1.0e-9
        && ((first_position.u - second_position.u).abs() - expected).abs() <= scale * 1.0e-9
    {
        Some(Definition::HorizontalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        })
    } else if (first_position.u - second_position.u).abs() <= scale * 1.0e-9
        && ((first_position.v - second_position.v).abs() - expected).abs() <= scale * 1.0e-9
    {
        Some(Definition::VerticalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        })
    } else {
        None
    }
}

fn recipe_linear_dimension_candidates(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    evaluated_mm: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Vec<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    let sketch_entities = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .collect::<Vec<_>>();
    let points = sketch_entities
        .iter()
        .copied()
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Point { .. }
            )
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for first in 0..points.len() {
        for second in first + 1..points.len() {
            if let Some(definition) = directional_point_dimension(
                &[points[first], points[second]],
                evaluated_mm,
                parameter.clone(),
            ) {
                candidates.push(definition);
            }
        }
    }
    let lines = sketch_entities
        .iter()
        .copied()
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Line { .. }
            )
        })
        .collect::<Vec<_>>();
    for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            if parallel_line_separation(lines[first], lines[second], evaluated_mm) {
                candidates.push(cadmpeg_ir::sketches::SketchConstraintDefinition::Distance {
                    entities: vec![lines[first].id.clone(), lines[second].id.clone()],
                    parameter: parameter.clone(),
                });
            }
        }
    }
    candidates
}

fn recipe_dimension_candidate_entities(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinition],
) -> Vec<cadmpeg_ir::sketches::SketchEntityId> {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchLocus};

    let mut entities = Vec::new();
    let locus_entity = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity.clone(),
    };
    for candidate in candidates {
        let candidate_entities = match candidate {
            Definition::Distance {
                entities: candidate_entities,
                ..
            } => candidate_entities.clone(),
            Definition::HorizontalDistance { first, second, .. }
            | Definition::VerticalDistance { first, second, .. } => {
                vec![locus_entity(first), locus_entity(second)]
            }
            _ => Vec::new(),
        };
        for entity in candidate_entities {
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    }
    entities
}

fn parallel_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return false;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return false;
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if first_length <= 1.0e-12 || second_length <= 1.0e-12 {
        return false;
    }
    let cross = first_direction.u * second_direction.v - first_direction.v * second_direction.u;
    if cross.abs() > 1.0e-9 * first_length * second_length {
        return false;
    }
    let offset = Point2::new(
        second_start.u - first_start.u,
        second_start.v - first_start.v,
    );
    let separation =
        (offset.u * first_direction.v - offset.v * first_direction.u).abs() / first_length;
    let expected = evaluated_mm.abs();
    (separation - expected).abs() <= 1.0e-9 * (1.0 + expected)
}

fn two_locus_distance_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Definition;

    (entities.len() == 2).then(|| Definition::Distance {
        entities: entities.iter().map(|entity| entity.id.clone()).collect(),
        parameter,
    })
}

fn exact_counted_dimension_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry, SketchLocus,
    };

    if let Some((first, second, axis)) = reflected_symmetry(entities) {
        return Some(Definition::Symmetric {
            first: SketchLocus::Entity(first.id.clone()),
            second: SketchLocus::Entity(second.id.clone()),
            axis: axis.id.clone(),
        });
    }
    let [first, second] = entities else {
        return None;
    };
    let point_on_geometry =
        |point: &cadmpeg_ir::sketches::SketchEntity,
         geometry: &cadmpeg_ir::sketches::SketchEntity| {
            let SketchGeometry::Point { position } = point.geometry else {
                return false;
            };
            point_lies_on_sketch_geometry(position, &geometry.geometry)
        };
    if point_on_geometry(first, second) || point_on_geometry(second, first) {
        return Some(Definition::Coincident {
            entities: vec![first.id.clone(), second.id.clone()],
        });
    }
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (&first.geometry, &second.geometry)
    else {
        return None;
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction
        .u
        .mul_add(first_direction.u, first_direction.v * first_direction.v)
        .sqrt();
    let second_length = second_direction
        .u
        .mul_add(second_direction.u, second_direction.v * second_direction.v)
        .sqrt();
    if first_length <= 1.0e-9 || second_length <= 1.0e-9 {
        return None;
    }
    let scale = first_length * second_length;
    let cross = first_direction
        .u
        .mul_add(second_direction.v, -first_direction.v * second_direction.u);
    if cross.abs() <= scale * 1.0e-9 {
        let signed_offset = parallel_line_offset(&first.geometry, &second.geometry)?;
        return Some(if signed_offset.abs() <= 1.0e-9 * (1.0 + first_length) {
            Definition::Collinear {
                first: first.id.clone(),
                second: second.id.clone(),
            }
        } else {
            Definition::Parallel {
                first: first.id.clone(),
                second: second.id.clone(),
            }
        });
    }
    let dot = first_direction
        .u
        .mul_add(second_direction.u, first_direction.v * second_direction.v);
    (dot.abs() <= scale * 1.0e-9).then(|| Definition::Perpendicular {
        first: first.id.clone(),
        second: second.id.clone(),
    })
}

fn point_lies_on_sketch_geometry(
    point: Point2,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * (1.0 + left.abs().max(right.abs()))
    };
    let angle_in_span = |angle: f64, start: f64, end: f64| {
        if !angle.is_finite() || !start.is_finite() || !end.is_finite() || end < start {
            return false;
        }
        let tau = std::f64::consts::TAU;
        if end - start >= tau - 1.0e-9 {
            return true;
        }
        let turns = ((start - angle) / tau).ceil().max(0.0);
        let lifted = angle + turns * tau;
        lifted >= start - 1.0e-9 && lifted <= end + 1.0e-9
    };
    match geometry {
        SketchGeometry::Point { position } => sketch_points_close(point, *position),
        SketchGeometry::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length_squared = direction.u.mul_add(direction.u, direction.v * direction.v);
            if length_squared <= 1.0e-18 {
                return false;
            }
            let relative = Point2::new(point.u - start.u, point.v - start.v);
            let parameter =
                relative.u.mul_add(direction.u, relative.v * direction.v) / length_squared;
            let cross = relative.u.mul_add(direction.v, -relative.v * direction.u);
            (-1.0e-9..=1.0 + 1.0e-9).contains(&parameter)
                && cross.abs() <= 1.0e-9 * (1.0 + length_squared.sqrt())
        }
        SketchGeometry::Circle { center, radius } => {
            close((point.u - center.u).hypot(point.v - center.v), radius.0)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            close(relative.u.hypot(relative.v), radius.0)
                && angle_in_span(relative.v.atan2(relative.u), start_angle.0, end_angle.0)
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            if major_radius.0 <= 0.0 || minor_radius.0 <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.0.sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.0;
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.0;
            close(x.mul_add(x, y * y), 1.0)
                && match (start_angle, end_angle) {
                    (Some(start), Some(end)) => angle_in_span(y.atan2(x), start.0, end.0),
                    (None, None) => true,
                    _ => false,
                }
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            periodic: false,
            ..
        } => {
            let multiplicity = usize::try_from(*degree)
                .ok()
                .and_then(|degree| degree.checked_add(1));
            let Some(multiplicity) = multiplicity else {
                return false;
            };
            let Some((&first_knot, &last_knot)) = knots.first().zip(knots.last()) else {
                return false;
            };
            knots.len() >= multiplicity * 2
                && knots[..multiplicity].iter().all(|knot| *knot == first_knot)
                && knots[knots.len() - multiplicity..]
                    .iter()
                    .all(|knot| *knot == last_knot)
                && control_points
                    .first()
                    .zip(control_points.last())
                    .is_some_and(|(first, last)| {
                        sketch_points_close(point, *first) || sketch_points_close(point, *last)
                    })
        }
        SketchGeometry::Nurbs { periodic: true, .. } | SketchGeometry::Native { .. } => false,
    }
}

fn exact_counted_offset(
    loci: &[(u32, u32)],
    return_members: &[u32],
    entities: &HashMap<u32, &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchOffsetPair};

    if loci.len() != entities.len()
        || loci.len() != return_members.len()
        || loci.len() < 2
        || !loci.len().is_multiple_of(2)
        || !return_members.len().is_multiple_of(2)
    {
        return None;
    }
    let source_count = loci.iter().position(|(_, role)| *role == 0)?;
    if source_count == 0
        || source_count * 2 != loci.len()
        || loci[..source_count].iter().any(|(_, role)| *role == 0)
        || loci[source_count..].iter().any(|(_, role)| *role != 0)
    {
        return None;
    }
    let roles = loci.iter().copied().collect::<HashMap<_, _>>();
    if roles.len() != loci.len() {
        return None;
    }
    let mut used_members = HashSet::new();
    let mut pairs = Vec::with_capacity(source_count);
    let mut signed_distance: Option<f64> = None;
    for members in return_members.chunks_exact(2) {
        let [source_record_index, result_record_index] = members else {
            unreachable!("chunks_exact(2) always yields pairs")
        };
        if roles.get(source_record_index).copied()? == 0
            || roles.get(result_record_index).copied()? != 0
            || !used_members.insert(*source_record_index)
            || !used_members.insert(*result_record_index)
        {
            return None;
        }
        let source = entities.get(source_record_index)?;
        let result = entities.get(result_record_index)?;
        let distance = parallel_line_offset(&source.geometry, &result.geometry)?;
        if distance.abs() <= 1.0e-9 {
            return None;
        }
        if let Some(expected) = signed_distance {
            let scale = 1.0 + distance.abs().max(expected.abs());
            if (distance - expected).abs() > scale * 1.0e-9 {
                return None;
            }
        } else {
            signed_distance = Some(distance);
        }
        pairs.push(SketchOffsetPair {
            source: source.id.clone(),
            result: result.id.clone(),
        });
    }
    if used_members.len() != loci.len() {
        return None;
    }
    Some(Definition::Offset {
        pairs,
        signed_distance: Length(signed_distance?),
    })
}

fn line_angle_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = first
    else {
        return false;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = second
    else {
        return false;
    };
    let first_du = first_end.u - first_start.u;
    let first_dv = first_end.v - first_start.v;
    let second_du = second_end.u - second_start.u;
    let second_dv = second_end.v - second_start.v;
    let denominator = first_du.hypot(first_dv) * second_du.hypot(second_dv);
    if denominator <= 1.0e-18 {
        return false;
    }
    let cosine = ((first_du * second_du + first_dv * second_dv) / denominator).clamp(-1.0, 1.0);
    let angle = cosine.acos();
    let supplementary = std::f64::consts::PI - angle;
    let scale = 1.0 + expected.abs();
    (angle - expected).abs() <= scale * 1.0e-9 || (supplementary - expected).abs() <= scale * 1.0e-9
}

fn exact_offset_constraint(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchOffsetPair};

    if relation.unknown_constraint_bits != 0
        || relation.constraint_kinds != [SketchConstraintKind::Perpendicular]
        || relation.return_members.len() < 4
        || !relation.return_members.len().is_multiple_of(2)
        || relation.resolved_return_members.len() != relation.return_members.len()
    {
        return None;
    }
    let mut pairs = Vec::new();
    let mut signed_distance: Option<f64> = None;
    for operands in relation.resolved_return_members.chunks_exact(2) {
        let (source_record_index, result_record_index) = match operands {
            [SketchRelationOperand::Curve {
                record_index: source_record_index,
                secondary_id: 0,
                ..
            }, SketchRelationOperand::Curve {
                record_index: result_record_index,
                secondary_id,
                ..
            }] if *secondary_id != 0 => (*source_record_index, *result_record_index),
            _ => return None,
        };
        let source = projected.get(&(scope, source_record_index))?;
        let result = projected.get(&(scope, result_record_index))?;
        let distance = parallel_line_offset(&source.geometry, &result.geometry)?;
        if distance.abs() <= 1.0e-9 {
            return None;
        }
        if let Some(expected) = signed_distance {
            let scale = 1.0 + distance.abs().max(expected.abs());
            if (distance - expected).abs() > scale * 1.0e-9 {
                return None;
            }
        } else {
            signed_distance = Some(distance);
        }
        pairs.push(SketchOffsetPair {
            source: source.id.clone(),
            result: result.id.clone(),
        });
    }
    Some(Definition::Offset {
        pairs,
        signed_distance: Length(signed_distance?),
    })
}

fn parallel_line_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: source_start,
        end: source_end,
    } = source
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: result_start,
        end: result_end,
    } = result
    else {
        return None;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= 1.0e-12 || result_length <= 1.0e-12 {
        return None;
    }
    let parallel_error =
        (source_du * result_dv - source_dv * result_du).abs() / (source_length * result_length);
    if parallel_error > 1.0e-9 {
        return None;
    }
    let normal_u = -source_dv / source_length;
    let normal_v = source_du / source_length;
    let distance_at = |point: &Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    let start_distance = distance_at(result_start);
    let end_distance = distance_at(result_end);
    let scale = 1.0 + start_distance.abs().max(end_distance.abs());
    ((start_distance - end_distance).abs() <= scale * 1.0e-9).then_some(start_distance)
}

fn reflected_symmetry<'a>(
    entities: &[&'a cadmpeg_ir::sketches::SketchEntity],
) -> Option<(
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
)> {
    use cadmpeg_ir::sketches::SketchGeometry;

    if entities.len() != 3 {
        return None;
    }
    let mut candidates = Vec::new();
    for axis_ordinal in 0..entities.len() {
        let axis = entities[axis_ordinal];
        let SketchGeometry::Line {
            start: axis_start,
            end: axis_end,
        } = &axis.geometry
        else {
            continue;
        };
        let others = entities
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal != axis_ordinal)
            .map(|(_, entity)| *entity)
            .collect::<Vec<_>>();
        if reflected_geometry_matches(
            &others[0].geometry,
            &others[1].geometry,
            axis_start,
            axis_end,
        ) {
            candidates.push((others[0], others[1], axis));
        }
    }
    (candidates.len() == 1).then(|| candidates.remove(0))
}

fn reflected_geometry_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    axis_start: &Point2,
    axis_end: &Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    match (first, second) {
        (
            SketchGeometry::Point {
                position: first_position,
            },
            SketchGeometry::Point {
                position: second_position,
            },
        ) => reflect_point(*first_position, *axis_start, *axis_end)
            .is_some_and(|reflected| sketch_points_close(reflected, *second_position)),
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            let Some(reflected_start) = reflect_point(*first_start, *axis_start, *axis_end) else {
                return false;
            };
            let Some(reflected_end) = reflect_point(*first_end, *axis_start, *axis_end) else {
                return false;
            };
            sketch_points_close(reflected_start, *second_start)
                && sketch_points_close(reflected_end, *second_end)
                || sketch_points_close(reflected_start, *second_end)
                    && sketch_points_close(reflected_end, *second_start)
        }
        _ => false,
    }
}

fn reflect_point(point: Point2, axis_start: Point2, axis_end: Point2) -> Option<Point2> {
    let du = axis_end.u - axis_start.u;
    let dv = axis_end.v - axis_start.v;
    let norm_squared = du * du + dv * dv;
    (norm_squared > 1.0e-18).then(|| {
        let projection =
            ((point.u - axis_start.u) * du + (point.v - axis_start.v) * dv) / norm_squared;
        Point2::new(
            2.0 * (axis_start.u + projection * du) - point.u,
            2.0 * (axis_start.v + projection * dv) - point.v,
        )
    })
}

fn sketch_points_close(first: Point2, second: Point2) -> bool {
    let scale = 1.0
        + first
            .u
            .abs()
            .max(first.v.abs())
            .max(second.u.abs())
            .max(second.v.abs());
    (first.u - second.u).abs() <= scale * 1.0e-9 && (first.v - second.v).abs() <= scale * 1.0e-9
}

fn relation_operand_index(operand: &SketchRelationOperand) -> u32 {
    match operand {
        SketchRelationOperand::Point { record_index, .. }
        | SketchRelationOperand::Curve { record_index, .. }
        | SketchRelationOperand::Record { record_index } => *record_index,
    }
}

fn relation_operand_kind(operand: &SketchRelationOperand) -> &'static str {
    match operand {
        SketchRelationOperand::Point { .. } => "point",
        SketchRelationOperand::Curve { .. } => "curve",
        SketchRelationOperand::Record { .. } => "record",
    }
}

fn relation_kind_name(relation: &SketchRelation) -> String {
    let mut names = relation
        .constraint_kinds
        .iter()
        .map(|kind| match kind {
            SketchConstraintKind::Coincident => "coincident",
            SketchConstraintKind::Colinear => "collinear",
            SketchConstraintKind::Concentric => "concentric",
            SketchConstraintKind::EqualLength => "equal_length",
            SketchConstraintKind::Parallel => "parallel",
            SketchConstraintKind::Perpendicular => "perpendicular",
            SketchConstraintKind::Horizontal => "horizontal",
            SketchConstraintKind::Vertical => "vertical",
            SketchConstraintKind::Tangent => "tangent",
            SketchConstraintKind::Curvature => "curvature",
            SketchConstraintKind::Symmetry => "symmetry",
            SketchConstraintKind::Equal => "equal",
            SketchConstraintKind::Midpoint => "midpoint",
            SketchConstraintKind::Polygon => "polygon",
            SketchConstraintKind::CircularPattern => "circular_pattern",
            SketchConstraintKind::RectangularPattern => "rectangular_pattern",
        })
        .collect::<Vec<_>>();
    if relation.unknown_constraint_bits != 0 {
        names.push("unknown_bits");
    }
    names.join("+")
}

fn planar_point(point: &Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite() && point.z.abs() <= 1.0e-9
}

fn positive_sketch_normal(normal: &Vector3) -> bool {
    normal.x.abs() <= 1.0e-9 && normal.y.abs() <= 1.0e-9 && (normal.z - 1.0).abs() <= 1.0e-9
}

fn clamped_nurbs(degree: u32, knots: &[f64]) -> bool {
    let multiplicity = degree as usize + 1;
    knots.len() >= multiplicity.saturating_mul(2)
        && knots[..multiplicity]
            .iter()
            .all(|knot| knot.to_bits() == knots[0].to_bits())
        && knots[knots.len() - multiplicity..]
            .iter()
            .all(|knot| knot.to_bits() == knots[knots.len() - 1].to_bits())
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> + '_ {
    expression
        .split(|character: char| {
            !(character.is_alphanumeric() || matches!(character, '_' | '"' | '$' | '°' | 'µ'))
        })
        .filter(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_alphabetic() || character == '_')
        })
        .map(str::to_owned)
}

fn json_scalar_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        value => value.to_string(),
    }
}

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub fn decode_recipes(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(bytes, &entry.name, &mut out);
    }
    Ok(out)
}

/// Decode every indexed parameter record in each Design `BulkStream`.
pub fn decode_parameters(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            if let Some(mut parameter) = parse_design_parameter(&bytes[at..end]) {
                parameter.id = format!("f3d:{}:design-parameter#{at}", entry.name);
                parameter.byte_offset = at as u64;
                parameter.prefix_value_offset += at as u64;
                parameter.expression_offset += at as u64;
                parameter.source_kind_offset += at as u64;
                parameter.unit_offset = parameter.unit_offset.map(|offset| offset + at as u64);
                parameter.name_offset += at as u64;
                parameter.evaluated_value_offset += at as u64;
                out.push(parameter);
                position = end;
            } else {
                position = at + 1;
            }
        }
    }
    out.sort_by_key(|parameter| parameter.id.clone());
    Ok(out)
}

fn parse_design_parameter(payload: &[u8]) -> Option<DesignParameter> {
    let (class_tag, after_tag) = lp_ascii(payload, 0)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != 7
        || payload.get(11..22) != Some(&[0; 11])
        || payload.get(30) != Some(&0)
    {
        return None;
    }
    let record_index = u32_at(payload, 7)?;
    let prefix_value = read_u64(payload, 22)?;
    let source_ordinal = u32_at(payload, 31)?;
    let (owner_record_index, expression_at, expression_trailer) = match payload.get(35)? {
        0 => (None, 36, [0, 0, 0, 0, 0, 0, 0, 0, 1]),
        1 if payload.get(40..46) == Some(&[0; 6]) => (Some(u32_at(payload, 36)?), 46, [0; 9]),
        _ => return None,
    };
    let (expression, expression_end) = lp_utf16(payload, expression_at)?;
    if payload.get(expression_end..expression_end + 9) != Some(&expression_trailer) {
        return None;
    }
    let source_kind_at = expression_end + 9;
    let (source_kind, source_kind_end) = lp_utf16(payload, source_kind_at)?;
    if u32_at(payload, source_kind_end) != Some(0)
        || prefix_value != design_parameter_prefix(&source_kind)
    {
        return None;
    }
    let first_at = source_kind_end + 4;
    let (unit, unit_offset, name, name_at, name_end) = if u32_at(payload, first_at) == Some(0) {
        let name_at = first_at + 4;
        let (name, name_end) = lp_utf16(payload, name_at)?;
        (None, None, name, name_at, name_end)
    } else {
        let (first, first_end) = lp_utf16(payload, first_at)?;
        if let Some((second, second_end)) = lp_utf16(payload, first_end) {
            (
                Some(first),
                Some(first_at + 4),
                second,
                first_end,
                second_end,
            )
        } else {
            (None, None, first, first_at, first_end)
        }
    };
    let evaluated_value = f64_at(payload, name_end)?;
    if payload.get(name_end + 8..) != Some(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || expression.is_empty()
        || source_kind.is_empty()
        || name.is_empty()
        || !evaluated_value.is_finite()
    {
        return None;
    }
    let kind = if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    };
    Some(DesignParameter {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        prefix_value,
        prefix_value_offset: 22,
        source_ordinal,
        owner_record_index,
        expression,
        expression_offset: (expression_at + 4) as u64,
        source_kind,
        source_kind_offset: (source_kind_at + 4) as u64,
        kind,
        unit,
        unit_offset: unit_offset.map(|offset| offset as u64),
        name,
        name_offset: (name_at + 4) as u64,
        evaluated_value,
        evaluated_value_offset: name_end as u64,
    })
}

pub(crate) fn design_parameter_prefix(source_kind: &str) -> u64 {
    if source_kind == "TangencyWeight" {
        6
    } else {
        0
    }
}

/// Decode the fixed-width owner frame for every owned Design parameter.
pub fn decode_parameter_owners(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index else {
            continue;
        };
        let Some(scope) = native_stream(&parameter.id) else {
            continue;
        };
        let Some(header) = headers.get(&(scope, owner_index)) else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && parameter.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let frame = at.and_then(|at| at.checked_add(104).and_then(|end| bytes.get(at..end)));
        let Some(mut owner) = frame.and_then(parse_parameter_owner) else {
            continue;
        };
        owner.id = format!(
            "f3d:{}:design-parameter-owner#{}",
            entry.name, header.byte_offset
        );
        owner.byte_offset = header.byte_offset;
        owner.evaluated_value_offset += header.byte_offset;
        out.push(owner);
    }
    out.sort_by_key(|owner| owner.id.clone());
    Ok(out)
}

fn parse_parameter_owner(frame: &[u8]) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii(frame, 0)?;
    if frame.len() != 104
        || after_tag != 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || frame.get(11..19) != Some(&[0; 8])
        || frame.get(19..24) != Some(&[1, 1, 0, 0, 0])
        || frame.get(24) != Some(&1)
        || frame.get(29..35) != Some(&[0; 6])
        || frame.get(39) != Some(&0)
        || frame.get(48) != Some(&1)
        || frame.get(53..59) != Some(&[0; 6])
        || frame.get(63..67) != Some(&[0; 4])
        || frame.get(67) != Some(&1)
        || frame.get(72..78) != Some(&[0; 6])
        || frame.get(78) != Some(&1)
        || frame.get(80) != Some(&0)
        || frame.get(81) != Some(&1)
        || frame.get(86..93) != Some(&[0; 7])
        || frame.get(93) != Some(&1)
        || frame.get(98..104) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = u32_at(frame, 7)?;
    let parameter_record_index = u32_at(frame, 49)?;
    let companion_record_index = u32_at(frame, 82)?;
    let owner_first = parameter_record_index == record_index.checked_add(1)?
        && companion_record_index == record_index.checked_add(2)?;
    let parameter_first = record_index == parameter_record_index.checked_add(1)?
        && companion_record_index == record_index.checked_add(1)?;
    let scope_record_index = u32_at(frame, 25)?;
    if u32_at(frame, 68)? != scope_record_index
        || u32_at(frame, 94)? != scope_record_index
        || !(owner_first || parameter_first)
    {
        return None;
    }
    let evaluated_value = f64_at(frame, 40)?;
    if !evaluated_value.is_finite() {
        return None;
    }
    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: u32_at(frame, 35)?,
        evaluated_value,
        evaluated_value_offset: 40,
        parameter_record_index,
        owned_ordinal: u32_at(frame, 59)?,
        variant: *frame.get(79)?,
        companion_record_index,
    })
}

/// Decode the fixed prefix of every indexed record paired with a parameter
/// owner. Record-specific payload after the prefix is decoded independently.
pub fn decode_parameter_companions(
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for owner in owners {
        let Some(scope) = native_stream(&owner.id) else {
            continue;
        };
        let Some(header) = headers.get(&(scope, owner.companion_record_index)) else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && owner.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let prefix = at.and_then(|at| at.checked_add(58).and_then(|end| bytes.get(at..end)));
        let Some(mut companion) = prefix.and_then(parse_parameter_companion) else {
            continue;
        };
        if companion.record_index != owner.companion_record_index
            || companion.owner_record_index != owner.record_index
        {
            continue;
        }
        companion.id = format!(
            "f3d:{}:design-parameter-companion#{}",
            entry.name, header.byte_offset
        );
        companion.byte_offset = header.byte_offset;
        companion.timestamp_micros_offset += header.byte_offset;
        companion.payload_byte_offset += header.byte_offset;
        out.push(companion);
    }
    out.sort_by_key(|companion| companion.id.clone());
    Ok(out)
}

fn parse_parameter_companion(prefix: &[u8]) -> Option<DesignParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii(prefix, 0)?;
    if prefix.len() != 58
        || after_tag != 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || prefix.get(11..31) != Some(&[0; 20])
        || prefix.get(31) != Some(&1)
        || prefix.get(36..42) != Some(&[0; 6])
        || prefix.get(50..58) != Some(&[0; 8])
    {
        return None;
    }
    let timestamp_micros = read_u64(prefix, 42)?;
    if timestamp_micros == 0 {
        return None;
    }
    Some(DesignParameterCompanion {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index: u32_at(prefix, 7)?,
        owner_record_index: u32_at(prefix, 32)?,
        timestamp_micros,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    })
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval.
pub fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    companions: &mut [DesignParameterCompanion],
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    stream_lengths: &HashMap<String, usize, S>,
) {
    for companion in companions {
        let Some(stream) = native_stream(&companion.id) else {
            continue;
        };
        let Some(stream_length) = stream_lengths.get(stream).copied() else {
            continue;
        };
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.iter(),
            owners,
            scopes,
            headers,
            stream_length,
        ) else {
            continue;
        };
        companion.payload_byte_offset = u64::try_from(start).unwrap_or(u64::MAX);
        companion.payload_byte_length = u64::try_from(end - start).unwrap_or(u64::MAX);
        let mut owned = recipes
            .iter()
            .filter(|recipe| {
                native_stream(&recipe.id) == Some(stream)
                    && usize::try_from(recipe.byte_offset)
                        .is_ok_and(|offset| offset >= start && offset < end)
            })
            .collect::<Vec<_>>();
        owned.sort_by_key(|recipe| recipe.byte_offset);
        companion.owned_recipe_ids = owned.into_iter().map(|recipe| recipe.id.clone()).collect();
    }
}

/// Decode the indexed record that directly contains each construction recipe
/// owned by a dimensional parameter companion.
pub fn decode_dimension_recipe_records(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignDimensionRecipeRecord>, CodecError> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_owners = owners
        .iter()
        .filter_map(|owner| {
            let stream = native_stream(&owner.id)?;
            parameters
                .get(&(stream, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
                .then_some((stream.to_owned(), owner.record_index))
        })
        .collect::<HashSet<_>>();
    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|stream| {
            dimension_owners.contains(&(stream.to_owned(), companion.owner_record_index))
        })
    }) {
        let Some(stream) = native_stream(&companion.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(companion.payload_byte_offset).ok() else {
            continue;
        };
        let Some(end) = usize::try_from(companion.payload_byte_length)
            .ok()
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
        else {
            continue;
        };
        for (recipe_ordinal, recipe_id) in companion.owned_recipe_ids.iter().enumerate() {
            let Some(recipe) = recipes.get(recipe_id.as_str()).copied() else {
                continue;
            };
            let Some(recipe_offset) = usize::try_from(recipe.byte_offset).ok() else {
                continue;
            };
            let Some((at, class_tag, record_index, record_end)) =
                indexed_record_containing(bytes, start, end, recipe_offset)
            else {
                continue;
            };
            let Some(program_offset) = recipe_offset
                .checked_add(construction_recipe_family_name_len(recipe.kind))
                .filter(|offset| *offset < record_end)
            else {
                continue;
            };
            let Some((prefix_offset, prefix_bytes)) = recipe_record_prefix(
                bytes,
                at,
                recipe_offset,
                construction_recipe_family_name_len(recipe.kind),
            ) else {
                continue;
            };
            let references = decode_recipe_references(
                &prefix_bytes,
                u64::try_from(prefix_offset).unwrap_or(u64::MAX),
            );
            let Some(program) = contiguous_i32_program(bytes, program_offset, record_end) else {
                continue;
            };
            out.push(DesignDimensionRecipeRecord {
                id: format!(
                    "f3d:{}:design-dimension-recipe-record#{}",
                    entry.name, recipe.byte_offset
                ),
                companion_record_index: companion.record_index,
                recipe_ordinal: u32::try_from(recipe_ordinal).unwrap_or(u32::MAX),
                recipe_id: recipe.id.clone(),
                byte_offset: u64::try_from(at).unwrap_or(u64::MAX),
                class_tag,
                record_index,
                frame_length: u64::try_from(record_end - at).unwrap_or(u64::MAX),
                prefix_offset: u64::try_from(prefix_offset).unwrap_or(u64::MAX),
                prefix_bytes,
                references,
                program_offset: u64::try_from(program_offset).unwrap_or(u64::MAX),
                program,
                matching_edge_operand_ids: Vec::new(),
            });
        }
    }
    out.sort_by_key(|record| record.id.clone());
    Ok(out)
}

pub(crate) fn decode_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
) -> Vec<crate::records::DesignRecipeReference> {
    if prefix
        .get(..10)
        .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
        || u32_at(prefix, 10) != Some(1)
        || u32_at(prefix, 14) != Some(3)
        || u32_at(prefix, 18).is_none_or(|value| value == 0)
        || u32_at(prefix, 22).is_none_or(|value| value == 0)
    {
        return Vec::new();
    }
    let mut references = Vec::new();
    let mut at = 22usize;
    while prefix.len().saturating_sub(at) > 4 {
        let Some(selector) = u32_at(prefix, at).filter(|value| *value != 0) else {
            return Vec::new();
        };
        let token_encoding_at = at + 4;
        let length_prefixed = lp_ascii(prefix, token_encoding_at).and_then(|(token, marker_at)| {
            (!token.is_empty()
                && token.bytes().all(|byte| byte.is_ascii_digit())
                && u32_at(prefix, marker_at) == Some(0))
            .then_some((token, token_encoding_at + 4, marker_at + 4))
        });
        let packed = (1usize..=8).find_map(|length| {
            let token = prefix.get(token_encoding_at..token_encoding_at + length)?;
            let zero_at = token_encoding_at.checked_add(length)?;
            (token.iter().all(u8::is_ascii_digit)
                && prefix.get(zero_at..zero_at + 4) == Some(&[0; 4]))
            .then(|| std::str::from_utf8(token).ok())
            .flatten()
            .map(|token| (token.to_owned(), token_encoding_at, zero_at + 4))
        });
        let Some((token, token_at, marker_at)) = length_prefixed.or(packed) else {
            return Vec::new();
        };
        if u32_at(prefix, marker_at) != Some(1) {
            return Vec::new();
        }
        let Some(design_reference) = u32_at(prefix, marker_at + 4).filter(|value| *value != 0)
        else {
            return Vec::new();
        };
        if u32_at(prefix, marker_at + 8) != Some(0) {
            return Vec::new();
        }
        references.push(crate::records::DesignRecipeReference {
            selector: i64::from(selector),
            selector_offset: prefix_offset.saturating_add(at as u64),
            token,
            token_offset: prefix_offset.saturating_add(token_at as u64),
            design_reference: i64::from(design_reference),
            design_reference_offset: prefix_offset.saturating_add((marker_at + 4) as u64),
            candidate_faces: Vec::new(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        });
        at = marker_at + 12;
    }
    if prefix.get(at..) == Some(&[0, 0, 0, 0]) {
        references
    } else {
        Vec::new()
    }
}

/// Join dimension-recipe selector/reference pairs to active solved subentities.
pub fn bind_dimension_recipe_reference_candidates(
    records: &mut [DesignDimensionRecipeRecord],
    tags: &[PersistentSubentityTag],
) {
    for reference in records.iter_mut().flat_map(|record| &mut record.references) {
        bind_recipe_reference_candidates(reference, tags);
    }
}

pub(crate) fn bind_recipe_reference_candidates(
    reference: &mut crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) {
    reference.candidate_faces = recipe_reference_candidate_faces(reference, tags);
    reference.candidate_edges = recipe_reference_candidate_edges(reference, tags);
    reference.alternate_selector_faces = recipe_reference_alternate_selector_faces(reference, tags);
    reference.alternate_selector_edges = recipe_reference_alternate_selector_edges(reference, tags);
}

/// Join dimension programs to byte-identical edge-recipe program tails.
pub fn bind_dimension_recipe_edge_operands(
    records: &mut [DesignDimensionRecipeRecord],
    operands: &[DesignEdgeOperand],
) {
    for record in records {
        record.matching_edge_operand_ids =
            dimension_recipe_matching_edge_operand_ids(record, operands);
    }
}

pub(crate) fn dimension_recipe_matching_edge_operand_ids(
    record: &DesignDimensionRecipeRecord,
    operands: &[DesignEdgeOperand],
) -> Vec<String> {
    let mut ids = operands
        .iter()
        .filter(|operand| {
            let Some(tail) = operand
                .recipe_program
                .get(7..)
                .filter(|tail| !tail.is_empty())
            else {
                return false;
            };
            record
                .program
                .windows(tail.len())
                .any(|window| window == tail)
        })
        .map(|operand| operand.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) fn recipe_reference_candidate_edges(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::EdgeId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut edges = tags
        .iter()
        .filter(|tag| {
            tag.selector == reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Edge(edge) => Some(edge.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.0.cmp(&right.0));
    edges.dedup();
    edges
}

pub(crate) fn recipe_reference_candidate_faces(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.selector == reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(face) => Some(face.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

pub(crate) fn recipe_reference_alternate_selector_edges(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::EdgeId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut edges = tags
        .iter()
        .filter(|tag| {
            tag.selector != reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Edge(edge) => Some(edge.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.0.cmp(&right.0));
    edges.dedup();
    edges
}

pub(crate) fn recipe_reference_alternate_selector_faces(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.selector != reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(face) => Some(face.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

fn recipe_record_prefix(
    bytes: &[u8],
    record_offset: usize,
    family_name_offset: usize,
    family_name_len: usize,
) -> Option<(usize, Vec<u8>)> {
    let prefix_offset = record_offset.checked_add(11)?;
    let prefix_end = family_name_offset.checked_sub(4)?;
    if u32_at(bytes, prefix_end)? != u32::try_from(family_name_len).ok()? {
        return None;
    }
    let prefix = bytes.get(prefix_offset..prefix_end)?;
    Some((prefix_offset, prefix.to_vec()))
}

fn indexed_record_containing(
    bytes: &[u8],
    start: usize,
    end: usize,
    member_offset: usize,
) -> Option<(usize, String, u32, usize)> {
    if start > member_offset || member_offset >= end || end > bytes.len() {
        return None;
    }
    let mut cursor = start;
    let mut containing = None;
    while let Some(at) = next_indexed_record_offset(bytes, cursor) {
        if at >= end {
            break;
        }
        if at > member_offset {
            return containing
                .map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, at));
        }
        let (class_tag, after_tag) = lp_ascii(bytes, at)?;
        containing = Some((at, class_tag, u32_at(bytes, after_tag)?));
        cursor = at + 11;
    }
    containing.map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, end))
}

fn contiguous_i32_program(bytes: &[u8], start: usize, end: usize) -> Option<Vec<i32>> {
    let program = bytes.get(start..end)?;
    if program.is_empty() || !program.len().is_multiple_of(4) {
        return None;
    }
    Some(
        program
            .chunks_exact(4)
            .map(|word| {
                i32::from_le_bytes(
                    word.try_into()
                        .expect("invariant: chunks_exact(4) yields four-byte slices"),
                )
            })
            .collect(),
    )
}

/// Decode paired typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
#[allow(clippy::too_many_arguments)]
pub fn decode_dimension_locus_pairs(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            dimension_companions.contains(&(scope.to_owned(), companion.record_index))
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let Some(mut pair) =
            find_dimension_locus_pair(bytes, start, end, companion.record_index, &geometry_indices)
        else {
            continue;
        };
        pair.id = format!(
            "f3d:{}:design-dimension-locus-pair#{}",
            entry.name, pair.byte_offset
        );
        out.push(pair);
    }
    out.sort_by_key(|pair| pair.id.clone());
    Ok(out)
}

fn find_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let parse = |at| {
        parse_dimension_locus_pair(bytes, at, companion_record_index, geometry_indices)
            .filter(|pair| usize::try_from(pair.paired_byte_offset).is_ok_and(|at| at < end))
    };
    if let Some(pair) = parse(start) {
        return Some(pair);
    }
    let mut candidates = Vec::new();
    let mut position = start.saturating_add(1);
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        if let Some(pair) = parse(at) {
            candidates.push(pair);
        }
        position = at.saturating_add(1);
    }
    let [pair] = candidates.as_slice() else {
        return None;
    };
    Some(pair.clone())
}

fn parse_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let (class_tag, after_tag) = lp_ascii(bytes, start)?;
    let record_index = u32_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20) != Some(3)
        || bytes.get(start + 24) != Some(&1)
        || u32_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
        || bytes.get(start + 54) != Some(&1)
        || bytes.get(start + 59..start + 65) != Some(&[0; 6])
    {
        return None;
    }
    let first_geometry_record_index = u32_at(bytes, start + 40)?;
    let second_geometry_record_index = u32_at(bytes, start + 55)?;
    if !geometry_indices.contains(&first_geometry_record_index)
        || !geometry_indices.contains(&second_geometry_record_index)
    {
        return None;
    }
    let mut position = start.checked_add(69)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) = lp_ascii(bytes, at)?;
        if u32_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    Some(DesignDimensionLocusPair {
        id: String::new(),
        companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        opaque_index: u32_at(bytes, start + 35)?,
        opaque_index_offset: (start + 35) as u64,
        first_geometry_record_index,
        first_geometry_reference_offset: (start + 40) as u64,
        first_role: u32_at(bytes, start + 50)?,
        first_role_offset: (start + 50) as u64,
        second_geometry_record_index,
        second_geometry_reference_offset: (start + 55) as u64,
        second_role: u32_at(bytes, start + 65)?,
        second_role_offset: (start + 65) as u64,
        paired_class_tag,
        paired_byte_offset: paired_byte_offset as u64,
    })
}

/// Decode dimension frames whose ordered operand run contains a null record
/// reference followed by one typed sketch-geometry reference.
#[allow(clippy::too_many_arguments)]
pub fn decode_dimension_null_locus_pairs(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    placements: &[DesignSketchPlacement],
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> Result<Vec<DesignDimensionNullLocusPair>, CodecError> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let typed_companions = pairs
        .iter()
        .filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        })
        .chain(groups.iter().filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.companion_record_index,
            ))
        }))
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            let key = (scope.to_owned(), companion.record_index);
            dimension_companions.contains(&key) && !typed_companions.contains(&key)
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let expected_owner = owners
            .iter()
            .find(|owner| {
                native_stream(&owner.id) == Some(scope)
                    && owner.companion_record_index == companion.record_index
            })
            .and_then(|owner| {
                placements.iter().find(|placement| {
                    native_stream(&placement.id) == Some(scope)
                        && placement.scope_record_index == owner.scope_record_index
                })
            })
            .and_then(|placement| u32::try_from(placement.entity_suffix).ok());
        let geometry_owners = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| (point.record_index, point.owner_reference))
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| (curve.record_index, curve.owner_reference)),
            )
            .collect::<HashMap<_, _>>();
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let parse = |at| {
            parse_dimension_null_locus_pair(bytes, at, companion.record_index, &geometry_indices)
                .filter(|pair| usize::try_from(pair.paired_byte_offset).is_ok_and(|at| at < end))
                .filter(|pair| {
                    geometry_owners
                        .get(&pair.geometry_record_index)
                        .copied()
                        .flatten()
                        .is_none_or(|owner| Some(owner) == expected_owner)
                })
        };
        let mut candidates = parse(start).into_iter().collect::<Vec<_>>();
        if candidates.is_empty() {
            let mut position = start.saturating_add(1);
            while let Some(at) = next_indexed_record_offset(bytes, position) {
                if at >= end {
                    break;
                }
                if let Some(pair) = parse(at) {
                    candidates.push(pair);
                }
                position = at.saturating_add(1);
            }
        }
        let [pair] = candidates.as_slice() else {
            continue;
        };
        let mut pair = pair.clone();
        pair.id = format!(
            "f3d:{}:design-dimension-null-locus-pair#{}",
            entry.name, pair.byte_offset
        );
        out.push(pair);
    }
    out.sort_by_key(|pair| pair.id.clone());
    Ok(out)
}

fn parse_dimension_null_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionNullLocusPair> {
    let (class_tag, after_tag) = lp_ascii(bytes, start)?;
    let record_index = u32_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20) != Some(2)
        || bytes.get(start + 24) != Some(&1)
        || u32_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
    {
        return None;
    }
    let geometry_record_index = u32_at(bytes, start + 40)?;
    if !geometry_indices.contains(&geometry_record_index) {
        return None;
    }
    let mut position = start.checked_add(54)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) = lp_ascii(bytes, at)?;
        if u32_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    Some(DesignDimensionNullLocusPair {
        id: String::new(),
        companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        null_reference_offset: (start + 25) as u64,
        null_role: u32_at(bytes, start + 35)?,
        null_role_offset: (start + 35) as u64,
        geometry_record_index,
        geometry_reference_offset: (start + 40) as u64,
        geometry_role: u32_at(bytes, start + 50)?,
        geometry_role_offset: (start + 50) as u64,
        paired_class_tag,
        paired_byte_offset: paired_byte_offset as u64,
    })
}

/// Decode counted typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
#[allow(clippy::too_many_arguments)]
pub fn decode_dimension_locus_groups(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> Result<Vec<DesignDimensionLocusGroup>, CodecError> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            dimension_companions.contains(&(scope.to_owned(), companion.record_index))
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let sketch_entities = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(scope)
                    && entity.object_kind == Some(DesignObjectKind::Sketch)
            })
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let candidates = find_dimension_locus_groups(
            bytes,
            start,
            end,
            companion.record_index,
            &geometry_indices,
            &sketch_entities,
        );
        out.extend(candidates.into_iter().map(|mut group| {
            group.id = format!(
                "f3d:{}:design-dimension-locus-group#{}",
                entry.name, group.byte_offset
            );
            group
        }));
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

fn find_dimension_locus_groups(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Vec<DesignDimensionLocusGroup> {
    let parse = |at| {
        parse_dimension_locus_group(
            bytes,
            at,
            companion_record_index,
            geometry_indices,
            sketch_entities,
        )
        .filter(|group| usize::try_from(group.next_byte_offset).is_ok_and(|at| at <= end))
    };
    let mut candidates = parse(start).into_iter().collect::<Vec<_>>();
    let mut position = start.saturating_add(1);
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        if let Some(group) = parse(at) {
            candidates.push(group);
        }
        position = at.saturating_add(1);
    }
    candidates.sort_by_key(|group| group.byte_offset);
    candidates.dedup_by_key(|group| group.byte_offset);
    candidates
}

pub(crate) fn companion_owned_interval<'a>(
    companion: &DesignParameterCompanion,
    parameters: impl IntoIterator<Item = &'a DesignParameter>,
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    stream_length: usize,
) -> Option<(usize, usize)> {
    let native_scope = native_stream(&companion.id)?;
    let owning_scope_record_index = owners
        .iter()
        .find(|owner| {
            native_stream(&owner.id) == Some(native_scope)
                && owner.record_index == companion.owner_record_index
        })
        .map(|owner| owner.scope_record_index);
    let foreign_scope_members = scopes
        .iter()
        .filter(|scope| {
            native_stream(&scope.id) == Some(native_scope)
                && Some(scope.record_index) != owning_scope_record_index
        })
        .flat_map(|scope| scope.reference_members.iter().copied())
        .collect::<HashSet<_>>();
    let start = usize::try_from(companion.byte_offset)
        .ok()?
        .checked_add(58)?;
    let end = owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(native_scope)
                && owner.byte_offset > companion.byte_offset
        })
        .filter_map(|owner| usize::try_from(owner.byte_offset).ok())
        .chain(
            parameters
                .into_iter()
                .filter(|parameter| {
                    native_stream(&parameter.id) == Some(native_scope)
                        && parameter.byte_offset > companion.byte_offset
                })
                .filter_map(|parameter| usize::try_from(parameter.byte_offset).ok()),
        )
        .chain(
            scopes
                .iter()
                .filter(|scope| {
                    native_stream(&scope.id) == Some(native_scope)
                        && scope.byte_offset > companion.byte_offset
                })
                .filter_map(|scope| usize::try_from(scope.byte_offset).ok()),
        )
        .chain(
            headers
                .iter()
                .filter(|header| {
                    native_stream(&header.id) == Some(native_scope)
                        && header.byte_offset > companion.byte_offset
                        && foreign_scope_members.contains(&header.record_index)
                })
                .filter_map(|header| usize::try_from(header.byte_offset).ok()),
        )
        .min()
        .unwrap_or(stream_length);
    (start <= end && end <= stream_length).then_some((start, end))
}

fn parse_dimension_locus_group(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Option<DesignDimensionLocusGroup> {
    let (class_tag, after_tag) = lp_ascii(bytes, start)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = u32_at(bytes, start + 7)?;
    let count = usize::try_from(u32_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;
    let mut loci = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index = u32_at(bytes, position + 1)?;
        if !geometry_indices.contains(&geometry_record_index) {
            return None;
        }
        loci.push(DesignDimensionLocus {
            geometry_record_index,
            geometry_reference_offset: (position + 1) as u64,
            role: u32_at(bytes, position + 11)?,
            role_offset: (position + 11) as u64,
        });
        position = position.checked_add(15)?;
    }
    if bytes.get(position) != Some(&0)
        || bytes.get(position + 1) != Some(&1)
        || bytes.get(position + 6..position + 12) != Some(&[0; 6])
    {
        return None;
    }
    let owner_reference = u32_at(bytes, position + 2)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    let owner_reference_offset = (position + 2) as u64;
    let owner_role = u32_at(bytes, position + 12)?;
    let owner_role_offset = (position + 12) as u64;
    position = position.checked_add(16)?;
    let state = u32_at(bytes, position)?;
    let state_offset = position as u64;
    let return_count = usize::try_from(u32_at(bytes, position + 4)?).ok()?;
    if return_count != count {
        return None;
    }
    position = position.checked_add(8)?;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for _ in 0..return_count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let record_index = u32_at(bytes, position + 1)?;
        if !geometry_indices.contains(&record_index) {
            return None;
        }
        return_members.push(record_index);
        return_member_offsets.push((position + 1) as u64);
        position = position.checked_add(11)?;
    }
    if bytes.get(position) != Some(&0) {
        return None;
    }
    let next_byte_offset = position.checked_add(1)?;
    let (next_class_tag, next_after_tag) = lp_ascii(bytes, next_byte_offset)?;
    if next_after_tag != next_byte_offset.checked_add(7)?
        || next_class_tag.len() != 3
        || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(state);
    Some(DesignDimensionLocusGroup {
        id: String::new(),
        companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(next_byte_offset.checked_sub(start)?).ok()?,
        loci,
        owner_reference,
        owner_reference_offset,
        owner_role,
        owner_role_offset,
        state,
        state_offset,
        constraint_kinds,
        unknown_constraint_bits,
        return_members,
        return_member_offsets,
        next_class_tag,
        next_record_index: u32_at(bytes, next_after_tag)?,
        next_byte_offset: next_byte_offset as u64,
    })
}

/// Decode each sketch or construction-operation record referenced by a
/// parameter owner frame.
pub fn decode_parameter_scopes(
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignParameterScope>, CodecError> {
    let wanted = owners
        .iter()
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.scope_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for header in headers.iter().filter(|header| {
        native_stream(&header.id)
            .is_some_and(|scope| wanted.contains(&(scope.to_owned(), header.record_index)))
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && header.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(mut scope) = parse_parameter_scope(bytes, header) else {
            continue;
        };
        if scope.kind == "Sketch" {
            let start = usize::try_from(scope.byte_offset).ok();
            let end = usize::try_from(scope.paired_byte_offset).ok();
            let frame = start
                .zip(end)
                .and_then(|(start, end)| bytes.get(start..end));
            let matches = frame
                .into_iter()
                .flat_map(|frame| {
                    entities.iter().filter_map(move |entity| {
                        if native_stream(&entity.id) != native_stream(&header.id)
                            || entity.object_kind != Some(DesignObjectKind::Sketch)
                            || entity.entity_suffix > u64::from(u32::MAX)
                        {
                            return None;
                        }
                        let mut pattern = [0; 11];
                        pattern[0] = 1;
                        pattern[1..5].copy_from_slice(&(entity.entity_suffix as u32).to_le_bytes());
                        frame
                            .windows(pattern.len())
                            .position(|window| window == pattern)
                            .map(|offset| (entity, offset + 1))
                    })
                })
                .collect::<Vec<_>>();
            if let [(entity, relative_offset)] = matches.as_slice() {
                scope.entity_id = Some(entity.entity_id.clone());
                scope.entity_suffix = Some(entity.entity_suffix);
                scope.entity_reference_offset =
                    Some(scope.byte_offset.saturating_add(*relative_offset as u64));
            }
        }
        scope.id = format!(
            "f3d:{}:design-parameter-scope#{}",
            entry.name, header.byte_offset
        );
        out.push(scope);
    }
    out.sort_by_key(|scope| scope.id.clone());
    Ok(out)
}

/// Decode the edge-recipe operand frames named by Fillet and Chamfer scopes.
pub fn decode_edge_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "Fillet" | "Chamfer"))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(operand) = parse_edge_operand(bytes, scope, ordinal, header, recipes) else {
                continue;
            };
            out.push(operand);
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

/// Decode the face-recipe operand frames named by Extrude face groups.
pub fn decode_face_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignFaceOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let scopes = scopes
        .iter()
        .filter_map(|scope| Some(((native_stream(&scope.id)?, scope.record_index), scope)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for group in groups
        .iter()
        .filter(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Faces))
    {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.get(&(stream, group.scope_record_index)) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for record_index in &group.members {
            if !seen.insert((stream, scope.record_index, *record_index)) {
                continue;
            }
            let Some(scope_reference_ordinal) = scope
                .reference_members
                .iter()
                .position(|member| member == record_index)
                .and_then(|ordinal| u32::try_from(ordinal).ok())
            else {
                continue;
            };
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            if let Some(operand) =
                parse_face_operand(bytes, scope, scope_reference_ordinal, header, recipes)
            {
                out.push(operand);
            }
        }
    }
    out.sort_by_key(|operand| operand.id.clone());
    Ok(out)
}

/// Join each face recipe's persistent Design reference to active solved faces.
pub fn bind_face_operand_candidates(
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        operand.alternate_selector_candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags);
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| recipe.design_id.as_deref())
            .and_then(|value| value.parse::<i64>().ok())
        else {
            continue;
        };
        operand.candidate_faces = tags
            .iter()
            .filter(|tag| tag.design_references.contains(&design_reference))
            .filter_map(|tag| match &tag.target {
                AttributeTarget::Face(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        operand
            .candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.candidate_faces.dedup();
        let referenced = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<HashSet<_>>();
        operand.unreferenced_candidate_faces = operand
            .candidate_faces
            .iter()
            .filter(|face| !referenced.contains(face))
            .cloned()
            .collect();
        operand.alternate_selector_candidate_faces = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .cloned()
            .collect();
        operand
            .alternate_selector_candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.alternate_selector_candidate_faces.dedup();
    }
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub fn bind_edge_operand_candidates(
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags);
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces = edge_operand_candidate_faces(design_reference, tags);
    }
}

pub(crate) fn edge_operand_candidate_faces(
    design_reference: i64,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| tag.design_references.contains(&design_reference))
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

/// Resolve the unique sketch-profile frame named by every Extrude scope.
pub fn bind_extrude_profiles(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for scope in scopes.iter_mut().filter(|scope| scope.kind == "Extrude") {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let candidates = scope
            .reference_members
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(ordinal, record_index)| {
                let ordinal = u32::try_from(ordinal).ok()?;
                let header = headers.get(&(stream, record_index))?;
                parse_extrude_profile(bytes, stream, ordinal, header, entities)
            })
            .collect::<Vec<_>>();
        if let [profile] = candidates.as_slice() {
            scope.extrude_profile = Some(profile.clone());
        }
    }
    Ok(())
}

/// Decode the counted selection group named by each Extrude scope.
pub fn decode_extrude_selection_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| scope.kind == "Extrude") {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut group) = parse_extrude_selection_group(bytes, scope, ordinal, header) {
                group.id = format!(
                    "f3d:{}:design-extrude-selection-group#{}",
                    entry.name, header.byte_offset
                );
                out.push(group);
            }
        }
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

/// Decode counted construction-operand groups named by feature scopes.
pub fn decode_construction_operand_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|h| Some(((native_stream(&h.id)?, h.record_index), h)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "Extrude" | "Fillet" | "Chamfer"))
    {
        let scope_group_start = out.len();
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            if let Some(mut group) = parse_construction_operand_group(bytes, scope, ordinal, header)
            {
                group.id = format!(
                    "f3d:{}:design-construction-operand-group#{}",
                    entry.name, header.byte_offset
                );
                out.push(group);
            }
        }
        if scope.kind == "Extrude" {
            assign_extrude_face_roles(scope, &mut out[scope_group_start..]);
        }
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
) {
    let mut face_groups = groups
        .iter_mut()
        .filter(|group| group.extrude_role == Some(DesignExtrudeOperandRole::Faces));
    if scope.extrude_start == Some(DesignExtrudeStart::FromFace) {
        if let Some(group) = face_groups.next() {
            group.extrude_face_role = Some(DesignExtrudeFaceRole::Start);
        }
    }
    for group in face_groups {
        group.extrude_face_role = Some(DesignExtrudeFaceRole::Termination);
    }
}

/// Pair Fillet construction-operand groups with their radius inputs.
pub fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| scope.kind == "Fillet") {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let mut scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let mut owned_parameters = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream)
                    && owner.scope_record_index == scope.record_index
            })
            .filter_map(|owner| {
                Some((
                    owner.local_ordinal,
                    *parameters.get(&(stream, owner.parameter_record_index))?,
                ))
            })
            .collect::<Vec<_>>();
        owned_parameters.sort_by_key(|(ordinal, _)| *ordinal);
        let radii = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind == "Radius").then_some(*parameter))
            .collect::<Vec<_>>();
        let weights = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind == "TangencyWeight").then_some(*parameter)
            })
            .collect::<Vec<_>>();
        if scope_groups.len() != radii.len()
            || (!weights.is_empty() && weights.len() != scope_groups.len())
        {
            continue;
        }
        for (ordinal, (group, radius)) in scope_groups.into_iter().zip(radii).enumerate() {
            let Ok(group_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            out.push(DesignFilletRadiusGroup {
                id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                scope_record_index: scope.record_index,
                group_ordinal,
                group_record_index: group.record_index,
                edge_operand_record_indices: group.members.clone(),
                radius_parameter_record_index: radius.record_index,
                tangency_weight_parameter_record_index: weights
                    .get(ordinal)
                    .map(|parameter| parameter.record_index),
            });
        }
    }
    out.sort_by_key(|group| group.id.clone());
    out
}

fn parse_construction_operand_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignConstructionOperandGroup> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] {
        return None;
    }
    let count = usize::try_from(u32_at(bytes, start + 21)?).ok()?;
    if count == 0 {
        return None;
    }
    let mut position = start.checked_add(25)?;
    let mut members = Vec::with_capacity(count);
    let mut member_offsets = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&1) || bytes.get(position + 5..position + 11)? != [0; 6] {
            return None;
        }
        members.push(u32_at(bytes, position + 1)?);
        member_offsets.push(u64::try_from(position + 1).ok()?);
        position = position.checked_add(11)?;
    }
    if bytes.get(position..position + 2)? != [0; 2]
        || u32_at(bytes, position + 2)? != 1
        || bytes.get(position + 6) != Some(&1)
        || bytes.get(position + 11..position + 17)? != [0; 6]
        || bytes.get(position + 25..position + 35)? != [0; 10]
    {
        return None;
    }
    let identity_record_index = u32_at(bytes, position + 7)?;
    let role = read_u64(bytes, position + 17)?;
    let extrude_role = if scope.kind == "Extrude" {
        Some(match role {
            0x0000_0008_0000_0000 => DesignExtrudeOperandRole::Bodies,
            0x0000_0041_0000_0000 => DesignExtrudeOperandRole::Profile,
            0x0000_0011_0000_0000 => DesignExtrudeOperandRole::Faces,
            _ => return None,
        })
    } else {
        None
    };
    let opaque_index = u32_at(bytes, position + 35)?;
    let opaque_scalar = f64_at(bytes, position + 39)?;
    if opaque_index == 0
        || !opaque_scalar.is_finite()
        || u32_at(bytes, position + 47)? != opaque_index
        || bytes.get(position + 51) != Some(&1)
        || u32_at(bytes, position + 52)? != header.record_index.checked_add(2)?
        || bytes.get(position + 56..position + 62)? != [0; 6]
        || bytes.get(position + 62) != Some(&1)
        || !matches!(bytes.get(position + 63), Some(0 | 1))
        || bytes.get(position + 64) != Some(&0)
        || bytes.get(position + 65) != Some(&1)
        || u32_at(bytes, position + 66)? != header.record_index.checked_add(1)?
        || bytes.get(position + 70..position + 77)? != [0; 7]
        || bytes.get(position + 77) != Some(&1)
        || u32_at(bytes, position + 78)? != scope.record_index
        || bytes.get(position + 82..position + 88)? != [0; 6]
    {
        return None;
    }
    let paired_at = position + 88;
    let (paired_class_tag, after_tag) = lp_ascii(bytes, paired_at)?;
    if u32_at(bytes, after_tag)? != header.record_index {
        return None;
    }
    Some(DesignConstructionOperandGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        member_count_offset: u64::try_from(start + 21).ok()?,
        members,
        lost_edge_references: Vec::new(),
        member_offsets,
        identity_record_index,
        identity_record_offset: u64::try_from(position + 7).ok()?,
        role,
        extrude_role,
        extrude_face_role: None,
        role_offset: u64::try_from(position + 17).ok()?,
        opaque_index,
        opaque_index_offset: u64::try_from(position + 35).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(position + 39).ok()?,
        variant: bytes[position + 63] != 0,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

/// Decode the persistent identity frame named by each construction-operand group.
pub fn decode_construction_operand_identities(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandIdentity>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let Some(wrapper_header) = headers.get(&(stream, group.identity_record_index)) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        if let Some(mut identity) =
            parse_construction_operand_identity(bytes, group, wrapper_header)
        {
            identity.id = format!(
                "f3d:{}:design-construction-operand-identity#{}",
                entry.name, wrapper_header.byte_offset
            );
            out.push(identity);
        }
    }
    out.sort_by_key(|identity| identity.id.clone());
    Ok(out)
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
pub fn bind_lost_edge_groups(
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for group in groups {
        group.lost_edge_references.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut identity_matches = identities.iter().filter(|identity| {
            native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == group.record_index
        });
        let Some(identity) = identity_matches.next() else {
            continue;
        };
        if identity_matches.next().is_some() {
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has multiple identity chains",
                group.record_index
            )));
        }
        let Some((wrapper_record_index, wrapper_byte_offset, wrapper_class_tag)) = identity
            .wrapper_record_indices
            .first()
            .zip(identity.wrapper_byte_offsets.first())
            .zip(identity.wrapper_class_tags.first())
            .map(|((record_index, byte_offset), class_tag)| {
                (*record_index, *byte_offset, class_tag.as_str())
            })
        else {
            continue;
        };
        let mut stream_edges = lost_edges
            .iter()
            .filter(|edge| native_stream(&edge.id) == Some(stream))
            .collect::<Vec<_>>();
        stream_edges.sort_by_key(|edge| edge.record_byte_offset);
        let terminals = stream_edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.next_record_index == wrapper_record_index
                    && edge.next_byte_offset == wrapper_byte_offset
                    && edge.next_class_tag == wrapper_class_tag
            })
            .map(|(ordinal, _)| ordinal)
            .collect::<Vec<_>>();
        let [terminal] = terminals.as_slice() else {
            if terminals.is_empty() {
                continue;
            }
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has multiple terminating lost-edge runs",
                group.record_index
            )));
        };
        let mut start = *terminal;
        while start > 0 {
            let previous = stream_edges[start - 1];
            let current = stream_edges[start];
            if previous.next_byte_offset != current.record_byte_offset
                || previous.next_record_index != current.record_index
                || previous.next_class_tag != current.class_tag
            {
                break;
            }
            start -= 1;
        }
        let run = &stream_edges[start..=*terminal];
        if run.len() != group.members.len() {
            return Err(CodecError::Malformed(format!(
                "Fusion construction group {} has {} operands but its lost-edge run has {} records",
                group.record_index,
                group.members.len(),
                run.len()
            )));
        }
        group.lost_edge_references = run.iter().map(|edge| edge.id.clone()).collect();
    }
    Ok(())
}

fn parse_construction_operand_identity(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
) -> Option<DesignConstructionOperandIdentity> {
    let mut current_at = usize::try_from(wrapper_header.byte_offset).ok()?;
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_header.class_tag.clone();
    let mut wrapper_record_indices = Vec::new();
    let mut wrapper_byte_offsets = Vec::new();
    let mut wrapper_class_tags = Vec::new();
    let mut seen = HashSet::new();
    while bytes.get(current_at + 11..current_at + 21)? == [0; 10]
        && bytes.get(current_at + 21..current_at + 24)? == [1, 1, 0]
    {
        if !seen.insert((current_record_index, current_at)) {
            return None;
        }
        wrapper_record_indices.push(current_record_index);
        wrapper_byte_offsets.push(u64::try_from(current_at).ok()?);
        wrapper_class_tags.push(current_class_tag);
        current_at = current_at.checked_add(24)?;
        let (next_class_tag, after_next_tag) = lp_ascii(bytes, current_at)?;
        current_record_index = u32_at(bytes, after_next_tag)?;
        current_class_tag = next_class_tag;
    }
    if wrapper_record_indices.is_empty() {
        return None;
    }
    let persistent_identity = parse_extrude_identity_member(bytes, current_at).map(|member| {
        DesignConstructionPersistentIdentity {
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id: member.asset_id,
            asset_id_offset: member.asset_id_offset,
            context_id: member.context_id,
            context_id_offset: member.context_id_offset,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        }
    });
    Some(DesignConstructionOperandIdentity {
        id: String::new(),
        group_record_index: group.record_index,
        wrapper_record_indices,
        wrapper_byte_offsets,
        wrapper_class_tags,
        following_record_index: current_record_index,
        following_byte_offset: u64::try_from(current_at).ok()?,
        following_class_tag: current_class_tag,
        persistent_identity,
    })
}

fn parse_extrude_selection_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionGroup> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != scope.record_index
        || bytes.get(start + 26..start + 32)? != [0; 6]
    {
        return None;
    }
    let member_count = usize::try_from(u32_at(bytes, start + 32)?).ok()?;
    if member_count == 0 {
        return None;
    }
    let mut position = start.checked_add(36)?;
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        if bytes.get(position) != Some(&1) || bytes.get(position + 5..position + 11)? != [0; 6] {
            return None;
        }
        members.push(u32_at(bytes, position + 1)?);
        member_offsets.push(u64::try_from(position + 1).ok()?);
        position = position.checked_add(11)?;
    }
    let opaque_index = u32_at(bytes, position)?;
    let opaque_scalar = f64_at(bytes, position + 4)?;
    if opaque_index == 0
        || !opaque_scalar.is_finite()
        || u32_at(bytes, position + 12)? != opaque_index
        || bytes.get(position + 16) != Some(&1)
        || u32_at(bytes, position + 17)? != header.record_index.checked_add(2)?
        || bytes.get(position + 21..position + 27)? != [0; 6]
        || bytes.get(position + 27) != Some(&1)
        || !matches!(bytes.get(position + 28), Some(0 | 1))
        || bytes.get(position + 29) != Some(&0)
        || bytes.get(position + 30) != Some(&1)
        || u32_at(bytes, position + 31)? != header.record_index.checked_add(1)?
        || bytes.get(position + 35..position + 42)? != [0; 7]
        || bytes.get(position + 42) != Some(&1)
        || u32_at(bytes, position + 43)? != scope.record_index
        || bytes.get(position + 47..position + 53)? != [0; 6]
    {
        return None;
    }
    let paired_at = position.checked_add(53)?;
    let (paired_class_tag, after_paired_tag) = lp_ascii(bytes, paired_at)?;
    if u32_at(bytes, after_paired_tag)? != header.record_index {
        return None;
    }
    Some(DesignExtrudeSelectionGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        member_count_offset: u64::try_from(start + 32).ok()?,
        members,
        member_offsets,
        opaque_index,
        opaque_index_offset: u64::try_from(position).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(position + 4).ok()?,
        variant: bytes[position + 28] != 0,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

/// Decode the fixed-width records named by Extrude selection groups.
pub fn decode_extrude_selection_members(
    scan: &ContainerScan,
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut member) = parse_extrude_selection_member(bytes, group, ordinal, header)
            {
                member.id = format!(
                    "f3d:{}:design-extrude-selection-member#{}",
                    entry.name, header.byte_offset
                );
                out.push(member);
            }
        }
    }
    out.sort_by_key(|member| member.id.clone());
    Ok(out)
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub fn bind_extrude_selection_geometry(
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) {
    let selected_sketches = groups
        .iter()
        .filter_map(|group| {
            let stream = native_stream(&group.id)?;
            let scope = scopes.iter().find(|scope| {
                native_stream(&scope.id) == Some(stream)
                    && scope.record_index == group.scope_record_index
            })?;
            Some((
                (stream, group.record_index),
                scope.extrude_profile.as_ref()?.entity_suffix,
            ))
        })
        .collect::<HashMap<_, _>>();
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let Some(entity_suffix) = selected_sketches.get(&(stream, member.group_record_index))
        else {
            continue;
        };
        let Ok(entity_suffix) = u32::try_from(*entity_suffix) else {
            continue;
        };
        let point_operands = points.iter().filter_map(|point| {
            (native_stream(&point.id) == Some(stream)
                && point.owner_reference == Some(entity_suffix)
                && point.persistent_id == member.local_id)
                .then_some(SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                })
        });
        let curve_operands = curves.iter().filter_map(|curve| {
            (native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(entity_suffix)
                && (curve.primary_id == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                })
        });
        let matches = point_operands.chain(curve_operands).collect::<Vec<_>>();
        if let [resolved] = matches.as_slice() {
            member.resolved_geometry = Some(resolved.clone());
        }
    }
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub fn bind_extrude_selection_identities(
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) {
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let mut matches = identities
            .iter()
            .filter(|identity| {
                native_stream(&identity.id) == Some(stream)
                    && identity.following_record_index == member.record_index
                    && identity.following_byte_offset == member.byte_offset
                    && identity
                        .persistent_identity
                        .as_ref()
                        .is_some_and(|persistent| {
                            persistent.local_id == member.local_id
                                && persistent.asset_id == member.asset_id
                                && persistent.context_id == member.context_id
                        })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|identity| identity.wrapper_byte_offsets.first().copied());
        member.operand_identity_ids = matches
            .into_iter()
            .map(|identity| identity.id.clone())
            .collect();
    }
}

fn parse_extrude_selection_member(
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionMember> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let member = parse_extrude_identity_member(bytes, start)?;
    Some(DesignExtrudeSelectionMember {
        id: String::new(),
        group_record_index: group.record_index,
        group_member_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        local_id: member.local_id,
        local_id_offset: member.local_id_offset,
        asset_id: member.asset_id,
        asset_id_offset: member.asset_id_offset,
        context_id: member.context_id,
        context_id_offset: member.context_id_offset,
        resolved_geometry: None,
        operand_identity_ids: Vec::new(),
        historical_entity_kind: None,
        historical_entity_ref: None,
        historical_state_ids: Vec::new(),
        next_record_index: member.next_record_index,
        next_byte_offset: member.next_byte_offset,
    })
}

struct ParsedExtrudeIdentityMember {
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

fn parse_extrude_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedExtrudeIdentityMember> {
    if bytes.get(start + 11..start + 21)? != [0; 10] {
        return None;
    }
    let local_id = read_u64(bytes, start + 21)?;
    let (asset_id, after_asset_id) = lp_utf16(bytes, start + 29)?;
    let (context_id, after_context_id) = lp_utf16(bytes, after_asset_id)?;
    if !is_guid(&asset_id)
        || !is_guid(&context_id)
        || u32_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 9)? != [0; 5]
        || after_context_id.checked_add(9)? != start.checked_add(190)?
    {
        return None;
    }
    let next_at = start.checked_add(190)?;
    let (_, after_next_tag) = lp_ascii(bytes, next_at)?;
    Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset: u64::try_from(start + 21).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + 33).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        next_record_index: u32_at(bytes, after_next_tag)?,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

fn parse_extrude_profile(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Option<DesignExtrudeProfileOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || u32_at(bytes, start + 32)? != 1
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16(bytes, start + 36)?;
    if !is_guid(&asset_id) {
        return None;
    }
    let (entity_suffix_text, after_entity_suffix) = lp_utf16(bytes, after_asset_id)?;
    let entity_suffix = entity_suffix_text.parse::<u64>().ok()?;
    let paired_at = next_indexed_record_offset(bytes, start + 11)?;
    let (paired_class_tag, after_paired_tag) = lp_ascii(bytes, paired_at)?;
    if u32_at(bytes, after_paired_tag)? != header.record_index
        || after_entity_suffix.checked_add(94)? != paired_at
    {
        return None;
    }
    let matches = entities
        .iter()
        .filter(|entity| {
            native_stream(&entity.id) == Some(stream)
                && entity.object_kind == Some(DesignObjectKind::Sketch)
                && entity.entity_suffix == entity_suffix
        })
        .collect::<Vec<_>>();
    let [entity] = matches.as_slice() else {
        return None;
    };
    Some(DesignExtrudeProfileOperand {
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(start + 40).ok()?,
        entity_id: entity.entity_id.clone(),
        entity_suffix,
        entity_reference_offset: u64::try_from(after_asset_id + 4).ok()?,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

fn parse_edge_operand(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignEdgeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for _ in 0..5 {
        let offset = next_indexed_record_offset(bytes, position)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    let mut indexed = Vec::with_capacity(offsets.len());
    for offset in &offsets {
        let (class_tag, after_tag) = lp_ascii(bytes, *offset)?;
        indexed.push((class_tag, u32_at(bytes, after_tag)?));
    }
    let next_one = header.record_index.checked_add(1)?;
    let next_two = header.record_index.checked_add(2)?;
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != next_one
        || indexed[2].1 != next_two
        || indexed[3].1 != recipe_record_index
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && recipe.kind == ConstructionRecipeKind::Edge
                && recipe.byte_offset > recipe_start
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        b"edge_recipe_data".len(),
    )?;
    let recipe_references =
        decode_recipe_references(&recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(b"edge_recipe_data".len())?;
    let recipe_program_bytes =
        bytes.get(recipe_program_at..usize::try_from(next_byte_offset).ok()?)?;
    if recipe_program_bytes.is_empty()
        || recipe_program_bytes.len() % 4 != 0
        || recipe_program_bytes.len() > 64 * 1024
    {
        return None;
    }
    let recipe_program = recipe_program_bytes
        .chunks_exact(4)
        .map(|raw| {
            i32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: chunks_exact(4) yields four-byte slices"),
            )
        })
        .collect::<Vec<_>>();
    if recipe_program.get(0..7) != Some(&[-1, -1, 2, 0, -1, 1, -1]) {
        return None;
    }
    let recipe_structure = edge_recipe_structure(&recipe_program);
    Some(DesignEdgeOperand {
        id: format!(
            "f3d:{}:design-edge-operand#{}",
            stream.strip_prefix("f3d:").unwrap_or(stream),
            header.byte_offset
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset: u64::try_from(recipe_program_at).ok()?,
        recipe_program,
        recipe_structure,
        candidate_faces: Vec::new(),
        result_candidate_faces: Vec::new(),
        result_boundary_edge_slots: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        preceding_boundary_edge_slots: Vec::new(),
        changed_boundary_edge_slots: Vec::new(),
        deleted_boundary_edge_slots: Vec::new(),
        updated_boundary_edge_slots: Vec::new(),
        changed_boundary_edge_contexts: Vec::new(),
        recipe_reference_contexts: Vec::new(),
        recipe_selectors: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: indexed[4].1,
        next_byte_offset,
    })
}

pub(crate) fn edge_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    edge_recipe_structure_tail(program.get(7..)?)
}

fn edge_recipe_structure_tail(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    let runs = program
        .split(|word| *word == -1)
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>();
    if runs.first().is_none_or(|run| run.len() != 1) {
        return None;
    }
    let sides = [4usize, 5]
        .into_iter()
        .filter_map(|first_len| {
            let second_len = runs.len().checked_sub(1 + first_len)?;
            if !matches!(second_len, 4 | 5) {
                return None;
            }
            Some([
                edge_recipe_side(&runs[1..=first_len])?,
                edge_recipe_side(&runs[1 + first_len..])?,
            ])
        })
        .collect::<Vec<_>>();
    let [sides] = sides.as_slice() else {
        return None;
    };
    Some(crate::records::DesignEdgeRecipeStructure {
        root: runs[0][0],
        sides: sides.clone(),
    })
}

pub(crate) fn face_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignFaceRecipeStructure> {
    let runs = program
        .split(|word| *word == -1)
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>();
    if runs
        .get(0..3)
        .is_none_or(|prefix| prefix.iter().any(|run| run.len() != 1))
    {
        return None;
    }
    let sides = [4usize, 5]
        .into_iter()
        .filter_map(|first_len| {
            let second_len = runs.len().checked_sub(3 + first_len)?;
            if !matches!(second_len, 4 | 5) {
                return None;
            }
            Some([
                edge_recipe_side(&runs[3..3 + first_len])?,
                edge_recipe_side(&runs[3 + first_len..])?,
            ])
        })
        .collect::<Vec<_>>();
    let [sides] = sides.as_slice() else {
        return None;
    };
    Some(crate::records::DesignFaceRecipeStructure {
        root: runs[0][0],
        prelude: [runs[1][0], runs[2][0]],
        sides: sides.clone(),
    })
}

pub(crate) fn face_recipe_local_topology_references(
    structure: &crate::records::DesignFaceRecipeStructure,
    node_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    let words = std::iter::once(structure.root)
        .chain(std::iter::once(structure.prelude[0]))
        .chain(structure.sides.iter().flat_map(|side| {
            [Some(side.first), Some(side.second), side.third]
                .into_iter()
                .flatten()
        }));
    words
        .filter(|word| *word != 0)
        .map(|word| {
            let ordinal = std::num::NonZeroU32::new(u32::try_from(word).ok()?)?;
            (usize::try_from(ordinal.get()).ok()? <= node_count).then_some(ordinal)
        })
        .collect()
}

fn edge_recipe_side(runs: &[&[i32]]) -> Option<DesignTopologyRecipeSide> {
    if !matches!(runs.len(), 4 | 5)
        || runs[0].len() != 2
        || runs[1].len() != 1
        || runs[2].len() != 1
        || (runs.len() == 5 && runs[3].len() != 1)
    {
        return None;
    }
    let payload = runs.last()?;
    if payload.len() < 2 || (payload.len() - 2) % 8 != 0 || payload[0] != 0 {
        return None;
    }
    let field_count = std::num::NonZeroU32::new(u32::try_from(runs.len() - 1).ok()?)?;
    if i32::try_from(field_count.get()).ok()? != runs[0][0] {
        return None;
    }
    let payload_entry_count = u32::try_from((payload.len() - 2) / 8).ok()?;
    if i32::try_from(payload_entry_count).ok()? != payload[1] {
        return None;
    }
    Some(DesignTopologyRecipeSide {
        field_count,
        header_value: runs[0][1],
        first: runs[1][0],
        second: runs[2][0],
        third: (runs.len() == 5).then_some(runs[3][0]),
        payload_entry_count,
        entries: edge_recipe_entries(&payload[2..])?,
    })
}

fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    let entries = words
        .chunks_exact(8)
        .map(|entry| {
            let selector = entry[0];
            if !(0..=2).contains(&selector) {
                return None;
            }
            let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(entry[1]).ok()?)?;
            let topology_triplets = [
                edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
                edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
            ];
            topology_triplets
                .iter()
                .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
                .then_some(DesignTopologyRecipeEntry {
                    selector,
                    boundary_edge_count,
                    common_incident_edge_ordinal: (topology_triplets[0].incident_edge_ordinal
                        == topology_triplets[1].incident_edge_ordinal)
                        .then_some(topology_triplets[0].incident_edge_ordinal),
                    topology_triplets,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    entries
        .windows(2)
        .all(|pair| pair[0].selector < pair[1].selector)
        .then_some(entries)
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
    let middle = u32::try_from(*middle).ok()?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let (incident_side, incident_edge_ordinal) = if middle == outer.get() {
        (
            crate::records::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        )
    } else if middle.checked_add(1) == Some(outer.get()) {
        (
            crate::records::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        )
    } else {
        return None;
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle,
        vertex_ordinal,
        incident_edge_ordinal,
        incident_side,
    })
}

fn parse_face_operand(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignFaceOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for _ in 0..5 {
        let offset = next_indexed_record_offset(bytes, position)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    let mut indexed = Vec::with_capacity(offsets.len());
    for offset in &offsets {
        let (class_tag, after_tag) = lp_ascii(bytes, *offset)?;
        indexed.push((class_tag, u32_at(bytes, after_tag)?));
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != header.record_index.checked_add(1)?
        || indexed[2].1 != header.record_index.checked_add(2)?
        || indexed[3].1 != recipe_record_index
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && matches!(
                    recipe.kind,
                    ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
                )
                && recipe.byte_offset > recipe_start
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name_len,
    )?;
    let recipe_references =
        decode_recipe_references(&recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let recipe_program_bytes =
        bytes.get(recipe_program_at..usize::try_from(next_byte_offset).ok()?)?;
    if recipe_program_bytes.is_empty()
        || recipe_program_bytes.len() % 4 != 0
        || recipe_program_bytes.len() > 64 * 1024
    {
        return None;
    }
    let recipe_program = recipe_program_bytes
        .chunks_exact(4)
        .map(|raw| {
            i32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: chunks_exact(4) yields four-byte slices"),
            )
        })
        .collect::<Vec<_>>();
    if recipe_program.get(0..2) != Some(&[0, -1]) {
        return None;
    }
    let node_count = usize::try_from(*recipe_program.get(2)?).ok()?;
    if node_count == 0 || node_count > 100_000 {
        return None;
    }
    let recipe_program_offset = u64::try_from(recipe_program_at).ok()?;
    let recipe_node_indices = recipe_program
        .windows(3)
        .enumerate()
        .filter(|(_, values)| *values == [-1, -1, 2])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if recipe_node_indices.len() != node_count || recipe_node_indices.first() != Some(&3) {
        return None;
    }
    let recipe_node_offsets = recipe_node_indices
        .iter()
        .map(|index| u64::try_from(recipe_program_at.checked_add(index.checked_mul(4)?)?).ok())
        .collect::<Option<Vec<_>>>()?;
    let recipe_nodes = recipe_node_indices
        .iter()
        .copied()
        .zip(
            recipe_node_indices
                .iter()
                .copied()
                .skip(1)
                .chain(std::iter::once(recipe_program.len())),
        )
        .map(|(start, end)| {
            let program = recipe_program.get(start..end)?.to_vec();
            let recipe_structure = program.get(3..).and_then(face_recipe_structure);
            let local_topology_references = match &recipe_structure {
                Some(structure) => face_recipe_local_topology_references(structure, node_count)?,
                None => Vec::new(),
            };
            Some(crate::records::DesignFaceRecipeNode {
                byte_offset: u64::try_from(recipe_program_at.checked_add(start.checked_mul(4)?)?)
                    .ok()?,
                end_byte_offset: u64::try_from(recipe_program_at.checked_add(end.checked_mul(4)?)?)
                    .ok()?,
                recipe_structure,
                local_topology_references,
                program,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(DesignFaceOperand {
        id: format!(
            "f3d:{}:design-face-operand#{}",
            stream.strip_prefix("f3d:").unwrap_or(stream),
            header.byte_offset
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_kind: recipe.kind,
        recipe_program_offset,
        recipe_program,
        recipe_node_offsets,
        recipe_nodes,
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slot: None,
        next_record_index: indexed[4].1,
        next_byte_offset,
    })
}

fn parse_parameter_scope(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<DesignParameterScope> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut position = start.checked_add(11)?;
    let (paired_at, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (class_tag, after_tag) = lp_ascii(bytes, at)?;
        if u32_at(bytes, after_tag)? == header.record_index {
            break (at, class_tag);
        }
        position = at.checked_add(1)?;
    };
    let kind_end = paired_at.checked_sub(78)?;
    let mut candidates = Vec::new();
    for at in start + 11..kind_end {
        if let Some((kind, end)) = lp_utf16(bytes, at) {
            if end == kind_end && kind.chars().all(|character| !character.is_control()) {
                candidates.push((at, kind));
            }
        }
    }
    let [(kind_at, kind)] = candidates.as_slice() else {
        return None;
    };
    let reference_table_end = kind_at.checked_sub(4)?;
    let feature_ordinal = u32_at(bytes, kind_end)?;
    if feature_ordinal == 0 {
        return None;
    }
    let history_state_id_offset = reference_table_end;
    let history_state_id = match u32_at(bytes, history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let previous_history_state_id_offset = kind_end.checked_add(31)?;
    let previous_history_state_id = match u32_at(bytes, previous_history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let mut reference_tables = Vec::new();
    for count_at in start + 11..reference_table_end {
        let count = usize::try_from(u32_at(bytes, count_at)?).ok()?;
        if count == 0
            || count_at
                .checked_add(4)?
                .checked_add(count.checked_mul(11)?)?
                != reference_table_end
        {
            continue;
        }
        let first = count_at.checked_add(4)?;
        let mut members = Vec::with_capacity(count);
        let mut offsets = Vec::with_capacity(count);
        for ordinal in 0..count {
            let marker = first.checked_add(ordinal.checked_mul(11)?)?;
            if bytes.get(marker) != Some(&1) || bytes.get(marker + 5..marker + 11)? != [0; 6] {
                members.clear();
                break;
            }
            members.push(u32_at(bytes, marker + 1)?);
            offsets.push(u64::try_from(marker + 1).ok()?);
        }
        if members.len() == count {
            reference_tables.push((count_at, members, offsets));
        }
    }
    let [(reference_count_at, reference_members, reference_member_offsets)] =
        reference_tables.as_slice()
    else {
        return None;
    };
    let (
        extrude_operation,
        extrude_operation_offset,
        extrude_extent,
        extrude_extent_offsets,
        extrude_direction_reversed,
        extrude_direction_reversed_offset,
        extrude_start,
        extrude_start_offset,
    ) = if kind == "Extrude" {
        let direct_offset = start.checked_add(28)?;
        let referenced_offset = start.checked_add(38)?;
        let operation_offset = if bytes.get(start.checked_add(25)?) == Some(&1)
            && bytes.get(start.checked_add(30)?..start.checked_add(36)?)? == [0; 6]
        {
            referenced_offset
        } else {
            direct_offset
        };
        let operation = match u32_at(bytes, operation_offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        };
        let side_offset = operation_offset.checked_add(4)?;
        let termination_offset = operation_offset.checked_add(8)?;
        let extent = match (
            u32_at(bytes, side_offset)?,
            u32_at(bytes, termination_offset)?,
        ) {
            (1, 1) => DesignExtrudeExtent::OneSidedToFace,
            (1, 2) => DesignExtrudeExtent::OneSidedDistance,
            (2, 0) => DesignExtrudeExtent::TwoSidedDistance,
            _ => return None,
        };
        let direction_reversed_offset = operation_offset.checked_add(12)?;
        let direction_reversed = match bytes.get(direction_reversed_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        if bytes.get(operation_offset.checked_add(13)?)? != &1 {
            return None;
        }
        let start_offset = operation_offset.checked_add(14)?;
        let start = match bytes.get(start_offset)? {
            0 => DesignExtrudeStart::ProfilePlane,
            1 => DesignExtrudeStart::OffsetProfilePlane,
            2 => DesignExtrudeStart::FromFace,
            _ => return None,
        };
        (
            Some(operation),
            Some(operation_offset as u64),
            Some(extent),
            Some([side_offset as u64, termination_offset as u64]),
            Some(direction_reversed),
            Some(direction_reversed_offset as u64),
            Some(start),
            Some(start_offset as u64),
        )
    } else {
        (None, None, None, None, None, None, None, None)
    };
    Some(DesignParameterScope {
        id: String::new(),
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        record_index: header.record_index,
        frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
        kind: kind.clone(),
        kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
        extrude_operation,
        extrude_operation_offset,
        extrude_extent,
        extrude_extent_offsets,
        extrude_direction_reversed,
        extrude_direction_reversed_offset,
        extrude_start,
        extrude_start_offset,
        feature_ordinal,
        feature_ordinal_offset: u64::try_from(kind_end).ok()?,
        history_state_id,
        history_state_id_offset: u64::try_from(history_state_id_offset).ok()?,
        previous_history_state_id,
        previous_history_state_id_offset: u64::try_from(previous_history_state_id_offset).ok()?,
        reference_count_offset: u64::try_from(*reference_count_at).ok()?,
        reference_members: reference_members.clone(),
        reference_member_offsets: reference_member_offsets.clone(),
        extrude_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag,
        paired_byte_offset: paired_at as u64,
    })
}

/// Decode the unique local-to-model placement frame referenced by every
/// parameter-owning sketch scope.
pub fn decode_sketch_placements(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| scope.kind == "Sketch") {
        let (Some(entity_id), Some(entity_suffix)) =
            (scope.entity_id.as_deref(), scope.entity_suffix)
        else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && scope.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let start = usize::try_from(scope.byte_offset).ok();
        let end = usize::try_from(scope.paired_byte_offset).ok();
        let Some(frame) = start
            .zip(end)
            .and_then(|(start, end)| bytes.get(start..end))
        else {
            continue;
        };
        let mut referenced_indices = Vec::new();
        for window in frame.windows(11) {
            if window[0] == 1 && window[5..11] == [0; 6] {
                let record_index = u32::from_le_bytes([window[1], window[2], window[3], window[4]]);
                if !referenced_indices.contains(&record_index) {
                    referenced_indices.push(record_index);
                }
            }
        }
        let mut candidates = Vec::new();
        for record_index in referenced_indices {
            candidates.extend(parse_sketch_placement_candidates(
                bytes,
                scope.record_index,
                entity_id,
                entity_suffix,
                record_index,
            ));
        }
        if candidates.len() == 1 {
            let Some(mut placement) = candidates.pop() else {
                continue;
            };
            placement.id = format!(
                "f3d:{}:design-sketch-placement#{}",
                entry.name, placement.byte_offset
            );
            out.push(placement);
        }
    }
    out.sort_by_key(|placement| placement.id.clone());
    Ok(out)
}

fn parse_sketch_placement_candidates(
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &str,
    entity_suffix: u64,
    record_index: u32,
) -> Vec<DesignSketchPlacement> {
    let mut headers = Vec::new();
    let mut position = 0usize;
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if u32_at(bytes, at + 7) == Some(record_index) {
            headers.push(at);
        }
        position = at + 1;
    }
    let mut out = Vec::new();
    for pair in headers.windows(2) {
        let start = pair[0];
        let paired_at = pair[1];
        let frame_length = paired_at.saturating_sub(start);
        if frame_length != 201 && frame_length != 329 {
            continue;
        }
        let Some((class_tag, after_tag)) = lp_ascii(bytes, start) else {
            continue;
        };
        let Some((paired_class_tag, paired_after_tag)) = lp_ascii(bytes, paired_at) else {
            continue;
        };
        if after_tag != start + 7
            || paired_after_tag != paired_at + 7
            || class_tag.len() != 3
            || paired_class_tag.len() != 3
            || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || u32_at(bytes, paired_after_tag) != Some(record_index)
        {
            continue;
        }
        let (transform, transform_offset) = if frame_length == 201 {
            (identity_matrix(), None)
        } else {
            let Some(values) = f64s_at(bytes, start + 55, 16) else {
                continue;
            };
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.iter().copied().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            if !valid_sketch_transform(&transform) {
                continue;
            }
            (transform, Some((start + 55) as u64))
        };
        out.push(DesignSketchPlacement {
            id: String::new(),
            scope_record_index,
            entity_id: entity_id.to_owned(),
            entity_suffix,
            byte_offset: start as u64,
            class_tag,
            record_index,
            frame_length: frame_length as u64,
            transform,
            transform_offset,
            paired_class_tag,
            paired_byte_offset: paired_at as u64,
        });
    }
    out
}

fn identity_matrix() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(crate) fn valid_sketch_transform(transform: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-10;
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPSILON {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub fn decode_persistent_references(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for (entry_ordinal, entry) in scan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for &(name, kind) in &[
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
            let mut cursor = 0;
            while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
                let offset = cursor + relative;
                cursor = offset + name.len();
                let compact_type_offset = offset + name.len();
                let type_offset = if u32_at(bytes, compact_type_offset) == Some(23) {
                    compact_type_offset
                } else if u32_at(bytes, compact_type_offset) == Some(2)
                    && u32_at(bytes, compact_type_offset + 4) == Some(14)
                    && bytes
                        .get(compact_type_offset + 8..compact_type_offset + 22)
                        .is_some()
                    && u32_at(bytes, compact_type_offset + 22) == Some(23)
                {
                    compact_type_offset + 22
                } else {
                    continue;
                };
                let Some(length_bytes) = bytes.get(type_offset..type_offset + 4) else {
                    continue;
                };
                if u32::from_le_bytes(length_bytes.try_into().expect(
                    "invariant: length_bytes is a 4-byte slice from bytes.get(range) of length 4",
                )) != 23
                {
                    continue;
                }
                let type_name = b"IntrinsicMetaTypeuint64";
                if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name)
                {
                    continue;
                }
                let value_offset = type_offset + 4 + type_name.len();
                let Some(raw) = bytes.get(value_offset..value_offset + 8) else {
                    continue;
                };
                out.push((
                    entry_ordinal,
                    PersistentReference {
                        id: format!("f3d:{}:persistent-reference#{offset}", entry.name),
                        byte_offset: offset as u64,
                        value_offset: (value_offset - offset) as u32,
                        kind,
                        value: u64::from_le_bytes(raw.try_into().expect(
                            "invariant: raw is an 8-byte slice from bytes.get(range) of length 8",
                        )),
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(entry_ordinal, reference)| (*entry_ordinal, reference.byte_offset));
    Ok(out.into_iter().map(|(_, reference)| reference).collect())
}

/// Decode every indexed `EDGE_REFERENCE_LOST` record from each design
/// `BulkStream` entry in `scan`.
pub fn decode_lost_edge_references(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    let marker = b"EDGE_REFERENCE_LOST";
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..]
            .windows(marker.len())
            .position(|window| window == marker)
        {
            let offset = cursor + relative;
            cursor = offset + marker.len();
            let Some(header_offset) = offset.checked_sub(29) else {
                continue;
            };
            let Some((class_tag, after_tag)) = lp_ascii(bytes, header_offset) else {
                continue;
            };
            if after_tag != header_offset + 7
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || bytes.get(header_offset + 11..header_offset + 25) != Some(&[0; 14])
                || u32_at(bytes, header_offset + 25) != Some(marker.len() as u32)
            {
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                continue;
            };
            let next_byte_offset = offset + marker.len();
            let Some((next_class_tag, after_next_tag)) = lp_ascii(bytes, next_byte_offset) else {
                continue;
            };
            if after_next_tag != next_byte_offset + 7
                || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let Some(next_record_index) = u32_at(bytes, after_next_tag) else {
                continue;
            };
            out.push(LostEdgeReference {
                id: format!("f3d:{}:lost-edge-reference#{header_offset}", entry.name),
                record_byte_offset: header_offset as u64,
                class_tag_offset: (header_offset + 4) as u64,
                class_tag,
                record_index,
                record_index_offset: (header_offset + 7) as u64,
                byte_offset: offset as u64,
                next_byte_offset: next_byte_offset as u64,
                next_class_tag,
                next_record_index,
            });
        }
    }
    Ok(out)
}

/// Decode every GUID-owned design object record from each design
/// `MetaStream` entry in `scan` ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): an ASCII type name, the design
/// entity IDs it owns, its self GUID, an optional parent GUID, and a
/// revision. Unrecognized type names remain exact native object kinds.
pub fn decode_objects(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignObject>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let Some((name, after_name)) = lp_ascii(bytes, offset) else {
                offset += 1;
                continue;
            };
            if name.is_empty()
                || is_guid(&name)
                || !name.bytes().all(|byte| byte.is_ascii_graphic())
            {
                offset += 1;
                continue;
            }
            let kind = object_kind(&name);
            let Some(count_raw) = bytes.get(after_name..after_name + 4) else {
                break;
            };
            let count = usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
            let ids_end = after_name
                .checked_add(4)
                .and_then(|at| count.checked_mul(8).and_then(|size| at.checked_add(size)));
            let Some(ids_end) = ids_end.filter(|end| count <= 200 && *end <= bytes.len()) else {
                offset += 1;
                continue;
            };
            let entity_ids = bytes[after_name + 4..ids_end]
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect::<Vec<_>>();
            let entity_id_offsets = (0..entity_ids.len())
                .map(|index| (after_name + 4 + index * 8) as u64)
                .collect();
            let Some((self_guid, after_self)) =
                lp_ascii(bytes, ids_end).filter(|(guid, _)| is_guid(guid))
            else {
                offset += 1;
                continue;
            };
            let mut tail = after_self;
            while bytes.get(tail) == Some(&0) {
                tail += 1;
            }
            let zero_run_length = u32::try_from(tail - after_self).unwrap_or(u32::MAX);
            let (parent_guid, parent_guid_offset, revision_offset) = lp_ascii(bytes, tail)
                .filter(|(guid, _)| is_guid(guid))
                .map_or((None, None, tail), |(guid, end)| {
                    (Some(guid), Some((tail + 4) as u64), end)
                });
            let Some(revision_raw) = bytes.get(revision_offset..revision_offset + 4) else {
                offset += 1;
                continue;
            };
            let revision = u32::from_le_bytes(revision_raw.try_into().expect(
                "invariant: revision_raw is a 4-byte slice from bytes.get(range) of length 4",
            ));
            if revision > 10_000 {
                offset += 1;
                continue;
            }
            out.push(DesignObject {
                id: format!("f3d:{}:design-object#{offset}", entry.name),
                byte_offset: offset as u64,
                kind,
                entity_ids,
                entity_id_offsets,
                self_guid,
                self_guid_offset: (ids_end + 4) as u64,
                zero_run_length,
                parent_guid,
                parent_guid_offset,
                revision,
                revision_offset: revision_offset as u64,
            });
            offset = revision_offset + 4;
        }
    }
    Ok(out)
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header.
pub fn decode_entity_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    let mut object_kinds = HashMap::new();
    for object in decode_objects(reader, scan)? {
        for entity_id in object.entity_ids {
            object_kinds
                .entry(entity_id)
                .or_insert_with(|| object.kind.clone());
        }
    }
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 30 <= bytes.len() {
            let Some(relative) = bytes[offset..]
                .windows(4)
                .position(|window| window == [3, 0, 0, 0])
            else {
                break;
            };
            let start = offset + relative;
            offset = start + 1;
            let Some(class_tag) = bytes.get(start + 4..start + 7) else {
                break;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(entity_raw) = bytes.get(start + 7..start + 15) else {
                break;
            };
            let entity_suffix = u64::from_le_bytes(entity_raw.try_into().expect(
                "invariant: entity_raw is an 8-byte slice from bytes.get(range) of length 8",
            ));
            if entity_suffix == 0
                || entity_suffix >= 1 << 32
                || bytes.get(start + 15..start + 20) != Some(&[0u8; 5])
            {
                continue;
            }
            let (optional_slot_present, string_offset) = match bytes[start + 20] {
                0 => (false, start + 21),
                1 if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
                _ => continue,
            };
            let Some((entity_id, end)) = lp_utf16(bytes, string_offset) else {
                continue;
            };
            let Some((_, suffix)) = entity_id.rsplit_once('_') else {
                continue;
            };
            if suffix.parse::<u64>().ok() != Some(entity_suffix) {
                continue;
            }
            let object_kind = object_kinds.get(&entity_suffix).cloned();
            let (
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                record_end,
            ) = if object_kind == Some(DesignObjectKind::Sketch) {
                decode_reference_list(bytes, end).map_or_else(
                    || (None, None, None, Vec::new(), Vec::new(), end),
                    |list| {
                        (
                            Some(list.record_reference),
                            Some(list.record_reference_offset as u64),
                            Some(list.declared_count),
                            list.references,
                            list.reference_offsets
                                .into_iter()
                                .map(|offset| offset as u64)
                                .collect(),
                            list.end,
                        )
                    },
                )
            } else {
                (None, None, None, Vec::new(), Vec::new(), end)
            };
            out.push(DesignEntityHeader {
                id: format!("f3d:{}:design-entity-header#{start}", entry.name),
                byte_offset: start as u64,
                entity_suffix,
                entity_id,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                optional_slot_present,
                object_kind,
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
            });
            offset = record_end;
        }
    }
    Ok(out)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub fn decode_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = entities
        .iter()
        .filter_map(|entity| {
            let scope = native_stream(&entity.id)?;
            Some(
                entity
                    .reference_indices
                    .iter()
                    .map(move |record_index| (scope.to_owned(), *record_index)),
            )
        })
        .flatten()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub fn decode_related_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = indices
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

fn decode_headers_for_indices(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<(String, u32)>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 11 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, position) else {
                position += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                position += 1;
                continue;
            }
            let Some(raw) = bytes.get(after_tag..after_tag + 4) else {
                break;
            };
            let record_index = u32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: raw is a 4-byte slice from bytes.get(range) of length 4"),
            );
            let scope = format!("f3d:{}", entry.name);
            if wanted.contains(&(scope, record_index)) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    id: format!("f3d:{}:design-record-header#{position}", entry.name),
                    record_index,
                    class_tag,
                    byte_offset: position as u64,
                });
            }
            // Headers are located in an otherwise heterogeneous stream. Keep
            // the scan byte-aligned so a plausible length-prefixed string in
            // an enclosing payload cannot skip a real nested header.
            position += 1;
        }
    }
    out.sort_by_key(|record| record.id.clone());
    Ok(out)
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub fn decode_sketch_relations(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let scope = format!("f3d:{}", entry.name);
        let owners = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(scope.as_str())
                    && entity.object_kind == Some(DesignObjectKind::Sketch)
            })
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<std::collections::HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in records
            .iter()
            .filter(|record| native_stream(&record.id) == Some(scope.as_str()))
        {
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                continue;
            };
            let Some((
                members,
                member_offsets,
                auxiliary_references,
                auxiliary_reference_offsets,
                owner_reference,
                owner_reference_offset,
                state,
                state_offset,
                return_members,
                return_member_offsets,
                parsed_end,
            )) = parse_sketch_relation(payload, &owners)
            else {
                continue;
            };
            if payload
                .get(parsed_end..)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
            {
                continue;
            }
            let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(state);
            out.push(SketchRelation {
                id: format!("f3d:{}:sketch-relation#{}", entry.name, record.record_index),
                record_index: record.record_index,
                class_tag: record.class_tag.clone(),
                byte_offset: record.byte_offset,
                state_offset: state_offset as u32,
                owner_reference,
                owner_entity_id: String::new(),
                owner_reference_offset: owner_reference_offset as u32,
                auxiliary_references,
                auxiliary_reference_offsets: auxiliary_reference_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                members,
                resolved_members: Vec::new(),
                member_offsets: member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                state,
                constraint_kinds,
                unknown_constraint_bits,
                return_members,
                resolved_return_members: Vec::new(),
                return_member_offsets: return_member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                raw_bytes: payload.to_vec(),
            });
        }
    }
    Ok(out)
}

pub(crate) const SKETCH_CONSTRAINT_MASK: u32 = 0x3000_3fff;

pub(crate) fn decode_constraint_kinds(state: u32) -> (Vec<SketchConstraintKind>, u32) {
    let definitions = [
        (0x0000_0001, SketchConstraintKind::Coincident),
        (0x0000_0002, SketchConstraintKind::Colinear),
        (0x0000_0004, SketchConstraintKind::Concentric),
        (0x0000_0008, SketchConstraintKind::EqualLength),
        (0x0000_0010, SketchConstraintKind::Parallel),
        (0x0000_0020, SketchConstraintKind::Perpendicular),
        (0x0000_0040, SketchConstraintKind::Horizontal),
        (0x0000_0080, SketchConstraintKind::Vertical),
        (0x0000_0100, SketchConstraintKind::Tangent),
        (0x0000_0200, SketchConstraintKind::Curvature),
        (0x0000_0400, SketchConstraintKind::Symmetry),
        (0x0000_0800, SketchConstraintKind::Equal),
        (0x0000_1000, SketchConstraintKind::Midpoint),
        (0x0000_2000, SketchConstraintKind::Polygon),
        (0x1000_0000, SketchConstraintKind::CircularPattern),
        (0x2000_0000, SketchConstraintKind::RectangularPattern),
    ];
    let mut kinds = if state == 0 {
        vec![SketchConstraintKind::Coincident]
    } else {
        Vec::new()
    };
    let mut recognized = 0u32;
    for (bit, kind) in definitions {
        if state & bit != 0 {
            kinds.push(kind);
            recognized |= bit;
        }
    }
    debug_assert_eq!(recognized, state & SKETCH_CONSTRAINT_MASK);
    (kinds, state & !SKETCH_CONSTRAINT_MASK)
}

/// Decode every sketch-point record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: the persistent point id, a paired record
/// reference, and the sketch `(u, v)` coordinates, converted centimetre→
/// millimetre. Records whose scaled coordinates are non-finite are skipped.
pub fn decode_sketch_points(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 112 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((persistent_id, paired_reference, x, y, shift, entity_genesis)) =
                decode_sketch_point(payload)
            else {
                at += 1;
                continue;
            };
            let (u, v) = (x * 10.0, y * 10.0);
            if !u.is_finite() || !v.is_finite() {
                at += 1;
                continue;
            }
            if emitted.insert(record_index) {
                out.push(SketchPoint {
                    id: format!("f3d:{}:sketch-point#{at}", entry.name),
                    record_index,
                    owner_reference: None,
                    class_tag,
                    byte_offset: at as u64,
                    coordinate_offset: (89 + shift) as u32,
                    entity_genesis,
                    persistent_id,
                    paired_reference,
                    coordinates: Point2::new(u, v),
                    raw_bytes: payload[..112 + shift].to_vec(),
                });
            }
            at += 112;
        }
    }
    Ok(out)
}

fn decode_sketch_point(payload: &[u8]) -> Option<(u64, u32, f64, f64, usize, Option<u64>)> {
    if let Some(point) = decode_sketch_point_variant(payload, 0, 1) {
        return Some((point.0, point.1, point.2, point.3, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_point_variant(payload, 52, 2)
        .map(|point| (point.0, point.1, point.2, point.3, 52, Some(entity_genesis)))
}

fn decode_sketch_point_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u32, f64, f64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(6)
        || payload.get(29 + shift..35 + shift) != Some(b"pt_tag")
        || u32_at(payload, 35 + shift) != Some(23)
        || payload.get(39 + shift..62 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || payload.get(70 + shift) != Some(&1)
        || !payload
            .get(75 + shift..89 + shift)?
            .iter()
            .all(|&byte| byte <= 1)
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(62 + shift..70 + shift)?.try_into().ok()?),
        u32_at(payload, 71 + shift)?,
        f64_at(payload, 89 + shift)?,
        f64_at(payload, 97 + shift)?,
    ))
}

/// Decode every sketch-curve record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub fn decode_sketch_curve_identities(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 133 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
                decode_sketch_curve_identity(payload)
            else {
                at += 1;
                continue;
            };
            if emitted.insert(record_index) {
                let geometry_payload = payload
                    .get(geometry_shift..)
                    .expect("invariant: geometry_shift (0 or 52) is <= payload.len() (checked >= 133 by the at + 133 <= bytes.len() loop guard)");
                out.push(SketchCurveIdentity {
                    id: format!("f3d:{}:sketch-curve-identity#{at}", entry.name),
                    record_index,
                    owner_reference: None,
                    class_tag,
                    byte_offset: at as u64,
                    geometry_offset: (133 + geometry_shift) as u32,
                    entity_genesis,
                    primary_id,
                    secondary_id,
                    geometry: decode_sketch_nurbs(geometry_payload)
                        .or_else(|| decode_circular_arc(geometry_payload))
                        .or_else(|| decode_line(geometry_payload))
                        .or_else(|| decode_referenced_analytic(geometry_payload)),
                });
            }
            at += 133;
        }
    }
    Ok(out)
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    let sketch_owners = entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
        .filter_map(|entity| {
            Some((
                (native_stream(&entity.id)?, entity.entity_suffix as u32),
                entity.entity_id.as_str(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    for relation in relations.iter_mut() {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            CodecError::Malformed(format!(
                "Fusion sketch relation {} has no Design stream identity",
                relation.record_index
            ))
        })?;
        relation.owner_entity_id = sketch_owners
            .get(&(scope, relation.owner_reference))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "Fusion sketch relation {} in {scope} has no owning Design entity {}",
                    relation.record_index, relation.owner_reference,
                ))
            })?
            .to_string();
    }
    let typed_records = points
        .iter()
        .filter_map(|point| Some((native_stream(&point.id)?, point.record_index)))
        .chain(
            curves
                .iter()
                .filter_map(|curve| Some((native_stream(&curve.id)?, curve.record_index))),
        )
        .collect::<std::collections::HashSet<_>>();
    let mut owners = std::collections::HashMap::new();
    for relation in relations.iter() {
        let scope = native_stream(&relation.id).expect("relation stream checked above");
        for record_index in relation.members.iter().chain(&relation.return_members) {
            if !typed_records.contains(&(scope, *record_index)) {
                continue;
            }
            if owners
                .insert((scope, *record_index), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                return Err(CodecError::Malformed(format!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    for point in points.iter_mut() {
        point.owner_reference = native_stream(&point.id)
            .and_then(|scope| owners.get(&(scope, point.record_index)))
            .copied();
    }
    for curve in curves.iter_mut() {
        curve.owner_reference = native_stream(&curve.id)
            .and_then(|scope| owners.get(&(scope, curve.record_index)))
            .copied();
    }
    let operands = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                },
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                },
            ))
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let resolve = |scope: &str, indices: &[u32]| {
        indices
            .iter()
            .map(|record_index| {
                operands.get(&(scope, *record_index)).cloned().unwrap_or(
                    SketchRelationOperand::Record {
                        record_index: *record_index,
                    },
                )
            })
            .collect()
    };
    for relation in relations {
        let scope = native_stream(&relation.id).expect("relation stream checked above");
        relation.resolved_members = resolve(scope, &relation.members);
        relation.resolved_return_members = resolve(scope, &relation.return_members);
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || u32_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || u32_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(70 + shift..78 + shift)?.try_into().ok()?),
        u64::from_le_bytes(payload.get(125 + shift..133 + shift)?.try_into().ok()?),
    ))
}

fn decode_circular_arc(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let normal = Vector3::new(values[3], values[4], values[5]);
    let reference_direction = Vector3::new(values[6], values[7], values[8]);
    let dot = normal.x * reference_direction.x
        + normal.y * reference_direction.y
        + normal.z * reference_direction.z;
    if (normal.norm() - 1.0).abs() > 1.0e-9
        || (reference_direction.norm() - 1.0).abs() > 1.0e-9
        || dot.abs() > 1.0e-9
        || values[9] <= 0.0
        || values[10].abs() > std::f64::consts::TAU + 1.0e-9
        || values[11].abs() > std::f64::consts::TAU + 1.0e-9
        || (values[11] - values[10]).abs() < 1.0e-12
    {
        return None;
    }
    Some(SketchCurveGeometry::Arc {
        center: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
        normal,
        reference_direction,
        radius: values[9] * 10.0,
        start_angle: values[10],
        end_angle: values[11],
    })
}

fn decode_referenced_analytic(payload: &[u8]) -> Option<SketchCurveGeometry> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    let shifted = payload.get(11..)?;
    decode_circular_arc(shifted).or_else(|| decode_line(shifted))
}

fn decode_sketch_nurbs(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let base = 133usize;
    let prefix = payload.get(base..base + 8)?;
    let carrier_reference = (prefix != [0xff; 8]).then(|| {
        u64::from_le_bytes(
            prefix
                .try_into()
                .expect("invariant: prefix is an 8-byte slice from payload.get(range) of length 8"),
        )
    });
    if u32_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    if !subtype_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let degree = u32_at(payload, base + 90)?;
    let fit_tolerance = f64_at(payload, base + 94)?;
    let knot_count = usize::try_from(u32_at(payload, base + 102)?).ok()?;
    if u32_at(payload, base + 106)? as usize != knot_count
        || u32_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(u32_at(payload, weights_at)?).ok()?;
    if u32_at(payload, weights_at + 4)? as usize != weight_count
        || u32_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(u32_at(payload, points_at)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || u32_at(payload, points_at + 4)? as usize != point_count
        || u32_at(payload, points_at + 8)? != 8
        || knot_count != point_count.checked_add(degree as usize + 1)?
    {
        return None;
    }
    let coordinates = f64s_at(payload, points_at + 12, point_count.checked_mul(3)?)?;
    if knots.windows(2).any(|pair| pair[0] > pair[1])
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        || coordinates.iter().any(|value| !value.is_finite())
        || !fit_tolerance.is_finite()
    {
        return None;
    }
    let control_points = coordinates
        .chunks_exact(3)
        .map(|point| Point3::new(point[0] * 10.0, point[1] * 10.0, point[2] * 10.0))
        .collect();
    Some(SketchCurveGeometry::Nurbs {
        carrier_reference,
        subtype_class_tag,
        subtype_record_index: u32_at(payload, base + 15)?,
        degree,
        fit_tolerance: fit_tolerance * 10.0,
        scalar_width: 8,
        knots,
        weights,
        control_points,
    })
}

fn decode_line(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let direction = Vector3::new(values[6], values[7], values[8]);
    let normal = Vector3::new(values[9], values[10], values[11]);
    let length = displacement.norm();
    if length <= 0.0 {
        return None;
    }
    let parallel_error = Vector3::new(
        displacement.x / length - direction.x,
        displacement.y / length - direction.y,
        displacement.z / length - direction.z,
    )
    .norm();
    let dot = direction.x * normal.x + direction.y * normal.y + direction.z * normal.z;
    if (direction.norm() - 1.0).abs() > 1.0e-9
        || (normal.norm() - 1.0).abs() > 1.0e-9
        || parallel_error > 1.0e-9
        || dot.abs() > 1.0e-9
    {
        return None;
    }
    let start = Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0);
    Some(SketchCurveGeometry::Line {
        start,
        end: Point3::new(
            start.x + displacement.x * 10.0,
            start.y + displacement.y * 10.0,
            start.z + displacement.z * 10.0,
        ),
        direction,
        normal,
    })
}

type ParsedSketchRelation = (
    Vec<u32>,
    Vec<usize>,
    Vec<u32>,
    Vec<usize>,
    u32,
    usize,
    u32,
    usize,
    Vec<u32>,
    Vec<usize>,
    usize,
);

fn parse_sketch_relation(
    payload: &[u8],
    owners: &std::collections::HashSet<u32>,
) -> Option<ParsedSketchRelation> {
    if payload.get(19) != Some(&1) {
        return None;
    }
    let member_count = usize::try_from(u32_at(payload, 20)?).ok()?;
    if member_count > 64 {
        return None;
    }
    let mut cursor = 24;
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let (value, end) = marked_u32(payload, cursor)?;
        members.push(value);
        member_offsets.push(cursor + 1);
        cursor = next_reference_marker(payload, end)?;
    }
    let mut auxiliary_references = Vec::new();
    let mut auxiliary_reference_offsets = Vec::new();
    let (owner_reference, owner_reference_offset, end) = loop {
        let (reference, end) = marked_u32(payload, cursor)?;
        if owners.contains(&reference) {
            break (reference, cursor + 1, end);
        }
        auxiliary_references.push(reference);
        auxiliary_reference_offsets.push(cursor + 1);
        cursor = next_reference_marker(payload, end)?;
    };
    cursor = next_nonzero(payload, end)?;
    let state_offset = cursor + usize::from(payload.get(cursor) == Some(&1));
    let (state, end) = if payload.get(cursor) == Some(&1) {
        marked_u32(payload, cursor)?
    } else {
        (u32_at(payload, cursor)?, cursor + 4)
    };
    cursor = next_nonzero(payload, end)?;
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > 64 {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for ordinal in 0..return_count {
        cursor = next_reference_marker(payload, cursor)?;
        let (value, end) = marked_u32(payload, cursor)?;
        return_members.push(value);
        return_member_offsets.push(cursor + 1);
        cursor = end;
        if ordinal + 1 < return_count {
            cursor = next_reference_marker(payload, cursor)?;
        }
    }
    let parsed_end = cursor;
    Some((
        members,
        member_offsets,
        auxiliary_references,
        auxiliary_reference_offsets,
        owner_reference,
        owner_reference_offset,
        state,
        state_offset,
        return_members,
        return_member_offsets,
        parsed_end,
    ))
}

fn next_indexed_record_offset(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 11 <= bytes.len() {
        let Some((class_tag, after_tag)) = lp_ascii(bytes, position) else {
            position += 1;
            continue;
        };
        if class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && bytes.get(after_tag..after_tag + 4).is_some()
        {
            return Some(position);
        }
        position += 1;
    }
    None
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1)).then_some((u32_at(bytes, position + 1)?, position + 5))
}

fn next_reference_marker(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 5 <= bytes.len() {
        if bytes.get(position) == Some(&1) {
            let reference = u32_at(bytes, position + 1)?;
            if reference <= 10_000_000 {
                return Some(position);
            }
        }
        position += 1;
    }
    None
}

fn next_nonzero(bytes: &[u8], mut position: usize) -> Option<usize> {
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    (position + 4 <= bytes.len()).then_some(position)
}

struct SketchReferenceList {
    record_reference: u32,
    record_reference_offset: usize,
    declared_count: u32,
    references: Vec<u32>,
    reference_offsets: Vec<usize>,
    end: usize,
}

fn decode_reference_list(bytes: &[u8], position: usize) -> Option<SketchReferenceList> {
    let record_reference = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?);
    if bytes.get(position + 4..position + 8) != Some(&[0; 4]) || bytes.get(position + 8) != Some(&1)
    {
        return None;
    }
    let declared_count =
        u32::from_le_bytes(bytes.get(position + 9..position + 13)?.try_into().ok()?);
    let mut cursor = position + 13;
    let mut references = Vec::new();
    let mut reference_offsets = Vec::new();
    while bytes.get(cursor) == Some(&1) && bytes.get(cursor + 5..cursor + 11) == Some(&[0; 6]) {
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        reference_offsets.push(cursor + 1);
        cursor += 11;
    }
    (references.len() == declared_count as usize).then_some(SketchReferenceList {
        record_reference,
        record_reference_offset: position,
        declared_count,
        references,
        reference_offsets,
        end: cursor,
    })
}

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub fn decode_body_members(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    prefix.extend_from_slice(&0u16.to_le_bytes());
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let Some(count_raw) = bytes.get(count_offset..count_offset + 4) else {
            continue;
        };
        let count =
            usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
        if count > 100_000 {
            continue;
        }
        let mut cursor = count_offset + 4;
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                decoded.clear();
                break;
            }
            let Some(id_raw) = bytes.get(cursor + 1..cursor + 9) else {
                decoded.clear();
                break;
            };
            let Some(flags_raw) = bytes.get(cursor + 9..cursor + 11) else {
                decoded.clear();
                break;
            };
            decoded.push(DesignBodyMember {
                id: format!("f3d:{}:design-body-member#{cursor}", entry.name),
                byte_offset: cursor as u64,
                entity_suffix: u64::from_le_bytes(id_raw.try_into().expect(
                    "invariant: id_raw is an 8-byte slice from bytes.get(range) of length 8",
                )),
                flags: u16::from_le_bytes(flags_raw.try_into().expect(
                    "invariant: flags_raw is a 2-byte slice from bytes.get(range) of length 2",
                )),
            });
            cursor += 11;
        }
        if decoded.len() == count && bytes.get(cursor) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
pub fn decode_body_bounds(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Body))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == format!("f3d:{}", entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(entity.byte_offset).ok() else {
            continue;
        };
        let end = entities
            .iter()
            .filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .filter_map(|candidate| usize::try_from(candidate.byte_offset).ok())
            .min()
            .unwrap_or(bytes.len());
        let Ok(record_index) = u32::try_from(entity.entity_suffix) else {
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
        let mut record_offsets = Vec::with_capacity(3);
        for wanted in record_indices {
            let matches = indexed_headers_in(bytes, start, end)
                .filter(|(_, record_index)| *record_index == wanted)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            let [offset] = matches.as_slice() else {
                record_offsets.clear();
                break;
            };
            record_offsets.push(*offset);
        }
        let [first, second, third] = record_offsets.as_slice() else {
            continue;
        };
        if !(first < second && second < third) {
            continue;
        }
        let third_end = next_indexed_record_offset(bytes, third.saturating_add(11))
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let intervals = [(*first, *second), (*second, *third), (*third, third_end)];
        let mut repeated = body_bound_candidates(bytes, intervals[0].0, intervals[0].1)
            .filter_map(|(marker_offset, values)| {
                let frame = bytes.get(marker_offset..marker_offset + 49)?;
                let mut value_offsets = [marker_offset + 1, 0, 0];
                for (ordinal, (record_start, record_end)) in
                    intervals.iter().copied().enumerate().skip(1)
                {
                    let matches = body_bound_candidates(bytes, record_start, record_end)
                        .filter(|(offset, _)| {
                            bytes.get(*offset..offset.saturating_add(49)) == Some(frame)
                        })
                        .map(|(offset, _)| offset + 1)
                        .collect::<Vec<_>>();
                    let [offset] = matches.as_slice() else {
                        return None;
                    };
                    value_offsets[ordinal] = *offset;
                }
                Some((values, value_offsets))
            })
            .collect::<Vec<_>>();
        repeated.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        let [(values, value_offsets)] = repeated.as_slice() else {
            continue;
        };
        out.push(DesignBodyBounds {
            id: format!(
                "f3d:{}:design-body-bounds#{}",
                entry.name, entity.byte_offset
            ),
            entity_suffix: entity.entity_suffix,
            entity_byte_offset: entity.byte_offset,
            record_indices,
            record_byte_offsets: [*first as u64, *second as u64, *third as u64],
            value_byte_offsets: value_offsets.map(|offset| offset as u64),
            body_binding_ids: Vec::new(),
            maximum: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
            minimum: Point3::new(values[3] * 10.0, values[4] * 10.0, values[5] * 10.0),
        });
    }
    out.sort_by_key(|bounds| bounds.id.clone());
    Ok(out)
}

fn indexed_headers_in(
    bytes: &[u8],
    mut position: usize,
    end: usize,
) -> impl Iterator<Item = (usize, u32)> + '_ {
    std::iter::from_fn(move || {
        while position + 11 <= end {
            let at = position;
            position += 1;
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
                continue;
            };
            if class_tag.len() == 3 && class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                let Some(record_index) = u32_at(bytes, after_tag) else {
                    continue;
                };
                return Some((at, record_index));
            }
        }
        None
    })
}

fn body_bound_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> impl Iterator<Item = (usize, [f64; 6])> + '_ {
    (start..end.saturating_sub(48)).filter_map(move |offset| {
        if bytes.get(offset) != Some(&1) {
            return None;
        }
        let values = [
            f64_at(bytes, offset + 1)?,
            f64_at(bytes, offset + 9)?,
            f64_at(bytes, offset + 17)?,
            f64_at(bytes, offset + 25)?,
            f64_at(bytes, offset + 33)?,
            f64_at(bytes, offset + 41)?,
        ];
        (values.iter().all(|value| value.is_finite())
            && (0..3).all(|axis| values[axis] >= values[axis + 3])
            && (0..3).any(|axis| values[axis] > values[axis + 3]))
        .then_some((offset, values))
    })
}

fn object_kind(name: &str) -> DesignObjectKind {
    match name {
        "Fusion" => DesignObjectKind::Fusion,
        "Body" => DesignObjectKind::Body,
        "Component" => DesignObjectKind::Component,
        "Geometry" => DesignObjectKind::Geometry,
        "MSketch" => DesignObjectKind::Sketch,
        "Dimension" => DesignObjectKind::Dimension,
        "Scene" => DesignObjectKind::Scene,
        "EntityTracking" => DesignObjectKind::EntityTracking,
        "CommonData" => DesignObjectKind::CommonData,
        _ => DesignObjectKind::Other(name.to_owned()),
    }
}

fn lp_ascii(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if length > 2_000 {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, offset)?;
    raw.iter()
        .all(u8::is_ascii_graphic)
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

fn lp_utf16(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if !(1..=256).contains(&length) {
        return None;
    }
    utf16le_at(bytes, offset.checked_add(4)?, length)
}

pub(crate) fn is_guid(value: &str) -> bool {
    matches!(value.len(), 36..=38)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn decode_stream(bytes: &[u8], stream: &str, out: &mut Vec<ConstructionRecipe>) {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
            let offset = cursor + relative;
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| u32_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let design_id_field = recipe_design_id(bytes, offset, name);
            let design_id = design_id_field.as_ref().map(|field| field.0.clone());
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index_offset = offset.checked_sub(16);
            let record_index = record_index_offset
                .and_then(|at| bytes.get(at..at + 4))
                .map(|raw| {
                    i32::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: bytes.get(at..at+4) is a 4-byte slice"),
                    )
                })
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                id: format!("f3d:{stream}:construction-recipe#{offset}"),
                byte_offset: offset as u64,
                record_index_offset: record_index_offset.map(|offset| offset as u64),
                kind,
                design_id,
                design_id_offset: design_id_field.as_ref().map(|field| field.1 as u64),
                design_id_binary_u32: design_id_field.is_some_and(|field| field.2),
                recipe_index,
                record_index,
            });
        }
    }
    out.sort_by_key(|recipe| recipe.record_index);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<(String, usize, bool)> {
    let pre = offset.checked_sub(27)?;
    if let Some((id, value_offset)) = ascii_id_at(bytes, pre) {
        return Some((id, value_offset, false));
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((
                String::from_utf8_lossy(candidate).into_owned(),
                offset - 23,
                false,
            ));
        }
    }
    if name == b"bounded_face_recipe_data" && offset >= 16 {
        let id = u32::from_le_bytes(bytes[offset - 16..offset - 12].try_into().ok()?);
        let zeros = bytes.get(offset - 12..offset - 4)?;
        if (100..100_000).contains(&id) && zeros.iter().all(|byte| *byte == 0) {
            return Some((id.to_string(), offset - 16, true));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8).map(|(id, value_offset)| (id, value_offset, false))
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32::from_le_bytes(
        bytes
            .get(length_offset..length_offset + 4)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value = bytes.get(length_offset + 4..length_offset + 4 + length)?;
    value.iter().all(u8::is_ascii_alphanumeric).then(|| {
        (
            String::from_utf8_lossy(value).into_owned(),
            length_offset + 4,
        )
    })
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
pub(crate) struct BodyBinding {
    /// Basename of the B-rep blob entry the ASM key resolves in.
    pub blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    pub blob_name_offset: usize,
    /// Number of pairs in the enclosing map.
    pub pair_count: u32,
    /// Zero-based position in the enclosing map.
    pub pair_ordinal: u32,
    /// The referenced ASM body key.
    pub asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within the stream.
    pub entity_suffix_offset: usize,
}

/// Parse every BREP body-map record in a Design `BulkStream`: a `u32` pair
/// count, `count` pairs of `(u64 asm_body_key, u64 entity_suffix)`, the
/// trailing record ref and pad, then the length-prefixed UTF-16 blob name
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
pub(crate) fn body_bindings(bytes: &[u8]) -> Vec<BodyBinding> {
    let needle: Vec<u8> = "BREP.".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = Vec::new();
    for offset in bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
    {
        let Some(name_chars) = offset
            .checked_sub(4)
            .and_then(|at| read_u32(bytes, at))
            .map(|chars| chars as usize)
        else {
            continue;
        };
        let Some(blob_name) = bytes
            .get(offset..offset + name_chars * 2)
            .map(utf16_le_string)
        else {
            continue;
        };
        // 16 bytes separate the pairs from the name: the 12-byte record tail
        // and the name's u32 length prefix.
        let Some(pairs_end) = offset.checked_sub(16) else {
            continue;
        };
        // The pair count precedes the pairs; scanning ascending is unambiguous
        // because the high halves of the little-endian ids are zero.
        for count in 1usize..=64 {
            let span = 16 * count;
            let Some(count_at) = pairs_end.checked_sub(span + 4) else {
                break;
            };
            if read_u32(bytes, count_at) != Some(count as u32) {
                continue;
            }
            for pair in 0..count {
                let at = count_at + 4 + pair * 16;
                if let (Some(key), Some(suffix)) = (read_u64(bytes, at), read_u64(bytes, at + 8)) {
                    out.push(BodyBinding {
                        blob_name: blob_name.clone(),
                        blob_name_offset: offset,
                        pair_count: count as u32,
                        pair_ordinal: pair as u32,
                        asm_key: key,
                        asm_key_offset: at,
                        entity_suffix: suffix,
                        entity_suffix_offset: at + 8,
                    });
                }
            }
            break;
        }
    }
    out
}

/// Decode every ordered Design BREP body-map pair and resolve pairs targeting
/// the selected BREP to solved ASM bodies.
pub fn decode_design_body_bindings(
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let active_basename = active_brep_entry.and_then(|entry| entry.rsplit('/').next());
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for binding in body_bindings(bytes) {
            let body = (active_basename == Some(binding.blob_name.as_str()))
                .then(|| {
                    let matches = body_keys
                        .iter()
                        .filter(|key| key.asm_body_key == Some(binding.asm_key))
                        .map(|key| key.body.clone())
                        .collect::<Vec<_>>();
                    let [body] = matches.as_slice() else {
                        return None;
                    };
                    Some(body.clone())
                })
                .flatten();
            out.push(DesignBodyBinding {
                id: format!(
                    "f3d:{}:design-body-binding#{}",
                    entry.name, binding.asm_key_offset
                ),
                stream: entry.name.clone(),
                pair_count: binding.pair_count,
                pair_ordinal: binding.pair_ordinal,
                asm_body_key: binding.asm_key,
                asm_body_key_offset: binding.asm_key_offset as u64,
                entity_suffix: binding.entity_suffix,
                entity_suffix_offset: binding.entity_suffix_offset as u64,
                blob_name: binding.blob_name,
                blob_name_offset: binding.blob_name_offset as u64,
                body,
            });
        }
    }
    out.sort_by_key(|binding| binding.id.clone());
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream.
pub fn bind_body_bounds(bounds: &mut [DesignBodyBounds], bindings: &[DesignBodyBinding]) {
    for bounds in bounds {
        let Some(stream) = native_stream(&bounds.id) else {
            continue;
        };
        let mut matches = bindings
            .iter()
            .filter(|binding| {
                stream == format!("f3d:{}", binding.stream)
                    && binding.entity_suffix == bounds.entity_suffix
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|binding| binding.asm_body_key_offset);
        bounds.body_binding_ids = matches
            .into_iter()
            .map(|binding| binding.id.clone())
            .collect();
    }
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// The BREP body-map record resolves ASM body keys of `active_brep_entry` to
/// design-entity suffixes, and each entity's browser-node record carries a
/// hidden flag directly after the node GUID
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
/// The result maps each ASM body key to its display visibility; bodies
/// without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub stream: String,
    pub byte_offset: u64,
    pub asm_body_key_offset: u64,
    pub entity_suffix: u64,
    pub visible: bool,
}

pub(crate) fn decode_body_visibility(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    active_brep_entry: &str,
) -> Result<HashMap<u64, DecodedBodyVisibility>, CodecError> {
    let Some(basename) = active_brep_entry
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
    else {
        return Ok(HashMap::new());
    };
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let hidden_by_entity = browser_node_hidden_flags(bytes);
        for binding in body_bindings(bytes) {
            if binding.blob_name != basename {
                continue;
            }
            if let Some(node) = hidden_by_entity.get(&binding.entity_suffix) {
                out.insert(
                    binding.asm_key,
                    DecodedBodyVisibility {
                        stream: entry.name.clone(),
                        byte_offset: node.byte_offset,
                        asm_body_key_offset: binding.asm_key_offset as u64,
                        entity_suffix: binding.entity_suffix,
                        visible: !node.hidden,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Scan for browser-node records: a length-prefixed 36-character UTF-16 GUID,
/// one hidden-flag byte, the `01 01` marker, and the `u64` design-entity
/// suffix.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn browser_node_hidden_flags(bytes: &[u8]) -> HashMap<u64, BrowserNodeVisibility> {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = HashMap::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if read_u32(bytes, at) != Some(GUID_CHARS as u32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (flag @ (0 | 1), Some(member)) = (bytes[flag_at], read_u64(bytes, flag_at + 3)) {
                out.insert(
                    member,
                    BrowserNodeVisibility {
                        byte_offset: flag_at as u64,
                        hidden: flag == 1,
                    },
                );
            }
        }
        at += 1;
    }
    out
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

#[cfg(test)]
mod relation_tests {
    use super::{
        assign_extrude_face_roles, bind_dimension_loci, bind_edge_operand_candidates,
        bind_extrude_selection_geometry, bind_extrude_selection_identities,
        bind_face_operand_candidates, bind_lost_edge_groups, bind_parameter_companion_payloads,
        bind_sketch_graph, body_bound_candidates, closed_sketch_profiles, companion_owned_interval,
        contiguous_i32_program, decode_fillet_radius_groups, design_parameter_prefix,
        directional_point_dimension, exact_atomic_constraint, exact_counted_dimension_relation,
        exact_counted_offset, exact_offset_constraint, expression_identifiers,
        find_dimension_locus_groups, find_dimension_locus_pair, identity_matrix,
        indexed_record_containing, indirect_angular_lines, neutral_dimension_constraint_id,
        neutral_feature_id_parts, neutral_parameter_id_parts, neutral_sketch_curve_id,
        neutral_sketch_id, neutral_sketch_point_id, next_indexed_record_offset,
        null_locus_dimension_definition, parse_construction_operand_group,
        parse_construction_operand_identity, parse_design_parameter, parse_dimension_locus_group,
        parse_dimension_locus_pair, parse_dimension_null_locus_pair, parse_edge_operand,
        parse_extrude_profile, parse_extrude_selection_group, parse_extrude_selection_member,
        parse_face_operand, parse_parameter_companion, parse_parameter_owner,
        parse_parameter_scope, parse_sketch_placement_candidates, parse_sketch_relation,
        point_on_sketch_entity, project_configurations, project_dimension_constraints,
        project_extrude, project_parameter_design, project_sketch_constraints,
        project_sketch_design, radial_dimension_definition, recipe_record_prefix,
        region_containing_points, remove_dimension_frame_relations, repeated_linear_dimension,
        resolved_edge_candidate_intersection, resolved_extrude_profile_selection,
        resolved_face_group, two_locus_distance_dimension, unresolved_configuration_rule_count,
    };
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, DesignConfiguration, DesignConfigurationKind,
        DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
        DesignConstructionPersistentIdentity, DesignDimensionLocusPair,
        DesignDimensionRecipeRecord, DesignEntityHeader, DesignExtrudeExtent,
        DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignExtrudeOperation,
        DesignExtrudeProfileOperand, DesignExtrudeStart, DesignObjectKind, DesignParameter,
        DesignParameterCompanion, DesignParameterKind, DesignParameterOwner, DesignParameterScope,
        DesignRecipeReference, DesignRecordHeader, DesignSketchPlacement, LostEdgeReference,
        PersistentSubentityTag, SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity,
        SketchPoint, SketchRelation, SketchRelationOperand,
    };
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::features::{
        Angle, FaceSelection, FeatureDefinition, Length, ParameterValue, SketchProfileRegion,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchAxis, SketchConstraintDefinition, SketchEntity, SketchEntityId,
        SketchEntityUse, SketchGeometry, SketchId,
    };
    use std::collections::{HashMap, HashSet};

    #[test]
    fn configuration_identity_is_stable_across_table_order_and_delimiter_names() {
        let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
            id: format!("f3d:configuration:entry#{entry_name}"),
            entry_name: entry_name.into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({"configurations": {variant_name: {}}}),
        };
        let first = table("asset/a#b.dsgcfg", "c");
        let second = table("asset/a.dsgcfg", "b#c");
        let first_id = first.id.clone();

        let forward = project_configurations(&[first.clone(), second.clone()]);
        let reversed = project_configurations(&[second, first]);
        let forward_ids = forward
            .iter()
            .map(|configuration| configuration.id.clone())
            .collect::<HashSet<_>>();
        let reversed_ids = reversed
            .iter()
            .map(|configuration| configuration.id.clone())
            .collect::<HashSet<_>>();

        assert_eq!(forward_ids, reversed_ids);
        assert_eq!(forward_ids.len(), 2);
        assert_ne!(forward[0].id, forward[1].id);
        assert_eq!(forward[0].native_ref.as_deref(), Some(first_id.as_str()));
    }

    #[test]
    fn configuration_rules_bind_only_one_named_variant() {
        let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
            id: format!("f3d:configuration:entry#{entry_name}"),
            entry_name: entry_name.into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({"configurations": {variant_name: {}}}),
        };
        let rule = DesignConfiguration {
            id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
            entry_name: "rule.dsgcfgrule".into(),
            kind: DesignConfigurationKind::Rule,
            payload: serde_json::json!({"when": "width > 20 mm", "activate": "wide"}),
        };
        let native = [table("table.dsgcfg", "wide"), rule.clone()];
        let projected = project_configurations(&native);
        assert_eq!(
            projected[0].properties["activation_rule:rule.dsgcfgrule"],
            "width > 20 mm"
        );
        assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

        let ambiguous = [
            table("first.dsgcfg", "wide"),
            table("second.dsgcfg", "wide"),
            rule,
        ];
        let projected = project_configurations(&ambiguous);
        assert!(projected
            .iter()
            .all(|configuration| configuration.properties.is_empty()));
        assert_eq!(
            unresolved_configuration_rule_count(&ambiguous, &projected),
            1
        );
    }

    #[test]
    fn feature_identity_uses_stream_family_and_native_ordinal() {
        let first = neutral_feature_id_parts("Design/A:B", "Kind:12", 3);
        let same = neutral_feature_id_parts("Design/A:B", "Kind:12", 3);
        let different_stream = neutral_feature_id_parts("Design/A", "B:Kind:12", 3);
        let different_family = neutral_feature_id_parts("Design/A:B", "Kind", 123);

        assert_eq!(first, same);
        assert_ne!(first, different_stream);
        assert_ne!(first, different_family);
    }

    #[test]
    fn parameter_identity_uses_stream_and_native_source_ordinal() {
        let first = neutral_parameter_id_parts("Design/A:12", 3);
        let same = neutral_parameter_id_parts("Design/A:12", 3);
        let different_stream = neutral_parameter_id_parts("Design/A", 123);
        let different_ordinal = neutral_parameter_id_parts("Design/A:12", 4);

        assert_eq!(first, same);
        assert_ne!(first, different_stream);
        assert_ne!(first, different_ordinal);
    }

    #[test]
    fn sketch_geometry_identity_uses_native_persistent_ids() {
        let point = neutral_sketch_point_id("f3d:Design/A:point#10", 42);
        let relocated_point = neutral_sketch_point_id("f3d:Design/A:point#999", 42);
        let curve = neutral_sketch_curve_id("f3d:Design/A:curve#10", 42, 0);
        let relocated_curve = neutral_sketch_curve_id("f3d:Design/A:curve#999", 42, 0);

        assert_eq!(point, relocated_point);
        assert_eq!(curve, relocated_curve);
        assert_ne!(point, curve);
        assert_ne!(
            curve,
            neutral_sketch_curve_id("f3d:Design/A:curve#10", 42, 1)
        );
        assert_ne!(point, neutral_sketch_point_id("f3d:Design", 42));
    }

    #[test]
    fn governing_dimension_identity_uses_parameter_identity() {
        let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#Design/A:12".into());
        let relocated = neutral_dimension_constraint_id(&parameter, "pair");
        let same = neutral_dimension_constraint_id(&parameter, "pair");
        let other_form = neutral_dimension_constraint_id(&parameter, "null-pair");
        let other_parameter = neutral_dimension_constraint_id(
            &cadmpeg_ir::features::ParameterId("parameter:Design/A".into()),
            "12:pair",
        );

        assert_eq!(relocated, same);
        assert_ne!(relocated, other_form);
        assert_ne!(relocated, other_parameter);
        assert_eq!(relocated.0.matches('#').count(), 1);
    }

    #[test]
    fn historical_points_on_profile_boundaries_are_ambiguous() {
        let sketch_id = SketchId("sketch".into());
        let entity_id = SketchEntityId("line".into());
        let mut sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: vec![vec![SketchEntityUse {
                entity: entity_id.clone(),
                reversed: false,
            }]],
            native_ref: None,
        };
        let entity = SketchEntity {
            id: entity_id,
            sketch: sketch_id,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 0.0),
            },
        };
        let point = Point3::new(11.0, 20.0, 9.0);
        assert_eq!(
            region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
            None
        );
        assert_eq!(
            super::selection_containing_points(
                &sketch,
                std::slice::from_ref(&entity),
                &[point],
                1.0e-6,
            ),
            Some(super::ResolvedProfileSelection::Loops(vec![0]))
        );

        sketch.profiles.push(sketch.profiles[0].clone());
        assert_eq!(
            region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
            None
        );
        assert_eq!(
            super::selection_containing_points(
                &sketch,
                std::slice::from_ref(&entity),
                &[point],
                1.0e-6,
            ),
            None
        );
    }

    #[test]
    fn historical_point_inside_unique_closed_line_profile_selects_region() {
        let sketch_id = SketchId("sketch".into());
        let mut entities = Vec::new();
        let mut profile = Vec::new();
        for (ordinal, (start, end)) in [
            (Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
            (Point2::new(4.0, 0.0), Point2::new(4.0, 3.0)),
            (Point2::new(4.0, 3.0), Point2::new(0.0, 3.0)),
            (Point2::new(0.0, 3.0), Point2::new(0.0, 0.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let id = SketchEntityId(format!("line-{ordinal}"));
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line { start, end },
            });
        }
        let circle_id = SketchEntityId("unrelated-circle".into());
        let profiles = vec![
            profile,
            vec![SketchEntityUse {
                entity: circle_id.clone(),
                reversed: false,
            }],
        ];
        entities.push(SketchEntity {
            id: circle_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(20.0, 20.0),
                radius: Length(1.0),
            },
        });
        let sketch = Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles,
            native_ref: None,
        };

        assert_eq!(
            region_containing_points(&sketch, &entities, &[Point3::new(12.0, 21.0, 12.0)], 1.0e-6,),
            Some(SketchProfileRegion {
                outer: 0,
                holes: Vec::new(),
            })
        );
        assert_eq!(
            region_containing_points(&sketch, &entities, &[Point3::new(15.0, 21.0, 12.0)], 1.0e-6,),
            None
        );
    }

    #[test]
    fn nested_line_profiles_resolve_atomic_regions_and_immediate_holes() {
        let sketch_id = SketchId("sketch".into());
        let mut entities = Vec::new();
        let mut profiles = Vec::new();
        for (profile_index, (minimum, maximum)) in [
            (Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)),
            (Point2::new(2.0, 2.0), Point2::new(8.0, 8.0)),
            (Point2::new(4.0, 4.0), Point2::new(6.0, 6.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let corners = [
                minimum,
                Point2::new(maximum.u, minimum.v),
                maximum,
                Point2::new(minimum.u, maximum.v),
            ];
            let mut profile = Vec::new();
            for edge_index in 0..corners.len() {
                let id = SketchEntityId(format!("line-{profile_index}-{edge_index}"));
                profile.push(SketchEntityUse {
                    entity: id.clone(),
                    reversed: false,
                });
                entities.push(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref: None,
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry: SketchGeometry::Line {
                        start: corners[edge_index],
                        end: corners[(edge_index + 1) % corners.len()],
                    },
                });
            }
            profiles.push(profile);
        }
        let sketch = Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles,
            native_ref: None,
        };

        assert_eq!(
            region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6,),
            Some(SketchProfileRegion {
                outer: 0,
                holes: vec![1],
            })
        );
        assert_eq!(
            region_containing_points(&sketch, &entities, &[Point3::new(3.0, 3.0, 0.0)], 1.0e-6,),
            Some(SketchProfileRegion {
                outer: 1,
                holes: vec![2],
            })
        );
        assert_eq!(
            region_containing_points(&sketch, &entities, &[Point3::new(5.0, 5.0, 0.0)], 1.0e-6,),
            Some(SketchProfileRegion {
                outer: 2,
                holes: Vec::new(),
            })
        );
    }

    #[test]
    fn historical_selection_preserves_first_member_region_order() {
        let region = |outer| SketchProfileRegion {
            outer,
            holes: Vec::new(),
        };
        assert_eq!(
            super::ordered_unique_profile_selections([
                Some(super::ResolvedProfileSelection::Regions(vec![region(3)])),
                Some(super::ResolvedProfileSelection::Regions(vec![region(1)])),
                Some(super::ResolvedProfileSelection::Regions(vec![region(3)])),
                Some(super::ResolvedProfileSelection::Regions(vec![region(2)])),
            ]),
            Some(super::ResolvedProfileSelection::Regions(vec![
                region(3),
                region(1),
                region(2),
            ]))
        );
        assert_eq!(
            super::ordered_unique_profile_selections([
                Some(super::ResolvedProfileSelection::Regions(vec![region(3)])),
                None,
            ]),
            None
        );
    }

    #[test]
    fn historical_edge_positions_require_a_complete_state_chain() {
        let mut topology = crate::history_records::AsmHistoricalTopology {
            edges: vec![7],
            vertices: vec![8, 9],
            points: vec![18, 19],
            edge_vertices: vec![crate::history_records::AsmHistoricalEdge {
                edge: 7,
                start_vertex: 8,
                end_vertex: 9,
            }],
            vertex_points: vec![
                crate::history_records::AsmHistoricalCarrierBinding {
                    entity: 8,
                    carrier: 18,
                },
                crate::history_records::AsmHistoricalCarrierBinding {
                    entity: 9,
                    carrier: 19,
                },
            ],
            point_positions: vec![
                crate::history_records::AsmHistoricalPoint {
                    point: 18,
                    position: Point3::new(1.0, 2.0, 3.0),
                },
                crate::history_records::AsmHistoricalPoint {
                    point: 19,
                    position: Point3::new(4.0, 5.0, 6.0),
                },
            ],
            ..crate::history_records::AsmHistoricalTopology::default()
        };
        assert_eq!(
            super::historical_entity_positions(
                crate::records::AsmHistoricalEntityKind::Edge,
                7,
                &topology,
            ),
            Some(vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0),])
        );
        topology.point_positions.pop();
        assert_eq!(
            super::historical_entity_positions(
                crate::records::AsmHistoricalEntityKind::Edge,
                7,
                &topology,
            ),
            None
        );
    }

    #[test]
    fn historical_face_points_require_complete_boundary_topology() {
        let mut topology = crate::history_records::AsmHistoricalTopology {
            faces: vec![10],
            loops: vec![11],
            coedges: vec![12, 13, 14],
            edges: vec![20, 21, 22],
            vertices: vec![30, 31, 32],
            points: vec![40, 41, 42],
            face_loops: vec![crate::history_records::AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![11],
            }],
            loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![12, 13, 14],
            }],
            coedge_topology: vec![
                crate::history_records::AsmHistoricalCoedge {
                    coedge: 12,
                    owner_loop: 11,
                    edge: 20,
                    next: 13,
                    previous: 14,
                    radial_next: 12,
                },
                crate::history_records::AsmHistoricalCoedge {
                    coedge: 13,
                    owner_loop: 11,
                    edge: 21,
                    next: 14,
                    previous: 12,
                    radial_next: 13,
                },
                crate::history_records::AsmHistoricalCoedge {
                    coedge: 14,
                    owner_loop: 11,
                    edge: 22,
                    next: 12,
                    previous: 13,
                    radial_next: 14,
                },
            ],
            edge_vertices: vec![
                crate::history_records::AsmHistoricalEdge {
                    edge: 20,
                    start_vertex: 30,
                    end_vertex: 31,
                },
                crate::history_records::AsmHistoricalEdge {
                    edge: 21,
                    start_vertex: 31,
                    end_vertex: 32,
                },
                crate::history_records::AsmHistoricalEdge {
                    edge: 22,
                    start_vertex: 32,
                    end_vertex: 30,
                },
            ],
            vertex_points: vec![
                crate::history_records::AsmHistoricalCarrierBinding {
                    entity: 30,
                    carrier: 40,
                },
                crate::history_records::AsmHistoricalCarrierBinding {
                    entity: 31,
                    carrier: 41,
                },
                crate::history_records::AsmHistoricalCarrierBinding {
                    entity: 32,
                    carrier: 42,
                },
            ],
            point_positions: vec![
                crate::history_records::AsmHistoricalPoint {
                    point: 40,
                    position: Point3::new(0.0, 0.0, 0.0),
                },
                crate::history_records::AsmHistoricalPoint {
                    point: 41,
                    position: Point3::new(2.0, 0.0, 0.0),
                },
                crate::history_records::AsmHistoricalPoint {
                    point: 42,
                    position: Point3::new(0.0, 1.0, 0.0),
                },
            ],
            ..crate::history_records::AsmHistoricalTopology::default()
        };
        assert_eq!(
            super::historical_face_points(10, &topology),
            Some(vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ])
        );

        topology.point_positions.pop();
        assert_eq!(super::historical_face_points(10, &topology), None);
    }

    #[test]
    fn historical_point_membership_respects_conic_domains_and_nurbs_endpoints() {
        let sketch = SketchId("sketch".into());
        let entity = |geometry| SketchEntity {
            id: SketchEntityId("curve".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let arc = entity(SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        });
        assert!(point_on_sketch_entity(Point2::new(0.0, 2.0), &arc, 1.0e-6));
        assert!(!point_on_sketch_entity(
            Point2::new(-2.0, 0.0),
            &arc,
            1.0e-6
        ));

        let ellipse = entity(SketchGeometry::Ellipse {
            center: Point2::new(1.0, -1.0),
            major_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
            major_radius: Length(4.0),
            minor_radius: Length(2.0),
            start_angle: Some(cadmpeg_ir::features::Angle(0.0)),
            end_angle: Some(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)),
        });
        assert!(point_on_sketch_entity(
            Point2::new(-1.0, -1.0),
            &ellipse,
            1.0e-6
        ));
        assert!(!point_on_sketch_entity(
            Point2::new(3.0, -1.0),
            &ellipse,
            1.0e-6
        ));
        assert!(!point_on_sketch_entity(
            Point2::new(-1.0, -0.9),
            &ellipse,
            1.0e-6
        ));

        let nurbs = entity(SketchGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(1.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(3.0, 2.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        });
        assert!(point_on_sketch_entity(
            Point2::new(3.0, 2.0),
            &nurbs,
            1.0e-6
        ));
        assert!(!point_on_sketch_entity(
            Point2::new(2.0, 4.0),
            &nurbs,
            1.0e-6
        ));
        let SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } = &nurbs.geometry
        else {
            unreachable!()
        };
        let interior = cadmpeg_ir::eval::nurbs_pcurve_uv(
            *degree,
            knots,
            control_points,
            weights.as_deref(),
            0.375,
        )
        .unwrap();
        assert!(point_on_sketch_entity(interior, &nurbs, 1.0e-9));
    }

    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn parameter_record(
        owner: Option<u32>,
        expression: &str,
        source_kind: &str,
        unit: Option<&str>,
        name: &str,
        evaluated_value: f64,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"305");
        out.extend_from_slice(&71u32.to_le_bytes());
        out.extend_from_slice(&[0; 11]);
        out.extend_from_slice(&design_parameter_prefix(source_kind).to_le_bytes());
        out.push(0);
        out.extend_from_slice(&9u32.to_le_bytes());
        match owner {
            Some(owner) => {
                out.push(1);
                out.extend_from_slice(&owner.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
            None => out.push(0),
        }
        lp_utf16(&mut out, expression);
        out.extend_from_slice(if owner.is_some() {
            &[0; 9]
        } else {
            &[0, 0, 0, 0, 0, 0, 0, 0, 1]
        });
        lp_utf16(&mut out, source_kind);
        out.extend_from_slice(&0u32.to_le_bytes());
        if let Some(unit) = unit {
            lp_utf16(&mut out, unit);
        }
        lp_utf16(&mut out, name);
        out.extend_from_slice(&evaluated_value.to_le_bytes());
        out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    #[test]
    fn body_bound_candidate_has_one_marker_and_six_ordered_f64_values() {
        let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
        let mut bytes = vec![1];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let candidates = body_bound_candidates(&bytes, 0, bytes.len()).collect::<Vec<_>>();
        assert_eq!(candidates, [(0, values)]);

        bytes[0] = 0;
        assert!(body_bound_candidates(&bytes, 0, bytes.len())
            .next()
            .is_none());
    }

    #[test]
    fn parameter_variants_have_exact_string_and_scalar_boundaries() {
        let user = parse_design_parameter(&parameter_record(
            None,
            "60 mm",
            "User Parameter",
            Some("mm"),
            "Width",
            6.0,
        ))
        .unwrap();
        assert_eq!(user.kind, DesignParameterKind::User);
        assert_eq!(user.owner_record_index, None);
        assert_eq!(user.unit.as_deref(), Some("mm"));
        assert_eq!(user.evaluated_value, 6.0);

        let feature = parse_design_parameter(&parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        ))
        .unwrap();
        assert_eq!(feature.kind, DesignParameterKind::Feature);
        assert_eq!(feature.owner_record_index, Some(44));
        assert_eq!(feature.expression, "Width / 2");

        let boolean = parse_design_parameter(&parameter_record(
            None,
            "1",
            "User Parameter",
            None,
            "OnOff",
            1.0,
        ))
        .unwrap();
        assert_eq!(boolean.unit, None);
        assert_eq!(boolean.name, "OnOff");

        let mut tangency =
            parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
        tangency[22..30].copy_from_slice(&6u64.to_le_bytes());
        let tangency = parse_design_parameter(&tangency).expect("prefixed unitless parameter");
        assert_eq!(tangency.prefix_value, 6);
        assert_eq!(tangency.unit, None);
        assert_eq!(tangency.name, "d81");
        assert_eq!(tangency.evaluated_value, 1.0);

        let mut invalid_tangency =
            parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
        invalid_tangency[22..30].copy_from_slice(&0u64.to_le_bytes());
        assert!(parse_design_parameter(&invalid_tangency).is_none());

        let mut invalid_distance = parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        );
        invalid_distance[22..30].copy_from_slice(&6u64.to_le_bytes());
        assert!(parse_design_parameter(&invalid_distance).is_none());
    }

    #[test]
    fn parameter_record_rejects_noncanonical_tail() {
        let mut record = parameter_record(
            Some(44),
            "45 deg",
            "TaperAngle",
            Some("deg"),
            "d13",
            std::f64::consts::FRAC_PI_4,
        );
        *record.last_mut().unwrap() = 1;
        assert!(parse_design_parameter(&record).is_none());
    }

    fn parameter_owner_frame() -> Vec<u8> {
        let mut frame = vec![0; 104];
        frame[0..4].copy_from_slice(&3u32.to_le_bytes());
        frame[4..7].copy_from_slice(b"292");
        frame[7..11].copy_from_slice(&44u32.to_le_bytes());
        frame[19] = 1;
        frame[20..24].copy_from_slice(&1u32.to_le_bytes());
        frame[24] = 1;
        frame[25..29].copy_from_slice(&12u32.to_le_bytes());
        frame[35..39].copy_from_slice(&2u32.to_le_bytes());
        frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
        frame[48] = 1;
        frame[49..53].copy_from_slice(&45u32.to_le_bytes());
        frame[59..63].copy_from_slice(&9u32.to_le_bytes());
        frame[67] = 1;
        frame[68..72].copy_from_slice(&12u32.to_le_bytes());
        frame[78] = 1;
        frame[79] = 1;
        frame[81] = 1;
        frame[82..86].copy_from_slice(&46u32.to_le_bytes());
        frame[93] = 1;
        frame[94..98].copy_from_slice(&12u32.to_le_bytes());
        frame
    }

    #[test]
    fn parameter_owner_frame_has_repeated_scope_and_both_record_orders() {
        let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
        assert_eq!(parsed.record_index, 44);
        assert_eq!(parsed.scope_record_index, 12);
        assert_eq!(parsed.local_ordinal, 2);
        assert_eq!(parsed.evaluated_value, 6.0);
        assert_eq!(parsed.parameter_record_index, 45);
        assert_eq!(parsed.owned_ordinal, 9);
        assert_eq!(parsed.variant, 1);
        assert_eq!(parsed.companion_record_index, 46);

        let mut parameter_first = parameter_owner_frame();
        parameter_first[49..53].copy_from_slice(&43u32.to_le_bytes());
        parameter_first[82..86].copy_from_slice(&45u32.to_le_bytes());
        let parsed = parse_parameter_owner(&parameter_first).expect("parameter-first owner frame");
        assert_eq!(parsed.parameter_record_index, 43);
        assert_eq!(parsed.record_index, 44);
        assert_eq!(parsed.companion_record_index, 45);

        let mut malformed = parameter_owner_frame();
        malformed[94..98].copy_from_slice(&13u32.to_le_bytes());
        assert!(parse_parameter_owner(&malformed).is_none());
    }

    #[test]
    fn parameter_companion_prefix_has_owner_backlink_and_timestamp() {
        let mut prefix = vec![0; 58];
        prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
        prefix[4..7].copy_from_slice(b"408");
        prefix[7..11].copy_from_slice(&46u32.to_le_bytes());
        prefix[31] = 1;
        prefix[32..36].copy_from_slice(&44u32.to_le_bytes());
        prefix[42..50].copy_from_slice(&1_678_000_000_000_000u64.to_le_bytes());

        let parsed = parse_parameter_companion(&prefix).unwrap();
        assert_eq!(parsed.record_index, 46);
        assert_eq!(parsed.owner_record_index, 44);
        assert_eq!(parsed.timestamp_micros, 1_678_000_000_000_000);
        assert_eq!(parsed.timestamp_micros_offset, 42);

        prefix[32..36].copy_from_slice(&45u32.to_le_bytes());
        assert_eq!(
            parse_parameter_companion(&prefix)
                .unwrap()
                .owner_record_index,
            45
        );
        prefix[42..50].fill(0);
        assert!(parse_parameter_companion(&prefix).is_none());
    }

    #[test]
    fn dimension_recipe_uses_its_immediate_indexed_record_boundary() {
        let mut bytes = vec![0xaa; 5];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"415");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 17]);
        let recipe_offset = bytes.len();
        bytes.extend_from_slice(b"edge_recipe_data");
        bytes.extend_from_slice(&[0; 13]);
        let next_offset = bytes.len();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"423");
        bytes.extend_from_slice(&41u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 9]);

        assert_eq!(
            indexed_record_containing(&bytes, 5, bytes.len(), recipe_offset),
            Some((5, "415".into(), 40, next_offset))
        );
        assert_eq!(
            indexed_record_containing(&bytes, 5, bytes.len(), next_offset + 11),
            Some((next_offset, "423".into(), 41, bytes.len()))
        );
        assert_eq!(indexed_record_containing(&bytes, 6, bytes.len(), 7), None);
        assert_eq!(
            contiguous_i32_program(&[u8::MAX; 8], 0, 8),
            Some(vec![-1, -1])
        );
        assert_eq!(contiguous_i32_program(&[0; 7], 0, 7), None);

        let mut framed = vec![0; 11];
        framed.extend_from_slice(&[7, 8, 9]);
        framed.extend_from_slice(&16u32.to_le_bytes());
        let family_name_offset = framed.len();
        framed.extend_from_slice(b"edge_recipe_data");
        assert_eq!(
            recipe_record_prefix(&framed, 0, family_name_offset, 16),
            Some((11, vec![7, 8, 9]))
        );
        framed[14..18].copy_from_slice(&15u32.to_le_bytes());
        assert_eq!(
            recipe_record_prefix(&framed, 0, family_name_offset, 16),
            None
        );
    }

    #[test]
    fn dimension_recipe_decodes_ordered_persistent_reference_entries() {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&3u32.to_le_bytes());
        prefix.extend_from_slice(&4u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&2u32.to_le_bytes());
        let first_token_at = prefix.len();
        prefix.extend_from_slice(b"13");
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        let first_reference_at = prefix.len();
        prefix.extend_from_slice(&331u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());

        prefix.extend_from_slice(&2u32.to_le_bytes());
        let second_token_at = prefix.len();
        prefix.extend_from_slice(&[b'9', 0, 0, 0]);
        prefix.push(0);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        let second_reference_at = prefix.len();
        prefix.extend_from_slice(&303u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());

        let references = super::decode_recipe_references(&prefix, 1_000);
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].selector, 1);
        assert_eq!(references[0].selector_offset, 1_022);
        assert_eq!(references[0].token, "13");
        assert_eq!(references[0].token_offset, 1_000 + first_token_at as u64);
        assert_eq!(references[0].design_reference, 331);
        assert_eq!(
            references[0].design_reference_offset,
            1_000 + first_reference_at as u64
        );
        assert_eq!(references[1].selector, 2);
        assert_eq!(references[1].selector_offset, 1_048);
        assert_eq!(references[1].token, "9");
        assert_eq!(references[1].token_offset, 1_000 + second_token_at as u64);
        assert_eq!(references[1].design_reference, 303);
        assert_eq!(
            references[1].design_reference_offset,
            1_000 + second_reference_at as u64
        );
        assert_eq!(
            super::recipe_reference_candidate_faces(
                &references[0],
                &[
                    PersistentSubentityTag {
                        id: "matching".into(),
                        target: AttributeTarget::Face(FaceId("face-b".into())),
                        selector: 1,
                        token: "13".into(),
                        design_references: vec![331],
                        ordinal: 0,
                    },
                    PersistentSubentityTag {
                        id: "other".into(),
                        target: AttributeTarget::Face(FaceId("face-a".into())),
                        selector: 1,
                        token: "13".into(),
                        design_references: vec![999],
                        ordinal: 0,
                    },
                    PersistentSubentityTag {
                        id: "wrong-selector".into(),
                        target: AttributeTarget::Face(FaceId("face-c".into())),
                        selector: 2,
                        token: "13".into(),
                        design_references: vec![331],
                        ordinal: 0,
                    },
                    PersistentSubentityTag {
                        id: "matching-edge".into(),
                        target: AttributeTarget::Edge(EdgeId("edge-b".into())),
                        selector: 1,
                        token: "13".into(),
                        design_references: vec![331],
                        ordinal: 0,
                    },
                ],
            ),
            [FaceId("face-b".into())]
        );
        assert_eq!(
            super::recipe_reference_candidate_edges(
                &references[0],
                &[PersistentSubentityTag {
                    id: "matching-edge".into(),
                    target: AttributeTarget::Edge(EdgeId("edge-b".into())),
                    selector: 1,
                    token: "13".into(),
                    design_references: vec![331],
                    ordinal: 0,
                }],
            ),
            [EdgeId("edge-b".into())]
        );
        assert_eq!(
            super::recipe_reference_alternate_selector_faces(
                &references[0],
                &[PersistentSubentityTag {
                    id: "alternate-face".into(),
                    target: AttributeTarget::Face(FaceId("face-c".into())),
                    selector: 2,
                    token: "13".into(),
                    design_references: vec![331],
                    ordinal: 0,
                }],
            ),
            [FaceId("face-c".into())]
        );
        assert_eq!(
            super::recipe_reference_alternate_selector_edges(
                &references[0],
                &[PersistentSubentityTag {
                    id: "alternate-edge".into(),
                    target: AttributeTarget::Edge(EdgeId("edge-c".into())),
                    selector: 2,
                    token: "13".into(),
                    design_references: vec![331],
                    ordinal: 0,
                }],
            ),
            [EdgeId("edge-c".into())]
        );
    }

    #[test]
    fn dimension_locus_pair_resolves_two_typed_geometry_records() {
        let mut bytes = vec![0; 80];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"277");
        bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
        bytes[24] = 1;
        bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
        bytes[39] = 1;
        bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
        bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
        bytes[54] = 1;
        bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
        bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"273");
        bytes.extend_from_slice(&233u32.to_le_bytes());

        let pair = parse_dimension_locus_pair(&bytes, 0, 228, &HashSet::from([192, 194]))
            .expect("paired dimension locus frame");
        assert_eq!(pair.companion_record_index, 228);
        assert_eq!(pair.record_index, 233);
        assert_eq!(pair.frame_length, 80);
        assert_eq!(pair.first_geometry_record_index, 192);
        assert_eq!(pair.first_role, 0);
        assert_eq!(pair.second_geometry_record_index, 194);
        assert_eq!(pair.second_role, 1);
        assert_eq!(pair.paired_class_tag, "273");

        let mut nested = Vec::new();
        nested.extend_from_slice(&3u32.to_le_bytes());
        nested.extend_from_slice(b"341");
        nested.extend_from_slice(&229u32.to_le_bytes());
        nested.extend_from_slice(&bytes);
        let nested_end = nested.len();
        let nested =
            find_dimension_locus_pair(&nested, 0, nested_end, 228, &HashSet::from([192, 194]))
                .expect("nested paired dimension locus frame");
        assert_eq!(nested.byte_offset, 11);
        assert_eq!(nested.paired_byte_offset, 91);
    }

    #[test]
    fn dimension_null_locus_pair_preserves_null_and_typed_roles() {
        let mut bytes = vec![0; 74];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"277");
        bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
        bytes[24] = 1;
        bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
        bytes[39] = 1;
        bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
        bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"273");
        bytes.extend_from_slice(&1394u32.to_le_bytes());

        let pair = parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1109]))
            .expect("null-locus dimension frame");
        assert_eq!(pair.companion_record_index, 1290);
        assert_eq!(pair.record_index, 1394);
        assert_eq!(pair.frame_length, 74);
        assert_eq!(pair.null_role, 10);
        assert_eq!(pair.geometry_record_index, 1109);
        assert_eq!(pair.geometry_role, 7);
        assert_eq!(pair.paired_class_tag, "273");

        assert!(
            parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1110]),).is_none()
        );

        let mut axis_pair = pair;
        axis_pair.null_role = 14;
        axis_pair.geometry_role = 3;
        let entity = SketchEntity {
            id: SketchEntityId("f3d:model:sketch-entity#line".into()),
            sketch: SketchId("f3d:model:sketch#axis-angle".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 1.0),
            },
        };
        let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#angle".into());
        assert!(matches!(
            null_locus_dimension_definition(
                &axis_pair,
                &entity,
                "Angular Dimension-2",
                0.5,
                parameter.clone(),
            ),
            Some(SketchConstraintDefinition::AngleToAxis {
                entity: ref actual_entity,
                axis: SketchAxis::Horizontal,
                parameter: ref actual_parameter,
            }) if actual_entity == &entity.id && actual_parameter == &parameter
        ));
        axis_pair.null_role = 13;
        assert!(null_locus_dimension_definition(
            &axis_pair,
            &entity,
            "Angular Dimension-2",
            0.5,
            parameter,
        )
        .is_none());
    }

    #[test]
    fn radial_dimensions_require_one_exact_circular_measurement() {
        let mut entity = SketchEntity {
            id: SketchEntityId("f3d:model:sketch-entity#circle".into()),
            sketch: SketchId("f3d:model:sketch#radial".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(2.0, 3.0),
                radius: Length(5.0),
            },
        };
        let radius_parameter = cadmpeg_ir::features::ParameterId("parameter#radius".into());
        assert!(matches!(
            radial_dimension_definition(
                &entity,
                "Radius Dimension-2",
                0.5,
                radius_parameter.clone(),
            ),
            Some(SketchConstraintDefinition::Radius { entity: ref actual, parameter: ref p })
                if actual == &entity.id && p == &radius_parameter
        ));
        let diameter_parameter = cadmpeg_ir::features::ParameterId("parameter#diameter".into());
        assert!(matches!(
            radial_dimension_definition(
                &entity,
                "Diameter Dimension-2",
                1.0,
                diameter_parameter.clone(),
            ),
            Some(SketchConstraintDefinition::Diameter { entity: ref actual, parameter: ref p })
                if actual == &entity.id && p == &diameter_parameter
        ));
        assert!(radial_dimension_definition(
            &entity,
            "Diameter Dimension-2",
            0.5,
            diameter_parameter.clone(),
        )
        .is_none());

        entity.geometry = SketchGeometry::Arc {
            center: Point2::new(2.0, 3.0),
            radius: Length(5.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(1.0),
        };
        assert!(radial_dimension_definition(
            &entity,
            "Diameter Dimension",
            1.0,
            diameter_parameter,
        )
        .is_some());
        entity.geometry = SketchGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            major_angle: cadmpeg_ir::features::Angle(0.0),
            major_radius: Length(5.0),
            minor_radius: Length(3.0),
            start_angle: None,
            end_angle: None,
        };
        assert!(
            radial_dimension_definition(&entity, "Radius Dimension-2", 0.5, radius_parameter,)
                .is_none()
        );
    }

    #[test]
    fn dimension_locus_group_preserves_roles_owner_state_and_return_order() {
        let mut bytes = vec![0; 101];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"286");
        bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
        bytes[24] = 1;
        bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
        bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
        bytes[39] = 1;
        bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
        bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
        bytes[55] = 1;
        bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
        bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
        bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
        bytes[78] = 1;
        bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
        bytes[89] = 1;
        bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"314");
        bytes.extend_from_slice(&250u32.to_le_bytes());

        let group = parse_dimension_locus_group(
            &bytes,
            0,
            240,
            &HashSet::from([175, 217]),
            &HashSet::from([172]),
        )
        .expect("counted dimension locus frame");
        assert_eq!(group.companion_record_index, 240);
        assert_eq!(group.record_index, 249);
        assert_eq!(group.frame_length, 101);
        assert_eq!(group.owner_reference, 172);
        assert_eq!(group.owner_role, 1);
        assert_eq!(group.state, 0);
        assert_eq!(group.loci[0].geometry_record_index, 175);
        assert_eq!(group.loci[0].role, 2);
        assert_eq!(group.loci[1].geometry_record_index, 217);
        assert_eq!(group.loci[1].role, 1);
        assert_eq!(group.return_members, [217, 175]);
        assert_eq!(group.next_class_tag, "314");
        assert_eq!(group.next_record_index, 250);

        let relation_at = |stream: &str, byte_offset| SketchRelation {
            id: format!("f3d:{stream}:sketch-relation#{byte_offset}"),
            record_index: 249,
            class_tag: "286".into(),
            byte_offset,
            state_offset: 66,
            owner_reference: 172,
            owner_entity_id: "0_172".into(),
            auxiliary_references: Vec::new(),
            auxiliary_reference_offsets: Vec::new(),
            members: vec![175, 217],
            resolved_members: Vec::new(),
            member_offsets: vec![25, 40],
            owner_reference_offset: 56,
            state: 0,
            constraint_kinds: vec![SketchConstraintKind::Coincident],
            unknown_constraint_bits: 0,
            return_members: vec![217, 175],
            resolved_return_members: Vec::new(),
            return_member_offsets: vec![79, 90],
            raw_bytes: bytes[..101].to_vec(),
        };
        let mut relations = vec![relation_at("native", 0), relation_at("other", 0)];
        let mut group = group;
        group.id = "f3d:native:design-dimension-locus-group#0".into();
        remove_dimension_frame_relations(&mut relations, &[], &[group], &[]);
        assert_eq!(relations.len(), 1);
        assert!(relations[0].id.starts_with("f3d:other:"));

        let body = bytes[11..101].to_vec();
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"315");
        bytes.extend_from_slice(&251u32.to_le_bytes());
        let groups = find_dimension_locus_groups(
            &bytes,
            0,
            bytes.len(),
            240,
            &HashSet::from([175, 217]),
            &HashSet::from([172]),
        );
        assert_eq!(
            groups
                .iter()
                .map(|group| group.record_index)
                .collect::<Vec<_>>(),
            [249, 250]
        );
    }

    #[test]
    fn parameter_scope_uses_same_index_pair_and_fixed_kind_tail() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"301");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        let reference_count_at = bytes.len();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        let reference_at = bytes.len();
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Sketch");
        let feature_ordinal_at = bytes.len();
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        let paired_at = bytes.len();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "301".into(),
            byte_offset: 0,
        };

        let mut scope = parse_parameter_scope(&bytes, &header).unwrap();
        assert_eq!(scope.kind, "Sketch");
        assert_eq!(scope.feature_ordinal, 1);
        assert_eq!(scope.feature_ordinal_offset, feature_ordinal_at as u64);
        assert_eq!(scope.history_state_id, Some(7));
        assert_eq!(scope.previous_history_state_id, Some(2));
        assert_eq!(scope.reference_count_offset, reference_count_at as u64);
        assert_eq!(scope.reference_members, [55]);
        assert_eq!(scope.reference_member_offsets, [reference_at as u64]);
        assert_eq!(scope.frame_length, paired_at as u64);
        assert_eq!(scope.paired_class_tag, "261");
        assert_eq!(scope.paired_byte_offset, paired_at as u64);

        let mut companion = DesignParameterCompanion {
            id: "f3d:native:parameter-companion#11".into(),
            byte_offset: 0,
            class_tag: "300".into(),
            record_index: 11,
            owner_record_index: 10,
            timestamp_micros: 1,
            timestamp_micros_offset: 42,
            payload_byte_offset: 58,
            payload_byte_length: 0,
            owned_recipe_ids: Vec::new(),
        };
        scope.id = "f3d:native:parameter-scope#12".into();
        scope.byte_offset = 58;
        assert_eq!(
            companion_owned_interval(
                &companion,
                std::iter::empty(),
                &[],
                &[scope.clone()],
                &[],
                100,
            ),
            Some((58, 58))
        );
        scope.byte_offset = 80;
        assert_eq!(
            companion_owned_interval(
                &companion,
                std::iter::empty(),
                &[],
                &[scope.clone()],
                &[],
                100,
            ),
            Some((58, 80))
        );
        scope.byte_offset = 90;
        let foreign_header = DesignRecordHeader {
            id: "f3d:native:record-header#55".into(),
            record_index: 55,
            class_tag: "301".into(),
            byte_offset: 70,
        };
        assert_eq!(
            companion_owned_interval(
                &companion,
                std::iter::empty(),
                &[],
                &[scope],
                &[foreign_header],
                100,
            ),
            Some((58, 70))
        );

        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            "1",
            "User Parameter",
            None,
            "p",
            1.0,
        ))
        .expect("generated parameter");
        parameter.id = "f3d:native:design-parameter#65".into();
        parameter.byte_offset = 65;
        assert_eq!(
            companion_owned_interval(&companion, std::iter::once(&parameter), &[], &[], &[], 100,),
            Some((58, 65))
        );
        let recipe = ConstructionRecipe {
            id: "f3d:native:construction-recipe#60".into(),
            byte_offset: 60,
            record_index_offset: None,
            kind: ConstructionRecipeKind::Edge,
            design_id: None,
            design_id_offset: None,
            design_id_binary_u32: false,
            recipe_index: 0,
            record_index: 303,
        };
        bind_parameter_companion_payloads(
            std::slice::from_mut(&mut companion),
            std::slice::from_ref(&parameter),
            &[],
            &[],
            &[],
            std::slice::from_ref(&recipe),
            &HashMap::from([("f3d:native".into(), 100)]),
        );
        assert_eq!(companion.payload_byte_offset, 58);
        assert_eq!(companion.payload_byte_length, 7);
        assert_eq!(companion.owned_recipe_ids, [recipe.id]);
    }

    #[test]
    fn extrude_scope_discriminators_follow_optional_indexed_reference() {
        let scope = |operation: u32,
                     extent: (u32, u32),
                     direction_reversed: bool,
                     start: u8,
                     conditional_reference: bool| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(b"301");
            bytes.extend_from_slice(&12u32.to_le_bytes());
            bytes.resize(100, 0);
            bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
            let operation_offset = if conditional_reference {
                bytes[25] = 1;
                bytes[26..30].copy_from_slice(&77u32.to_le_bytes());
                38
            } else {
                28
            };
            bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
            bytes[operation_offset + 4..operation_offset + 8]
                .copy_from_slice(&extent.0.to_le_bytes());
            bytes[operation_offset + 8..operation_offset + 12]
                .copy_from_slice(&extent.1.to_le_bytes());
            bytes[operation_offset + 12] = u8::from(direction_reversed);
            bytes[operation_offset + 13] = 1;
            bytes[operation_offset + 14] = start;
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.push(1);
            bytes.extend_from_slice(&55u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&7u32.to_le_bytes());
            lp_utf16(&mut bytes, "Extrude");
            let mut tail = [0; 78];
            tail[0..4].copy_from_slice(&1u32.to_le_bytes());
            tail[31..35].copy_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&tail);
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(b"261");
            bytes.extend_from_slice(&12u32.to_le_bytes());
            let header = DesignRecordHeader {
                id: "generated:scope-header#0".into(),
                record_index: 12,
                class_tag: "301".into(),
                byte_offset: 0,
            };
            parse_parameter_scope(&bytes, &header).unwrap()
        };

        let direct = scope(1, (1, 2), false, 0, false);
        assert_eq!(direct.extrude_operation, Some(DesignExtrudeOperation::Join));
        assert_eq!(direct.extrude_operation_offset, Some(28));
        assert_eq!(
            direct.extrude_extent,
            Some(DesignExtrudeExtent::OneSidedDistance)
        );
        assert_eq!(direct.extrude_extent_offsets, Some([32, 36]));
        assert_eq!(direct.extrude_direction_reversed, Some(false));
        assert_eq!(direct.extrude_direction_reversed_offset, Some(40));
        assert_eq!(direct.extrude_start, Some(DesignExtrudeStart::ProfilePlane));
        assert_eq!(direct.extrude_start_offset, Some(42));
        let shifted = scope(3, (2, 0), false, 1, true);
        assert_eq!(
            shifted.extrude_operation,
            Some(DesignExtrudeOperation::Intersect)
        );
        assert_eq!(shifted.extrude_operation_offset, Some(38));
        assert_eq!(
            shifted.extrude_extent,
            Some(DesignExtrudeExtent::TwoSidedDistance)
        );
        assert_eq!(shifted.extrude_extent_offsets, Some([42, 46]));
        assert_eq!(
            shifted.extrude_start,
            Some(DesignExtrudeStart::OffsetProfilePlane)
        );
        assert_eq!(shifted.extrude_start_offset, Some(52));
        let to_face = scope(2, (1, 1), true, 2, false);
        assert_eq!(
            to_face.extrude_extent,
            Some(DesignExtrudeExtent::OneSidedToFace)
        );
        assert_eq!(to_face.extrude_direction_reversed, Some(true));
        assert_eq!(to_face.extrude_start, Some(DesignExtrudeStart::FromFace));
    }

    #[test]
    fn extrude_profile_resolves_its_decimal_sketch_entity_suffix() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"308");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&103u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
        lp_utf16(&mut bytes, "172");
        bytes.extend_from_slice(&[0; 94]);
        let paired_at = bytes.len();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: "308".into(),
            record_index: 100,
        };
        let entity = DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity#172".into(),
            byte_offset: 1000,
            entity_suffix: 172,
            entity_id: "0_172".into(),
            class_tag: "269".into(),
            optional_slot_present: false,
            object_kind: Some(DesignObjectKind::Sketch),
            record_reference: Some(200),
            record_reference_offset: Some(1010),
            declared_reference_count: Some(0),
            reference_indices: Vec::new(),
            reference_offsets: Vec::new(),
        };

        let profile =
            parse_extrude_profile(&bytes, "f3d:Design/BulkStream.dat", 4, &header, &[entity])
                .expect("Extrude sketch-profile operand");
        assert_eq!(profile.scope_reference_ordinal, 4);
        assert_eq!(profile.entity_suffix, 172);
        assert_eq!(profile.entity_id, "0_172");
        assert_eq!(profile.paired_byte_offset, paired_at as u64);
    }

    #[test]
    fn extrude_operand_group_has_an_exact_counted_frame() {
        fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&class_tag);
            bytes.extend_from_slice(&record_index.to_le_bytes());
        }

        let scope = DesignParameterScope {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 1000,
            class_tag: "301".into(),
            record_index: 12,
            frame_length: 200,
            kind: "Extrude".into(),
            kind_offset: 1100,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 1080,
            reference_members: vec![100, 200, 201],
            reference_member_offsets: vec![1085, 1096, 1107],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 1200,
        };
        let record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: "332".into(),
            record_index: 100,
        };
        let mut bytes = Vec::new();
        header(&mut bytes, *b"332", 100);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for member in [200u32, 201] {
            bytes.push(1);
            bytes.extend_from_slice(&member.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&300u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&180u32.to_le_bytes());
        bytes.extend_from_slice(&0.125f64.to_le_bytes());
        bytes.extend_from_slice(&180u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&102u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&[1, 1, 0, 1]);
        bytes.extend_from_slice(&101u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 7]);
        bytes.push(1);
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        let paired_at = bytes.len();
        header(&mut bytes, *b"259", 100);

        let group = parse_construction_operand_group(&bytes, &scope, 0, &record)
            .expect("counted Extrude operand group");
        assert_eq!(group.member_count_offset, 21);
        assert_eq!(group.members, [200, 201]);
        assert_eq!(group.member_offsets, [26, 37]);
        assert_eq!(group.identity_record_index, 300);
        assert_eq!(group.role, 0x0000_0008_0000_0000);
        assert_eq!(group.extrude_role, Some(DesignExtrudeOperandRole::Bodies));
        assert_eq!(group.opaque_index, 180);
        assert_eq!(group.opaque_scalar, 0.125);
        assert!(group.variant);
        assert_eq!(group.paired_byte_offset, paired_at as u64);
    }

    #[test]
    fn extrude_operand_identity_walks_shared_wrapper_grammar_to_a_fixed_leaf() {
        fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&class_tag);
            bytes.extend_from_slice(&record_index.to_le_bytes());
        }

        let group = DesignConstructionOperandGroup {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1000,
            class_tag: "332".into(),
            member_count_offset: 1021,
            members: vec![200],
            lost_edge_references: Vec::new(),
            member_offsets: vec![1026],
            identity_record_index: 300,
            identity_record_offset: 1043,
            role: 0x0000_0008_0000_0000,
            extrude_role: Some(DesignExtrudeOperandRole::Bodies),
            extrude_face_role: None,
            role_offset: 1053,
            opaque_index: 180,
            opaque_index_offset: 1071,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1075,
            variant: false,
            paired_class_tag: "259".into(),
            paired_byte_offset: 1124,
        };
        let wrapper_header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#300".into(),
            byte_offset: 0,
            class_tag: "326".into(),
            record_index: 300,
        };
        let mut bytes = Vec::new();
        header(&mut bytes, *b"326", 300);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&[1, 1, 0]);
        header(&mut bytes, *b"326", 305);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&[1, 1, 0]);
        header(&mut bytes, *b"324", 400);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&586u64.to_le_bytes());
        lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
        lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 5]);
        header(&mut bytes, *b"301", 900);

        let identity = parse_construction_operand_identity(&bytes, &group, &wrapper_header)
            .expect("identity chain");
        assert_eq!(identity.wrapper_record_indices, [300, 305]);
        assert_eq!(identity.wrapper_byte_offsets, [0, 24]);
        assert_eq!(identity.following_record_index, 400);
        assert_eq!(identity.following_byte_offset, 48);
        let persistent = identity
            .persistent_identity
            .as_ref()
            .expect("fixed persistent identity leaf");
        assert_eq!(persistent.local_id, 586);
        assert_eq!(persistent.next_record_index, 900);
        assert_eq!(persistent.next_byte_offset, 238);

        let mut bound_group = group;
        let mut terminating_identity = identity;
        terminating_identity.id =
            "f3d:Design/BulkStream.dat:design-construction-operand-identity#200".into();
        terminating_identity.wrapper_byte_offsets[0] = 200;
        bind_lost_edge_groups(
            std::slice::from_mut(&mut bound_group),
            std::slice::from_ref(&terminating_identity),
            &[LostEdgeReference {
                id: "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
                record_byte_offset: 152,
                class_tag_offset: 156,
                class_tag: "419".into(),
                record_index: 299,
                record_index_offset: 159,
                byte_offset: 181,
                next_byte_offset: 200,
                next_class_tag: "326".into(),
                next_record_index: 300,
            }],
        )
        .expect("lost-edge run terminates at the group identity");
        assert_eq!(
            bound_group.lost_edge_references,
            ["f3d:Design/BulkStream.dat:lost-edge-reference#152"]
        );
    }

    #[test]
    fn extrude_selection_group_and_members_have_exact_counted_frames() {
        fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&class_tag);
            bytes.extend_from_slice(&record_index.to_le_bytes());
        }

        let scope = DesignParameterScope {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 1000,
            class_tag: "301".into(),
            record_index: 12,
            frame_length: 200,
            kind: "Extrude".into(),
            kind_offset: 1100,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 1080,
            reference_members: vec![100],
            reference_member_offsets: vec![1085],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 1200,
        };
        let record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: "331".into(),
            record_index: 100,
        };
        let mut group_bytes = Vec::new();
        header(&mut group_bytes, *b"331", 100);
        group_bytes.extend_from_slice(&[0; 10]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&2u32.to_le_bytes());
        for member in [200u32, 201] {
            group_bytes.push(1);
            group_bytes.extend_from_slice(&member.to_le_bytes());
            group_bytes.extend_from_slice(&[0; 6]);
        }
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.extend_from_slice(&0.25f64.to_le_bytes());
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.push(1);
        group_bytes.extend_from_slice(&102u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&[1, 1, 0, 1]);
        group_bytes.extend_from_slice(&101u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 7]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        let paired_at = group_bytes.len();
        header(&mut group_bytes, *b"259", 100);

        let mut group = parse_extrude_selection_group(&group_bytes, &scope, 0, &record)
            .expect("counted Extrude selection group");
        assert_eq!(group.members, [200, 201]);
        assert_eq!(group.opaque_index, 180);
        assert_eq!(group.opaque_scalar, 0.25);
        assert!(group.variant);
        assert_eq!(group.paired_byte_offset, paired_at as u64);

        let member_record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#200".into(),
            byte_offset: 0,
            class_tag: "290".into(),
            record_index: 200,
        };
        let mut member_bytes = Vec::new();
        header(&mut member_bytes, *b"290", 200);
        member_bytes.extend_from_slice(&[0; 10]);
        member_bytes.extend_from_slice(&586u64.to_le_bytes());
        lp_utf16(&mut member_bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
        lp_utf16(&mut member_bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
        member_bytes.extend_from_slice(&2u32.to_le_bytes());
        member_bytes.extend_from_slice(&[0; 5]);
        header(&mut member_bytes, *b"290", 201);

        let mut member = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
            .expect("fixed Extrude selection member");
        assert_eq!(member.local_id, 586);
        assert_eq!(member.next_byte_offset, 190);
        assert_eq!(member.next_record_index, 201);

        group.id = "f3d:Design/BulkStream.dat:selection-group#100".into();
        member.id = "f3d:Design/BulkStream.dat:selection-member#200".into();
        let identity = DesignConstructionOperandIdentity {
            id: "f3d:Design/BulkStream.dat:operand-identity#50".into(),
            group_record_index: 50,
            wrapper_record_indices: vec![150],
            wrapper_byte_offsets: vec![50],
            wrapper_class_tags: vec!["289".into()],
            following_record_index: 200,
            following_byte_offset: 0,
            following_class_tag: "290".into(),
            persistent_identity: Some(DesignConstructionPersistentIdentity {
                local_id: 586,
                local_id_offset: 21,
                asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
                asset_id_offset: 33,
                context_id: "0b2382d1-caaf-4eb9-b40d-a6322a7ed829".into(),
                context_id_offset: 113,
                next_record_index: 201,
                next_byte_offset: 190,
            }),
        };
        bind_extrude_selection_identities(
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&identity),
        );
        assert_eq!(member.operand_identity_ids, [identity.id]);
        let mut owning_scope = scope;
        owning_scope.extrude_profile = Some(DesignExtrudeProfileOperand {
            scope_reference_ordinal: 1,
            record_index: 300,
            byte_offset: 3000,
            class_tag: "308".into(),
            asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
            asset_id_offset: 3040,
            entity_id: "0_172".into(),
            entity_suffix: 172,
            entity_reference_offset: 3120,
            paired_class_tag: "259".into(),
            paired_byte_offset: 3200,
        });
        let curve = SketchCurveIdentity {
            id: "f3d:Design/BulkStream.dat:sketch-curve#400".into(),
            record_index: 400,
            owner_reference: Some(172),
            class_tag: "270".into(),
            byte_offset: 4000,
            geometry_offset: 100,
            entity_genesis: None,
            primary_id: 586,
            secondary_id: 0,
            geometry: None,
        };
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[],
            &[curve],
        );
        assert!(matches!(
            member.resolved_geometry,
            Some(SketchRelationOperand::Curve {
                record_index: 400,
                primary_id: 586,
                secondary_id: 0,
            })
        ));

        group.members.truncate(1);
        let sketch_id = SketchId("f3d:model:sketch#172".into());
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: vec![vec![SketchEntityUse {
                entity: neutral_sketch_curve_id(&member.id, 586, 0),
                reversed: false,
            }]],
            native_ref: None,
        };
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                super::ExtrudeProfileResolution {
                    entities: &[],
                    histories: &[],
                    linear_tolerance: 1.0e-6,
                },
                None,
                None,
            ),
            cadmpeg_ir::features::ProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            } if actual_sketch == &sketch_id && profiles == &[0]
        ));
        member.resolved_geometry = None;
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                super::ExtrudeProfileResolution {
                    entities: &[],
                    histories: &[],
                    linear_tolerance: 1.0e-6,
                },
                None,
                None,
            ),
            cadmpeg_ir::features::ProfileRef::SketchSelection {
                sketch: ref actual_sketch,
                selections: ref actual_selections,
            } if actual_sketch == &sketch_id && actual_selections == &[group.id.clone()]
        ));
    }

    #[test]
    fn topology_operands_follow_consecutive_nested_records_to_their_recipes() {
        fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> u64 {
            let offset = u64::try_from(bytes.len()).expect("generated frame length fits u64");
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&class_tag);
            bytes.extend_from_slice(&record_index.to_le_bytes());
            offset
        }

        let mut bytes = Vec::new();
        header(&mut bytes, *b"306", 100);
        let paired_at = header(&mut bytes, *b"259", 100);
        header(&mut bytes, *b"408", 101);
        header(&mut bytes, *b"414", 102);
        let recipe_record_at = header(&mut bytes, *b"423", 103);
        let recipe_name_at = bytes.len() + 4;
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"edge_recipe_data");
        for value in [-1i32, -1, 2, 0, -1, 1, -1, 7] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let next_at = header(&mut bytes, *b"306", 104);
        let scope = DesignParameterScope {
            id: "f3d:Design/BulkStream.dat:scope#1".into(),
            byte_offset: 1000,
            class_tag: "301".into(),
            record_index: 1,
            frame_length: 200,
            kind: "Fillet".into(),
            kind_offset: 1100,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 1080,
            reference_members: vec![100],
            reference_member_offsets: vec![1085],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 1200,
        };
        let record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: "306".into(),
            record_index: 100,
        };
        let recipe = ConstructionRecipe {
            id: "f3d:Design/BulkStream.dat:construction-recipe#60".into(),
            byte_offset: recipe_name_at as u64,
            record_index_offset: Some(recipe_record_at + 8),
            kind: ConstructionRecipeKind::Edge,
            design_id: None,
            design_id_offset: None,
            design_id_binary_u32: false,
            recipe_index: 7,
            record_index: 303,
        };

        let mut edge_operand =
            parse_edge_operand(&bytes, &scope, 0, &record, std::slice::from_ref(&recipe))
                .expect("edge recipe operand");
        assert_eq!(edge_operand.record_index, 100);
        assert_eq!(edge_operand.paired_byte_offset, paired_at);
        assert_eq!(edge_operand.recipe_record_index, 103);
        assert_eq!(edge_operand.recipe_record_byte_offset, recipe_record_at);
        assert_eq!(edge_operand.recipe_id, recipe.id);
        assert_eq!(edge_operand.resolved_edge_slot, None);
        edge_operand.resolved_edge_slot = Some(17);
        assert_eq!(super::resolved_edge_operand(&edge_operand), Some(17));
        edge_operand.resolved_edge_slot = None;
        assert_eq!(
            edge_operand.recipe_program_offset,
            recipe_name_at as u64 + 16
        );
        assert_eq!(edge_operand.recipe_program, [-1, -1, 2, 0, -1, 1, -1, 7]);
        assert!(edge_operand.recipe_structure.is_none());
        let structured = super::edge_recipe_structure(&[
            -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 1, 1, 5, 4, 4, 4, 4, 3, 4,
            -1, 3, 0, -1, 1, -1, 3, -1, 0, 1, 2, 5, 3, 3, 3, 1, 1, 1, -1,
        ])
        .expect("standard two-side recipe structure");
        assert_eq!(structured.root, 2);
        assert_eq!(structured.sides[0].field_count.get(), 3);
        assert_eq!(structured.sides[0].header_value, 0);
        assert_eq!(structured.sides[0].first, 2);
        assert_eq!(structured.sides[0].second, 1);
        assert_eq!(structured.sides[0].third, None);
        assert_eq!(structured.sides[0].payload_entry_count, 1);
        assert_eq!(structured.sides[0].entries[0].selector, 1);
        assert_eq!(structured.sides[0].entries[0].boundary_edge_count.get(), 5);
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[0]
                .outer
                .get(),
            4
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[0].middle,
            4
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[0].vertex_ordinal,
            3
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[0].incident_edge_ordinal,
            3
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[0].incident_side,
            crate::records::DesignTopologyIncidentSide::Following
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[1]
                .outer
                .get(),
            4
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[1].middle,
            3
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[1].incident_edge_ordinal,
            2
        );
        assert_eq!(
            structured.sides[0].entries[0].topology_triplets[1].incident_side,
            crate::records::DesignTopologyIncidentSide::Preceding
        );
        assert_eq!(structured.sides[1].field_count.get(), 3);
        assert_eq!(structured.sides[1].header_value, 0);
        assert_eq!(structured.sides[1].third, None);
        assert_eq!(structured.sides[1].payload_entry_count, 1);
        assert_eq!(structured.sides[1].entries[0].selector, 2);
        assert_eq!(structured.sides[1].entries[0].boundary_edge_count.get(), 5);
        assert_eq!(
            structured.sides[1].entries[0].topology_triplets[0]
                .outer
                .get(),
            3
        );
        assert_eq!(
            structured.sides[1].entries[0].topology_triplets[0].middle,
            3
        );
        assert_eq!(
            structured.sides[1].entries[0].topology_triplets[1]
                .outer
                .get(),
            1
        );
        assert_eq!(
            structured.sides[1].entries[0].topology_triplets[1].middle,
            1
        );
        let wrap = super::edge_recipe_entries(&[1, 5, 1, 0, 1, 1, 1, 1]).unwrap();
        assert_eq!(wrap[0].topology_triplets[0].vertex_ordinal, 0);
        assert_eq!(wrap[0].topology_triplets[0].incident_edge_ordinal, 4);
        assert_eq!(wrap[0].common_incident_edge_ordinal, None);
        assert_eq!(
            wrap[0].topology_triplets[0].incident_side,
            crate::records::DesignTopologyIncidentSide::Preceding
        );
        let common = super::edge_recipe_entries(&[1, 5, 1, 1, 1, 1, 1, 1]).unwrap();
        assert_eq!(common[0].common_incident_edge_ordinal, Some(0));
        assert!(super::edge_recipe_entries(&[3, 5, 1, 1, 1, 2, 1, 2]).is_none());
        assert!(super::edge_recipe_entries(&[1, 5, 6, 5, 6, 2, 1, 2]).is_none());
        assert!(
            super::edge_recipe_entries(&[1, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,])
                .is_none()
        );
        assert!(
            super::edge_recipe_entries(&[2, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,])
                .is_none()
        );
        let extended = super::edge_recipe_structure(&[
            -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 2, -1, 1, -1, 0, -1, 0, 0, -1, 4, 3, -1, 0, -1, 1,
            -1, 4, -1, 0, 0, -1,
        ])
        .expect("recipe structure with a third scalar on its second side");
        assert_eq!(extended.sides[0].third, None);
        assert_eq!(extended.sides[1].third, Some(4));
        assert_eq!(extended.sides[1].field_count.get(), 4);
        assert!(extended.sides[0].entries.is_empty());
        assert!(extended.sides[1].entries.is_empty());
        let face = super::face_recipe_structure(&[
            0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0, 0, -1,
        ])
        .expect("face node topology recipe structure");
        assert_eq!(face.root, 0);
        assert_eq!(face.prelude, [1, 2]);
        assert_eq!(face.sides[0].field_count.get(), 3);
        assert_eq!(face.sides[0].header_value, 0);
        assert_eq!(face.sides[0].first, 2);
        assert_eq!(face.sides[0].second, 1);
        assert_eq!(face.sides[1].field_count.get(), 3);
        assert_eq!(face.sides[1].header_value, 0);
        assert_eq!(face.sides[1].first, 1);
        assert_eq!(face.sides[1].second, 3);
        assert_eq!(
            super::face_recipe_local_topology_references(&face, 3)
                .expect("bounded face-node topology references")
                .iter()
                .map(|ordinal| ordinal.get())
                .collect::<Vec<_>>(),
            [1, 2, 1, 1, 3]
        );
        assert!(super::face_recipe_local_topology_references(&face, 2).is_none());
        assert_eq!(edge_operand.next_record_index, 104);
        assert_eq!(edge_operand.next_byte_offset, next_at);
        bind_edge_operand_candidates(
            std::slice::from_mut(&mut edge_operand),
            std::slice::from_ref(&recipe),
            &[PersistentSubentityTag {
                id: "f3d:asm:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            }],
        );
        assert_eq!(
            edge_operand.candidate_faces,
            [FaceId("f3d:brep:entity#50".into())]
        );
        let mut embedded_program = vec![99];
        embedded_program.extend_from_slice(&edge_operand.recipe_program[7..]);
        embedded_program.push(88);
        let dimension_recipe = DesignDimensionRecipeRecord {
            id: "dimension-recipe".into(),
            companion_record_index: 1,
            recipe_ordinal: 0,
            recipe_id: "recipe".into(),
            byte_offset: 0,
            class_tag: "423".into(),
            record_index: 1,
            frame_length: 4,
            prefix_offset: 0,
            prefix_bytes: vec![1],
            references: Vec::new(),
            program_offset: 0,
            program: embedded_program,
            matching_edge_operand_ids: Vec::new(),
        };
        assert_eq!(
            super::dimension_recipe_matching_edge_operand_ids(
                &dimension_recipe,
                std::slice::from_ref(&edge_operand),
            ),
            [edge_operand.id.clone()]
        );

        let mut face_bytes = Vec::new();
        header(&mut face_bytes, *b"306", 100);
        let face_paired_at = header(&mut face_bytes, *b"259", 100);
        header(&mut face_bytes, *b"408", 101);
        header(&mut face_bytes, *b"414", 102);
        let face_recipe_record_at = header(&mut face_bytes, *b"423", 103);
        let face_recipe_name_at = face_bytes.len() + 4;
        face_bytes.extend_from_slice(&24u32.to_le_bytes());
        face_bytes.extend_from_slice(b"bounded_face_recipe_data");
        for value in [0i32, -1, 1, -1, -1, 2, 7] {
            face_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let face_next_at = header(&mut face_bytes, *b"306", 104);
        let mut face_scope = scope;
        face_scope.kind = "Extrude".into();
        let mut face_recipe = recipe;
        face_recipe.kind = ConstructionRecipeKind::BoundedFace;
        face_recipe.design_id = Some("303".into());
        face_recipe.byte_offset = face_recipe_name_at as u64;
        face_recipe.record_index_offset = Some(face_recipe_record_at + 8);
        let mut operand = parse_face_operand(
            &face_bytes,
            &face_scope,
            0,
            &record,
            std::slice::from_ref(&face_recipe),
        )
        .expect("face recipe operand");
        assert_eq!(operand.record_index, 100);
        assert_eq!(operand.paired_byte_offset, face_paired_at);
        assert_eq!(operand.recipe_record_index, 103);
        assert_eq!(operand.recipe_kind, ConstructionRecipeKind::BoundedFace);
        assert_eq!(operand.recipe_id, face_recipe.id);
        assert_eq!(operand.resolved_face_slot, None);
        assert_eq!(
            operand.recipe_program_offset,
            face_recipe_name_at as u64 + 24
        );
        assert_eq!(operand.recipe_program, [0, -1, 1, -1, -1, 2, 7]);
        assert_eq!(
            operand.recipe_node_offsets,
            [face_recipe_name_at as u64 + 36]
        );
        assert_eq!(operand.recipe_nodes.len(), 1);
        assert_eq!(
            operand.recipe_nodes[0].byte_offset,
            face_recipe_name_at as u64 + 36
        );
        assert_eq!(operand.recipe_nodes[0].end_byte_offset, face_next_at);
        assert_eq!(operand.recipe_nodes[0].program, [-1, -1, 2, 7]);
        assert_eq!(operand.next_record_index, 104);
        assert_eq!(operand.next_byte_offset, face_next_at);
        operand.recipe_references.push(DesignRecipeReference {
            selector: 1,
            selector_offset: 1_101,
            token: "3".into(),
            token_offset: 1,
            design_reference: 303,
            design_reference_offset: 2,
            candidate_faces: Vec::new(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        });
        bind_face_operand_candidates(
            std::slice::from_mut(&mut operand),
            std::slice::from_ref(&face_recipe),
            &[
                PersistentSubentityTag {
                    id: "f3d:asm:persistent-subentity-tag#1".into(),
                    target: AttributeTarget::Face(FaceId("f3d:brep:entity#50".into())),
                    selector: 1,
                    token: "3".into(),
                    design_references: vec![303],
                    ordinal: 0,
                },
                PersistentSubentityTag {
                    id: "f3d:asm:persistent-subentity-tag#2".into(),
                    target: AttributeTarget::Face(FaceId("f3d:brep:entity#51".into())),
                    selector: 1,
                    token: "4".into(),
                    design_references: vec![303],
                    ordinal: 1,
                },
            ],
        );
        assert_eq!(
            operand.candidate_faces,
            [
                FaceId("f3d:brep:entity#50".into()),
                FaceId("f3d:brep:entity#51".into())
            ]
        );
        assert_eq!(
            operand.unreferenced_candidate_faces,
            [FaceId("f3d:brep:entity#51".into())]
        );
        let group = DesignConstructionOperandGroup {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: face_scope.record_index,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: "306".into(),
            member_count_offset: 920,
            members: vec![operand.record_index],
            lost_edge_references: Vec::new(),
            member_offsets: vec![924],
            identity_record_index: 91,
            identity_record_offset: 935,
            role: 0x0000_0011_0000_0000,
            extrude_role: Some(DesignExtrudeOperandRole::Faces),
            extrude_face_role: Some(DesignExtrudeFaceRole::Termination),
            role_offset: 946,
            opaque_index: 1,
            opaque_index_offset: 954,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 958,
            variant: false,
            paired_class_tag: "259".into(),
            paired_byte_offset: 980,
        };
        assert!(matches!(
            resolved_face_group(&group, std::slice::from_ref(&operand)),
            Some(FaceSelection::Resolved { faces, native })
                if faces == [FaceId("f3d:brep:entity#51".into())] && native == group.id
        ));
        operand
            .unreferenced_candidate_faces
            .push(FaceId("f3d:brep:entity#50".into()));
        assert!(resolved_face_group(&group, std::slice::from_ref(&operand)).is_none());
        operand.preceding_candidate_faces = vec![FaceId("f3d:brep:entity#50".into())];
        assert_eq!(
            super::resolve_face_operand_history_candidates(&operand),
            Some(50)
        );
        operand.resolved_face_slot = Some(50);
        assert!(matches!(
            resolved_face_group(&group, std::slice::from_ref(&operand)),
            Some(FaceSelection::Resolved { faces, native })
                if faces == [FaceId("f3d:brep:entity#50".into())] && native == group.id
        ));
        operand.resolved_face_slot = None;
        assert!(super::retain_face_operand_resolution(
            &group,
            std::slice::from_mut(&mut operand),
            &FaceId("f3d:brep:entity#50".into()),
        ));
        assert_eq!(operand.resolved_face_slot, Some(50));
        operand.resolved_face_slot = None;
        operand.alternate_selector_candidate_faces = vec![
            FaceId("f3d:brep:entity#50".into()),
            FaceId("f3d:brep:entity#51".into()),
        ];
        assert!(matches!(
            resolved_face_group(&group, std::slice::from_ref(&operand)),
            Some(FaceSelection::Resolved { faces, native })
                if faces == operand.alternate_selector_candidate_faces && native == group.id
        ));
        operand.alternate_selector_candidate_faces.clear();
        operand.resolved_face_slot = Some(50);
        let mut ambiguous = [operand.clone(), operand];
        assert!(!super::retain_face_operand_resolution(
            &group,
            &mut ambiguous,
            &FaceId("f3d:brep:entity#50".into()),
        ));
    }

    #[test]
    fn selected_face_start_requires_unique_sketch_plane_coincidence() {
        use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
        use cadmpeg_ir::topology::{Face, Sense};

        let sketch = Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 2.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: Vec::new(),
            native_ref: None,
        };
        let face = |id: &str, surface: &str| Face {
            id: FaceId(id.into()),
            shell: ShellId("shell".into()),
            surface: SurfaceId(surface.into()),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        };
        let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
            id: SurfaceId(id.into()),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        };
        let faces = [
            face("coincident", "surface-coincident"),
            face("offset", "surface-offset"),
            face("tilted", "surface-tilted"),
        ];
        let surfaces = [
            plane(
                "surface-coincident",
                Point3::new(5.0, -3.0, 2.0),
                Vector3::new(0.0, 0.0, -2.0),
            ),
            plane(
                "surface-offset",
                Point3::new(0.0, 0.0, 2.1),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            plane(
                "surface-tilted",
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];

        assert!(super::face_coincident_with_sketch(
            &faces[0].id,
            &sketch,
            &faces,
            &surfaces,
            1.0e-6,
            1.0e-10,
        ));
        for candidate in &faces[1..] {
            assert!(!super::face_coincident_with_sketch(
                &candidate.id,
                &sketch,
                &faces,
                &surfaces,
                1.0e-6,
                1.0e-10,
            ));
        }
    }

    #[test]
    fn sketch_placement_decodes_compact_identity_and_explicit_affine_frame() {
        fn placement_frame(
            record_index: u32,
            length: usize,
            transform: Option<[[f64; 4]; 4]>,
        ) -> Vec<u8> {
            let mut bytes = vec![0; length];
            bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
            bytes[4..7].copy_from_slice(b"356");
            bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
            if let Some(transform) = transform {
                for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                    let at = 55 + ordinal * 8;
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(b"259");
            bytes.extend_from_slice(&record_index.to_le_bytes());
            bytes
        }

        let compact = parse_sketch_placement_candidates(
            &placement_frame(185, 201, None),
            177,
            "0_172",
            172,
            185,
        );
        assert_eq!(compact.len(), 1);
        assert_eq!(compact[0].frame_length, 201);
        assert_eq!(compact[0].transform, identity_matrix());
        assert_eq!(compact[0].transform_offset, None);

        let transform = [
            [0.0, 0.0, 1.0, 12.0],
            [1.0, 0.0, 0.0, 34.0],
            [0.0, 1.0, 0.0, 56.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let explicit = parse_sketch_placement_candidates(
            &placement_frame(1773, 329, Some(transform)),
            1765,
            "0_1761",
            1761,
            1773,
        );
        assert_eq!(explicit.len(), 1);
        assert_eq!(explicit[0].frame_length, 329);
        assert_eq!(explicit[0].transform, transform);
        assert_eq!(explicit[0].transform_offset, Some(55));
    }

    #[test]
    fn unbranched_closed_sketch_components_project_as_ordered_profiles() {
        let sketch = SketchId("f3d:model:sketch#profile".into());
        let line = |id: &str, start: Point2, end: Point2| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let entities = vec![
            line("line-a", Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)),
            line("line-b", Point2::new(2.0, 2.0), Point2::new(2.0, 0.0)),
            line("line-c", Point2::new(2.0, 2.0), Point2::new(0.0, 2.0)),
            line(
                "line-d",
                Point2::new(0.0, 2.0 + 5.0e-7),
                Point2::new(0.0, 0.0),
            ),
            line("open-line", Point2::new(10.0, 0.0), Point2::new(11.0, 0.0)),
            SketchEntity {
                id: SketchEntityId("circle".into()),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center: Point2::new(20.0, 20.0),
                    radius: Length(3.0),
                },
            },
        ];

        let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].len(), 1);
        assert_eq!(profiles[0][0].entity, SketchEntityId("circle".into()));
        assert_eq!(
            profiles[1]
                .iter()
                .map(|entity_use| (entity_use.entity.0.as_str(), entity_use.reversed))
                .collect::<Vec<_>>(),
            [
                ("line-a", false),
                ("line-b", true),
                ("line-c", false),
                ("line-d", false),
            ]
        );
    }

    #[test]
    fn branched_line_graph_projects_each_bounded_face() {
        let sketch = SketchId("f3d:model:sketch#branched-profile".into());
        let line = |id: &str, start: (f64, f64), end: (f64, f64)| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            },
        };
        let entities = vec![
            line("bottom-left", (0.0, 0.0), (1.0, 0.0)),
            line("bottom-right", (1.0, 0.0), (2.0, 0.0)),
            line("right", (2.0, 0.0), (2.0, 1.0)),
            line("top-right", (2.0, 1.0), (1.0, 1.0)),
            line("top-left", (1.0, 1.0), (0.0, 1.0)),
            line("left", (0.0, 1.0), (0.0, 0.0)),
            line("divider", (1.0, 0.0), (1.0, 1.0)),
        ];

        let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
        assert_eq!(profiles.len(), 2);
        assert!(profiles.iter().all(|profile| profile.len() == 4));
        assert!(profiles.iter().all(|profile| profile
            .iter()
            .any(|entity_use| entity_use.entity.0 == "divider")));
    }

    #[test]
    fn placed_sketch_projects_point_and_line_in_local_coordinates() {
        let placement = DesignSketchPlacement {
            id: "f3d:native:placement#0".into(),
            scope_record_index: 177,
            entity_id: "0_172".into(),
            entity_suffix: 172,
            byte_offset: 100,
            class_tag: "356".into(),
            record_index: 185,
            frame_length: 329,
            transform: [
                [0.0, 0.0, 1.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 1.0, 0.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            transform_offset: Some(155),
            paired_class_tag: "259".into(),
            paired_byte_offset: 429,
        };
        let point = SketchPoint {
            id: "f3d:native:point#175".into(),
            record_index: 175,
            owner_reference: Some(172),
            class_tag: "300".into(),
            byte_offset: 400,
            coordinate_offset: 89,
            entity_genesis: None,
            persistent_id: 10,
            paired_reference: 0,
            coordinates: Point2::new(2.5, 4.0),
            raw_bytes: Vec::new(),
        };
        let line = SketchCurveIdentity {
            id: "f3d:native:curve#217".into(),
            record_index: 217,
            owner_reference: Some(172),
            class_tag: "301".into(),
            byte_offset: 500,
            geometry_offset: 100,
            entity_genesis: None,
            primary_id: 20,
            secondary_id: 0,
            geometry: Some(SketchCurveGeometry::Line {
                start: Point3::new(1.0, 2.0, 0.0),
                end: Point3::new(4.0, 6.0, 0.0),
                direction: Vector3::new(0.6, 0.8, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            }),
        };

        let placements = vec![placement];
        let points = vec![point];
        let curves = vec![line];
        let (sketches, entities) = project_sketch_design(&placements, &points, &curves, 1.0e-6);
        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].origin, Point3::new(10.0, 20.0, 30.0));
        assert_eq!(sketches[0].u_axis, Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(sketches[0].normal, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(entities.len(), 2);
        assert!(entities.iter().any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Point { position } if position == Point2::new(2.5, 4.0)
        )));
        assert!(entities.iter().any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 6.0)
        )));

        let relation = |record_index, member, operand| SketchRelation {
            id: format!("f3d:native:relation#{record_index}"),
            record_index,
            class_tag: "302".into(),
            byte_offset: 600,
            state_offset: 70,
            owner_reference: 172,
            owner_entity_id: "0_172".into(),
            auxiliary_references: Vec::new(),
            auxiliary_reference_offsets: Vec::new(),
            members: vec![member],
            resolved_members: vec![operand],
            member_offsets: vec![25],
            owner_reference_offset: 55,
            state: 0x40,
            constraint_kinds: vec![SketchConstraintKind::Horizontal],
            unknown_constraint_bits: 0,
            return_members: vec![member],
            resolved_return_members: Vec::new(),
            return_member_offsets: vec![80],
            raw_bytes: Vec::new(),
        };
        let mut curve_point_coincidence = relation(
            702,
            217,
            SketchRelationOperand::Curve {
                record_index: 217,
                primary_id: 20,
                secondary_id: 0,
            },
        );
        curve_point_coincidence.members.push(175);
        curve_point_coincidence
            .resolved_members
            .push(SketchRelationOperand::Point {
                record_index: 175,
                persistent_id: 10,
            });
        curve_point_coincidence.member_offsets.push(40);
        curve_point_coincidence.state = 1;
        curve_point_coincidence.constraint_kinds = vec![SketchConstraintKind::Coincident];
        let mut midpoint = curve_point_coincidence.clone();
        midpoint.record_index = 703;
        midpoint.id = "f3d:native:relation#703".into();
        midpoint.state = 0x10;
        midpoint.constraint_kinds = vec![SketchConstraintKind::Parallel];
        let mut curvature = curve_point_coincidence.clone();
        curvature.record_index = 704;
        curvature.id = "f3d:native:relation#704".into();
        curvature.state = 0x200;
        curvature.constraint_kinds = vec![SketchConstraintKind::Curvature];
        let constraints = project_sketch_constraints(
            &placements,
            &points,
            &curves,
            &[
                relation(
                    700,
                    217,
                    SketchRelationOperand::Curve {
                        record_index: 217,
                        primary_id: 20,
                        secondary_id: 0,
                    },
                ),
                relation(
                    701,
                    175,
                    SketchRelationOperand::Point {
                        record_index: 175,
                        persistent_id: 10,
                    },
                ),
                curve_point_coincidence,
                midpoint,
                curvature,
            ],
            &entities,
        );
        assert!(matches!(
            constraints[0].definition,
            SketchConstraintDefinition::Horizontal { .. }
        ));
        assert!(matches!(
            constraints[1].definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ref entities,
                ref operands,
                ..
            } if native_kind == "horizontal" && entities.len() == 1 && operands.is_empty()
        ));
        assert!(matches!(
            constraints[2].definition,
            SketchConstraintDefinition::Coincident { ref entities } if entities.len() == 2
        ));
        assert!(matches!(
            constraints[3].definition,
            SketchConstraintDefinition::Midpoint { .. }
        ));
        assert!(matches!(
            constraints[4].definition,
            SketchConstraintDefinition::Curvature { .. }
        ));
        let line = entities
            .iter()
            .find(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .unwrap();
        let point = entities
            .iter()
            .find(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
            .unwrap();
        assert!(matches!(
            exact_atomic_constraint(SketchConstraintKind::Midpoint, &[line, point]),
            Some(SketchConstraintDefinition::Midpoint { .. })
        ));
    }

    #[test]
    fn three_member_symmetry_states_project_unique_reflection_axis() {
        let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let first = entity(
            "generated:point#left",
            SketchGeometry::Point {
                position: Point2::new(-2.0, 3.0),
            },
        );
        let axis_entity = entity(
            "generated:line#axis",
            SketchGeometry::Line {
                start: Point2::new(0.0, -5.0),
                end: Point2::new(0.0, 5.0),
            },
        );
        let second = entity(
            "generated:point#right",
            SketchGeometry::Point {
                position: Point2::new(2.0, 3.0),
            },
        );

        for kind in [
            SketchConstraintKind::Concentric,
            SketchConstraintKind::Symmetry,
        ] {
            let definition =
                exact_atomic_constraint(kind, &[&first, &axis_entity, &second]).unwrap();
            assert!(matches!(
                definition,
                SketchConstraintDefinition::Symmetric {
                    first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
                    second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
                    axis: ref axis_id,
                } if first_id == &first.id
                    && second_id == &second.id
                    && axis_id == &axis_entity.id
            ));
        }

        let off_axis = entity(
            "generated:line#off-axis",
            SketchGeometry::Line {
                start: Point2::new(1.0, -5.0),
                end: Point2::new(1.0, 5.0),
            },
        );
        assert!(exact_atomic_constraint(
            SketchConstraintKind::Concentric,
            &[&first, &off_axis, &second],
        )
        .is_none());
    }

    #[test]
    fn aggregate_offset_relation_projects_ordered_pairs_and_signed_distance() {
        let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let source_horizontal = entity(
            "generated:line#source-horizontal",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
        );
        let result_horizontal = entity(
            "generated:line#result-horizontal",
            SketchGeometry::Line {
                start: Point2::new(2.0, -2.0),
                end: Point2::new(8.0, -2.0),
            },
        );
        let source_vertical = entity(
            "generated:line#source-vertical",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 10.0),
            },
        );
        let result_vertical = entity(
            "generated:line#result-vertical",
            SketchGeometry::Line {
                start: Point2::new(2.0, 2.0),
                end: Point2::new(2.0, 8.0),
            },
        );
        let curve = |record_index, secondary_id| SketchRelationOperand::Curve {
            record_index,
            primary_id: u64::from(record_index),
            secondary_id,
        };
        let relation = SketchRelation {
            id: "f3d:native:sketch-relation#0".into(),
            record_index: 10,
            class_tag: "300".into(),
            byte_offset: 0,
            state_offset: 100,
            owner_reference: 1,
            owner_entity_id: "0_1".into(),
            auxiliary_references: vec![0],
            auxiliary_reference_offsets: vec![80],
            members: vec![1, 2, 3, 4],
            resolved_members: Vec::new(),
            member_offsets: vec![25, 40, 55, 70],
            owner_reference_offset: 90,
            state: 0x20,
            constraint_kinds: vec![SketchConstraintKind::Perpendicular],
            unknown_constraint_bits: 0,
            return_members: vec![1, 3, 2, 4],
            resolved_return_members: vec![curve(1, 0), curve(3, 30), curve(2, 0), curve(4, 40)],
            return_member_offsets: vec![120, 131, 142, 153],
            raw_bytes: Vec::new(),
        };
        let projected = HashMap::from([
            (("native", 1), &source_horizontal),
            (("native", 2), &source_vertical),
            (("native", 3), &result_horizontal),
            (("native", 4), &result_vertical),
        ]);

        let definition = exact_offset_constraint(&relation, "native", &projected).unwrap();
        let SketchConstraintDefinition::Offset {
            pairs,
            signed_distance,
        } = definition
        else {
            panic!("expected neutral offset constraint")
        };
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].source, source_horizontal.id);
        assert_eq!(pairs[0].result, result_horizontal.id);
        assert_eq!(pairs[1].source, source_vertical.id);
        assert_eq!(pairs[1].result, result_vertical.id);
        assert!((signed_distance.0 + 2.0).abs() <= 1.0e-9);
    }

    #[test]
    fn angular_point_operand_selects_unique_incident_line_by_value() {
        let entity = |id: &str, geometry: SketchGeometry| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let point = entity(
            "generated:point#vertex",
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        );
        let explicit = entity(
            "generated:line#explicit",
            SketchGeometry::Line {
                start: Point2::new(2.0, -2.0),
                end: Point2::new(2.0, 2.0),
            },
        );
        let diagonal = entity(
            "generated:line#diagonal",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 2.0),
            },
        );
        let horizontal = entity(
            "generated:line#horizontal",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 0.0),
            },
        );
        let projected = HashMap::from([
            (("native", 1), &point),
            (("native", 2), &explicit),
            (("native", 3), &diagonal),
            (("native", 4), &horizontal),
        ]);

        let lines = indirect_angular_lines(
            "native",
            &[&point, &explicit],
            std::f64::consts::FRAC_PI_4,
            &projected,
        )
        .unwrap();
        assert_eq!(lines, (diagonal.id.clone(), explicit.id.clone()));
        let supplementary = indirect_angular_lines(
            "native",
            &[&point, &explicit],
            3.0 * std::f64::consts::FRAC_PI_4,
            &projected,
        )
        .unwrap();
        assert_eq!(supplementary, lines);
    }

    #[test]
    fn counted_linear_graph_selects_one_parameter_backed_direction() {
        let entity = |id: &str, position| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point { position },
        };
        let first = entity("generated:point#first", Point2::new(4.0, 16.0));
        let second = entity("generated:point#second", Point2::new(4.0, 14.0));
        let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

        let definition =
            directional_point_dimension(&[&first, &second], 2.0, parameter.clone()).unwrap();
        assert!(matches!(
            definition,
            SketchConstraintDefinition::VerticalDistance {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
                parameter: ref parameter_id,
            } if first_id == &first.id && second_id == &second.id && parameter_id == &parameter
        ));
        assert!(directional_point_dimension(&[&first, &second], 3.0, parameter).is_none());
    }

    #[test]
    fn unclassified_two_locus_linear_group_is_parameter_backed_distance() {
        let entity = |id: &str, geometry| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let point = entity(
            "generated:point#dimension",
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        );
        let line = entity(
            "generated:line#dimension",
            SketchGeometry::Line {
                start: Point2::new(-10.0, 0.0),
                end: Point2::new(-50.0, 0.0),
            },
        );
        let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

        assert!(exact_counted_dimension_relation(&[&point, &line]).is_none());
        assert!(matches!(
            two_locus_distance_dimension(&[&point, &line], parameter.clone()),
            Some(SketchConstraintDefinition::Distance {
                ref entities,
                parameter: ref actual_parameter,
            }) if entities == &[point.id, line.id] && actual_parameter == &parameter
        ));
    }

    #[test]
    fn counted_linear_graph_projects_exact_auxiliary_relations() {
        let entity = |id: &str, geometry| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let horizontal = entity(
            "generated:line#horizontal",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
        );
        let vertical = entity(
            "generated:line#vertical",
            SketchGeometry::Line {
                start: Point2::new(0.0, -2.0),
                end: Point2::new(0.0, 2.0),
            },
        );
        let parallel = entity(
            "generated:line#parallel",
            SketchGeometry::Line {
                start: Point2::new(0.0, 2.0),
                end: Point2::new(10.0, 2.0),
            },
        );
        let point = entity(
            "generated:point#on-line",
            SketchGeometry::Point {
                position: Point2::new(4.0, 0.0),
            },
        );
        let duplicate_point = entity(
            "generated:point#duplicate",
            SketchGeometry::Point {
                position: Point2::new(4.0, 0.0),
            },
        );
        let arc = entity(
            "generated:arc#bounded",
            SketchGeometry::Arc {
                center: Point2::new(3.0, 0.0),
                radius: cadmpeg_ir::features::Length(1.0),
                start_angle: cadmpeg_ir::features::Angle(0.0),
                end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
            },
        );
        let arc_start = entity(
            "generated:point#arc-start",
            SketchGeometry::Point {
                position: Point2::new(4.0, 0.0),
            },
        );
        let outside_arc = entity(
            "generated:point#outside-arc",
            SketchGeometry::Point {
                position: Point2::new(2.0, 0.0),
            },
        );

        assert!(matches!(
            exact_counted_dimension_relation(&[&horizontal, &vertical]),
            Some(SketchConstraintDefinition::Perpendicular { .. })
        ));
        assert!(matches!(
            exact_counted_dimension_relation(&[&horizontal, &parallel]),
            Some(SketchConstraintDefinition::Parallel { .. })
        ));
        assert!(matches!(
            exact_counted_dimension_relation(&[&horizontal, &point]),
            Some(SketchConstraintDefinition::Coincident { .. })
        ));
        assert!(matches!(
            exact_counted_dimension_relation(&[&point, &duplicate_point]),
            Some(SketchConstraintDefinition::Coincident { .. })
        ));
        assert!(matches!(
            exact_counted_dimension_relation(&[&arc_start, &arc]),
            Some(SketchConstraintDefinition::Coincident { .. })
        ));
        assert!(exact_counted_dimension_relation(&[&outside_arc, &arc]).is_none());
    }

    #[test]
    fn counted_offset_return_run_pairs_sources_and_results() {
        let entity = |id: &str, start, end| cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("generated:sketch#0".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let bottom = entity(
            "generated:line#bottom",
            Point2::new(10.0, 0.0),
            Point2::new(0.0, 0.0),
        );
        let top = entity(
            "generated:line#top",
            Point2::new(0.0, 10.0),
            Point2::new(10.0, 10.0),
        );
        let inset_top = entity(
            "generated:line#inset-top",
            Point2::new(2.0, 8.0),
            Point2::new(8.0, 8.0),
        );
        let inset_bottom = entity(
            "generated:line#inset-bottom",
            Point2::new(8.0, 2.0),
            Point2::new(2.0, 2.0),
        );

        let entities =
            HashMap::from([(1, &bottom), (2, &top), (3, &inset_top), (4, &inset_bottom)]);
        let definition =
            exact_counted_offset(&[(1, 3), (2, 2), (3, 0), (4, 0)], &[1, 4, 2, 3], &entities)
                .expect("counted offset graph");
        let SketchConstraintDefinition::Offset {
            pairs,
            signed_distance,
        } = definition
        else {
            panic!("expected offset")
        };
        assert_eq!(pairs[0].source, bottom.id);
        assert_eq!(pairs[0].result, inset_bottom.id);
        assert_eq!(pairs[1].source, top.id);
        assert_eq!(pairs[1].result, inset_top.id);
        assert!((signed_distance.0 + 2.0).abs() <= 1.0e-9);
    }

    #[test]
    fn paired_dimensions_bind_geometry_with_stream_local_record_indices() {
        let placement = |stream: &str, suffix| DesignSketchPlacement {
            id: format!("f3d:{stream}:design-sketch-placement#0"),
            scope_record_index: 10,
            entity_id: format!("0_{suffix}"),
            entity_suffix: suffix,
            byte_offset: 0,
            class_tag: "356".into(),
            record_index: 11,
            frame_length: 201,
            transform: identity_matrix(),
            transform_offset: None,
            paired_class_tag: "259".into(),
            paired_byte_offset: 201,
        };
        let owner = |stream: &str| DesignParameterOwner {
            id: format!("f3d:{stream}:design-parameter-owner#0"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 9,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 40,
            parameter_record_index: 11,
            owned_ordinal: 0,
            variant: 0,
            companion_record_index: 12,
        };
        let pair = |stream: &str| DesignDimensionLocusPair {
            id: format!("f3d:{stream}:design-dimension-locus-pair#0"),
            companion_record_index: 12,
            byte_offset: 0,
            class_tag: "277".into(),
            record_index: 13,
            frame_length: 100,
            opaque_index: 0,
            opaque_index_offset: 35,
            first_geometry_record_index: 20,
            first_geometry_reference_offset: 40,
            first_role: 0,
            first_role_offset: 50,
            second_geometry_record_index: 21,
            second_geometry_reference_offset: 55,
            second_role: 0,
            second_role_offset: 65,
            paired_class_tag: "273".into(),
            paired_byte_offset: 100,
        };
        let point = |stream: &str, record_index| SketchPoint {
            id: format!("f3d:{stream}:sketch-point#{record_index}"),
            record_index,
            owner_reference: None,
            class_tag: "300".into(),
            byte_offset: 0,
            coordinate_offset: 89,
            entity_genesis: None,
            persistent_id: u64::from(record_index),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
            raw_bytes: Vec::new(),
        };
        let mut points = vec![
            point("A", 20),
            point("A", 21),
            point("B", 20),
            point("B", 21),
        ];

        bind_dimension_loci(
            &[placement("A", 100), placement("B", 200)],
            &[owner("A"), owner("B")],
            &[pair("A"), pair("B")],
            &[],
            &[],
            &mut points,
            &mut [],
        )
        .unwrap();
        assert_eq!(
            points
                .iter()
                .map(|point| point.owner_reference)
                .collect::<Vec<_>>(),
            [Some(100), Some(100), Some(200), Some(200)]
        );
    }

    #[test]
    fn recipe_backed_dimension_projects_disjoint_repeated_distance() {
        let stream = "f3d:A";
        let placement = DesignSketchPlacement {
            id: format!("{stream}:design-sketch-placement#0"),
            scope_record_index: 10,
            entity_id: "0_100".into(),
            entity_suffix: 100,
            byte_offset: 0,
            class_tag: "356".into(),
            record_index: 11,
            frame_length: 201,
            transform: identity_matrix(),
            transform_offset: None,
            paired_class_tag: "259".into(),
            paired_byte_offset: 201,
        };
        let parameter = DesignParameter {
            id: format!("{stream}:design-parameter#20"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 20,
            prefix_value: 0,
            prefix_value_offset: 0,
            source_ordinal: 4,
            owner_record_index: Some(21),
            expression: "thickness".into(),
            expression_offset: 0,
            source_kind: "Linear Dimension-4".into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some("mm".into()),
            unit_offset: Some(0),
            name: "d4".into(),
            name_offset: 0,
            evaluated_value: 0.2,
            evaluated_value_offset: 0,
        };
        let owner = DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#21"),
            byte_offset: 0,
            class_tag: "292".into(),
            record_index: 21,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 0.2,
            evaluated_value_offset: 0,
            parameter_record_index: 20,
            owned_ordinal: 0,
            variant: 0,
            companion_record_index: 22,
        };
        let companion = DesignParameterCompanion {
            id: format!("{stream}:design-parameter-companion#22"),
            byte_offset: 0,
            class_tag: "408".into(),
            record_index: 22,
            owner_record_index: 21,
            timestamp_micros: 1,
            timestamp_micros_offset: 0,
            payload_byte_offset: 58,
            payload_byte_length: 200,
            owned_recipe_ids: Vec::new(),
        };
        let recipe = |ordinal, record_index| DesignDimensionRecipeRecord {
            id: format!("{stream}:design-dimension-recipe-record#{record_index}"),
            companion_record_index: 22,
            recipe_ordinal: ordinal,
            recipe_id: format!("{stream}:construction-recipe#{record_index}"),
            byte_offset: 0,
            class_tag: "423".into(),
            record_index,
            frame_length: 10,
            prefix_offset: 0,
            prefix_bytes: Vec::new(),
            references: Vec::new(),
            program_offset: 0,
            program: vec![-1],
            matching_edge_operand_ids: Vec::new(),
        };
        let sketch = neutral_sketch_id(&placement);
        let line = |name: &str, start, end| SketchEntity {
            id: SketchEntityId(name.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let entities = [
            line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
            line("second", Point2::new(0.0, 2.0), Point2::new(4.0, 2.0)),
            line("third", Point2::new(10.0, 0.0), Point2::new(10.0, 4.0)),
            line("fourth", Point2::new(12.0, 0.0), Point2::new(12.0, 4.0)),
        ];
        let constraints = project_dimension_constraints(
            &[placement],
            &[parameter],
            &[owner],
            &[],
            &[],
            &[],
            &[companion],
            &[recipe(1, 31), recipe(0, 30)],
            &[],
            &[],
            &entities,
        );
        let [constraint] = constraints.as_slice() else {
            panic!("expected one recipe-backed dimension")
        };
        let SketchConstraintDefinition::RepeatedDistance {
            measurements,
            parameter,
            ..
        } = &constraint.definition
        else {
            panic!("expected repeated recipe-backed dimension")
        };
        assert_eq!(
            parameter.0,
            format!("f3d:model:parameter#{}:{stream}4", stream.len())
        );
        assert_eq!(measurements.len(), 2);
        assert!(measurements.iter().all(|measurement| matches!(
            measurement,
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance { .. }
        )));
    }

    #[test]
    fn recipe_dimension_requires_one_axis_aligned_point_pair() {
        let sketch = SketchId("sketch".into());
        let point = |name: &str, u, v| SketchEntity {
            id: SketchEntityId(name.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        };
        let parameter = cadmpeg_ir::features::ParameterId("parameter".into());
        let mut entities = vec![
            point("first", -30.0, 2.0),
            point("second", -30.0, 0.0),
            point("unrelated", 10.0, 10.0),
        ];
        assert!(matches!(
            super::recipe_linear_dimension_candidates(
                &entities,
                &sketch,
                2.0,
                &parameter,
            ).as_slice(),
            [SketchConstraintDefinition::VerticalDistance { first, second, parameter: actual }]
                if *first == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("first".into()))
                    && *second == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("second".into()))
                    && *actual == parameter
        ));
        entities.push(point("ambiguous", 10.0, 8.0));
        let candidates =
            super::recipe_linear_dimension_candidates(&entities, &sketch, 2.0, &parameter);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            super::recipe_dimension_candidate_entities(&candidates),
            [
                SketchEntityId("first".into()),
                SketchEntityId("second".into()),
                SketchEntityId("unrelated".into()),
                SketchEntityId("ambiguous".into()),
            ]
        );
    }

    #[test]
    fn recipe_dimension_resolves_one_parallel_line_pair() {
        let sketch = SketchId("sketch".into());
        let line = |name: &str, start, end| SketchEntity {
            id: SketchEntityId(name.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let entities = vec![
            line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
            line("second", Point2::new(1.0, 2.0), Point2::new(5.0, 2.0)),
            line("unrelated", Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
        ];
        assert!(matches!(
            super::recipe_linear_dimension_candidates(
                &entities,
                &sketch,
                2.0,
                &cadmpeg_ir::features::ParameterId("parameter".into()),
            ).as_slice(),
            [SketchConstraintDefinition::Distance { entities, .. }]
                if entities.as_slice() == [SketchEntityId("first".into()), SketchEntityId("second".into())]
        ));
    }

    #[test]
    fn design_streams_scope_sketch_graphs_identities_and_parameter_names() {
        let placement = |stream: &str| DesignSketchPlacement {
            id: format!("f3d:{stream}:design-sketch-placement#0"),
            scope_record_index: 10,
            entity_id: format!("{stream}_100"),
            entity_suffix: 100,
            byte_offset: 0,
            class_tag: "356".into(),
            record_index: 11,
            frame_length: 201,
            transform: identity_matrix(),
            transform_offset: None,
            paired_class_tag: "259".into(),
            paired_byte_offset: 201,
        };
        let header = |stream: &str| DesignEntityHeader {
            id: format!("f3d:{stream}:design-entity-header#0"),
            byte_offset: 0,
            entity_suffix: 100,
            entity_id: format!("{stream}_100"),
            class_tag: "300".into(),
            optional_slot_present: true,
            object_kind: Some(DesignObjectKind::Sketch),
            record_reference: None,
            record_reference_offset: None,
            declared_reference_count: Some(1),
            reference_indices: vec![30],
            reference_offsets: vec![0],
        };
        let point = |stream: &str| SketchPoint {
            id: format!("f3d:{stream}:sketch-point#0"),
            record_index: 20,
            owner_reference: None,
            class_tag: "301".into(),
            byte_offset: 0,
            coordinate_offset: 89,
            entity_genesis: None,
            persistent_id: 20,
            paired_reference: 0,
            coordinates: Point2::new(1.0, 2.0),
            raw_bytes: Vec::new(),
        };
        let relation = |stream: &str| SketchRelation {
            id: format!("f3d:{stream}:sketch-relation#30"),
            record_index: 30,
            class_tag: "302".into(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 100,
            owner_entity_id: String::new(),
            auxiliary_references: Vec::new(),
            auxiliary_reference_offsets: Vec::new(),
            members: vec![20],
            resolved_members: Vec::new(),
            member_offsets: vec![0],
            owner_reference_offset: 0,
            state: 0,
            constraint_kinds: vec![SketchConstraintKind::Coincident],
            unknown_constraint_bits: 0,
            return_members: vec![20],
            resolved_return_members: Vec::new(),
            return_member_offsets: vec![0],
            raw_bytes: Vec::new(),
        };

        let placements = [placement("A"), placement("B")];
        let mut points = [point("A"), point("B")];
        let mut relations = [relation("A"), relation("B")];
        bind_sketch_graph(
            &[header("A"), header("B")],
            &mut points,
            &mut [],
            &mut relations,
        )
        .expect("stream-local sketch graphs bind independently");
        assert_eq!(relations[0].owner_entity_id, "A_100");
        assert_eq!(relations[1].owner_entity_id, "B_100");

        let (mut sketches, mut entities) = project_sketch_design(&placements, &points, &[], 1.0e-6);
        let mut constraints =
            project_sketch_constraints(&placements, &points, &[], &relations, &entities);
        assert_eq!(sketches.len(), 2);
        assert_eq!(entities.len(), 2);
        assert_eq!(constraints.len(), 2);
        assert_eq!(
            sketches
                .iter()
                .map(|item| &item.id)
                .collect::<HashSet<_>>()
                .len(),
            2
        );
        assert_eq!(
            entities
                .iter()
                .map(|item| &item.id)
                .collect::<HashSet<_>>()
                .len(),
            2
        );
        assert_eq!(
            constraints
                .iter()
                .map(|item| &item.id)
                .collect::<HashSet<_>>()
                .len(),
            2
        );

        let parameter = |stream: &str, record_index, name: &str, expression: &str| {
            let mut parameter = parse_design_parameter(&parameter_record(
                None,
                expression,
                "User Parameter",
                Some("mm"),
                name,
                1.0,
            ))
            .expect("generated user parameter is canonical");
            parameter.id = format!("f3d:{stream}:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter.source_ordinal = record_index;
            parameter
        };
        let (_, parameters) = project_parameter_design(
            &[
                parameter("A", 40, "Width", "1 mm"),
                parameter("A", 41, "Half", "Width / 2"),
                parameter("B", 40, "Width", "2 mm"),
            ],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let half = parameters
            .iter()
            .find(|parameter| parameter.name == "Half")
            .expect("projected Half parameter");
        let a_width = parameters
            .iter()
            .find(|parameter| {
                parameter.name == "Width"
                    && parameter.native_ref.as_deref() == Some("f3d:A:parameter#40")
            })
            .expect("projected stream A Width parameter");
        assert_eq!(half.dependencies, std::slice::from_ref(&a_width.id));
        assert_eq!(
            parameters
                .iter()
                .map(|item| &item.id)
                .collect::<HashSet<_>>()
                .len(),
            3
        );

        for sketch in &mut sketches {
            sketch.native_ref = None;
        }
        for entity in &mut entities {
            entity.native_ref = None;
        }
        for constraint in &mut constraints {
            constraint.native_ref = None;
        }
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.sketches = sketches;
        ir.model.sketch_entities = entities;
        ir.model.sketch_constraints = constraints;
        ir.finalize();
        let report = cadmpeg_ir::validate::validate(&ir, Vec::new());
        assert!(report.is_ok(), "validation findings: {:?}", report.findings);
    }

    #[test]
    fn user_parameters_project_in_source_order_with_units_and_dependencies() {
        let mut width = parse_design_parameter(&parameter_record(
            None,
            "60 mm",
            "User Parameter",
            Some("mm"),
            "Width",
            6.0,
        ))
        .unwrap();
        width.id = "f3d:native:parameter#width".into();
        width.record_index = 20;
        width.source_ordinal = 4;
        let mut half = parse_design_parameter(&parameter_record(
            None,
            "Width / 2",
            "User Parameter",
            Some("mm"),
            "HalfWidth",
            3.0,
        ))
        .unwrap();
        half.id = "f3d:native:parameter#half".into();
        half.record_index = 21;
        half.source_ordinal = 5;

        let (features, projected) =
            project_parameter_design(&[half, width], &[], &[], &[], &[], &[], &[], &[]);
        assert!(features.is_empty());
        assert_eq!(projected[0].name, "Width");
        assert_eq!(projected[0].owner, None);
        assert_eq!(
            projected[0].value,
            Some(ParameterValue::Length(Length(60.0)))
        );
        assert_eq!(projected[1].dependencies, [projected[0].id.clone()]);
        assert_eq!(
            projected[1].native_ref.as_deref(),
            Some("f3d:native:parameter#half")
        );
    }

    #[test]
    fn parameters_project_all_design_database_unit_tokens() {
        let mut native = ["mm", "cm", "m", "in", "ft", "deg", "rad"]
            .into_iter()
            .enumerate()
            .map(|(ordinal, unit)| {
                let mut parameter = parse_design_parameter(&parameter_record(
                    None,
                    "value",
                    "User Parameter",
                    Some(unit),
                    &format!("Value{ordinal}"),
                    1.25,
                ))
                .expect("generated database-unit parameter");
                parameter.id = format!("f3d:native:parameter#{ordinal}");
                parameter.record_index = u32::try_from(ordinal).unwrap();
                parameter.source_ordinal = u32::try_from(ordinal).unwrap();
                parameter
            })
            .collect::<Vec<_>>();
        native.reverse();

        let (_, projected) = project_parameter_design(&native, &[], &[], &[], &[], &[], &[], &[]);
        for ordinal in 0..5 {
            assert_eq!(
                projected
                    .iter()
                    .find(|parameter| parameter.name == format!("Value{ordinal}"))
                    .and_then(|parameter| parameter.value.clone()),
                Some(ParameterValue::Length(Length(12.5)))
            );
        }
        for ordinal in 5..7 {
            assert_eq!(
                projected
                    .iter()
                    .find(|parameter| parameter.name == format!("Value{ordinal}"))
                    .and_then(|parameter| parameter.value.clone()),
                Some(ParameterValue::Angle(Angle(1.25)))
            );
        }
    }

    #[test]
    fn expression_dependencies_preserve_fusion_parameter_name_symbols() {
        let name = "Width$µ°\"A";
        assert_eq!(
            expression_identifiers(&format!("{name} / 2 + sin(30 deg)")).collect::<Vec<_>>(),
            [name, "sin", "deg"]
        );
        let parameter = |record_index, source_ordinal, expression: &str, name: &str| {
            let mut parameter = parse_design_parameter(&parameter_record(
                None,
                expression,
                "User Parameter",
                Some("mm"),
                name,
                1.0,
            ))
            .expect("generated symbolic-name parameter");
            parameter.id = format!("f3d:native:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter.source_ordinal = source_ordinal;
            parameter
        };
        let (_, projected) = project_parameter_design(
            &[
                parameter(20, 0, "10 mm", name),
                parameter(21, 1, &format!("{name} / 2"), "Half"),
            ],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let source = projected
            .iter()
            .find(|parameter| parameter.name == name)
            .expect("symbolic-name source parameter");
        let half = projected
            .iter()
            .find(|parameter| parameter.name == "Half")
            .expect("dependent parameter");
        assert_eq!(half.dependencies, [source.id.clone()]);
    }

    #[test]
    fn owned_parameter_projects_under_its_real_scope_feature() {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(44),
            "60 mm",
            "AlongDistance",
            Some("mm"),
            "d12",
            6.0,
        ))
        .unwrap();
        parameter.id = "f3d:native:parameter#45".into();
        parameter.record_index = 45;
        let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
        owner.id = "f3d:native:parameter-owner#44".into();
        let scope = DesignParameterScope {
            id: "f3d:native:parameter-scope#12".into(),
            byte_offset: 100,
            class_tag: "301".into(),
            record_index: 12,
            frame_length: 200,
            kind: "Extrude".into(),
            kind_offset: 210,
            extrude_operation: Some(DesignExtrudeOperation::NewBody),
            extrude_operation_offset: Some(128),
            extrude_extent: Some(DesignExtrudeExtent::OneSidedDistance),
            extrude_extent_offsets: Some([132, 136]),
            extrude_direction_reversed: Some(false),
            extrude_direction_reversed_offset: Some(140),
            extrude_start: Some(DesignExtrudeStart::ProfilePlane),
            extrude_start_offset: Some(142),
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 180,
            reference_members: vec![44],
            reference_member_offsets: vec![185],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 300,
        };

        let (features, parameters) =
            project_parameter_design(&[parameter], &[owner], &[scope], &[], &[], &[], &[], &[]);
        assert_eq!(features.len(), 1);
        assert!(features[0].suppressed);
        assert!(matches!(
            &features[0].definition,
            FeatureDefinition::Native { kind, parameters, .. }
                if kind == "Extrude" && parameters.get("d12").map(String::as_str) == Some("60 mm")
        ));
        assert_eq!(parameters[0].owner.as_ref(), Some(&features[0].id));
        assert_eq!(parameters[0].ordinal, 2);
        assert_eq!(
            parameters[0]
                .properties
                .get("source_kind")
                .map(String::as_str),
            Some("AlongDistance")
        );
    }

    #[test]
    fn parameter_dependencies_resolve_feature_scope_before_document_scope() {
        let parameter = |owner, record_index, expression: &str, name: &str| {
            let mut parameter = parse_design_parameter(&parameter_record(
                owner,
                expression,
                if owner.is_some() {
                    "FeatureInput"
                } else {
                    "User Parameter"
                },
                Some("mm"),
                name,
                1.0,
            ))
            .unwrap();
            parameter.id = format!("f3d:Design/BulkStream.dat:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter.source_ordinal = record_index;
            parameter
        };
        let owner =
            |record_index, parameter_record_index, scope_record_index| DesignParameterOwner {
                id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
                byte_offset: 0,
                class_tag: "292".into(),
                record_index,
                scope_record_index,
                local_ordinal: parameter_record_index,
                evaluated_value: 1.0,
                evaluated_value_offset: 0,
                parameter_record_index,
                owned_ordinal: parameter_record_index,
                variant: 0,
                companion_record_index: record_index + 1,
            };
        let scope = |record_index| DesignParameterScope {
            id: format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
            byte_offset: u64::from(record_index),
            class_tag: "301".into(),
            record_index,
            frame_length: 100,
            kind: "CustomFeature".into(),
            kind_offset: 0,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: record_index,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "302".into(),
            paired_byte_offset: u64::from(record_index) + 100,
        };

        let document_width = parameter(None, 20, "60 mm", "Width");
        let local_width = parameter(Some(101), 21, "20 mm", "Width");
        let local_half = parameter(Some(102), 22, "Width / 2", "Half");
        let remote_half = parameter(Some(103), 23, "Width / 2", "Half");
        let owned_depth = parameter(Some(104), 24, "10 mm", "OwnedDepth");
        let document_half = parameter(None, 25, "OwnedDepth / 2", "DocumentHalf");
        let document_forward = parameter(None, 26, "Later / 2", "DocumentForward");
        let document_later = parameter(None, 27, "10 mm", "Later");
        let cycle_a = parameter(None, 28, "CycleB / 2", "CycleA");
        let cycle_b = parameter(None, 29, "CycleA / 2", "CycleB");
        let (_, parameters) = project_parameter_design(
            &[
                document_width,
                local_width,
                local_half,
                remote_half,
                owned_depth,
                document_half,
                document_forward,
                document_later,
                cycle_a,
                cycle_b,
            ],
            &[
                owner(101, 21, 201),
                owner(102, 22, 201),
                owner(103, 23, 202),
                owner(104, 24, 201),
            ],
            &[scope(201), scope(202)],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let by_name_and_owner = |name: &str, owner_record_index: u32| {
            parameters
                .iter()
                .find(|parameter| {
                    parameter.name == name
                        && parameter.native_ref.as_deref()
                            == Some(
                                format!(
                                    "f3d:Design/BulkStream.dat:parameter#{}",
                                    owner_record_index
                                )
                                .as_str(),
                            )
                })
                .unwrap()
        };
        let document = by_name_and_owner("Width", 20);
        let local = by_name_and_owner("Width", 21);
        assert_eq!(
            by_name_and_owner("Half", 22).dependencies,
            [local.id.clone()]
        );
        assert_eq!(
            by_name_and_owner("Half", 23).dependencies,
            [document.id.clone()]
        );
        assert!(by_name_and_owner("DocumentHalf", 25)
            .dependencies
            .is_empty());
        let document_forward = by_name_and_owner("DocumentForward", 26);
        let document_later = by_name_and_owner("Later", 27);
        assert_eq!(document_forward.dependencies, [document_later.id.clone()]);
        assert!(document_later.ordinal < document_forward.ordinal);
        let cycle_a = by_name_and_owner("CycleA", 28);
        let cycle_b = by_name_and_owner("CycleB", 29);
        assert!(cycle_a.dependencies.is_empty());
        assert_eq!(cycle_b.dependencies, [cycle_a.id.clone()]);
        assert!(cycle_a.ordinal < cycle_b.ordinal);
    }

    #[test]
    fn extrude_parameters_project_blind_two_sided_and_reversed_extents() {
        use cadmpeg_ir::features::{
            Angle, BooleanOp, Extent, ExtrudeStart, FaceSelection, ProfileRef,
        };

        let parameter = |source_kind: &str, unit: &str, value| {
            parse_design_parameter(&parameter_record(
                Some(44),
                "value",
                source_kind,
                Some(unit),
                "d1",
                value,
            ))
            .expect("generated feature parameter is canonical")
        };
        let mut scope = DesignParameterScope {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: "301".into(),
            record_index: 12,
            frame_length: 200,
            kind: "Extrude".into(),
            kind_offset: 210,
            extrude_operation: Some(DesignExtrudeOperation::NewBody),
            extrude_operation_offset: Some(128),
            extrude_extent: Some(DesignExtrudeExtent::OneSidedDistance),
            extrude_extent_offsets: Some([132, 136]),
            extrude_direction_reversed: Some(false),
            extrude_direction_reversed_offset: Some(140),
            extrude_start: Some(DesignExtrudeStart::ProfilePlane),
            extrude_start_offset: Some(142),
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 180,
            reference_members: vec![100],
            reference_member_offsets: vec![185],
            extrude_profile: Some(DesignExtrudeProfileOperand {
                scope_reference_ordinal: 0,
                record_index: 100,
                byte_offset: 300,
                class_tag: "308".into(),
                asset_id: "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".into(),
                asset_id_offset: 330,
                entity_id: "0_172".into(),
                entity_suffix: 172,
                entity_reference_offset: 420,
                paired_class_tag: "259".into(),
                paired_byte_offset: 520,
            }),
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 300,
        };
        let placement = DesignSketchPlacement {
            id: "f3d:Design/BulkStream.dat:placement#200".into(),
            scope_record_index: 11,
            entity_id: "0_172".into(),
            entity_suffix: 172,
            byte_offset: 600,
            class_tag: "300".into(),
            record_index: 200,
            frame_length: 329,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            transform_offset: Some(655),
            paired_class_tag: "260".into(),
            paired_byte_offset: 929,
        };
        let along = parameter("AlongDistance", "mm", 0.55);
        let taper = parameter("TaperAngle", "deg", 0.2);
        let blind = project_extrude(
            &scope,
            &[(0, &along), (1, &taper)],
            &[],
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed blind Extrude");
        assert!(matches!(
            blind,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Sketch(ref profile),
                direction: None,
                extent: Extent::Blind { length: Length(5.5) },
                op: BooleanOp::NewBody,
                draft: Some(Angle(0.2)),
                ..
            } if profile == &neutral_sketch_id(&placement)
        ));
        let mut owned_along = along.clone();
        owned_along.id = "f3d:Design/BulkStream.dat:parameter#45".into();
        owned_along.record_index = 45;
        owned_along.owner_record_index = Some(44);
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical");
        owner.id = "f3d:Design/BulkStream.dat:owner#44".into();
        owner.record_index = 44;
        owner.scope_record_index = scope.record_index;
        owner.parameter_record_index = owned_along.record_index;
        let mut sketch_scope = scope.clone();
        sketch_scope.id = "f3d:Design/BulkStream.dat:scope#11".into();
        sketch_scope.record_index = placement.scope_record_index;
        sketch_scope.kind = "Sketch".into();
        sketch_scope.extrude_operation = None;
        sketch_scope.extrude_extent = None;
        sketch_scope.extrude_start = None;
        sketch_scope.extrude_profile = None;
        let (features, _) = project_parameter_design(
            &[owned_along],
            &[owner],
            &[sketch_scope, scope.clone()],
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&placement),
        );
        let sketch_feature = features
            .iter()
            .find(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
            .expect("neutral Sketch feature");
        let extrude_feature = features
            .iter()
            .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
            .expect("neutral Extrude feature");
        assert_eq!(extrude_feature.dependencies, [sketch_feature.id.clone()]);

        let body_group = DesignConstructionOperandGroup {
            id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 1,
            record_index: 101,
            byte_offset: 1000,
            class_tag: "332".into(),
            member_count_offset: 1021,
            members: vec![200],
            lost_edge_references: Vec::new(),
            member_offsets: vec![1026],
            identity_record_index: 300,
            identity_record_offset: 1044,
            role: 0x0000_0008_0000_0000,
            extrude_role: Some(DesignExtrudeOperandRole::Bodies),
            extrude_face_role: None,
            role_offset: 1054,
            opaque_index: 180,
            opaque_index_offset: 1072,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1076,
            variant: false,
            paired_class_tag: "259".into(),
            paired_byte_offset: 1125,
        };
        scope.extrude_operation = Some(DesignExtrudeOperation::Join);
        let target_body = project_extrude(
            &scope,
            &[(0, &along), (1, &taper)],
            std::slice::from_ref(&body_group),
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed target-body Extrude");
        assert!(matches!(
            target_body,
            FeatureDefinition::Extrude {
                op: BooleanOp::Join,
                ..
            }
        ));

        let mut face_group = body_group.clone();
        face_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
        face_group.extrude_role = Some(DesignExtrudeOperandRole::Faces);
        face_group.role = 0x0000_0011_0000_0000;
        let mut ordered_faces = [face_group.clone(), face_group.clone()];
        scope.extrude_start = Some(DesignExtrudeStart::FromFace);
        assign_extrude_face_roles(&scope, &mut ordered_faces);
        assert_eq!(
            ordered_faces.map(|group| group.extrude_face_role),
            [
                Some(DesignExtrudeFaceRole::Start),
                Some(DesignExtrudeFaceRole::Termination)
            ]
        );
        scope.extrude_start = Some(DesignExtrudeStart::ProfilePlane);
        assert!(project_extrude(
            &scope,
            &[(0, &along), (1, &taper)],
            &[body_group.clone(), face_group.clone()],
            &[],
            std::slice::from_ref(&placement),
        )
        .is_none());

        let profile_offset = parameter("ProfileOffset", "mm", 0.1);
        assert!(project_extrude(
            &scope,
            &[(0, &along), (1, &profile_offset)],
            std::slice::from_ref(&body_group),
            &[],
            std::slice::from_ref(&placement),
        )
        .is_none());
        scope.extrude_start = Some(DesignExtrudeStart::OffsetProfilePlane);
        let offset_start = project_extrude(
            &scope,
            &[(0, &along), (1, &profile_offset)],
            std::slice::from_ref(&body_group),
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed offset-profile-plane Extrude");
        assert!(matches!(
            offset_start,
            FeatureDefinition::Extrude {
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: Length(1.0)
                },
                ..
            }
        ));
        scope.extrude_start = Some(DesignExtrudeStart::ProfilePlane);

        scope.extrude_operation = Some(DesignExtrudeOperation::NewBody);
        let against = parameter("AgainstDistance", "mm", -0.05);
        assert!(project_extrude(
            &scope,
            &[(0, &along), (1, &against)],
            &[],
            &[],
            std::slice::from_ref(&placement),
        )
        .is_none());
        scope.extrude_extent = Some(DesignExtrudeExtent::TwoSidedDistance);
        let two_sided = project_extrude(
            &scope,
            &[(0, &along), (1, &against)],
            &[],
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed two-sided Extrude");
        assert!(matches!(
            two_sided,
            FeatureDefinition::Extrude {
                extent: Extent::TwoSided {
                    first: Length(5.5),
                    second: Length(0.5),
                },
                ..
            }
        ));

        scope.extrude_extent = Some(DesignExtrudeExtent::OneSidedDistance);
        let reversed_along = parameter("AlongDistance", "mm", -0.6);
        let reversed = project_extrude(
            &scope,
            &[(0, &reversed_along)],
            &[],
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed reversed Extrude");
        assert!(matches!(
            reversed,
            FeatureDefinition::Extrude {
                direction: Some(Vector3 {
                    x: 0.0,
                    y: -1.0,
                    z: 0.0
                }),
                extent: Extent::Blind {
                    length: Length(6.0)
                },
                ..
            }
        ));

        scope.extrude_operation = Some(DesignExtrudeOperation::Join);
        scope.extrude_extent = Some(DesignExtrudeExtent::OneSidedToFace);
        scope.extrude_direction_reversed = Some(true);
        face_group.extrude_face_role = Some(DesignExtrudeFaceRole::Termination);
        let side_offset = parameter("Side1Offset", "mm", 0.025);
        let to_face = project_extrude(
            &scope,
            &[(0, &side_offset), (1, &taper)],
            &[body_group.clone(), face_group.clone()],
            &[],
            std::slice::from_ref(&placement),
        )
        .expect("typed reversed to-face Extrude");
        assert!(matches!(
            to_face,
            FeatureDefinition::Extrude {
                direction: Some(Vector3 {
                    x: 0.0,
                    y: -1.0,
                    z: 0.0
                }),
                extent: Extent::ToFace {
                    face: FaceSelection::Native(ref id),
                    offset: Some(Length(0.25)),
                },
                ..
            } if id == &face_group.id
        ));

        scope.extrude_start = Some(DesignExtrudeStart::FromFace);
        let mut start_group = face_group.clone();
        start_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
        start_group.extrude_face_role = Some(DesignExtrudeFaceRole::Start);
        let from_face = project_extrude(
            &scope,
            &[
                (0, &parameter("ProfileOffset", "mm", 0.0)),
                (1, &side_offset),
                (2, &taper),
            ],
            &[body_group, start_group.clone(), face_group],
            &[],
            &[placement],
        )
        .expect("typed selected-face start Extrude");
        assert!(matches!(
            from_face,
            FeatureDefinition::Extrude {
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Native(ref id),
                    offset: None,
                },
                ..
            } if id == &start_group.id
        ));
    }

    #[test]
    fn edge_treatments_project_typed_dimensions_and_native_selections() {
        use cadmpeg_ir::features::{ChamferGroup, ChamferSpec, EdgeSelection, RadiusSpec};

        let parameter = |owner_record_index,
                         record_index,
                         source_kind: &str,
                         name: &str,
                         expression: &str,
                         value| {
            let mut parameter = parse_design_parameter(&parameter_record(
                Some(owner_record_index),
                expression,
                source_kind,
                Some("mm"),
                name,
                value,
            ))
            .expect("generated feature parameter is canonical");
            parameter.id = format!("f3d:native:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter.source_ordinal = record_index;
            parameter
        };
        let owner = |record_index, scope_record_index, parameter_record_index, local_ordinal| {
            let mut owner = parse_parameter_owner(&parameter_owner_frame())
                .expect("generated parameter owner is canonical");
            owner.id = format!("f3d:native:owner#{record_index}");
            owner.record_index = record_index;
            owner.scope_record_index = scope_record_index;
            owner.parameter_record_index = parameter_record_index;
            owner.companion_record_index = parameter_record_index + 1;
            owner.local_ordinal = local_ordinal;
            owner
        };
        let scope = |record_index, byte_offset, kind: &str| DesignParameterScope {
            id: format!("f3d:native:scope#{record_index}"),
            byte_offset,
            class_tag: "301".into(),
            record_index,
            frame_length: 200,
            kind: kind.into(),
            kind_offset: byte_offset + 100,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: byte_offset + 80,
            reference_members: vec![record_index + 1],
            reference_member_offsets: vec![byte_offset + 85],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: byte_offset + 200,
        };
        let scopes = [scope(12, 100, "Fillet"), scope(22, 400, "Chamfer")];
        let (features, _) = project_parameter_design(
            &[
                parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
                parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
                parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
            ],
            &[
                owner(44, 12, 45, 0),
                owner(54, 22, 55, 0),
                owner(64, 22, 65, 1),
            ],
            &scopes,
            &[],
            &[],
            &[],
            &[],
            &[],
        );

        let fillet = features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Fillet"))
            .expect("typed fillet");
        let FeatureDefinition::Fillet { groups } = &fillet.definition else {
            panic!("expected typed fillet");
        };
        assert!(matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Native(selection),
                radius: RadiusSpec::Constant { radius },
                tangency_weight: None,
            }] if selection == &scopes[0].id && radius.0 == 5.0
        ));
        let chamfer = features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Chamfer"))
            .expect("typed chamfer");
        assert!(matches!(
            &chamfer.definition,
            FeatureDefinition::Chamfer { groups }
                if matches!(groups.as_slice(), [ChamferGroup {
                    edges: EdgeSelection::Native(selection),
                    spec: ChamferSpec::TwoDistances { first, second },
                }] if selection == &scopes[1].id && first.0 == 1.0 && second.0 == 2.0)
        ));

        let construction_group =
            |record_index, scope_reference_ordinal| DesignConstructionOperandGroup {
                id: format!("f3d:native:construction-group#{record_index}"),
                scope_record_index: 22,
                scope_reference_ordinal,
                record_index,
                byte_offset: 1_000 + u64::from(scope_reference_ordinal),
                class_tag: "288".into(),
                member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
                members: vec![record_index + 100],
                lost_edge_references: Vec::new(),
                member_offsets: vec![1_026 + u64::from(scope_reference_ordinal)],
                identity_record_index: record_index + 1,
                identity_record_offset: 1_050 + u64::from(scope_reference_ordinal),
                role: 0x0000_0008_0000_0000,
                extrude_role: None,
                extrude_face_role: None,
                role_offset: 1_060 + u64::from(scope_reference_ordinal),
                opaque_index: 100,
                opaque_index_offset: 1_068 + u64::from(scope_reference_ordinal),
                opaque_scalar: 0.5,
                opaque_scalar_offset: 1_072 + u64::from(scope_reference_ordinal),
                variant: false,
                paired_class_tag: "259".into(),
                paired_byte_offset: 1_100 + u64::from(scope_reference_ordinal),
            };
        let mut construction_groups = [construction_group(90, 17), construction_group(80, 4)];
        construction_groups[1]
            .lost_edge_references
            .push("f3d:native:lost-edge-reference#1".into());
        let (features, _) = project_parameter_design(
            &[
                parameter(74, 75, "Distance", "d5", "2 mm", 0.2),
                parameter(84, 85, "Distance", "d4", "2.5 mm", 0.25),
            ],
            &[owner(74, 22, 75, 1), owner(84, 22, 85, 0)],
            &scopes[1..],
            &construction_groups,
            &[],
            &[],
            &[],
            &[],
        );
        assert!(matches!(
            &features[0].definition,
            FeatureDefinition::Chamfer { groups }
                if matches!(groups.as_slice(), [
                    ChamferGroup {
                        edges: EdgeSelection::Unresolved,
                        spec: ChamferSpec::Distance { distance: Length(2.5) },
                    },
                    ChamferGroup {
                        edges: EdgeSelection::Native(selection),
                        spec: ChamferSpec::Distance { distance: Length(2.0) },
                    },
                ] if selection == &construction_groups[0].id)
        ));
    }

    #[test]
    fn edge_recipe_candidate_intersection_must_be_uniquely_corroborated() {
        use crate::records::{
            DesignEdgeRecipeSelectorContext, DesignTopologyIncidentSide, DesignTopologyRecipeEntry,
            DesignTopologyRecipeTriplet,
        };

        let selector = |selector, edges: &[i64]| DesignEdgeRecipeSelectorContext {
            selector,
            clause_entries: [None, None],
            clause_triplet_edge_slots: [None, None],
            incidence_matching_edge_slots: edges.to_vec(),
            unique_incidence_edge_slot: (edges.len() == 1).then(|| edges[0]),
            boundary_count_matching_edge_slots: Vec::new(),
        };
        let selector_with_counts = |ordinal: i32, incidence: &[i64], counts: &[i64]| {
            let mut context = selector(ordinal, incidence);
            context.boundary_count_matching_edge_slots = counts.to_vec();
            context
        };
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector(0, &[17, 18]), selector(1, &[17, 19])],
                [&[17, 20][..], &[15, 17][..]],
            ),
            Some(17)
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector(0, &[17, 18]), selector(1, &[17, 18])],
                [&[17, 18][..]],
            ),
            None
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector(0, &[17]), selector(1, &[18])],
                [&[17, 18][..]],
            ),
            None
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector(0, &[17]), selector(1, &[])],
                [&[17][..]],
            ),
            None
        );
        assert_eq!(
            resolved_edge_candidate_intersection(&[selector(0, &[17])], [&[][..]]),
            None
        );
        assert_eq!(resolved_edge_candidate_intersection(&[], [&[17][..]]), None);
        assert_eq!(
            resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 19][..]]),
            Some(17)
        );
        assert_eq!(
            resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 18][..]]),
            None
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector(0, &[18])],
                [&[17, 18][..], &[17, 19][..]],
            ),
            Some(17)
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[
                    selector_with_counts(0, &[], &[17, 18]),
                    selector_with_counts(1, &[], &[17, 19]),
                ],
                [&[17, 20][..]],
            ),
            Some(17)
        );
        assert_eq!(
            resolved_edge_candidate_intersection(
                &[selector_with_counts(0, &[17], &[18])],
                [&[17, 18][..]],
            ),
            None
        );
        assert_eq!(
            super::edge_assignment_candidates(
                &[selector_with_counts(0, &[], &[17, 18])],
                [&[17][..]],
            ),
            Some(vec![17])
        );
        assert_eq!(
            super::edge_assignment_candidates(
                &[selector_with_counts(0, &[18], &[17, 18])],
                [&[17, 18][..]],
            ),
            Some(vec![18])
        );
        assert_eq!(
            super::edge_assignment_candidates(
                &[selector_with_counts(0, &[18], &[17, 18])],
                [&[17][..]],
            ),
            None
        );
        let assignment_candidates = [
            super::edge_assignment_candidates(
                &[selector_with_counts(0, &[], &[17, 18])],
                [&[17, 18][..]],
            )
            .unwrap(),
            super::edge_assignment_candidates(
                &[selector_with_counts(0, &[18], &[17, 18])],
                [&[17, 18][..]],
            )
            .unwrap(),
        ];
        assert_eq!(
            super::unique_bipartite_assignment(&assignment_candidates),
            Some(vec![17, 18])
        );
        let triplet = DesignTopologyRecipeTriplet {
            outer: std::num::NonZeroU32::new(3).unwrap(),
            middle: 2,
            vertex_ordinal: 2,
            incident_edge_ordinal: 1,
            incident_side: DesignTopologyIncidentSide::Preceding,
        };
        let mut common = selector(0, &[]);
        common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
            selector: 0,
            boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
            topology_triplets: [triplet.clone(), triplet.clone()],
            common_incident_edge_ordinal: Some(1),
        });
        common.clause_triplet_edge_slots[0] = Some([vec![17, 18], vec![17]]);
        assert_eq!(
            resolved_edge_candidate_intersection(&[common], [&[17, 18][..]]),
            Some(17)
        );
        let mut common = selector(0, &[]);
        common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
            selector: 0,
            boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
            topology_triplets: [triplet.clone(), triplet],
            common_incident_edge_ordinal: Some(1),
        });
        common.clause_triplet_edge_slots[0] = Some([vec![17, 18, 19], vec![17, 18]]);
        assert_eq!(
            resolved_edge_candidate_intersection(&[common.clone()], [&[17][..]]),
            Some(17)
        );
        assert_eq!(
            resolved_edge_candidate_intersection(&[common], [&[19][..]]),
            None
        );
    }

    #[test]
    fn edge_group_cardinality_resolves_one_common_deleted_candidate_set() {
        assert_eq!(
            super::common_deleted_edge_group_candidates(
                [&[19, 17, 18, 17][..], &[18, 19, 17][..], &[17, 18, 19][..]],
                3,
            ),
            Some(vec![17, 18, 19])
        );
        assert_eq!(
            super::common_deleted_edge_group_candidates(
                [&[17, 18, 19][..], &[17, 18][..], &[17, 18, 19][..]],
                3,
            ),
            None
        );
        assert_eq!(
            super::common_deleted_edge_group_candidates([&[17, 18, 19][..], &[17, 18, 19][..]], 2,),
            None
        );
        assert_eq!(
            super::common_deleted_edge_group_candidates(std::iter::empty::<&[i64]>(), 0),
            None
        );
        let deleted = vec![17, 18, 19, 20];
        let groups = vec![
            vec![
                (10, Some(17), deleted.clone()),
                (11, Some(19), deleted.clone()),
            ],
            vec![(12, None, deleted.clone()), (13, None, deleted.clone())],
        ];
        assert_eq!(
            super::partition_unique_incomplete_edge_group(1, &groups),
            Some(vec![18, 20])
        );
        assert_eq!(
            super::partition_unique_incomplete_edge_group(0, &groups),
            None
        );
        let mut two_incomplete = groups.clone();
        two_incomplete[0][0].1 = None;
        assert_eq!(
            super::partition_unique_incomplete_edge_group(1, &two_incomplete),
            None
        );
        let mut duplicate_identity = groups;
        duplicate_identity[1][0].0 = 11;
        assert_eq!(
            super::partition_unique_incomplete_edge_group(1, &duplicate_identity),
            None
        );
    }

    #[test]
    fn edge_group_ignores_members_without_changed_edge_candidates() {
        assert_eq!(
            super::context_only_edge_group_candidates([
                (None, &[][..]),
                (Some(17), &[17, 18][..]),
                (Some(17), &[17][..]),
                (None, &[][..]),
            ]),
            Some(vec![17])
        );
        assert_eq!(
            super::context_only_edge_group_candidates([(Some(17), &[17][..]), (None, &[18][..]),]),
            None
        );
        assert_eq!(
            super::context_only_edge_group_candidates([(None, &[][..])]),
            None
        );
    }

    #[test]
    fn edge_group_resolves_only_one_perfect_candidate_assignment() {
        assert_eq!(
            super::unique_bipartite_assignment(&[vec![17, 18], vec![18, 19], vec![19],]),
            Some(vec![17, 18, 19])
        );
        assert_eq!(
            super::unique_bipartite_assignment(&[vec![17, 18], vec![17, 18]]),
            None
        );
        assert_eq!(
            super::unique_bipartite_assignment(&[vec![17], vec![17]]),
            None
        );
        assert_eq!(
            super::unique_bipartite_assignment(&[vec![17], Vec::new()]),
            None
        );
        assert_eq!(super::unique_bipartite_assignment(&[]), None);
        assert_eq!(
            super::unique_edge_assignment_with_context(
                &[Some(vec![17, 18]), None, Some(vec![18]),]
            ),
            Some(vec![17, 18])
        );
        assert_eq!(
            super::unique_edge_assignment_with_context(&[None, None]),
            None
        );
    }

    #[test]
    fn fillet_radius_parameters_pair_with_counted_edge_groups_in_order() {
        let scope = DesignParameterScope {
            id: "f3d:native:scope#12".into(),
            byte_offset: 100,
            class_tag: "301".into(),
            record_index: 12,
            frame_length: 200,
            kind: "Fillet".into(),
            kind_offset: 210,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 180,
            reference_members: vec![100, 101],
            reference_member_offsets: vec![185, 196],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: 300,
        };
        let group = |record_index, ordinal, members: Vec<u32>| DesignConstructionOperandGroup {
            id: format!("f3d:native:construction-group#{record_index}"),
            scope_record_index: 12,
            scope_reference_ordinal: ordinal,
            record_index,
            byte_offset: 1000 + u64::from(ordinal) * 200,
            class_tag: "288".into(),
            member_count_offset: 1021 + u64::from(ordinal) * 200,
            member_offsets: (0..members.len())
                .map(|index| 1026 + u64::from(ordinal) * 200 + index as u64 * 11)
                .collect(),
            members,
            lost_edge_references: Vec::new(),
            identity_record_index: 300 + ordinal,
            identity_record_offset: 1100 + u64::from(ordinal) * 200,
            role: 0x0000_0008_0000_0000,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 1110 + u64::from(ordinal) * 200,
            opaque_index: 100,
            opaque_index_offset: 1128 + u64::from(ordinal) * 200,
            opaque_scalar: 0.5,
            opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
            variant: false,
            paired_class_tag: "259".into(),
            paired_byte_offset: 1200 + u64::from(ordinal) * 200,
        };
        let mut operand_groups = [group(100, 0, vec![200]), group(101, 1, vec![201, 202])];
        let parameter = |owner_index, record_index, source_kind: &str, unit, value| {
            let mut parameter = parse_design_parameter(&parameter_record(
                Some(owner_index),
                "value",
                source_kind,
                unit,
                "d1",
                value,
            ))
            .expect("canonical Fillet parameter");
            parameter.id = format!("f3d:native:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter
        };
        let owner = |record_index, parameter_record_index, local_ordinal| {
            let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
            owner.id = format!("f3d:native:owner#{record_index}");
            owner.record_index = record_index;
            owner.scope_record_index = 12;
            owner.parameter_record_index = parameter_record_index;
            owner.local_ordinal = local_ordinal;
            owner
        };
        let parameters = [
            parameter(10, 11, "Radius", Some("mm"), 0.5),
            parameter(20, 21, "Radius", Some("mm"), 0.3),
            parameter(30, 31, "TangencyWeight", None, 1.0),
            parameter(40, 41, "TangencyWeight", None, 0.75),
        ];
        let owners = [
            owner(10, 11, 0),
            owner(20, 21, 1),
            owner(30, 31, 2),
            owner(40, 41, 3),
        ];

        let assignments = decode_fillet_radius_groups(
            std::slice::from_ref(&scope),
            &operand_groups,
            &owners,
            &parameters,
        );
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].edge_operand_record_indices, [200]);
        assert_eq!(assignments[0].radius_parameter_record_index, 11);
        assert_eq!(
            assignments[0].tangency_weight_parameter_record_index,
            Some(31)
        );
        assert_eq!(assignments[1].edge_operand_record_indices, [201, 202]);
        assert_eq!(assignments[1].radius_parameter_record_index, 21);
        assert_eq!(
            assignments[1].tangency_weight_parameter_record_index,
            Some(41)
        );
        operand_groups[0]
            .lost_edge_references
            .push("f3d:native:lost-edge-reference#1".into());

        let (features, _) = project_parameter_design(
            &parameters,
            &owners,
            std::slice::from_ref(&scope),
            &operand_groups,
            &assignments,
            &[],
            &[],
            &[],
        );
        let FeatureDefinition::Fillet { groups } = &features[0].definition else {
            panic!("expected typed Fillet");
        };
        assert_eq!(groups.len(), 2);
        assert!(matches!(
            &groups[0],
            cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                radius: cadmpeg_ir::features::RadiusSpec::Constant {
                    radius: cadmpeg_ir::features::Length(5.0),
                },
                tangency_weight: Some(1.0),
            }
        ));
        assert!(matches!(
            &groups[1],
            cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
                radius: cadmpeg_ir::features::RadiusSpec::Constant {
                    radius: cadmpeg_ir::features::Length(3.0),
                },
                tangency_weight: Some(0.75),
            } if selection == &operand_groups[1].id
        ));
    }

    #[test]
    fn parameter_expressions_project_feature_dependencies() {
        let parameter = |owner_record_index, record_index, name: &str, expression: &str| {
            let mut parameter = parse_design_parameter(&parameter_record(
                Some(owner_record_index),
                expression,
                "AlongDistance",
                Some("mm"),
                name,
                1.0,
            ))
            .expect("generated owned parameter is canonical");
            parameter.id = format!("f3d:native:parameter#{record_index}");
            parameter.record_index = record_index;
            parameter.source_ordinal = record_index;
            parameter
        };
        let owner = |record_index, scope_record_index, parameter_record_index| {
            let mut owner = parse_parameter_owner(&parameter_owner_frame())
                .expect("generated parameter owner is canonical");
            owner.id = format!("f3d:native:owner#{record_index}");
            owner.record_index = record_index;
            owner.scope_record_index = scope_record_index;
            owner.parameter_record_index = parameter_record_index;
            owner.companion_record_index = parameter_record_index + 1;
            owner
        };
        let scope = |record_index, byte_offset, kind: &str| DesignParameterScope {
            id: format!("f3d:native:scope#{record_index}"),
            byte_offset,
            class_tag: "301".into(),
            record_index,
            frame_length: 200,
            kind: kind.into(),
            kind_offset: byte_offset + 100,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: byte_offset + 80,
            reference_members: vec![record_index + 1],
            reference_member_offsets: vec![byte_offset + 85],
            extrude_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "261".into(),
            paired_byte_offset: byte_offset + 200,
        };
        let (features, parameters) = project_parameter_design(
            &[
                parameter(44, 45, "Width", "10 mm"),
                parameter(54, 55, "Depth", "Width / 2"),
                parameter(74, 75, "Premature", "Future / 2"),
                parameter(84, 85, "Future", "20 mm"),
            ],
            &[
                owner(44, 12, 45),
                owner(54, 22, 55),
                owner(74, 22, 75),
                owner(84, 32, 85),
            ],
            &[
                scope(12, 100, "Sketch"),
                scope(22, 200, "Extrude"),
                scope(32, 300, "Fillet"),
            ],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let width = parameters
            .iter()
            .find(|parameter| parameter.name == "Width")
            .expect("Width parameter");
        let depth = parameters
            .iter()
            .find(|parameter| parameter.name == "Depth")
            .expect("Depth parameter");
        assert_eq!(depth.dependencies, std::slice::from_ref(&width.id));
        let premature = parameters
            .iter()
            .find(|parameter| parameter.name == "Premature")
            .expect("Premature parameter");
        assert!(premature.dependencies.is_empty());
        let source = features
            .iter()
            .find(|feature| feature.id == width.owner.clone().expect("Width owner"))
            .expect("source feature");
        let target = features
            .iter()
            .find(|feature| feature.id == depth.owner.clone().expect("Depth owner"))
            .expect("target feature");
        assert_eq!(target.dependencies, std::slice::from_ref(&source.id));
    }

    #[test]
    fn history_state_identity_orders_cross_family_feature_dependencies() {
        let scope =
            |record_index, byte_offset, kind: &str, current, previous| DesignParameterScope {
                id: format!("f3d:native:scope#{record_index}"),
                byte_offset,
                class_tag: "301".into(),
                record_index,
                frame_length: 200,
                kind: kind.into(),
                kind_offset: byte_offset + 100,
                extrude_operation: None,
                extrude_operation_offset: None,
                extrude_extent: None,
                extrude_extent_offsets: None,
                extrude_direction_reversed: None,
                extrude_direction_reversed_offset: None,
                extrude_start: None,
                extrude_start_offset: None,
                feature_ordinal: 1,
                feature_ordinal_offset: 0,
                history_state_id: current,
                history_state_id_offset: byte_offset + 60,
                previous_history_state_id: previous,
                previous_history_state_id_offset: byte_offset + 120,
                reference_count_offset: byte_offset + 80,
                reference_members: Vec::new(),
                reference_member_offsets: Vec::new(),
                extrude_profile: None,
                entity_id: None,
                entity_suffix: None,
                entity_reference_offset: None,
                paired_class_tag: "261".into(),
                paired_byte_offset: byte_offset + 200,
            };
        let predecessor = scope(12, 200, "Fillet", Some(10), Some(9));
        let successor = scope(22, 100, "Chamfer", Some(11), Some(10));
        let (features, _) =
            project_parameter_design(&[], &[], &[successor, predecessor], &[], &[], &[], &[], &[]);
        let predecessor = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#12"))
            .expect("predecessor feature");
        let successor = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#22"))
            .expect("successor feature");
        assert_eq!(successor.dependencies, [predecessor.id.clone()]);
        assert!(predecessor.ordinal < successor.ordinal);
    }

    #[test]
    fn variable_width_relation_uses_counted_runs_and_next_record_boundary() {
        let mut record = vec![0u8; 127];
        record[0..4].copy_from_slice(&3u32.to_le_bytes());
        record[4..7].copy_from_slice(b"286");
        record[7..11].copy_from_slice(&1239u32.to_le_bytes());
        record[19] = 1;
        record[20..24].copy_from_slice(&3u32.to_le_bytes());
        for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236), (65, 0), (70, 1041)] {
            record[marker] = 1;
            record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
        }
        record[82..86].copy_from_slice(&4u32.to_le_bytes());
        record[89..93].copy_from_slice(&3u32.to_le_bytes());
        for (marker, reference) in [(93, 1224u32), (104, 1228), (115, 1236)] {
            record[marker] = 1;
            record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
        }
        let mut bytes = record.clone();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"277");
        bytes.extend_from_slice(&1240u32.to_le_bytes());

        assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
        let parsed = parse_sketch_relation(&record, &HashSet::from([1041])).unwrap();
        assert_eq!(parsed.0, [1224, 1228, 1236]);
        assert_eq!(parsed.2, [0]);
        assert_eq!(parsed.4, 1041);
        assert_eq!(parsed.6, 4);
        assert_eq!(parsed.8, [1224, 1228, 1236]);
        assert_eq!(parsed.10, 120);
    }

    #[test]
    fn repeated_linear_dimension_requires_disjoint_measurement_pairs() {
        use cadmpeg_ir::features::ParameterId;
        use cadmpeg_ir::sketches::{
            SketchConstraintDefinition as Definition, SketchDistanceMeasurement as Measurement,
            SketchEntityId, SketchLocus,
        };

        let entity = |name: &str| SketchEntityId(format!("generated:{name}"));
        let parameter = ParameterId("generated:distance".into());
        let horizontal = |first: &str, second: &str| Definition::HorizontalDistance {
            first: SketchLocus::Entity(entity(first)),
            second: SketchLocus::Entity(entity(second)),
            parameter: parameter.clone(),
        };
        let candidates = vec![horizontal("a", "b"), horizontal("c", "d")];
        let Definition::RepeatedDistance {
            measurements,
            parameter: actual,
        } = repeated_linear_dimension(&candidates, parameter.clone()).unwrap()
        else {
            panic!("expected repeated distance")
        };
        assert_eq!(actual, parameter);
        assert!(matches!(
            measurements.as_slice(),
            [
                Measurement::Horizontal { .. },
                Measurement::Horizontal { .. }
            ]
        ));

        let ambiguous = vec![horizontal("a", "b"), horizontal("a", "c")];
        assert!(repeated_linear_dimension(&ambiguous, parameter).is_none());
    }
}
