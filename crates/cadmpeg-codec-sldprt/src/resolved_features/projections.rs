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
    owned_relation_parameters, relation_display_scalar_for_parameter,
    RELATION_DISPLAY_SCALAR_ID_PROPERTY, RELATION_PARAMETER_ID_PROPERTY,
    RELATION_PARAMETER_ROLE_PROPERTY, RELATION_PARAMETER_ROLE_REFERENCE,
};
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::selections::{
    cosmetic_thread_cylinder_marker_reference, variable_fillet_control_references,
    variable_fillet_dimension_index_for_feature,
};
use super::terminations::compact_surface_selection_value;
use crate::records::{
    FeatureInputEdgeSelection, FeatureInputLane, FeatureInputRelationFamily,
    FeatureInputScalarRole, FeatureInputSurfaceSelection,
};
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometryDefinition};
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::{
    features::{
        edge_treatments::{FilletGroup, RadiusSpec, VariableRadius},
        patterns::PatternSeed,
        BodySelection, DesignParameter, DimensionDisplay, EdgeSelection, FaceSelection,
        FeatureDefinition, FeatureOperation, ParameterId, ParameterValue, UnresolvedFamily,
    },
    scalar::{Angle, Length, PositiveLength},
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

const EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9: f64 = 1.0e-9;
const EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E8: f64 = 1.0e-8;
const EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E9: f64 = 1.0e-9;

fn reference_name(source_name: &str, offset: Option<u64>, suffix: Option<u32>) -> String {
    let offset = offset
        .map(|offset| format!(":{offset}"))
        .unwrap_or_default();
    let suffix = suffix
        .map(|suffix| format!(":{suffix}"))
        .unwrap_or_default();
    format!("{source_name}@reference{offset}{suffix}")
}

pub(super) fn bind_circular_profile_by_dimension(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) {
    let mut proposals = Vec::new();
    for (sketch_index, sketch) in sketches.iter().enumerate() {
        let [profile] = sketch.profiles.as_slice() else {
            continue;
        };
        let [entity_use] = profile.as_slice() else {
            continue;
        };
        let mut geometry = None;
        for entity in sketch_entities.iter().rev() {
            if entity.id() == &entity_use.entity {
                geometry = Some(&entity.geometry);
                break;
            }
        }
        let Some(SketchGeometryDefinition::Circle { radius, .. }) =
            geometry.map(cadmpeg_ir::SketchGeometry::definition)
        else {
            continue;
        };
        let radius = radius.get();
        let mut matched = None;
        let mut ambiguous = false;
        for (feature_index, feature) in features.iter().enumerate() {
            if !matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ) {
                continue;
            }
            let mut matches_dimension = false;
            for parameter in parameters {
                if parameter.owner.as_ref() != Some(&feature.id) {
                    continue;
                }
                let Some(ParameterValue::Length(value)) = &parameter.value else {
                    continue;
                };
                let expected = match parameter.display {
                    Some(DimensionDisplay::Radius) => value.get(),
                    Some(DimensionDisplay::Diameter) => value.get() * 0.5,
                    None => continue,
                };
                if same_dimension_length(expected, radius) {
                    matches_dimension = true;
                    break;
                }
            }
            if matches_dimension {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some(feature_index);
            }
        }
        if !ambiguous {
            if let Some(feature_index) = matched {
                proposals.push((sketch_index, feature_index));
            }
        }
    }
    let mut feature_counts = HashMap::<usize, usize>::new();
    for &(_, feature_index) in &proposals {
        *feature_counts.entry(feature_index).or_default() += 1;
    }
    for (sketch_index, feature_index) in proposals {
        if feature_counts.get(&feature_index) != Some(&1) {
            continue;
        }
        let sketch_id = sketches[sketch_index].id.clone();
        let name_index = sketches.iter().position(|sketch| sketch.id == sketch_id);
        for feature in features.iter_mut() {
            feature.evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: bound, ..
                }) = definition
                else {
                    return;
                };
                if bound.id() == Some(&sketch_id) {
                    *bound = cadmpeg_ir::features::SketchFeatureBinding::Planar(None);
                }
            });
        }
        let name = features[feature_index].name.as_deref().map(str::to_owned);
        features[feature_index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
                definition
            {
                *sketch = cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id));
            }
        });
        if let Some(index) = name_index {
            sketches[index].name = name;
        }
    }
}

