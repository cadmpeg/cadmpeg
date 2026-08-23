//! Parameter scalar and compact selection projection.

use super::component_paths::{
    compact_body_selection_value, compact_edge_path_value, compact_edge_selection_set_value,
    component_path_feature, component_path_terminal_feature, ComponentPathEnd,
};
use super::drafts::{draft_operand_candidates, same_draft_operands, DraftAnchor, DraftOperands};
use super::holes::feature_object_byte_ranges;
use super::is_class_token;
use super::parameters::value_only_scalar_offset;
use super::relation_geometry::{
    owned_relation_parameters, relation_display_scalar, RELATION_DISPLAY_SCALAR_ID_PROPERTY,
    RELATION_PARAMETER_ID_PROPERTY, RELATION_PARAMETER_ROLE_PROPERTY,
    RELATION_PARAMETER_ROLE_REFERENCE,
};
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::selections::{
    cosmetic_thread_cylinder_marker_reference, variable_fillet_control_references,
    variable_fillet_dimension_index_for_feature,
};
use super::terminations::compact_surface_selection_value;
use crate::records::{
    FeatureInputBodySelection, FeatureInputEdgeSelection, FeatureInputLane,
    FeatureInputRelationFamily, FeatureInputScalarRole, FeatureInputSurfaceSelection,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{
    Angle, BodySelection, DesignParameter, DimensionDisplay, EdgeSelection, FaceSelection,
    FeatureDefinition, FilletGroup, Length, ParameterId, ParameterValue, PatternSeed, RadiusSpec,
    VariableRadius,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry};
use cadmpeg_ir::topology::Face;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) fn bind_circular_profile_by_dimension(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) {
    let geometry_by_entity = sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let circular_profiles = sketches
        .iter()
        .filter_map(|sketch| {
            let [profile] = sketch.profiles.as_slice() else {
                return None;
            };
            let [entity] = profile.as_slice() else {
                return None;
            };
            let SketchGeometry::Circle { radius, .. } = geometry_by_entity.get(&entity.entity)?
            else {
                return None;
            };
            Some((sketch.id.clone(), radius.0))
        })
        .collect::<Vec<_>>();
    let mut proposals = Vec::new();
    for (sketch, radius) in circular_profiles {
        let matches = features
            .iter()
            .enumerate()
            .filter(|(_, feature)| {
                matches!(
                    feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
                )
            })
            .filter(|(_, feature)| {
                parameters.iter().any(|parameter| {
                    if parameter.owner.as_ref() != Some(&feature.id) {
                        return false;
                    }
                    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                        &parameter.value
                    else {
                        return false;
                    };
                    let expected = match parameter.display {
                        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                        None => return false,
                    };
                    same_dimension_length(expected, radius)
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if let [feature] = matches.as_slice() {
            proposals.push((sketch, *feature));
        }
    }
    let mut feature_counts = HashMap::new();
    for (_, feature) in &proposals {
        *feature_counts.entry(*feature).or_insert(0usize) += 1;
    }
    for (sketch_id, feature_index) in proposals {
        if feature_counts.get(&feature_index) != Some(&1) {
            continue;
        }
        for feature in features.iter_mut() {
            let cadmpeg_ir::features::FeatureDefinition::Sketch { sketch: bound, .. } =
                &mut feature.definition
            else {
                continue;
            };
            if bound.as_ref() == Some(&sketch_id) {
                *bound = None;
            }
        }
        let name = features[feature_index].name.clone();
        let cadmpeg_ir::features::FeatureDefinition::Sketch { sketch, .. } =
            &mut features[feature_index].definition
        else {
            continue;
        };
        *sketch = Some(sketch_id.clone());
        if let Some(native) = sketches.iter_mut().find(|sketch| sketch.id == sketch_id) {
            native.name = name;
        }
    }
}

/// Bind neutral parameters to uniquely owned native scalar records.
pub(crate) fn bind_parameter_scalars<'a>(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) {
    let neutral_owners = features
        .iter()
        .filter_map(|feature| Some((&feature.id, feature.native_ref.as_deref()?)))
        .collect::<HashMap<_, _>>();
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let length_scalars = lane
            .relation_instances
            .iter()
            .filter(|relation| relation.family != FeatureInputRelationFamily::Angle)
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .collect::<HashSet<_>>();
        let angle_scalars = lane
            .relation_instances
            .iter()
            .filter(|relation| relation.family == FeatureInputRelationFamily::Angle)
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .collect::<HashSet<_>>();
        let detached_scalars = lane
            .relation_instances
            .iter()
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .filter(|id| {
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == **id)
                    .is_some_and(|scalar| scalar.operands.is_empty())
            })
            .collect::<HashSet<_>>();
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let start = feature_object_name(feature, lane).map_or(u64::MAX, |name| name.offset);
            starts.push((start, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let owner_parameters = parameters.iter_mut().filter(|parameter| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| neutral_owners.get(owner))
                    .copied()
                    == Some(native_feature.id.as_str())
            });
            for parameter in owner_parameters {
                if parameter.native_ref.is_some() {
                    continue;
                }
                let scalars = lane
                    .scalars
                    .iter()
                    .filter(|scalar| match scalar.feature_ref.as_deref() {
                        Some(owner) => owner == native_feature.id,
                        None => scalar.offset > start && scalar.offset < end,
                    })
                    .filter(|scalar| {
                        names_by_id.get(scalar.name.as_str()).is_some_and(|name| {
                            name.value == parameter.name
                                && value_only_scalar_offset(&lane.native_payload, name)
                                    != usize::try_from(scalar.offset).ok()
                        })
                    })
                    .collect::<Vec<_>>();
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                let compatible = candidates
                    .into_iter()
                    .filter(|scalar| match parameter.value.as_ref() {
                        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
                            let Some(expected) = crate::history::exact_integer_f64(*expected)
                            else {
                                return false;
                            };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value * 1000.0, expected)
                            } else {
                                scalar.value == expected
                            }
                        }
                        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
                            let expected = if *expected { 1.0 } else { 0.0 };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value * 1000.0, expected)
                            } else {
                                scalar.value == expected
                            }
                        }
                        _ => true,
                    })
                    .collect::<Vec<_>>();
                if let [scalar] = compatible.as_slice() {
                    parameter.native_ref = Some(scalar.id.clone());
                    let scalar_is_detached = detached_scalars.contains(scalar.id.as_str());
                    let scalar_is_untyped_real = matches!(
                        parameter.value,
                        Some(cadmpeg_ir::features::ParameterValue::Real(_))
                    ) && !scalar_is_detached;
                    if scalar_is_detached && length_scalars.contains(scalar.id.as_str()) {
                        parameter.expression =
                            crate::history::format_length_mm(scalar.value * 1000.0);
                    } else if scalar_is_detached && angle_scalars.contains(scalar.id.as_str()) {
                        parameter.expression = crate::history::format_angle_rad(scalar.value);
                    }
                    let evaluated = if length_scalars.contains(scalar.id.as_str())
                        && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Length(
                            cadmpeg_ir::features::Length(scalar.value * 1000.0),
                        ))
                    } else if angle_scalars.contains(scalar.id.as_str()) && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Angle(
                            cadmpeg_ir::features::Angle(scalar.value),
                        ))
                    } else {
                        match parameter.value.as_ref() {
                            Some(cadmpeg_ir::features::ParameterValue::Length(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Length(
                                    cadmpeg_ir::features::Length(scalar.value * 1000.0),
                                ))
                            }
                            Some(cadmpeg_ir::features::ParameterValue::Angle(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Angle(
                                    cadmpeg_ir::features::Angle(scalar.value),
                                ))
                            }
                            Some(cadmpeg_ir::features::ParameterValue::Real(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Real(scalar.value))
                            }
                            _ => None,
                        }
                    };
                    if let Some(evaluated) = evaluated {
                        parameter.value = Some(evaluated);
                    }
                }
            }
        }
    }
}