/// Bind neutral parameters to uniquely owned native scalar records.
pub(crate) fn bind_parameter_scalars<'a>(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut neutral_owners = HashMap::new();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        neutral_owners.insert(&feature.id, native_ref);
    }
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    for lane in lanes {
        let mut length_scalars = HashSet::new();
        let mut angle_scalars = HashSet::new();
        let mut detached_scalars = HashSet::new();
        for relation in &lane.relation_instances {
            let Some(id) = relation.parameter_scalar_ref() else {
                continue;
            };
            let family = if relation.family == FeatureInputRelationFamily::Angle {
                &mut angle_scalars
            } else {
                &mut length_scalars
            };
            family.insert(id);
            let mut detached = false;
            for scalar in &lane.scalars {
                if scalar.id == id {
                    detached = scalar.operands.is_empty();
                    break;
                }
            }
            if detached && !detached_scalars.contains(id) {
                detached_scalars.insert(id);
            }
        }
        let mut names_by_id = HashMap::new();
        for name in &lane.names {
            names_by_id.insert(name.id.as_str(), name);
        }
        let mut starts = Vec::<(Option<u64>, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let start = feature_object_name(feature, lane).map(|name| name.offset);
            starts.push((start, feature));
        }
        starts.sort_by_key(|left| (left.0.is_none(), left.0));
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).and_then(|next| next.0);
            for parameter in parameters.iter_mut() {
                let owner = parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| neutral_owners.get(owner))
                    .copied();
                if owner != Some(native_feature.id.as_str()) || parameter.native_ref.is_some() {
                    continue;
                }
                let mut scalars = Vec::new();
                for scalar in &lane.scalars {
                    let owned = match scalar.feature_ref.as_deref() {
                        Some(owner) => owner == native_feature.id,
                        None => {
                            start.is_some_and(|start| scalar.offset > start)
                                && end.is_none_or(|end| scalar.offset < end)
                        }
                    };
                    if !owned {
                        continue;
                    }
                    let name_matches = names_by_id.get(scalar.name.as_str()).is_some_and(|name| {
                        name.value == parameter.name
                            && value_only_scalar_offset(&lane.native_payload, name)
                                != usize::try_from(scalar.offset).ok()
                    });
                    if name_matches {
                        scalars.push(scalar);
                    }
                }
                let mut driving = Vec::new();
                for scalar in &scalars {
                    if scalar.role == FeatureInputScalarRole::Driving {
                        driving.push(*scalar);
                    }
                }
                let mut candidates = Vec::new();
                if driving.is_empty() {
                    for scalar in scalars {
                        if scalar.role == FeatureInputScalarRole::Native {
                            candidates.push(scalar);
                        }
                    }
                } else {
                    candidates = driving;
                }
                let mut compatible = Vec::new();
                for scalar in candidates {
                    let matches_value = match parameter.value.as_ref() {
                        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
                            let Some(expected) =
                                crate::history::parameters::eval::exact_integer_f64(*expected)
                            else {
                                continue;
                            };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value.get() * 1000.0, expected)
                            } else {
                                scalar.value.get() == expected
                            }
                        }
                        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
                            let expected = if *expected { 1.0 } else { 0.0 };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value.get() * 1000.0, expected)
                            } else {
                                scalar.value.get() == expected
                            }
                        }
                        _ => true,
                    };
                    if matches_value {
                        compatible.push(scalar);
                    }
                }
                if let [scalar] = compatible.as_slice() {
                    parameter.native_ref = Some(scalar.id.clone());
                    let scalar_is_detached = detached_scalars.contains(scalar.id.as_str());
                    let scalar_is_untyped_real = matches!(
                        parameter.value,
                        Some(cadmpeg_ir::features::ParameterValue::Real(_))
                    ) && !scalar_is_detached;
                    if scalar_is_detached && length_scalars.contains(scalar.id.as_str()) {
                        parameter.expression = crate::history::literals::format_length_mm(
                            cadmpeg_ir::scalar::Length::new(scalar.value.get() * 1000.0)
                                .ok_or_else(|| {
                                    cadmpeg_core::CodecError::Malformed(
                                        "SolidWorks projected length must be finite".into(),
                                    )
                                })?,
                        );
                    } else if scalar_is_detached && angle_scalars.contains(scalar.id.as_str()) {
                        parameter.expression = crate::history::literals::format_angle_rad(
                            cadmpeg_ir::scalar::Angle::from_assigned_real(scalar.value),
                        );
                    }
                    let evaluated = if length_scalars.contains(scalar.id.as_str())
                        && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Length(
                            cadmpeg_ir::scalar::Length::new(scalar.value.get() * 1000.0)
                                .ok_or_else(|| {
                                    cadmpeg_core::CodecError::Malformed(
                                        "SolidWorks projected length must be finite".into(),
                                    )
                                })?,
                        ))
                    } else if angle_scalars.contains(scalar.id.as_str()) && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Angle(
                            cadmpeg_ir::scalar::Angle::from_assigned_real(scalar.value),
                        ))
                    } else {
                        match parameter.value.as_ref() {
                            Some(cadmpeg_ir::features::ParameterValue::Length(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Length(
                                    cadmpeg_ir::scalar::Length::new(scalar.value.get() * 1000.0)
                                        .ok_or_else(|| {
                                            cadmpeg_core::CodecError::Malformed(
                                                "SolidWorks projected length must be finite".into(),
                                            )
                                        })?,
                                ))
                            }
                            Some(cadmpeg_ir::features::ParameterValue::Angle(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Angle(
                                    cadmpeg_ir::scalar::Angle::from_assigned_real(scalar.value),
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

    Ok(())
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
) -> Result<(), cadmpeg_core::CodecError> {
    let mut lane_refs = Vec::new();
    for lane in lanes {
        lane_refs.push(lane);
    }
    let owned = owned_relation_parameters(features, parameters, lane_refs.iter().copied());
    let mut features_by_native_ref = HashMap::new();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        features_by_native_ref.insert(native_ref, feature);
    }
    let mut relation_ids = HashSet::new();
    let mut parameter_ids = HashSet::new();
    let mut names_by_owner = HashSet::new();
    let mut next_ordinals = HashMap::<cadmpeg_ir::features::FeatureId, u32>::new();
    for parameter in parameters.iter() {
        if let Some(relation_id) = parameter.properties.get(RELATION_PARAMETER_ID_PROPERTY) {
            if !relation_ids.contains(relation_id.as_str()) {
                relation_ids.insert(relation_id.clone());
            }
        }
        if !parameter_ids.contains(&parameter.id) {
            parameter_ids.insert(parameter.id.clone());
        }
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        if !names_by_owner
            .iter()
            .any(|(known_owner, known_name)| known_owner == owner && known_name == &parameter.name)
        {
            names_by_owner.insert((owner.clone(), parameter.name.clone()));
        }
        let next = if parameter.ordinal == u32::MAX {
            parameter.ordinal
        } else {
            parameter.ordinal + 1
        };
        if let Some(current) = next_ordinals.get_mut(owner) {
            *current = (*current).max(next);
        } else {
            next_ordinals.insert(owner.clone(), next);
        }
    }

    for lane in lane_refs {
        for relation in &lane.relation_instances {
            if relation.parameter_scalar_ref().is_some()
                || owned.get(&relation.id).is_some_and(Option::is_some)
                || relation_ids.contains(&relation.id)
            {
                continue;
            }
            let Some(scalar) = relation_display_scalar_for_parameter(relation, lane) else {
                continue;
            };
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
            let Some((value, display, expression)) =
                relation_display_parameter_value(relation.family, scalar.value.get())
            else {
                continue;
            };
            let owner = &feature.id;
            let current_ordinal = next_ordinals.get(owner).copied().unwrap_or(0);
            let Some(next_ordinal) = current_ordinal.checked_add(1) else {
                continue;
            };
            if let Some(ordinal) = next_ordinals.get_mut(owner) {
                *ordinal = next_ordinal;
            } else {
                next_ordinals.insert(owner.clone(), next_ordinal);
            }
            let mut candidate = reference_name(source_name, None, None);
            if names_by_owner
                .iter()
                .any(|(known_owner, known_name)| known_owner == owner && known_name == &candidate)
            {
                candidate = reference_name(source_name, Some(relation.offset), None);
                let mut suffix = 0u32;
                loop {
                    if !names_by_owner.iter().any(|(known_owner, known_name)| {
                        known_owner == owner && known_name == &candidate
                    }) {
                        break;
                    }
                    suffix = suffix.checked_add(1).ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "SLDPRT reference parameter name suffix overflows u32",
                        )
                    })?;
                    candidate = reference_name(source_name, Some(relation.offset), Some(suffix));
                }
            }
            let relation_key = relation
                .id
                .rsplit_once('#')
                .map_or(relation.id.as_str(), |(_, key)| key);
            let Ok(relation_key) = cadmpeg_ir::ids::IdentityKey::try_new(relation_key.to_owned())
            else {
                continue;
            };
            let id_text = format!("sldprt:model:parameter#reference:{relation_key}");
            let Ok(id) = ParameterId::mint(id_text) else {
                continue;
            };
            if parameter_ids.contains(&id) {
                continue;
            }
            parameter_ids.insert(id.clone());
            let mut properties = BTreeMap::new();
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ID_PROPERTY),
                relation.id.clone(),
            );
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_DISPLAY_SCALAR_ID_PROPERTY),
                scalar.id.clone(),
            );
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ROLE_PROPERTY),
                RELATION_PARAMETER_ROLE_REFERENCE.into(),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("source_name"),
                source_name.to_string(),
            );
            parameters.push(DesignParameter {
                id,
                owner: Some(owner.clone()),
                ordinal: current_ordinal,
                name: candidate.clone(),
                expression,
                display,
                value: Some(value),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties,
                pmi: None,
                native_ref: None,
            });
            names_by_owner.insert((owner.clone(), candidate));
            if !relation_ids.contains(relation.id.as_str()) {
                relation_ids.insert(relation.id.clone());
            }
        }
    }
    Ok(())
}

fn relation_display_parameter_value(
    family: FeatureInputRelationFamily,
    value: f64,
) -> Option<(ParameterValue, Option<DimensionDisplay>, String)> {
    Some(match family {
        FeatureInputRelationFamily::Angle => {
            let angle = Angle::new(value)?;
            (
                ParameterValue::Angle(angle),
                None,
                crate::history::literals::format_angle_rad(angle),
            )
        }
        FeatureInputRelationFamily::CircleDiameter => {
            let millimetres = Length::new(value * 1000.0)?;
            (
                ParameterValue::Length(millimetres),
                Some(DimensionDisplay::Diameter),
                format!(
                    "<MOD-DIAM>{}",
                    crate::history::literals::format_length_mm(millimetres)
                ),
            )
        }
        FeatureInputRelationFamily::LineLineDistance
        | FeatureInputRelationFamily::PointPointDistance
        | FeatureInputRelationFamily::PointLineDistance
        | FeatureInputRelationFamily::PointPointHorizontalDistance
        | FeatureInputRelationFamily::PointPointVerticalDistance => {
            let millimetres = Length::new(value * 1000.0)?;
            (
                ParameterValue::Length(millimetres),
                None,
                crate::history::literals::format_length_mm(millimetres),
            )
        }
    })
}