/// Materialize evaluated relation dimensions that have no driving scalar.
///
/// A display scalar is a measurement, not a writable native parameter. Keep
/// its relation and scalar identities in parameter properties so later
/// relation projection can join the derived value without assigning a
/// display record to `native_ref`.
pub(crate) fn synthesize_display_relation_parameters<'a>(
    parameters: &mut Vec<DesignParameter>,
    features: &[cadmpeg_ir::features::Feature],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) {
    let lanes = lanes.into_iter().collect::<Vec<_>>();
    let owned = owned_relation_parameters(features, parameters, lanes.iter().copied());
    let features_by_native_ref = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();
    let mut relation_ids = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .properties
                .get(RELATION_PARAMETER_ID_PROPERTY)
                .cloned()
        })
        .collect::<HashSet<_>>();
    let mut parameter_ids = parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<HashSet<_>>();
    let mut names_by_owner = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.owner.clone()?, parameter.name.clone())))
        .collect::<HashSet<_>>();
    let mut next_ordinals = parameters.iter().fold(
        HashMap::<cadmpeg_ir::features::FeatureId, u32>::new(),
        |mut ordinals, parameter| {
            let Some(owner) = parameter.owner.clone() else {
                return ordinals;
            };
            let next = parameter.ordinal.saturating_add(1);
            ordinals
                .entry(owner)
                .and_modify(|current| *current = (*current).max(next))
                .or_insert(next);
            ordinals
        },
    );

    for lane in lanes {
        for relation in &lane.relation_instances {
            if relation.parameter_scalar_ref.is_some()
                || owned.get(&relation.id).is_some_and(Option::is_some)
                || relation_ids.contains(&relation.id)
            {
                continue;
            }
            let Some(scalar) = relation_display_scalar(relation, lane) else {
                continue;
            };
            if !scalar.value.is_finite() {
                continue;
            }
            let Some(feature) = features_by_native_ref.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let Some(source_name) = lane
                .names
                .iter()
                .find(|name| name.id == scalar.name)
                .map(|name| name.value.as_str())
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let (value, display, expression) =
                relation_display_parameter_value(relation.family, scalar.value);
            let owner = feature.id.clone();
            let ordinal = next_ordinals.entry(owner.clone()).or_insert(0);
            let current_ordinal = *ordinal;
            *ordinal = match current_ordinal.checked_add(1) {
                Some(next) => next,
                None => continue,
            };
            let base_name = format!("{source_name}@reference");
            let mut name = base_name.clone();
            if names_by_owner.contains(&(owner.clone(), name.clone())) {
                name = format!("{base_name}:{}", relation.offset);
                let mut suffix = 0u32;
                while names_by_owner.contains(&(owner.clone(), name.clone())) {
                    suffix = suffix.saturating_add(1);
                    name = format!("{base_name}:{}:{suffix}", relation.offset);
                }
            }
            let id = ParameterId(format!("sldprt:model:parameter#reference:{}", relation.id));
            if !parameter_ids.insert(id.clone()) {
                continue;
            }
            let mut properties = BTreeMap::new();
            properties.insert(RELATION_PARAMETER_ID_PROPERTY.into(), relation.id.clone());
            properties.insert(
                RELATION_DISPLAY_SCALAR_ID_PROPERTY.into(),
                scalar.id.clone(),
            );
            properties.insert(
                RELATION_PARAMETER_ROLE_PROPERTY.into(),
                RELATION_PARAMETER_ROLE_REFERENCE.into(),
            );
            properties.insert("source_name".into(), source_name.into());
            parameters.push(DesignParameter {
                id,
                owner: Some(owner.clone()),
                ordinal: current_ordinal,
                name: name.clone(),
                expression,
                display,
                value: Some(value),
                dependencies: Vec::new(),
                properties,
                pmi: None,
                native_ref: None,
            });
            names_by_owner.insert((owner, name));
            relation_ids.insert(relation.id.clone());
        }
    }
}

fn relation_display_parameter_value(
    family: FeatureInputRelationFamily,
    value: f64,
) -> (ParameterValue, Option<DimensionDisplay>, String) {
    match family {
        FeatureInputRelationFamily::Angle => (
            ParameterValue::Angle(Angle(value)),
            None,
            crate::history::format_angle_rad(value),
        ),
        FeatureInputRelationFamily::CircleDiameter => {
            let millimetres = value * 1000.0;
            (
                ParameterValue::Length(Length(millimetres)),
                Some(DimensionDisplay::Diameter),
                format!(
                    "<MOD-DIAM>{}",
                    crate::history::format_length_mm(millimetres)
                ),
            )
        }
        FeatureInputRelationFamily::LineLineDistance
        | FeatureInputRelationFamily::PointPointDistance
        | FeatureInputRelationFamily::PointLineDistance
        | FeatureInputRelationFamily::PointPointHorizontalDistance
        | FeatureInputRelationFamily::PointPointVerticalDistance => {
            let millimetres = value * 1000.0;
            (
                ParameterValue::Length(Length(millimetres)),
                None,
                crate::history::format_length_mm(millimetres),
            )
        }
    }
}

/// Apply relation-defined units and display semantics to parameters named by display scalars.
pub(crate) fn type_display_relation_parameters(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let mut families = HashMap::<cadmpeg_ir::features::ParameterId, HashSet<_>>::new();
    for relation in lanes.iter().flat_map(|lane| &lane.relation_instances) {
        if let Some(Some(parameter)) = ownership.get(&relation.id) {
            families
                .entry(parameter.clone())
                .or_default()
                .insert(relation.family);
        }
    }
    for parameter in parameters {
        let Some(families) = families.get(&parameter.id) else {
            continue;
        };
        if families.len() != 1 {
            continue;
        }
        let family = *families.iter().next().expect("one relation family");
        match family {
            FeatureInputRelationFamily::Angle => {
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    parameter.expression = crate::history::format_angle_rad(value);
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Angle(
                        cadmpeg_ir::features::Angle(value),
                    ));
                }
            }
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => {
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    let value = value * 1000.0;
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!("<MOD-DIAM>{}", crate::history::format_length_mm(value))
                    } else {
                        crate::history::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                        cadmpeg_ir::features::Length(value),
                    ));
                }
                if let Some(cadmpeg_ir::features::ParameterValue::Integer(value)) =
                    parameter.value.as_ref()
                {
                    let Some(value) = crate::history::exact_integer_f64(*value) else {
                        continue;
                    };
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!("<MOD-DIAM>{}", crate::history::format_length_mm(value))
                    } else {
                        crate::history::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                        cadmpeg_ir::features::Length(value),
                    ));
                }
                if family == FeatureInputRelationFamily::CircleDiameter
                    && matches!(
                        parameter.value,
                        Some(cadmpeg_ir::features::ParameterValue::Length(_))
                    )
                    && parameter.display.is_none()
                {
                    parameter.display = Some(cadmpeg_ir::features::DimensionDisplay::Diameter);
                }
            }
        }
    }
}