/// Apply relation-defined units and display semantics to parameters named by display scalars.
pub(crate) fn type_display_relation_parameters(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let mut families = HashMap::<&cadmpeg_ir::features::ParameterId, HashSet<_>>::new();
    for relation in lanes.iter().flat_map(|lane| &lane.relation_instances) {
        if let Some(Some(parameter)) = ownership.get(&relation.id) {
            let family_set = families.entry(parameter).or_default();
            family_set.insert(relation.family);
        }
    }
    for parameter in parameters {
        let Some(families) = families.get(&parameter.id) else {
            continue;
        };
        let mut families = families.iter();
        let (Some(&family), None) = (families.next(), families.next()) else {
            continue;
        };
        match family {
            FeatureInputRelationFamily::Angle => {
                // Every relation that owns the parameter is an angle relation.
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    let angle = cadmpeg_ir::scalar::Angle::from_assigned_real(value);
                    parameter.expression = crate::history::literals::format_angle_rad(angle);
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Angle(angle));
                }
            }
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => {
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    let value =
                        cadmpeg_ir::scalar::Length::new(value.get() * 1000.0).ok_or_else(|| {
                            cadmpeg_core::CodecError::Malformed(
                                "SolidWorks projected length must be finite".into(),
                            )
                        })?;
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!(
                            "<MOD-DIAM>{}",
                            crate::history::literals::format_length_mm(value)
                        )
                    } else {
                        crate::history::literals::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(value));
                }
                if let Some(cadmpeg_ir::features::ParameterValue::Integer(value)) =
                    parameter.value.as_ref()
                {
                    let Some(value) = crate::history::parameters::eval::exact_integer_f64(*value)
                    else {
                        continue;
                    };
                    let value = cadmpeg_ir::scalar::Length::new(value).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })?;
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!(
                            "<MOD-DIAM>{}",
                            crate::history::literals::format_length_mm(value)
                        )
                    } else {
                        crate::history::literals::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(value));
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

    Ok(())
}

pub(crate) fn project_compact_body_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut selected = None;
        let mut duplicate = false;
        for selection in lanes.iter().flat_map(|lane| &lane.body_selections) {
            if selection.feature_ref == native_ref && selected.replace(selection).is_some() {
                duplicate = true;
                break;
            }
        }
        let Some(selection) = selected.filter(|_| !duplicate) else {
            continue;
        };
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), cadmpeg_core::CodecError> {
                let (bodies, mode) = match definition {
                    FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) => {
                        (bodies, Some(mode))
                    }
                    FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                        (bodies, None)
                    }
                    _ => return Ok(()),
                };
                if matches!(bodies, cadmpeg_ir::features::BodySelection::Unresolved) {
                    let mut ids = Vec::new();
                    for id in &selection.local_body_ids {
                        ids.push(id.to_string());
                    }
                    let Ok(selection) = cadmpeg_ir::features::BodySelection::local(
                        ids,
                        compact_body_selection_value(&selection.local_body_ids),
                        ctx,
                    )?
                    else {
                        return Ok(());
                    };
                    *bodies = selection;
                }
                if let Some(mode) = mode {
                    if matches!(*mode, cadmpeg_ir::features::BodyRetentionMode::Unresolved) {
                        if let Some(native_mode) = selection.mode {
                            *mode = native_mode;
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

pub(crate) fn project_compact_edge_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_ref() else {
            continue;
        };
        let id = feature.id.clone();
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        feature_ids_by_native.insert(native_ref.to_owned(), id);
    }
    let mut selections = HashMap::<&str, Vec<&FeatureInputEdgeSelection>>::new();
    for selection in lanes.iter().flat_map(|lane| &lane.edge_selections) {
        let group = selections
            .entry(selection.feature_ref.as_str())
            .or_default();
        group.push(selection);
    }
    for feature in features {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let native_ref = feature.native_ref.as_deref();
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), cadmpeg_core::CodecError> {
            let Some(native_ref) = native_ref else {
                return Ok(());
            };
            let Some(edge_selections) = selections
                .get(native_ref)
                .filter(|selections| !selections.is_empty())
            else {
                return Ok(());
            };
            let projected_edges = |selections: &[&FeatureInputEdgeSelection]| -> Result<_, cadmpeg_core::CodecError> {
                let native = compact_edge_selection_set_value(selections);
                let mut generated = Vec::<cadmpeg_ir::features::GeneratedEdgeRef>::new();
                let mut complete = true;
                for selection in selections {
                    let Some(native_feature) = selection.terminal_feature_ref.as_ref() else {
                        complete = false;
                        break;
                    };
                    let Some(feature_id) = feature_ids_by_native.get(native_feature) else {
                        complete = false;
                        break;
                    };
                    let feature = feature_id.clone();
                    let local_id = compact_edge_path_value(selection);
                    let Ok(edge) = cadmpeg_ir::features::GeneratedEdgeRef::new(feature, local_id) else {
                        complete = false;
                        break;
                    };
                    if !generated.contains(&edge) {
                        generated.push(edge);
                    }
                }
                if complete && !generated.is_empty() {
                    EdgeSelection::generated(generated, native)
                        .map_err(cadmpeg_core::CodecError::malformed)
                } else {
                    Ok(EdgeSelection::Native(native))
                }
            };
            if let Some((existing_edges, tangency_weight)) =
                sole_unresolved_fillet_group(definition)
            {
                if let Some(radius_groups) =
                    variable_fillet_radius_groups(native_ref, histories, lanes, edge_selections)
                {
                    let unresolved_edges = matches!(existing_edges, EdgeSelection::Unresolved);
                    if unresolved_edges || radius_groups.len() == 1 {
                        let mut replacement_groups = Vec::new();
                        let mut carried_edges = match &mut *definition {
                            FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                                if !unresolved_edges => groups
                                    .iter_mut()
                                    .next()
                                    .map(|group| std::mem::replace(&mut group.edges, EdgeSelection::Unresolved)),
                            _ => None,
                        };
                        for RadiusSelectionGroup(radius, selections) in radius_groups {
                            let edges = if unresolved_edges {
                                projected_edges(&selections)?
                            } else {
                                carried_edges.take().ok_or_else(|| {
                                    cadmpeg_core::CodecError::malformed(
                                        "SLDPRT fillet replacement has no carried edges",
                                    )
                                })?
                            };
                            replacement_groups.push(FilletGroup { edges, radius, tangency_weight });
                        }
                        *definition = FeatureDefinition::Operation(FeatureOperation::Fillet {
                            groups: replacement_groups
                                .try_into()
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                        });
                    }
                }
            }
            match &mut *definition {
                FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                    for group in groups.iter_mut().filter(|group| matches!(group.edges, EdgeSelection::Unresolved)) {
                        group.edges = projected_edges(edge_selections)?;
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                    for group in groups.iter_mut() {
                        group.edges = projected_edges(edge_selections)?;
                    }
                }
                _ => return Ok(()),
            }
            for dependency in edge_selections
                .iter()
                .flat_map(|selection| &selection.producer_feature_refs)
                .filter_map(|native| feature_ids_by_native.get(native))
            {
                if dependency != feature_id {
                    const DEPENDENCY_OPERATION: &str = "add SLDPRT compact edge dependency";
                    if dependencies.contains(dependency) {
                        continue;
                    }
                    let id = dependency.clone();
                    dependencies.insert(ctx, id, DEPENDENCY_OPERATION)?;
                }
            }
            Ok(())
            })();
        });
        edit_result?;
    }

    Ok(())
}