pub(crate) fn project_compact_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    let selections = lanes.iter().flat_map(|lane| &lane.body_selections).fold(
        HashMap::<&str, Vec<&FeatureInputBodySelection>>::new(),
        |mut by_feature, selection| {
            by_feature
                .entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            by_feature
        },
    );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let (bodies, mode) = match &mut feature.definition {
            FeatureDefinition::DeleteBody { bodies, mode } => (bodies, Some(mode)),
            FeatureDefinition::MoveBody { bodies, .. } => (bodies, None),
            _ => continue,
        };
        if matches!(bodies, cadmpeg_ir::features::BodySelection::Unresolved) {
            *bodies = cadmpeg_ir::features::BodySelection::Local {
                bodies: selection
                    .local_body_ids
                    .iter()
                    .map(u32::to_string)
                    .collect(),
                native: compact_body_selection_value(&selection.local_body_ids),
            };
        }
        if mode
            .as_deref()
            .is_some_and(|mode| matches!(mode, cadmpeg_ir::features::BodyRetentionMode::Unresolved))
        {
            if let Some(native_mode) = selection.mode {
                *mode.expect("delete-body mode") = native_mode;
            }
        }
    }
}

pub(crate) fn project_compact_edge_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let selections = lanes.iter().flat_map(|lane| &lane.edge_selections).fold(
        HashMap::<&str, Vec<&FeatureInputEdgeSelection>>::new(),
        |mut by_feature, selection| {
            by_feature
                .entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            by_feature
        },
    );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(edge_selections) = selections
            .get(native_ref)
            .filter(|selections| !selections.is_empty())
        else {
            continue;
        };
        let projected_edges = |selections: &[&FeatureInputEdgeSelection]| {
            let native = compact_edge_selection_set_value(selections);
            let generated = selections.iter().try_fold(
                Vec::<cadmpeg_ir::features::GeneratedEdgeRef>::new(),
                |mut edges, selection| {
                    let native_feature = selection.terminal_feature_ref.as_ref()?;
                    let feature = feature_ids_by_native.get(native_feature)?.clone();
                    let local_id = compact_edge_path_value(selection);
                    let edge = cadmpeg_ir::features::GeneratedEdgeRef { feature, local_id };
                    if !edges.contains(&edge) {
                        edges.push(edge);
                    }
                    Some(edges)
                },
            );
            match generated.filter(|edges| !edges.is_empty()) {
                Some(edges) => EdgeSelection::Generated { edges, native },
                None => EdgeSelection::Native(native),
            }
        };
        let unresolved_variable_fillet = matches!(
            &feature.definition,
            FeatureDefinition::Fillet { groups }
                if matches!(groups.as_slice(), [FilletGroup {
                    radius: RadiusSpec::Unresolved { .. },
                    ..
                }])
        );
        if unresolved_variable_fillet {
            if let Some(radius_groups) =
                variable_fillet_radius_groups(native_ref, histories, lanes, edge_selections)
            {
                let FeatureDefinition::Fillet { groups } = &mut feature.definition else {
                    unreachable!("checked fillet definition")
                };
                let [group] = groups.as_slice() else {
                    unreachable!("checked one fillet group")
                };
                let existing_edges = group.edges.clone();
                if matches!(&existing_edges, EdgeSelection::Unresolved) || radius_groups.len() == 1
                {
                    let tangency_weight = group.tangency_weight;
                    *groups = radius_groups
                        .into_iter()
                        .map(|(radius, selections)| FilletGroup {
                            edges: if matches!(&existing_edges, EdgeSelection::Unresolved) {
                                projected_edges(&selections)
                            } else {
                                existing_edges.clone()
                            },
                            radius,
                            tangency_weight,
                        })
                        .collect();
                }
            }
        }
        let groups = match &mut feature.definition {
            FeatureDefinition::Fillet { groups } => groups
                .iter_mut()
                .filter(|group| matches!(group.edges, EdgeSelection::Unresolved))
                .map(|group| &mut group.edges)
                .collect::<Vec<_>>(),
            FeatureDefinition::Chamfer { groups, .. } => groups
                .iter_mut()
                .map(|group| &mut group.edges)
                .collect::<Vec<_>>(),
            _ => continue,
        };
        for edges in groups {
            *edges = projected_edges(edge_selections);
        }
        for dependency in edge_selections
            .iter()
            .flat_map(|selection| &selection.producer_feature_refs)
            .filter_map(|native| feature_ids_by_native.get(native))
        {
            if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
}