#[derive(Debug)]
struct RadiusSelectionGroup<'a>(RadiusSpec, Vec<&'a FeatureInputEdgeSelection>);

fn variable_fillet_radius_groups<'a>(
    feature_ref: &str,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    selections: &[&'a FeatureInputEdgeSelection],
) -> Option<Vec<RadiusSelectionGroup<'a>>> {
    let history = histories.iter().find(|history| {
        history
            .features
            .iter()
            .any(|feature| feature.id == feature_ref)
    })?;
    let feature = history.features.iter().find(|feature| {
        feature.id == feature_ref && feature.kind.eq_ignore_ascii_case("VarFillet")
    })?;
    let mut parameter_names = HashSet::new();
    for name in feature.parameters.keys() {
        if variable_fillet_dimension_index_for_feature(feature, name.as_str()).is_some() {
            parameter_names.insert(name);
        }
    }
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
        let mut ordered_parameters = Vec::new();
        for name in &parameter_names {
            let parameter = variable_fillet_dimension_index_for_feature(feature, name.as_str())
                .zip(feature.parameters.get(*name).and_then(|value| {
                    crate::history::literals::parse_positive_dimension_length_mm(value)
                }))?;
            ordered_parameters.push(parameter);
        }
        ordered_parameters.sort_unstable_by_key(|(left, _)| *left);
        if ordered_parameters
            .iter()
            .enumerate()
            .all(|(expected, (actual, _))| expected == *actual)
        {
            let mut selections = selections.to_vec();
            selections.sort_unstable_by_key(|left| left.ordinal);
            let mut points = Vec::new();
            for (parameter, (_, radius)) in ordered_parameters.into_iter().enumerate() {
                let parameter = f64_from_index(parameter)?;
                points.push(VariableRadius {
                    parameter,
                    radius: Length::from(radius),
                });
            }
            let points = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok()?;
            return Some(vec![RadiusSelectionGroup(
                RadiusSpec::Variable { points },
                selections,
            )]);
        }
    }

    let mut vertex_radii = HashMap::<[u8; 12], PositiveLength>::new();
    let mut control_names = HashSet::<String>::new();
    let mut non_vertex_control_names = HashSet::<String>::new();
    let mut non_vertex_control_references = Vec::new();
    for lane in lanes {
        let mut objects = Vec::new();
        for candidate in &history.features {
            if let Some(name) = feature_object_name(candidate, lane) {
                objects.push((name.offset, candidate));
            }
        }
        objects.sort_unstable_by_key(|(left, _)| *left);
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
        for super::selections::VariableFilletControl(name, references) in controls {
            let mut vertices = references
                .iter()
                .flat_map(|reference| reference.iter())
                .filter(|component| component.instance == Some(0x8083));
            match (vertices.next(), vertices.next()) {
                (Some(vertex), None) => {
                    if control_names.contains(&name) {
                        return None;
                    }
                    control_names.insert(name.clone());
                    let radius = feature.parameters.get(name.as_str()).and_then(|value| {
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    })?;
                    match vertex_radii.entry(*vertex.type_signature) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(radius);
                        }
                        std::collections::hash_map::Entry::Occupied(entry)
                            if !same_dimension_length(entry.get().get(), radius.get()) =>
                        {
                            return None;
                        }
                        std::collections::hash_map::Entry::Occupied(_) => {}
                    }
                }
                (None, None) => {
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
                .all(|name| non_vertex_control_names.contains(name.as_str()))
            || non_vertex_control_references.is_empty()
        {
            return None;
        }
        let mut ordered_parameters = Vec::new();
        for name in &parameter_names {
            let parameter = variable_fillet_dimension_index_for_feature(feature, name.as_str())
                .zip(feature.parameters.get(*name).and_then(|value| {
                    crate::history::literals::parse_positive_dimension_length_mm(value)
                }))?;
            ordered_parameters.push(parameter);
        }
        ordered_parameters.sort_unstable_by_key(|(left, _)| *left);
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
        for reference in &non_vertex_control_references {
            let mut present = false;
            for selected in selections
                .iter()
                .flat_map(|selection| selection.references.iter())
            {
                if selected == reference {
                    present = true;
                    break;
                }
            }
            if !present {
                return None;
            }
        }
        let mut selections = selections.to_vec();
        selections.sort_unstable_by_key(|left| left.ordinal);
        let mut points = Vec::new();
        for (parameter, (_, radius)) in ordered_parameters.into_iter().enumerate() {
            let parameter = f64_from_index(parameter)?;
            points.push(VariableRadius {
                parameter,
                radius: Length::from(radius),
            });
        }
        let points = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok()?;
        return Some(vec![RadiusSelectionGroup(
            RadiusSpec::Variable { points },
            selections,
        )]);
    }
    if control_names.len() != parameter_names.len()
        || !parameter_names
            .iter()
            .all(|name| control_names.contains(name.as_str()))
    {
        return None;
    }

    for signature in vertex_radii.keys() {
        let mut found = false;
        for component in selections
            .iter()
            .flat_map(|selection| selection.references.iter())
            .flat_map(|reference| reference.iter())
        {
            if component.instance == Some(0x8083) && component.type_signature == (*signature) {
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }

    let mut groups = Vec::<(
        (PositiveLength, PositiveLength),
        Vec<&FeatureInputEdgeSelection>,
    )>::new();
    let mut unassigned = Vec::new();
    for &selection in selections {
        let mut endpoints = selection
            .references
            .iter()
            .flat_map(|reference| reference.iter())
            .filter(|component| component.instance == Some(0x8083))
            .map(|component| component.type_signature);
        let first = endpoints.next();
        let second = endpoints.next();
        let third = endpoints.next();
        match (first, second, third) {
            (Some(first), Some(second), None) => {
                let (Some(first_radius), Some(second_radius)) =
                    (vertex_radii.get(&*first), vertex_radii.get(&*second))
                else {
                    return None;
                };
                let pair = (*first_radius, *second_radius);
                if let Some((_, grouped)) =
                    groups.iter_mut().find(|(candidate, _)| *candidate == pair)
                {
                    grouped.push(selection);
                } else {
                    groups.push((pair, vec![selection]));
                }
            }
            (None, None, None) => {
                unassigned.push(selection);
            }
            _ => return None,
        }
    }
    if groups.len() == 1 {
        groups[0].1.append(&mut unassigned);
        groups[0].1.sort_unstable_by_key(|left| left.ordinal);
    } else if !unassigned.is_empty() {
        return None;
    }
    if groups.is_empty() {
        return None;
    }
    let mut result = Vec::new();
    for ((first, second), selections) in groups {
        let raw_points = vec![
            VariableRadius {
                parameter: 0.0,
                radius: Length::from(first),
            },
            VariableRadius {
                parameter: 1.0,
                radius: Length::from(second),
            },
        ];
        let points = cadmpeg_ir::features::edge_treatments::VariableRadii::new(raw_points).ok()?;
        result.push(RadiusSelectionGroup(
            RadiusSpec::Variable { points },
            selections,
        ));
    }
    Some(result)
}

pub(crate) fn project_compact_surface_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const ALIAS_OPERATION: &str = "project SLDPRT face aliases";

    enum SelectionSlot<'a> {
        Face(&'a mut cadmpeg_ir::features::FaceSelection),
        Vertex(&'a mut cadmpeg_ir::features::VertexSelection),
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_ref() else {
            continue;
        };
        let id = feature.id.clone();
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        feature_ids_by_native.insert(native_ref.to_owned(), id);
    }
    let mut history_features = Vec::new();
    for history in histories {
        for feature in &history.features {
            history_features.push(feature);
        }
    }
    let mut selections = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in lanes.iter().flat_map(|lane| &lane.surface_selections) {
        let group = selections
            .entry(selection.feature_ref.as_str())
            .or_default();
        group.push(selection);
    }
    for feature in features.iter_mut() {
        let feature_id = &feature.id;
        let native_ref = feature.native_ref.as_deref();
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                'feature_edit: {
                    const OPERATION: &str = "project SLDPRT surface face and vertex slots";

                    let Some(native_ref) = native_ref else {
                        break 'feature_edit;
                    };
                    let Some(feature_selections) = selections.get(native_ref).map(Vec::as_slice) else {
                        break 'feature_edit;
                    };
                    if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
                        definition
                    {
                        const OPERATION: &str = "project SLDPRT pattern face seeds";
                        if seeds
                            .iter()
                            .any(|seed| matches!(seed, PatternSeed::Feature(_)))
                        {
                            break 'feature_edit;
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
                                    let producer_id = producer.clone();
                                    let local_id_text = local_id.to_string();
                                    let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) else {
                                        let seed = PatternSeed::Faces(
                                            cadmpeg_ir::features::FaceSelection::Native(native),
                                        );
                                        if !seeds.contains(&seed) {
                                            seeds.push(seed);
                                        }
                                        continue;
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
                                    if !dependencies.contains(producer) {
                                        let dependency = producer.clone();
                                        dependencies.insert(ctx, dependency, OPERATION)?;
                                    }
                                    let faces = vec![face];
                                    let native_copy = native.clone();
                                    PatternSeed::Faces(
                                        cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                        .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native)),
                                    )
                                }
                                None => {
                                    PatternSeed::Faces(cadmpeg_ir::features::FaceSelection::Native(native))
                                }
                            };
                            if !seeds.contains(&seed) {
                                seeds.push(seed);
                            }
                        }
                        break 'feature_edit;
                    }
                    if let FeatureDefinition::Operation(FeatureOperation::SplitFace { targets, .. }) =
                        definition
                    {
                        const OPERATION: &str = "project SLDPRT SplitFace selections";

                        if !matches!(
                            targets,
                            cadmpeg_ir::features::FaceSelection::Unresolved
                                | cadmpeg_ir::features::FaceSelection::Native(_)
                        ) {
                            break 'feature_edit;
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
                                let producer_id = producer.clone();
                                let local_id_text = local_id.to_string();
                                let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) else {
                                    complete = false;
                                    continue;
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
                                .filter(|producer| *producer != feature_id)
                            {
                                if !dependencies.contains(producer) {
                                    let dependency = producer.clone();
                                    dependencies.insert(ctx, dependency, OPERATION)?;
                                }
                            }
                        }
                        *targets = if complete && !faces.is_empty() {
                            let native_copy = native.clone();
                            cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                        } else {
                            cadmpeg_ir::features::FaceSelection::Native(native)
                        };
                        break 'feature_edit;
                    }
                    if let FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
                        targets,
                        tools,
                        ..
                    }) = definition
                    {
                        const OPERATION: &str = "project SLDPRT surface cut operands";
                        let Some((target, tool)) = cut_with_surface_selection_pair(feature_selections)
                        else {
                            break 'feature_edit;
                        };
                        let target_native = compact_surface_selection_value(&target.components);
                        let target_producer = target
                            .terminal_feature_ref
                            .as_ref()
                            .and_then(|producer| feature_ids_by_native.get(producer));
                        if let Some(producer) = target_producer {
                            let mut local_id = String::new();
                            for id in target.components.iter().filter_map(|component| component.local_id) {
                                if !local_id.is_empty() {
                                    local_id.push(',');
                                }
                                write!(local_id, "{id}").map_err(|_| {
                                    cadmpeg_core::CodecError::malformed("cannot format SLDPRT surface cut body id")
                                })?;
                            }
                            let producer_id = producer.clone();
                            let Ok(body) =
                                cadmpeg_ir::features::GeneratedBodyRef::new(producer_id, local_id)
                            else {
                                break 'feature_edit;
                            };
                            let bodies = vec![body];
                            let native_copy = target_native.clone();
                            *targets = BodySelection::generated(bodies, native_copy, ctx)?
                                .unwrap_or(BodySelection::Native(target_native));
                            if !dependencies.contains(producer) {
                                let dependency = producer.clone();
                                dependencies.insert(ctx, dependency, OPERATION)?;
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
                            let producer_id = producer.clone();
                            let local_id_text = local_id.to_string();
                            let generated_face = cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text);
                            *tools = if let Ok(face) = generated_face {
                                let faces = vec![face];
                                let native_copy = tool_native.clone();
                                FaceSelection::generated(faces, native_copy)
                                    .unwrap_or(FaceSelection::Native(tool_native))
                            } else {
                                FaceSelection::Native(tool_native)
                            };
                            if !dependencies.contains(producer) {
                                let dependency = producer.clone();
                                dependencies.insert(ctx, dependency, OPERATION)?;
                            }
                        }
                        break 'feature_edit;
                    }
                    let unresolved_full_round = match definition {
                        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => matches!(
                            groups.as_slice(),
                            [group]
                                if matches!(group.edges, EdgeSelection::Unresolved)
                                    && group.radius.is_unresolved()
                        ),
                        _ => false,
                    };
                    if unresolved_full_round {
                        const OPERATION: &str = "project SLDPRT full round fillet faces";
                        let Some([center_faces, side_one_faces, side_two_faces]) =
                            full_round_fillet_selection_triple(feature_selections)
                        else {
                            break 'feature_edit;
                        };
                        let [center_faces, side_one_faces, side_two_faces] =
                            [center_faces, side_one_faces, side_two_faces].map(|selection| -> Result<FaceSelection, cadmpeg_core::CodecError> {
                                let native = compact_surface_selection_value(&selection.components);
                                let generated = selection
                                    .terminal_feature_ref
                                    .as_ref()
                                    .and_then(|producer| feature_ids_by_native.get(producer))
                                    .zip(selection.components.last())
                                    .and_then(|(producer, component)| {
                                        Some((producer, component.local_id?))
                                    });
                                let face = match generated {
                                    Some((producer, local_id)) => {
                                        if producer != feature_id
                                            && !dependencies.contains(producer)
                                        {
                                            dependencies.insert(ctx, producer.clone(), OPERATION)?;
                                        }
                                        let local_id_text = local_id.to_string();
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(producer.clone(), local_id_text) {
                                            Ok(face) => {
                                                let faces = vec![face];
                                                let native_copy = native.clone();
                                                cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                                    .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                                            }
                                            Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                                        }
                                    }
                                    None => cadmpeg_ir::features::FaceSelection::Native(native),
                                };
                                for producer in selection
                                    .producer_feature_refs
                                    .iter()
                                    .filter_map(|producer| feature_ids_by_native.get(producer))
                                    .filter(|producer| *producer != feature_id)
                                {
                                    if !dependencies.contains(producer) {
                                        dependencies.insert(ctx, producer.clone(), OPERATION)?;
                                    }
                                }
                                Ok(face)
                            });
                        let [center_faces, side_one_faces, side_two_faces] =
                            [center_faces?, side_one_faces?, side_two_faces?];
                        *definition = FeatureDefinition::Operation(FeatureOperation::FullRoundFillet {
                            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                                cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                                    center_faces,
                                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(
                                        side_one_faces,
                                    ),
                                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(
                                        side_two_faces,
                                    ), ctx,
                                )?
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                            ),
                        });
                        break 'feature_edit;
                    }
                    if matches!(
                        definition,
                        FeatureDefinition::Operation(FeatureOperation::Unresolved {
                            family: UnresolvedFamily::DatumPlane
                        })
                    ) && feature_selections.len() == 2
                    {
                        const OPERATION: &str = "project SLDPRT datum plane dependencies";
                        for selection in feature_selections {
                            for producer in selection
                                .producer_feature_refs
                                .iter()
                                .filter_map(|producer| feature_ids_by_native.get(producer))
                                .filter(|producer| *producer != feature_id)
                            {
                                if !dependencies.contains(producer) {
                                    let dependency = producer.clone();
                                    dependencies.insert(ctx, dependency, OPERATION)?;
                                }
                            }
                        }
                        break 'feature_edit;
                    }
                    let first_component = matches!(
                        definition,
                        FeatureDefinition::Operation(FeatureOperation::CosmeticThread { .. })
                    );
                    let Some(selection) = (if first_component {
                        cosmetic_thread_surface_selection_consensus(feature_selections)
                    } else {
                        surface_selection_consensus(feature_selections)
                    }) else {
                        break 'feature_edit;
                    };
                    if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                        reference,
                        ..
                    }) = definition
                    {
                        const OPERATION: &str = "project SLDPRT datum offset face";
                        let native = compact_surface_selection_value(&selection.components);
                        let generated = selection
                            .terminal_feature_ref
                            .as_ref()
                            .and_then(|producer| feature_ids_by_native.get(producer))
                            .zip(selection.components.last())
                            .and_then(|(feature, component)| Some((feature, component.local_id?)));
                        let face = match generated {
                            Some((producer, local_id)) => {
                                if !dependencies.contains(producer) {
                                    let dependency = producer.clone();
                                    dependencies.insert(ctx, dependency, OPERATION)?;
                                }
                                let producer_id = producer.clone();
                                let local_id_text = local_id.to_string();
                                match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) {
                                    Ok(face) => {
                                        let faces = vec![face];
                                        let native_copy = native.clone();
                                        cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                            .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                                    }
                                    Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                                }
                            }
                            None => cadmpeg_ir::features::FaceSelection::Native(native),
                        };
                        match reference {
                            Some(cadmpeg_ir::features::DatumPlaneReference::Face { face: existing }) => {
                                *existing = face;
                            }
                            reference @ (None
                            | Some(
                                cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. },
                            )) => {
                                *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face { face });
                            }
                            Some(cadmpeg_ir::features::DatumPlaneReference::Feature { .. }) => {}
                        }
                        break 'feature_edit;
                    }
                    let slot = match definition {
                        FeatureDefinition::Operation(FeatureOperation::Thicken { faces, .. }) => {
                            SelectionSlot::Face(faces)
                        }
                        FeatureDefinition::Operation(FeatureOperation::Shell { removed_faces, .. }) => {
                            SelectionSlot::Face(removed_faces)
                        }
                        FeatureDefinition::Operation(
                            FeatureOperation::OffsetSurface { faces, .. }
                            | FeatureOperation::KnitSurface { faces, .. }
                            | FeatureOperation::TrimSurface { faces, .. }
                            | FeatureOperation::ExtendSurface { faces, .. }
                            | FeatureOperation::Dome { faces, .. },
                        ) => SelectionSlot::Face(faces),
                        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                            support_faces,
                            ..
                        }) => SelectionSlot::Face(support_faces),
                        FeatureDefinition::Operation(FeatureOperation::Draft { faces, .. }) => {
                            SelectionSlot::Face(faces)
                        }
                        FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) => {
                            SelectionSlot::Face(face)
                        }
                        FeatureDefinition::Operation(FeatureOperation::Extrude {
                            extent:
                                cadmpeg_ir::features::ExtrudeExtent::OneSided {
                                    side:
                                        cadmpeg_ir::features::ExtrudeSide {
                                            termination:
                                                cadmpeg_ir::features::LinearTermination::ToFace { face, .. }
                                                | cadmpeg_ir::features::LinearTermination::OffsetFromFace {
                                                    face,
                                                    ..
                                                },
                                            ..
                                        },
                                },
                            ..
                        }) => SelectionSlot::Face(face),
                        FeatureDefinition::Operation(FeatureOperation::Extrude {
                            extent:
                                cadmpeg_ir::features::ExtrudeExtent::OneSided {
                                    side:
                                        cadmpeg_ir::features::ExtrudeSide {
                                            termination:
                                                cadmpeg_ir::features::LinearTermination::ToVertex { vertex },
                                            ..
                                        },
                                },
                            ..
                        }) => SelectionSlot::Vertex(vertex),
                        _ => break 'feature_edit,
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
                                        let producer_id = feature.clone();
                                        let local_id_text = local_id.to_string();
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) {
                                            Ok(face) => {
                                                let generated_faces = vec![face];
                                                let native_copy = native.clone();
                                                cadmpeg_ir::features::FaceSelection::generated(generated_faces, native_copy)
                                                    .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                                            }
                                            Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
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
                                        let producer_id = feature.clone();
                                        let local_id_text = local_id.to_string();
                                        match cadmpeg_ir::features::GeneratedVertexRef::new(producer_id, local_id_text) {
                                            Ok(generated_vertex) => {
                                                let native_copy = native.clone();
                                                match cadmpeg_ir::features::VertexSelection::generated(generated_vertex, native_copy) {
                                                    Ok(selection) => selection,
                                                    Err(_) => cadmpeg_ir::features::VertexSelection::native(native).unwrap_or(
                                                            cadmpeg_ir::features::VertexSelection::Unresolved,
                                                        ),
                                                }
                                            }
                                            Err(_) => cadmpeg_ir::features::VertexSelection::native(native)
                                                .unwrap_or(cadmpeg_ir::features::VertexSelection::Unresolved),
                                        }
                                    }
                                    None => cadmpeg_ir::features::VertexSelection::native(native)
                                        .unwrap_or(cadmpeg_ir::features::VertexSelection::Unresolved),
                                };
                            }
                        }
                    }
                    for producer in selection
                        .producer_feature_refs
                        .iter()
                        .filter_map(|producer| feature_ids_by_native.get(producer))
                        .filter(|producer| *producer != feature_id)
                    {
                        if !dependencies.contains(producer) {
                            let dependency = producer.clone();
                            dependencies.insert(ctx, dependency, OPERATION)?;
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    let mut face_aliases = HashMap::new();
    for feature in features.iter() {
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if matches!(face, FaceSelection::Unresolved | FaceSelection::Native(_)) {
            continue;
        }
        let native_key = native.to_string();
        let face_copy = face.clone();
        face_aliases.insert(native_key, face_copy);
    }
    for feature in features {
        let Some(target) = feature.source_properties.get("ReferenceFaceFeature") else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. })
        ) {
            continue;
        }
        let Some(face) = face_aliases.get(target.as_str()) else {
            continue;
        };
        let face = face.clone();
        if let FaceSelection::Generated { faces, .. } = &face {
            for producer in faces.iter().map(|face| &face.feature) {
                if producer != &feature.id && !feature.dependencies.contains(producer) {
                    let copy = producer.clone();
                    feature.dependencies.insert(ctx, copy, ALIAS_OPERATION)?;
                }
            }
        }
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference, ..
            }) = definition
            else {
                return;
            };
            if let Some(cadmpeg_ir::features::DatumPlaneReference::Face { face: existing }) =
                reference
            {
                *existing = face;
            } else if !matches!(
                reference,
                Some(cadmpeg_ir::features::DatumPlaneReference::Feature { .. })
            ) {
                *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face { face });
            }
        });
    }

    Ok(())
}