fn variable_fillet_radius_groups<'a>(
    feature_ref: &str,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    selections: &[&'a FeatureInputEdgeSelection],
) -> Option<Vec<(RadiusSpec, Vec<&'a FeatureInputEdgeSelection>)>> {
    let history = histories.iter().find(|history| {
        history
            .features
            .iter()
            .any(|feature| feature.id == feature_ref)
    })?;
    let feature = history.features.iter().find(|feature| {
        feature.id == feature_ref && feature.kind.eq_ignore_ascii_case("VarFillet")
    })?;
    let parameter_names = feature
        .parameters
        .keys()
        .filter(|name| variable_fillet_dimension_index_for_feature(feature, name).is_some())
        .collect::<HashSet<_>>();
    if parameter_names.len() != feature.parameters.len() || parameter_names.len() < 2 {
        return None;
    }

    // A legacy VarFillet with exactly the two ordered controls 0 and 1 may
    // omit endpoint markers entirely. Its three-reference edge-control
    // roster supplies one feature-wide radius profile for every selected edge.
    // Require that roster shape and reject any endpoint-bearing reference so a
    // feature with several endpoint-specific profiles cannot enter this path.
    let has_legacy_edge_control_roster = selections.iter().any(|selection| {
        selection.references.len() == 3
            && selection.local_edge_ids.len() == selection.references.len()
            && selection
                .references
                .iter()
                .zip(&selection.local_edge_ids)
                .all(|(reference, local_id)| {
                    let [component] = reference.as_slice() else {
                        return false;
                    };
                    component.local_id == Some(*local_id)
                })
    });
    let has_endpoint_reference = selections
        .iter()
        .flat_map(|selection| selection.references.iter())
        .flat_map(|reference| reference.iter())
        .any(|component| component.instance == Some(0x8083));
    if parameter_names.len() == 2 && has_legacy_edge_control_roster && !has_endpoint_reference {
        let mut ordered_parameters = parameter_names
            .iter()
            .map(|name| {
                variable_fillet_dimension_index_for_feature(feature, name).zip(
                    feature.parameters.get(*name).and_then(|value| {
                        crate::history::parse_positive_dimension_length_mm(value)
                    }),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        ordered_parameters.sort_unstable_by_key(|(index, _)| *index);
        if ordered_parameters
            .iter()
            .enumerate()
            .all(|(expected, (actual, _))| expected == *actual)
        {
            let mut selections = selections.to_vec();
            selections.sort_unstable_by_key(|selection| selection.ordinal);
            let points = ordered_parameters
                .into_iter()
                .enumerate()
                .map(|(parameter, (_, radius))| VariableRadius {
                    parameter: parameter as f64,
                    radius: Length(radius),
                })
                .collect();
            return Some(vec![(RadiusSpec::Variable { points }, selections)]);
        }
    }

    let mut vertex_radii = HashMap::<[u8; 12], f64>::new();
    let mut control_names = HashSet::<String>::new();
    let mut non_vertex_control_names = HashSet::<String>::new();
    let mut non_vertex_control_references = Vec::new();
    for lane in lanes {
        let mut objects = history
            .features
            .iter()
            .filter_map(|candidate| Some((feature_object_name(candidate, lane)?.offset, candidate)))
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        let Some(index) = objects
            .iter()
            .position(|(_, candidate)| candidate.id == feature_ref)
        else {
            continue;
        };
        let object_end = objects
            .get(index + 1)
            .and_then(|(offset, _)| usize::try_from(*offset).ok())
            .unwrap_or(lane.native_payload.len());
        let Some(controls) = variable_fillet_control_references(feature, lane, object_end) else {
            continue;
        };
        for (name, references) in controls {
            let vertices = references
                .iter()
                .flat_map(|reference| reference.iter())
                .filter(|component| component.instance == Some(0x8083))
                .collect::<Vec<_>>();
            match vertices.as_slice() {
                [vertex] => {
                    if !control_names.insert(name.clone()) {
                        return None;
                    }
                    let radius = feature.parameters.get(&name).and_then(|value| {
                        crate::history::parse_positive_dimension_length_mm(value)
                    })?;
                    match vertex_radii.entry(vertex.type_signature) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(radius);
                        }
                        std::collections::hash_map::Entry::Occupied(entry)
                            if !same_dimension_length(*entry.get(), radius) =>
                        {
                            return None;
                        }
                        std::collections::hash_map::Entry::Occupied(_) => {}
                    }
                }
                [] => {
                    if !non_vertex_control_names.insert(name) {
                        return None;
                    }
                    non_vertex_control_references.extend(references);
                }
                _ => return None,
            }
        }
    }
    if vertex_radii.is_empty() {
        if parameter_names.len() != 2
            || !control_names.is_empty()
            || non_vertex_control_names.len() != parameter_names.len()
            || !parameter_names
                .iter()
                .all(|name| non_vertex_control_names.contains(*name))
            || non_vertex_control_references.is_empty()
        {
            return None;
        }
        let mut ordered_parameters = parameter_names
            .iter()
            .map(|name| {
                variable_fillet_dimension_index_for_feature(feature, name).zip(
                    feature.parameters.get(*name).and_then(|value| {
                        crate::history::parse_positive_dimension_length_mm(value)
                    }),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        ordered_parameters.sort_unstable_by_key(|(index, _)| *index);
        if ordered_parameters
            .iter()
            .enumerate()
            .any(|(expected, (actual, _))| expected != *actual)
            || selections.iter().any(|selection| {
                selection
                    .references
                    .iter()
                    .flat_map(|reference| reference.iter())
                    .any(|component| component.instance == Some(0x8083))
            })
        {
            return None;
        }
        let selected_references = selections
            .iter()
            .flat_map(|selection| selection.references.iter())
            .collect::<Vec<_>>();
        if non_vertex_control_references
            .iter()
            .any(|reference| !selected_references.contains(&reference))
        {
            return None;
        }
        let mut selections = selections.to_vec();
        selections.sort_unstable_by_key(|selection| selection.ordinal);
        let points = ordered_parameters
            .into_iter()
            .enumerate()
            .map(|(parameter, (_, radius))| VariableRadius {
                parameter: parameter as f64,
                radius: Length(radius),
            })
            .collect();
        return Some(vec![(RadiusSpec::Variable { points }, selections)]);
    }
    if control_names.len() != parameter_names.len()
        || !parameter_names
            .iter()
            .all(|name| control_names.contains(*name))
    {
        return None;
    }

    let endpoint_signatures = |selection: &FeatureInputEdgeSelection| {
        selection
            .references
            .iter()
            .flat_map(|reference| reference.iter())
            .filter(|component| component.instance == Some(0x8083))
            .map(|component| component.type_signature)
            .collect::<Vec<_>>()
    };
    let roster_vertices = selections
        .iter()
        .flat_map(|selection| endpoint_signatures(selection))
        .collect::<HashSet<_>>();
    if vertex_radii
        .keys()
        .any(|signature| !roster_vertices.contains(signature))
    {
        return None;
    }

    let mut groups = Vec::<((u64, u64), Vec<&FeatureInputEdgeSelection>)>::new();
    let mut unassigned = Vec::new();
    for &selection in selections {
        let endpoints = endpoint_signatures(selection);
        match endpoints.as_slice() {
            [first, second] => {
                let pair = (
                    vertex_radii.get(first)?.to_bits(),
                    vertex_radii.get(second)?.to_bits(),
                );
                if let Some((_, grouped)) =
                    groups.iter_mut().find(|(candidate, _)| *candidate == pair)
                {
                    grouped.push(selection);
                } else {
                    groups.push((pair, vec![selection]));
                }
            }
            [] => unassigned.push(selection),
            _ => return None,
        }
    }
    if groups.len() == 1 {
        groups[0].1.append(&mut unassigned);
        groups[0]
            .1
            .sort_unstable_by_key(|selection| selection.ordinal);
    } else if !unassigned.is_empty() {
        return None;
    }
    (!groups.is_empty()).then(|| {
        groups
            .into_iter()
            .map(|((first, second), selections)| {
                (
                    RadiusSpec::Variable {
                        points: vec![
                            VariableRadius {
                                parameter: 0.0,
                                radius: Length(f64::from_bits(first)),
                            },
                            VariableRadius {
                                parameter: 1.0,
                                radius: Length(f64::from_bits(second)),
                            },
                        ],
                    },
                    selections,
                )
            })
            .collect()
    })
}

pub(crate) fn project_compact_surface_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    enum SelectionSlot<'a> {
        Face(&'a mut cadmpeg_ir::features::FaceSelection),
        Vertex(&'a mut cadmpeg_ir::features::VertexSelection),
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let selections = lanes.iter().flat_map(|lane| &lane.surface_selections).fold(
        HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new(),
        |mut map, selection| {
            map.entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            map
        },
    );
    for feature in features.iter_mut() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(feature_selections) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        if let FeatureDefinition::Pattern { seeds, .. } = &mut feature.definition {
            if seeds
                .iter()
                .any(|seed| matches!(seed, PatternSeed::Feature(_)))
            {
                continue;
            }
            for selection in feature_selections {
                let native = compact_surface_selection_value(&selection.components);
                let generated = component_path_feature(
                    &selection.components,
                    &history_features,
                    &selection.feature_ref,
                    ComponentPathEnd::Trailing,
                )
                .and_then(|(component, producer)| {
                    feature_ids_by_native
                        .get(producer.id.as_str())
                        .zip(component.local_id.as_ref())
                });
                let seed = match generated {
                    Some((producer, local_id)) => {
                        let face = cadmpeg_ir::features::GeneratedFaceRef {
                            feature: producer.clone(),
                            local_id: local_id.to_string(),
                        };
                        if seeds.iter().any(|seed| {
                            matches!(
                                seed,
                                PatternSeed::Faces(
                                    cadmpeg_ir::features::FaceSelection::Generated { faces, .. }
                                ) if faces.contains(&face)
                            )
                        }) {
                            continue;
                        }
                        if !feature.dependencies.contains(producer) {
                            feature.dependencies.push(producer.clone());
                        }
                        PatternSeed::Faces(cadmpeg_ir::features::FaceSelection::Generated {
                            faces: vec![face],
                            native,
                        })
                    }
                    None => PatternSeed::Faces(cadmpeg_ir::features::FaceSelection::Native(native)),
                };
                if !seeds.contains(&seed) {
                    seeds.push(seed);
                }
            }
            continue;
        }
        if let FeatureDefinition::SplitFace { targets, .. } = &mut feature.definition {
            if !matches!(
                targets,
                cadmpeg_ir::features::FaceSelection::Unresolved
                    | cadmpeg_ir::features::FaceSelection::Native(_)
            ) {
                continue;
            }
            let native = compact_surface_selection_set_value(feature_selections);
            let mut faces = Vec::new();
            let mut complete = true;
            for selection in feature_selections {
                let generated = selection
                    .terminal_feature_ref
                    .as_ref()
                    .and_then(|producer| feature_ids_by_native.get(producer))
                    .zip(selection.components.last())
                    .and_then(|(producer, component)| Some((producer, component.local_id?)));
                if let Some((producer, local_id)) = generated {
                    let face = cadmpeg_ir::features::GeneratedFaceRef {
                        feature: producer.clone(),
                        local_id: local_id.to_string(),
                    };
                    if !faces.contains(&face) {
                        faces.push(face);
                    }
                } else {
                    complete = false;
                }
                for producer in selection
                    .producer_feature_refs
                    .iter()
                    .filter_map(|producer| feature_ids_by_native.get(producer))
                    .filter(|producer| *producer != &feature.id)
                {
                    if !feature.dependencies.contains(producer) {
                        feature.dependencies.push(producer.clone());
                    }
                }
            }
            *targets = if complete && !faces.is_empty() {
                cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            } else {
                cadmpeg_ir::features::FaceSelection::Native(native)
            };
            continue;
        }
        if let FeatureDefinition::CutWithSurface { targets, tools, .. } = &mut feature.definition {
            let Some((target, tool)) = cut_with_surface_selection_pair(feature_selections) else {
                continue;
            };
            let target_native = compact_surface_selection_value(&target.components);
            let target_producer = target
                .terminal_feature_ref
                .as_ref()
                .and_then(|producer| feature_ids_by_native.get(producer));
            if let Some(producer) = target_producer {
                let local_id = target
                    .components
                    .iter()
                    .filter_map(|component| component.local_id)
                    .map(|local_id| local_id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                *targets = BodySelection::Generated {
                    bodies: vec![cadmpeg_ir::features::GeneratedBodyRef {
                        feature: (*producer).clone(),
                        local_id,
                    }],
                    native: target_native,
                };
                if !feature.dependencies.contains(producer) {
                    feature.dependencies.push((*producer).clone());
                }
            }
            let tool_native = compact_surface_selection_value(&tool.components);
            let tool_generated = tool
                .terminal_feature_ref
                .as_ref()
                .and_then(|producer| feature_ids_by_native.get(producer))
                .zip(tool.components.last())
                .and_then(|(producer, component)| {
                    component.local_id.map(|local_id| (producer, local_id))
                });
            if let Some((producer, local_id)) = tool_generated {
                *tools = FaceSelection::Generated {
                    faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                        feature: (*producer).clone(),
                        local_id: local_id.to_string(),
                    }],
                    native: tool_native,
                };
                if !feature.dependencies.contains(producer) {
                    feature.dependencies.push((*producer).clone());
                }
            }
            continue;
        }
        let unresolved_full_round = matches!(
            &feature.definition,
            FeatureDefinition::Fillet { groups }
                if matches!(
                    groups.as_slice(),
                    [cadmpeg_ir::features::FilletGroup {
                        edges: EdgeSelection::Unresolved,
                        radius: RadiusSpec::Unresolved { .. },
                        ..
                    }]
                )
        );
        if unresolved_full_round {
            let Some([center_faces, side_one_faces, side_two_faces]) =
                full_round_fillet_selection_triple(feature_selections)
            else {
                continue;
            };
            let face_selections = [center_faces, side_one_faces, side_two_faces]
                .into_iter()
                .map(|selection| {
                    let native = compact_surface_selection_value(&selection.components);
                    let generated = selection
                        .terminal_feature_ref
                        .as_ref()
                        .and_then(|producer| feature_ids_by_native.get(producer))
                        .zip(selection.components.last())
                        .and_then(|(producer, component)| Some((producer, component.local_id?)));
                    let face = match generated {
                        Some((producer, local_id)) => {
                            if producer != &feature.id && !feature.dependencies.contains(producer) {
                                feature.dependencies.push(producer.clone());
                            }
                            cadmpeg_ir::features::FaceSelection::Generated {
                                faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                                    feature: producer.clone(),
                                    local_id: local_id.to_string(),
                                }],
                                native,
                            }
                        }
                        None => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                    for producer in selection
                        .producer_feature_refs
                        .iter()
                        .filter_map(|producer| feature_ids_by_native.get(producer))
                        .filter(|producer| *producer != &feature.id)
                    {
                        if !feature.dependencies.contains(producer) {
                            feature.dependencies.push(producer.clone());
                        }
                    }
                    face
                })
                .collect::<Vec<_>>();
            let [center_faces, side_one_faces, side_two_faces] = face_selections.as_slice() else {
                unreachable!("full-round candidate has three face selections")
            };
            feature.definition = FeatureDefinition::FullRoundFillet {
                groups: vec![cadmpeg_ir::features::FullRoundFilletGroup {
                    center_faces: center_faces.clone(),
                    side_one_faces: cadmpeg_ir::features::FullRoundSideSelection::Explicit(
                        side_one_faces.clone(),
                    ),
                    side_two_faces: cadmpeg_ir::features::FullRoundSideSelection::Explicit(
                        side_two_faces.clone(),
                    ),
                }],
            };
            continue;
        }
        if matches!(feature.definition, FeatureDefinition::DatumPlaneUnresolved)
            && feature_selections.len() == 2
        {
            for selection in feature_selections {
                for producer in selection
                    .producer_feature_refs
                    .iter()
                    .filter_map(|producer| feature_ids_by_native.get(producer))
                    .filter(|producer| *producer != &feature.id)
                {
                    if !feature.dependencies.contains(producer) {
                        feature.dependencies.push(producer.clone());
                    }
                }
            }
            continue;
        }
        let first_component = matches!(
            &feature.definition,
            FeatureDefinition::CosmeticThread { .. }
        );
        let Some(selection) = (if first_component {
            cosmetic_thread_surface_selection_consensus(feature_selections)
        } else {
            surface_selection_consensus(feature_selections)
        }) else {
            continue;
        };
        if let FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } = &mut feature.definition
        {
            let native = compact_surface_selection_value(&selection.components);
            let generated = selection
                .terminal_feature_ref
                .as_ref()
                .and_then(|producer| feature_ids_by_native.get(producer))
                .zip(selection.components.last())
                .and_then(|(feature, component)| Some((feature, component.local_id?)));
            let face = match generated {
                Some((producer, local_id)) => {
                    if !feature.dependencies.contains(producer) {
                        feature.dependencies.push(producer.clone());
                    }
                    cadmpeg_ir::features::FaceSelection::Generated {
                        faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                            feature: producer.clone(),
                            local_id: local_id.to_string(),
                        }],
                        native,
                    }
                }
                None => cadmpeg_ir::features::FaceSelection::Native(native),
            };
            match reference {
                Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                    face: existing, ..
                }) => *existing = face,
                None => {
                    let Some(origin) = feature
                        .source_properties
                        .get("Origin")
                        .and_then(|value| crate::history::parse_point3_mm(value))
                    else {
                        continue;
                    };
                    let Some(normal) = feature
                        .source_properties
                        .get("Normal")
                        .and_then(|value| crate::history::parse_vector3(value))
                    else {
                        continue;
                    };
                    let Some(u_axis) = feature
                        .source_properties
                        .get("UAxis")
                        .and_then(|value| crate::history::parse_vector3(value))
                    else {
                        continue;
                    };
                    let origin = crate::history::offset_plane_support_origin(
                        &feature.source_properties,
                        crate::history::face_selection_native(&face),
                        origin,
                        normal,
                        *distance,
                    );
                    *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                        face,
                        origin,
                        normal,
                        u_axis,
                    });
                }
                Some(cadmpeg_ir::features::DatumPlaneReference::Feature(_)) => {}
            }
            continue;
        }
        let slot = match &mut feature.definition {
            FeatureDefinition::Thicken { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::Shell { removed_faces, .. } => SelectionSlot::Face(removed_faces),
            FeatureDefinition::OffsetSurface { faces, .. }
            | FeatureDefinition::KnitSurface { faces, .. }
            | FeatureDefinition::TrimSurface { faces, .. }
            | FeatureDefinition::ExtendSurface { faces, .. }
            | FeatureDefinition::Dome { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::FilledSurface { support_faces, .. } => {
                SelectionSlot::Face(support_faces)
            }
            FeatureDefinition::Draft { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::CosmeticThread { face, .. } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent:
                    cadmpeg_ir::features::ExtrudeExtent::OneSided {
                        side:
                            cadmpeg_ir::features::ExtrudeSide {
                                termination:
                                    cadmpeg_ir::features::Termination::ToFace { face, .. }
                                    | cadmpeg_ir::features::Termination::OffsetFromFace { face, .. },
                                ..
                            },
                    },
                ..
            } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent:
                    cadmpeg_ir::features::ExtrudeExtent::OneSided {
                        side:
                            cadmpeg_ir::features::ExtrudeSide {
                                termination: cadmpeg_ir::features::Termination::ToVertex { vertex },
                                ..
                            },
                    },
                ..
            } => SelectionSlot::Vertex(vertex),
            _ => continue,
        };
        let native = compact_surface_selection_value(&selection.components);
        let producer = if first_component {
            selection.producer_feature_refs.first()
        } else {
            selection.terminal_feature_ref.as_ref()
        };
        let component = if first_component {
            selection.components.first()
        } else {
            selection.components.last()
        };
        let generated = producer
            .and_then(|producer| feature_ids_by_native.get(producer))
            .zip(component)
            .and_then(|(feature, component)| Some((feature, component.local_id?)));
        match slot {
            SelectionSlot::Face(faces) => {
                if matches!(
                    faces,
                    cadmpeg_ir::features::FaceSelection::Unresolved
                        | cadmpeg_ir::features::FaceSelection::Native(_)
                ) {
                    *faces = match generated {
                        Some((feature, local_id)) => {
                            cadmpeg_ir::features::FaceSelection::Generated {
                                faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                                    feature: feature.clone(),
                                    local_id: local_id.to_string(),
                                }],
                                native,
                            }
                        }
                        None => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                }
            }
            SelectionSlot::Vertex(vertex) => {
                // Edge-endpoint references keep the endpoint selector native.
                let retain_native = matches!(
                    &*vertex,
                    cadmpeg_ir::features::VertexSelection::Native(value)
                        if value.starts_with("sldprt:feature-input:edge-endpoint-ref:")
                );
                if !retain_native
                    && matches!(
                        vertex,
                        cadmpeg_ir::features::VertexSelection::Unresolved
                            | cadmpeg_ir::features::VertexSelection::Native(_)
                    )
                {
                    *vertex = match generated {
                        Some((feature, local_id)) => {
                            cadmpeg_ir::features::VertexSelection::Generated {
                                vertex: cadmpeg_ir::features::GeneratedVertexRef {
                                    feature: feature.clone(),
                                    local_id: local_id.to_string(),
                                },
                                native,
                            }
                        }
                        None => cadmpeg_ir::features::VertexSelection::Native(native),
                    };
                }
            }
        }
        for producer in selection
            .producer_feature_refs
            .iter()
            .filter_map(|producer| feature_ids_by_native.get(producer))
            .filter(|producer| *producer != &feature.id)
        {
            if !feature.dependencies.contains(producer) {
                feature.dependencies.push(producer.clone());
            }
        }
    }
    let face_aliases = features
        .iter()
        .filter_map(|feature| {
            let native = feature.native_ref.as_deref()?;
            let FeatureDefinition::CosmeticThread { face, .. } = &feature.definition else {
                return None;
            };
            (!matches!(
                face,
                cadmpeg_ir::features::FaceSelection::Unresolved
                    | cadmpeg_ir::features::FaceSelection::Native(_)
            ))
            .then_some((native.to_string(), face.clone()))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(target) = feature.source_properties.get("ReferenceFaceFeature") else {
            continue;
        };
        let Some(face) = face_aliases.get(target.as_str()).cloned() else {
            continue;
        };
        let FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } = &mut feature.definition
        else {
            continue;
        };
        if let cadmpeg_ir::features::FaceSelection::Generated { faces, .. } = &face {
            for producer in faces.iter().map(|face| &face.feature) {
                if producer != &feature.id && !feature.dependencies.contains(producer) {
                    feature.dependencies.push(producer.clone());
                }
            }
        }
        if let Some(cadmpeg_ir::features::DatumPlaneReference::Face { face: existing, .. }) =
            reference
        {
            *existing = face;
            continue;
        }
        if reference.is_some() {
            continue;
        }
        let Some(origin) = feature
            .source_properties
            .get("Origin")
            .and_then(|value| crate::history::parse_point3_mm(value))
        else {
            continue;
        };
        let Some(normal) = feature
            .source_properties
            .get("Normal")
            .and_then(|value| crate::history::parse_vector3(value))
        else {
            continue;
        };
        let Some(u_axis) = feature
            .source_properties
            .get("UAxis")
            .and_then(|value| crate::history::parse_vector3(value))
        else {
            continue;
        };
        let origin = crate::history::offset_plane_support_origin(
            &feature.source_properties,
            crate::history::face_selection_native(&face),
            origin,
            normal,
            *distance,
        );
        *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
            face,
            origin,
            normal,
            u_axis,
        });
    }
}

fn full_round_fillet_selection_triple<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Option<[&'a FeatureInputSurfaceSelection; 3]> {
    let mut by_lane = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in selections {
        by_lane
            .entry(selection.parent.as_str())
            .or_default()
            .push(*selection);
    }
    let mut consensus: Option<[&'a FeatureInputSurfaceSelection; 3]> = None;
    for mut lane_selections in by_lane.into_values() {
        lane_selections.sort_unstable_by_key(|selection| selection.offset);
        let [center, side_one, side_two] = lane_selections.as_slice() else {
            return None;
        };
        if let Some([expected_center, expected_side_one, expected_side_two]) = consensus {
            if !same_surface_selection_semantics(expected_center, center)
                || !same_surface_selection_semantics(expected_side_one, side_one)
                || !same_surface_selection_semantics(expected_side_two, side_two)
            {
                return None;
            }
        } else {
            consensus = Some([*center, *side_one, *side_two]);
        }
    }
    consensus
}

pub(crate) fn project_draft_operands(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let candidates = lanes
        .iter()
        .flat_map(|lane| draft_operand_candidates(histories, lane))
        .fold(
            HashMap::<String, Vec<DraftOperands>>::new(),
            |mut by_feature, (feature, operands)| {
                by_feature.entry(feature).or_default().push(operands);
                by_feature
            },
        );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(operands) = candidates.get(native_ref) else {
            continue;
        };
        let Some(first) = operands
            .first()
            .filter(|first| operands.iter().all(|item| same_draft_operands(first, item)))
        else {
            continue;
        };
        let FeatureDefinition::Draft {
            faces,
            neutral_plane,
            parting_tool,
            pull_direction,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        match &first.anchor {
            DraftAnchor::NeutralPlane(path)
                if matches!(
                    neutral_plane,
                    cadmpeg_ir::features::FaceSelection::Unresolved
                ) =>
            {
                *neutral_plane = draft_face_selection(
                    std::slice::from_ref(path),
                    native_ref,
                    &history_features,
                    &feature_ids_by_native,
                    &mut feature.dependencies,
                );
            }
            DraftAnchor::PartingTool(paths) if parting_tool.is_none() => {
                *parting_tool = Some(draft_face_selection(
                    paths,
                    native_ref,
                    &history_features,
                    &feature_ids_by_native,
                    &mut feature.dependencies,
                ));
            }
            _ => {}
        }
        if matches!(faces, cadmpeg_ir::features::FaceSelection::Unresolved) {
            *faces = draft_face_selection(
                &first.faces,
                native_ref,
                &history_features,
                &feature_ids_by_native,
                &mut feature.dependencies,
            );
        }
        if pull_direction.is_none() {
            *pull_direction = Some(first.pull_direction);
        }
    }
}

fn draft_face_selection(
    paths: &[Vec<crate::records::FeatureInputComponentPathEntry>],
    consumer_ref: &str,
    history_features: &[crate::records::Feature],
    feature_ids_by_native: &HashMap<String, cadmpeg_ir::features::FeatureId>,
    dependencies: &mut Vec<cadmpeg_ir::features::FeatureId>,
) -> cadmpeg_ir::features::FaceSelection {
    let mut native_values = paths
        .iter()
        .map(|path| compact_surface_selection_value(path))
        .collect::<Vec<_>>();
    let mut seen_native = HashSet::new();
    native_values.retain(|value| seen_native.insert(value.clone()));
    let native = if let [value] = native_values.as_slice() {
        value.clone()
    } else {
        format!(
            "sldprt:feature-input:draft-surface-vectors:{}",
            native_values.join(";")
        )
    };
    let mut generated = Vec::new();
    let mut generated_dependencies = Vec::new();
    for path in paths {
        let Some((producer, local_id)) = component_path_terminal_feature(path, history_features)
            .filter(|producer| producer != consumer_ref)
            .and_then(|producer| {
                feature_ids_by_native
                    .get(&producer)
                    .zip(path.last()?.local_id.as_ref())
            })
        else {
            return cadmpeg_ir::features::FaceSelection::Native(native);
        };
        let face = cadmpeg_ir::features::GeneratedFaceRef {
            feature: producer.clone(),
            local_id: local_id.to_string(),
        };
        if !generated.contains(&face) {
            generated.push(face);
        }
        if !generated_dependencies.contains(producer) {
            generated_dependencies.push(producer.clone());
        }
    }
    if generated.is_empty() {
        cadmpeg_ir::features::FaceSelection::Native(native)
    } else {
        for dependency in generated_dependencies {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        cadmpeg_ir::features::FaceSelection::Generated {
            faces: generated,
            native,
        }
    }
}

fn compact_surface_selection_set_value(selections: &[&FeatureInputSurfaceSelection]) -> String {
    let mut values = selections
        .iter()
        .map(|selection| compact_surface_selection_value(&selection.components))
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
    if let [value] = values.as_slice() {
        return value.clone();
    }
    format!(
        "sldprt:feature-input:surface-selection-vectors:{}",
        values.join(";")
    )
}

fn surface_selection_consensus<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Option<&'a FeatureInputSurfaceSelection> {
    let first = selections.first().copied()?;
    selections
        .iter()
        .all(|selection| same_surface_selection_semantics(first, selection))
        .then_some(first)
}

/// Configuration lanes repeat a cosmetic-thread cylinder reference, but only
/// its first typed component identifies the attached face.  The remaining
/// components retain the owning path and can vary with the lane's instance
/// path.  Reject only when the attached-face component itself disagrees.
fn cosmetic_thread_surface_selection_consensus<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Option<&'a FeatureInputSurfaceSelection> {
    let first = selections.first().copied()?;
    let first_component = first.components.first()?;
    selections
        .iter()
        .all(|selection| {
            selection.components.first().is_some_and(|component| {
                component.local_id == first_component.local_id
                    && component.type_signature[4..8] == first_component.type_signature[4..8]
            })
        })
        .then_some(first)
}