/// The edges and tangency weight of a fillet whose one group has no radius.
fn sole_unresolved_fillet_group(
    definition: &FeatureDefinition,
) -> Option<(&EdgeSelection, Option<cadmpeg_ir::scalar::FiniteReal>)> {
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
        return None;
    };
    let [group] = groups.as_slice() else {
        return None;
    };
    group
        .radius
        .is_unresolved()
        .then_some((&group.edges, group.tangency_weight))
}

fn full_round_fillet_selection_triple<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Option<[&'a FeatureInputSurfaceSelection; 3]> {
    let by_lane = surface_selections_by_lane(selections);
    let mut consensus: Option<[&'a FeatureInputSurfaceSelection; 3]> = None;
    for mut lane_selections in by_lane.into_values() {
        lane_selections.sort_unstable_by_key(|left| left.offset);
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

fn surface_selections_by_lane<'a>(
    selections: &[&'a FeatureInputSurfaceSelection],
) -> HashMap<&'a str, Vec<&'a FeatureInputSurfaceSelection>> {
    let mut by_lane = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in selections {
        let group = by_lane.entry(selection.parent.as_str()).or_default();
        group.push(*selection);
    }
    by_lane
}

pub(crate) fn project_draft_operands(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let id = feature.id.clone();
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        feature_ids_by_native.insert(native_ref.to_owned(), id);
    }
    let mut candidates = HashMap::<String, Vec<DraftOperands>>::new();
    for lane in lanes {
        for (feature, operands) in draft_operand_candidates(histories, lane)? {
            let by_feature = candidates.entry(feature).or_default();
            by_feature.push(operands);
        }
    }
    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                let Some(native_ref) = native_ref else {
                    return Ok(());
                };
                let Some(operands) = candidates.get(native_ref) else {
                    return Ok(());
                };
                let Some(first) = operands
                    .first()
                    .filter(|first| operands.iter().all(|item| same_draft_operands(first, item)))
                else {
                    return Ok(());
                };
                let pull_direction = first.pull_direction;

                let FeatureDefinition::Operation(FeatureOperation::Draft { faces, anchor, .. }) =
                    definition
                else {
                    return Ok(());
                };
                match (&first.anchor, &mut *anchor) {
                    (
                        DraftAnchor::NeutralPlane(path),
                        cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                            pull,
                        },
                    ) => {
                        let plane = draft_face_selection(
                            ctx,
                            std::slice::from_ref(path),
                            native_ref,
                            histories,
                            &feature_ids_by_native,
                            dependencies,
                        )?;
                        let pull = pull.take();
                        *anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, pull };
                    }
                    (
                        DraftAnchor::PartingTool(paths),
                        cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                            ..
                        },
                    ) => {
                        let tool = draft_face_selection(
                            ctx,
                            paths,
                            native_ref,
                            histories,
                            &feature_ids_by_native,
                            dependencies,
                        )?;
                        *anchor = cadmpeg_ir::features::DraftAnchor::PartingLine {
                            tool,
                            pull: cadmpeg_ir::features::DraftPull {
                                direction: pull_direction,
                                plane: None,
                            },
                        };
                    }
                    _ => {}
                }
                if matches!(faces, cadmpeg_ir::features::FaceSelection::Unresolved) {
                    *faces = draft_face_selection(
                        ctx,
                        &first.faces,
                        native_ref,
                        histories,
                        &feature_ids_by_native,
                        dependencies,
                    )?;
                }
                match anchor {
                    cadmpeg_ir::features::DraftAnchor::NeutralPlane { pull, .. }
                        if pull.is_none() =>
                    {
                        *pull = Some(cadmpeg_ir::features::DraftPull {
                            direction: pull_direction,
                            plane: None,
                        });
                    }
                    _ => {}
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn draft_face_selection(
    ctx: &DecodeContext<'_>,
    paths: &[Vec<crate::records::FeatureInputComponentPathEntry>],
    consumer_ref: &str,
    histories: &[crate::records::FeatureHistory],
    feature_ids_by_native: &HashMap<String, cadmpeg_ir::features::FeatureId>,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
) -> Result<cadmpeg_ir::features::FaceSelection, cadmpeg_core::CodecError> {
    const OPERATION: &str = "project SLDPRT draft face selections";
    let native = format_surface_path_set(
        paths.len(),
        |index| &paths[index],
        "sldprt:feature-input:draft-surface-vectors:",
    );
    let mut generated = Vec::new();
    let mut generated_dependencies = Vec::new();
    for path in paths {
        let Some((producer, local_id)) = component_path_terminal_feature(
            path,
            histories.iter().flat_map(|history| &history.features),
        )
        .filter(|producer| producer != consumer_ref)
        .and_then(|producer| {
            feature_ids_by_native
                .get(&producer)
                .zip(path.last()?.local_id.as_ref())
        }) else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        let producer_id = producer.clone();
        let local_id_text = local_id.to_string();
        let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text)
        else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        if !generated.contains(&face) {
            generated.push(face);
        }
        if !generated_dependencies.contains(producer) {
            let dependency = producer.clone();
            generated_dependencies.push(dependency);
        }
    }
    if generated.is_empty() {
        Ok(cadmpeg_ir::features::FaceSelection::Native(native))
    } else {
        for dependency in generated_dependencies {
            if !dependencies.contains(&dependency) {
                dependencies.insert(ctx, dependency, OPERATION)?;
            }
        }
        let native_copy = native.clone();
        Ok(
            cadmpeg_ir::features::FaceSelection::generated(generated, native_copy)
                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native)),
        )
    }
}