fn same_surface_selection_semantics(
    left: &FeatureInputSurfaceSelection,
    right: &FeatureInputSurfaceSelection,
) -> bool {
    left.producer_feature_refs == right.producer_feature_refs
        && left.terminal_feature_ref == right.terminal_feature_ref
        && compact_surface_selection_value(&left.components)
            == compact_surface_selection_value(&right.components)
        && left.components.len() == right.components.len()
        && left
            .components
            .iter()
            .zip(&right.components)
            .all(|(left, right)| left.type_signature[4..8] == right.type_signature[4..8])
}

/// Return the ordered target/tool pair retained by each `SurfaceCut` lane.
///
/// The role-02 vectors are ordered in the native object: the target-body
/// reference list precedes the `moCompSurfaceBody_c` cutting-surface vector.
/// Their low selector byte is a lane-local subtype and cannot identify the
/// semantic role.  Configuration lanes must agree on both ordered paths.
fn cut_with_surface_selection_pair<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Option<(
    &'a FeatureInputSurfaceSelection,
    &'a FeatureInputSurfaceSelection,
)> {
    let mut by_lane = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in selections {
        by_lane
            .entry(selection.parent.as_str())
            .or_default()
            .push(*selection);
    }
    let mut consensus = None;
    for mut lane_selections in by_lane.into_values() {
        if lane_selections.len() != 2 {
            return None;
        }
        lane_selections.sort_unstable_by_key(|selection| selection.offset);
        let pair = (lane_selections[0], lane_selections[1]);
        if let Some((target, tool)) = consensus {
            if !same_surface_selection_semantics(target, pair.0)
                || !same_surface_selection_semantics(tool, pair.1)
            {
                return None;
            }
        } else {
            consensus = Some(pair);
        }
    }
    consensus
}

/// Resolve an attached thread face when its persistent cylinder reference
/// cannot bind directly to a generated face.
pub(crate) fn project_unbound_cosmetic_thread_faces(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    faces: &[Face],
    surfaces: &[Surface],
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(native_feature) = native_features.get(native_ref).copied() else {
            continue;
        };
        let FeatureDefinition::CosmeticThread { face, diameter, .. } = &mut feature.definition
        else {
            continue;
        };
        if !matches!(
            face,
            cadmpeg_ir::features::FaceSelection::Unresolved
                | cadmpeg_ir::features::FaceSelection::Native(_)
        ) {
            continue;
        }
        let references = lanes
            .iter()
            .flat_map(|lane| {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                lane.surface_selections
                    .iter()
                    .filter(move |selection| selection.feature_ref == native_feature.id)
                    .map(move |selection| {
                        (
                            format!("{lane_key}:{}", selection.offset),
                            Some(selection.components.clone()),
                            selection.producer_feature_refs.first().cloned(),
                        )
                    })
            })
            .chain(lanes.iter().flat_map(|lane| {
                (|| {
                    let (_, start, end) = feature_object_byte_ranges(histories, lane)
                        .get(native_feature.id.as_str())
                        .copied()?;
                    let cylinder_tokens = lane
                        .classes
                        .iter()
                        .filter(|class| class.name == "moCylinderRef_w")
                        .filter_map(|class| {
                            let body = usize::try_from(class.offset)
                                .ok()?
                                .checked_add(6 + class.name.len())?;
                            let token = View::u16_le_at(&lane.native_payload, body)?;
                            is_class_token(token).then_some(token)
                        })
                        .collect::<HashSet<_>>();
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    Some(
                        cosmetic_thread_cylinder_marker_reference(
                            native_feature,
                            lane,
                            start,
                            end,
                            &cylinder_tokens,
                        )
                        .into_iter()
                        .map(|(marker, components)| {
                            (format!("{lane_key}:{marker}"), components, None)
                        })
                        .collect::<Vec<_>>(),
                    )
                })()
                .unwrap_or_default()
            }))
            .collect::<Vec<_>>();
        let mut native_references = references
            .iter()
            .map(|(reference, _, _)| reference.clone())
            .collect::<Vec<_>>();
        native_references.sort();
        native_references.dedup();
        let native = (!native_references.is_empty()).then(|| {
            format!(
                "sldprt:feature-input:cylinder-reference:{}",
                native_references.join(",")
            )
        });
        let generated = references
            .iter()
            .map(|(_, components, explicit_producer)| {
                let components = components.as_ref()?;
                let explicit = explicit_producer.as_deref().and_then(|producer_ref| {
                    let producer = history_features
                        .iter()
                        .copied()
                        .find(|candidate| candidate.id.as_str() == producer_ref)?;
                    let component = components.first()?;
                    component
                        .local_id
                        .is_some()
                        .then_some((component, producer))
                });
                let (component, producer) = explicit.or_else(|| {
                    component_path_feature(
                        components,
                        &history_features,
                        native_feature.id.as_str(),
                        ComponentPathEnd::Leading,
                    )
                })?;
                Some((
                    feature_ids_by_native.get(producer.id.as_str())?.clone(),
                    component.local_id?.to_string(),
                ))
            })
            .collect::<Option<Vec<_>>>()
            .filter(|candidates| {
                candidates
                    .first()
                    .is_some_and(|first| candidates.iter().all(|candidate| candidate == first))
            })
            .and_then(|mut candidates| candidates.pop());
        if let Some((producer, local_id)) = generated {
            let Some(native) = native else {
                continue;
            };
            *face = cadmpeg_ir::features::FaceSelection::Generated {
                faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                    feature: producer.clone(),
                    local_id,
                }],
                native,
            };
            if producer != feature.id && !feature.dependencies.contains(&producer) {
                feature.dependencies.push(producer);
            }
            continue;
        }
        let Some(Length(diameter)) = diameter else {
            continue;
        };
        let selected = unique_cylindrical_face(*diameter * 0.5, faces, surfaces).or_else(|| {
            native
                .as_ref()
                .and_then(|_| unique_topological_cylindrical_face(faces, surfaces))
        });
        let Some(selected) = selected else {
            continue;
        };
        *face = match native {
            Some(native) => cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![selected],
                native,
            },
            None => cadmpeg_ir::features::FaceSelection::Faces(vec![selected]),
        };
    }
}