fn compact_surface_selection_set_value(selections: &[&FeatureInputSurfaceSelection]) -> String {
    format_surface_path_set(
        selections.len(),
        |index| &selections[index].components,
        "sldprt:feature-input:surface-selection-vectors:",
    )
}

fn format_surface_path_set<'a>(
    path_count: usize,
    path_at: impl Fn(usize) -> &'a [crate::records::FeatureInputComponentPathEntry],
    set_prefix: &'static str,
) -> String {
    const PATH_PREFIX: &str = "sldprt:feature-input:surface-component-ids:";
    let same_ids = |left: &[crate::records::FeatureInputComponentPathEntry],
                    right: &[crate::records::FeatureInputComponentPathEntry]| {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| left.local_id == right.local_id)
    };
    let is_duplicate =
        |index: usize| (0..index).any(|previous| same_ids(path_at(previous), path_at(index)));
    let unique_count = (0..path_count)
        .filter(|&index| !is_duplicate(index))
        .count();
    let mut value = String::new();
    if unique_count != 1 {
        value.push_str(set_prefix);
    }
    let mut emitted = 0usize;
    for index in 0..path_count {
        if is_duplicate(index) {
            continue;
        }
        if emitted != 0 {
            value.push(';');
        }
        value.push_str(PATH_PREFIX);
        for (component_index, component) in path_at(index).iter().enumerate() {
            if component_index != 0 {
                value.push(',');
            }
            match component.local_id {
                Some(local_id) => value.push_str(&local_id.to_string()),
                None => value.push('_'),
            }
        }
        emitted += 1;
    }
    value
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
        && left.components.len() == right.components.len()
        && left
            .components
            .iter()
            .zip(&right.components)
            .all(|(left, right)| {
                left.local_id == right.local_id
                    && left.type_signature[4..8] == right.type_signature[4..8]
            })
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
    let by_lane = surface_selections_by_lane(selections);
    let mut consensus = None;
    for mut lane_selections in by_lane.into_values() {
        if lane_selections.len() != 2 {
            return None;
        }
        lane_selections.sort_unstable_by_key(|left| left.offset);
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
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut native_features = HashMap::new();
    let mut history_features = Vec::new();
    for native_feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(native_feature.id.as_str(), native_feature);
        history_features.push(native_feature);
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        feature_ids_by_native.insert(native_ref.to_owned(), feature.id.clone());
    }
    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                const GENERATED_OPERATION: &str = "resolve SLDPRT cosmetic thread generated face";

                let Some(native_ref) = native_ref else {
                    return Ok(());
                };
                let Some(native_feature) = native_features.get(native_ref).copied() else {
                    return Ok(());
                };
                let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                    face,
                    diameter,
                    ..
                }) = definition
                else {
                    return Ok(());
                };
                if !matches!(
                    face,
                    cadmpeg_ir::features::FaceSelection::Unresolved
                        | cadmpeg_ir::features::FaceSelection::Native(_)
                ) {
                    return Ok(());
                }
                let mut references = Vec::<(
                    String,
                    Option<std::borrow::Cow<'_, [crate::records::FeatureInputComponentPathEntry]>>,
                    Option<&str>,
                )>::new();
                for lane in lanes {
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    for selection in &lane.surface_selections {
                        if selection.feature_ref != native_feature.id {
                            continue;
                        }
                        references.push((
                            format!("{lane_key}:{}", selection.offset),
                            Some(std::borrow::Cow::Borrowed(selection.components.as_slice())),
                            selection.producer_feature_refs.first().map(String::as_str),
                        ));
                    }
                }
                for lane in lanes {
                    let Some((_, start, end)) = feature_object_byte_ranges(histories, lane)
                        .get(native_feature.id.as_str())
                        .copied()
                    else {
                        continue;
                    };
                    let mut cylinder_tokens = HashSet::new();
                    for class in &lane.classes {
                        if class.name != "moCylinderRef_w" {
                            continue;
                        }
                        let token = usize::try_from(class.offset)
                            .ok()
                            .and_then(|offset| {
                                class
                                    .name
                                    .len()
                                    .checked_add(6)
                                    .and_then(|width| offset.checked_add(width))
                            })
                            .and_then(|body| View::u16_le_at(&lane.native_payload, body))
                            .filter(|token| is_class_token(*token));
                        let Some(token) = token else {
                            continue;
                        };
                        cylinder_tokens.insert(token);
                    }
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    for super::selections::CylinderMarkerReference(marker, components) in
                        cosmetic_thread_cylinder_marker_reference(
                            ctx,
                            native_feature,
                            lane,
                            start,
                            end,
                            &cylinder_tokens,
                        )?
                    {
                        references.push((
                            format!("{lane_key}:{marker}"),
                            components.map(std::borrow::Cow::Owned),
                            None,
                        ));
                    }
                }
                references.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                let native = if references.is_empty() {
                    None
                } else {
                    const PREFIX: &str = "sldprt:feature-input:cylinder-reference:";
                    let mut native = PREFIX.to_owned();
                    let mut last = None;
                    for (reference, _, _) in &references {
                        if last == Some(reference.as_str()) {
                            continue;
                        }
                        if last.is_some() {
                            native.push(',');
                        }
                        native.push_str(reference);
                        last = Some(reference.as_str());
                    }
                    Some(native)
                };
                let mut generated = None;
                let mut complete = true;
                for (_, components, explicit_producer) in &references {
                    let candidate = match components.as_deref() {
                        None => None,
                        Some(components) => {
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
                            let selected = match explicit {
                                Some(selected) => Some(selected),
                                None => component_path_feature(
                                    components,
                                    &history_features,
                                    native_feature.id.as_str(),
                                    ComponentPathEnd::Leading,
                                ),
                            };
                            selected.and_then(|(component, producer)| {
                                Some((
                                    feature_ids_by_native.get(producer.id.as_str())?,
                                    component.local_id?,
                                ))
                            })
                        }
                    };
                    let Some(candidate) = candidate else {
                        complete = false;
                        break;
                    };
                    if let Some(previous) = generated {
                        if previous != candidate {
                            complete = false;
                            break;
                        }
                    } else {
                        generated = Some(candidate);
                    }
                }
                if let Some((producer, local_id)) = generated.filter(|_| complete) {
                    let Some(native) = native else {
                        return Ok(());
                    };
                    let producer_id = producer.clone();
                    let local_id_text = local_id.to_string();
                    *face = match cadmpeg_ir::features::GeneratedFaceRef::new(
                        producer_id,
                        local_id_text,
                    ) {
                        Ok(generated_face) => {
                            let faces = vec![generated_face];
                            let native_copy = native.clone();
                            cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                        }
                        Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                    if producer != feature_id && !dependencies.contains(producer) {
                        let dependency = producer.clone();
                        dependencies.insert(ctx, dependency, GENERATED_OPERATION)?;
                    }
                    return Ok(());
                }
                let Some(diameter) = diameter else {
                    return Ok(());
                };
                let diameter = diameter.get();

                let selected = match unique_cylindrical_face(diameter * 0.5, faces, surfaces) {
                    Some(selected) => Some(selected),
                    None if native.is_some() => {
                        unique_topological_cylindrical_face(faces, surfaces)
                    }
                    None => None,
                };
                let Some(selected) = selected else {
                    return Ok(());
                };
                let selected_faces = vec![selected];
                *face = match native {
                    Some(native) => cadmpeg_ir::features::FaceSelection::Resolved {
                        faces: selected_faces,
                        native,
                    },
                    None => cadmpeg_ir::features::FaceSelection::Faces(selected_faces),
                };
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn unique_cylindrical_face(radius: f64, faces: &[Face], surfaces: &[Surface]) -> Option<FaceId> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance = (radius.abs() * EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9)
        .max(EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9);
    unique_matching_face(faces, surfaces, |surface| match surface.geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            (cylinder_surface.radius().get() - radius).abs() <= tolerance
        }
        _ => false,
    })
}

fn unique_matching_face(
    faces: &[Face],
    surfaces: &[Surface],
    mut matches_surface: impl FnMut(&Surface) -> bool,
) -> Option<FaceId> {
    let mut selected = None;
    for face in faces {
        for surface in surfaces {
            if face.surface != surface.id || !matches_surface(surface) {
                continue;
            }
            if selected.is_some() {
                return None;
            }
            selected = Some(&face.id);
            break;
        }
    }
    selected.cloned()
}

fn unique_topological_cylindrical_face(faces: &[Face], surfaces: &[Surface]) -> Option<FaceId> {
    unique_matching_face(faces, surfaces, |surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
        )
    })
}

/// Resolve frame-only offset-plane supports when exactly one B-rep face lies
/// on the serialized support plane.
pub(crate) fn project_unbound_offset_plane_faces(
    features: &mut [cadmpeg_ir::features::Feature],
    faces: &[Face],
    surfaces: &[Surface],
) {
    for feature in features {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference, ..
            }) = definition
            else {
                return;
            };
            let (origin, normal) = match reference.as_ref() {
                Some(cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { frame }) => {
                    (frame.origin(), frame.normal().get())
                }
                _ => return,
            };
            let Some(selected) = unique_planar_face(origin.get(), normal, faces, surfaces) else {
                return;
            };
            *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                face: cadmpeg_ir::features::FaceSelection::Faces(vec![selected]),
            });
        });
    }
}

fn unique_planar_face(
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
    let tolerance = EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E8
        * origin
            .x
            .abs()
            .max(origin.y.abs())
            .max(origin.z.abs())
            .max(1.0);
    unique_matching_face(faces, surfaces, |surface| match surface.geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let candidate_origin = plane_surface.origin().get();
            let candidate_normal = *plane_surface.frame().axis().as_raw();
            let candidate_length = candidate_normal.norm();
            if !candidate_length.is_finite() || candidate_length <= f64::EPSILON {
                return false;
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
            let distance =
                displacement.x * normal.x + displacement.y * normal.y + displacement.z * normal.z;
            (alignment.abs() - 1.0).abs() <= EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E9
                && distance.abs() <= tolerance
        }
        _ => false,
    })
}

#[cfg(test)]
mod projections_tests;