pub(super) fn unique_cylindrical_face(
    radius: f64,
    faces: &[Face],
    surfaces: &[Surface],
) -> Option<FaceId> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance = (radius.abs() * 1.0e-9).max(1.0e-9);
    let cylindrical = surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                radius: candidate, ..
            } if (candidate - radius).abs() <= tolerance => Some(&surface.id),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut candidates = faces
        .iter()
        .filter(|face| cylindrical.contains(&face.surface))
        .map(|face| face.id.clone());
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

pub(super) fn unique_topological_cylindrical_face(
    faces: &[Face],
    surfaces: &[Surface],
) -> Option<FaceId> {
    let cylindrical = surfaces
        .iter()
        .filter_map(|surface| {
            matches!(surface.geometry, SurfaceGeometry::Cylinder { .. }).then_some(&surface.id)
        })
        .collect::<HashSet<_>>();
    let mut candidates = faces
        .iter()
        .filter(|face| cylindrical.contains(&face.surface))
        .map(|face| face.id.clone());
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

/// Resolve frame-only offset-plane supports when exactly one B-rep face lies
/// on the serialized support plane.
pub(crate) fn project_unbound_offset_plane_faces(
    features: &mut [cadmpeg_ir::features::Feature],
    faces: &[Face],
    surfaces: &[Surface],
) {
    for feature in features {
        let FeatureDefinition::DatumOffsetPlane {
            reference:
                Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                    face,
                    origin,
                    normal,
                    ..
                }),
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        let native = match face {
            cadmpeg_ir::features::FaceSelection::Unresolved => None,
            cadmpeg_ir::features::FaceSelection::Native(native)
                if native.starts_with("sldprt:feature-input:legacy-face-alias#") =>
            {
                Some(native.clone())
            }
            _ => continue,
        };
        let Some(selected) = unique_planar_face(*origin, *normal, faces, surfaces) else {
            continue;
        };
        *face = match native {
            Some(native) => cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![selected],
                native,
            },
            None => cadmpeg_ir::features::FaceSelection::Faces(vec![selected]),
        };
    }
}

pub(super) fn unique_planar_face(
    origin: Point3,
    normal: Vector3,
    faces: &[Face],
    surfaces: &[Surface],
) -> Option<FaceId> {
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return None;
    }
    let normal = Vector3::new(
        normal.x / normal_length,
        normal.y / normal_length,
        normal.z / normal_length,
    );
    let tolerance = 1.0e-8
        * origin
            .x
            .abs()
            .max(origin.y.abs())
            .max(origin.z.abs())
            .max(1.0);
    let planar = surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane {
                origin: candidate_origin,
                normal: candidate_normal,
                ..
            } => {
                let candidate_length = candidate_normal.norm();
                if !candidate_length.is_finite() || candidate_length <= f64::EPSILON {
                    return None;
                }
                let alignment = (normal.x * candidate_normal.x
                    + normal.y * candidate_normal.y
                    + normal.z * candidate_normal.z)
                    / candidate_length;
                let displacement = Vector3::new(
                    candidate_origin.x - origin.x,
                    candidate_origin.y - origin.y,
                    candidate_origin.z - origin.z,
                );
                let distance = displacement.x * normal.x
                    + displacement.y * normal.y
                    + displacement.z * normal.z;
                ((alignment.abs() - 1.0).abs() <= 1.0e-9 && distance.abs() <= tolerance)
                    .then_some(&surface.id)
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut candidates = faces
        .iter()
        .filter(|face| planar.contains(&face.surface))
        .map(|face| face.id.clone());
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

#[cfg(test)]
mod projections_tests;
