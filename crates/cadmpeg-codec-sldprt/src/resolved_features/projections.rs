//! Parameter scalar and compact selection projection.

use super::component_paths::{
    compact_body_selection_value_charged, compact_edge_path_value_charged,
    compact_edge_selection_set_value_charged, component_path_feature,
    component_path_terminal_feature, ComponentPathEnd,
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

fn copy_projection_feature_id(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::FeatureId,
    operation: &'static str,
) -> Result<cadmpeg_ir::features::FeatureId, cadmpeg_core::CodecError> {
    let text = crate::text_admission::format_retained(
        ctx,
        format_args!("{}", source.as_str()),
        operation,
    )?;
    cadmpeg_ir::features::FeatureId::mint(text)
        .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT feature id"))
}

fn scoped_reference_name<'a>(
    ctx: &'a DecodeContext<'_>,
    source_name: &str,
    offset: Option<u64>,
    suffix: Option<u32>,
    operation: &'static str,
) -> Result<(String, cadmpeg_core::decode::ScopedReservation<'a>), cadmpeg_core::CodecError> {
    let mut len = source_name
        .len()
        .checked_add("@reference".len())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    if let Some(offset) = offset {
        let digits = offset
            .checked_ilog10()
            .map_or(1, |digits| digits as usize + 1);
        len = len
            .checked_add(1 + digits)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
    if let Some(suffix) = suffix {
        let digits = suffix
            .checked_ilog10()
            .map_or(1, |digits| digits as usize + 1);
        len = len
            .checked_add(1 + digits)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
    let (mut name, reservation) =
        crate::text_admission::reserve_scoped_string(ctx, len, operation)?;
    name.push_str(source_name);
    name.push_str("@reference");
    if let Some(offset) = offset {
        write!(&mut name, ":{offset}").map_err(|_| {
            cadmpeg_core::CodecError::malformed("cannot format SLDPRT reference name")
        })?;
    }
    if let Some(suffix) = suffix {
        write!(&mut name, ":{suffix}").map_err(|_| {
            cadmpeg_core::CodecError::malformed("cannot format SLDPRT reference name")
        })?;
    }
    Ok((name, reservation))
}

pub(super) fn bind_circular_profile_by_dimension(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "bind SLDPRT circular profile by dimension";
    let mut proposals = Vec::new();
    for (sketch_index, sketch) in sketches.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let [profile] = sketch.profiles.as_slice() else {
            continue;
        };
        let [entity_use] = profile.as_slice() else {
            continue;
        };
        let mut geometry = None;
        for entity in sketch_entities.iter().rev() {
            ctx.charge_work(1, OPERATION)?;
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
            ctx.charge_work(1, OPERATION)?;
            if !matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ) {
                continue;
            }
            let mut matches_dimension = false;
            for parameter in parameters {
                ctx.charge_work(1, OPERATION)?;
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
                ctx.reserve_collection_vec(&mut proposals, 1, OPERATION)?;
                proposals.push((sketch_index, feature_index));
            }
        }
    }
    let mut feature_counts = HashMap::<usize, usize>::new();
    for &(_, feature_index) in &proposals {
        ctx.charge_work(1, OPERATION)?;
        if !feature_counts.contains_key(&feature_index) {
            ctx.charge_collection_items(1, OPERATION)?;
            feature_counts
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        let count = feature_counts.entry(feature_index).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    }
    for (sketch_index, feature_index) in proposals {
        if feature_counts.get(&feature_index) != Some(&1) {
            continue;
        }
        let sketch_id_text = crate::text_admission::format_retained(
            ctx,
            format_args!("{}", sketches[sketch_index].id.as_str()),
            OPERATION,
        )?;
        let sketch_id = cadmpeg_ir::sketches::SketchId::mint(sketch_id_text)
            .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT sketch ID"))?;
        let name_index = sketches.iter().position(|sketch| sketch.id == sketch_id);
        for feature in features.iter_mut() {
            ctx.charge_work(1, OPERATION)?;
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
        let name = features[feature_index]
            .name
            .as_deref()
            .map(|name| {
                crate::text_admission::format_retained(ctx, format_args!("{name}"), OPERATION)
            })
            .transpose()?;
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
    Ok(())
}

/// Bind neutral parameters to uniquely owned native scalar records.
pub(crate) fn bind_parameter_scalars<'a>(
    ctx: &DecodeContext<'_>,
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "bind SLDPRT parameter scalars";
    let mut neutral_owners = HashMap::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !neutral_owners.contains_key(&feature.id) {
            ctx.charge_collection_items(1, OPERATION)?;
            neutral_owners
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        neutral_owners.insert(&feature.id, native_ref);
    }
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        if !native_features.contains_key(feature.id.as_str()) {
            ctx.charge_collection_items(1, OPERATION)?;
            native_features
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        native_features.insert(feature.id.as_str(), feature);
    }
    for lane in lanes {
        ctx.charge_work(1, OPERATION)?;
        let mut length_scalars = HashSet::new();
        let mut angle_scalars = HashSet::new();
        let mut detached_scalars = HashSet::new();
        for relation in &lane.relation_instances {
            ctx.charge_work(1, OPERATION)?;
            let Some(id) = relation.parameter_scalar_ref() else {
                continue;
            };
            let family = if relation.family == FeatureInputRelationFamily::Angle {
                &mut angle_scalars
            } else {
                &mut length_scalars
            };
            if !family.contains(id) {
                ctx.charge_collection_items(1, OPERATION)?;
                family
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                family.insert(id);
            }
            let mut detached = false;
            for scalar in &lane.scalars {
                ctx.charge_work(1, OPERATION)?;
                if scalar.id == id {
                    detached = scalar.operands.is_empty();
                    break;
                }
            }
            if detached && !detached_scalars.contains(id) {
                ctx.charge_collection_items(1, OPERATION)?;
                detached_scalars
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                detached_scalars.insert(id);
            }
        }
        let mut names_by_id = HashMap::new();
        for name in &lane.names {
            ctx.charge_work(1, OPERATION)?;
            if !names_by_id.contains_key(name.id.as_str()) {
                ctx.charge_collection_items(1, OPERATION)?;
                names_by_id
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
            names_by_id.insert(name.id.as_str(), name);
        }
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            ctx.charge_work(1, OPERATION)?;
            let start = feature_object_name(feature, lane).map_or(u64::MAX, |name| name.offset);
            ctx.reserve_collection_vec(&mut starts, 1, OPERATION)?;
            starts.push((start, feature));
        }
        ctx.charge_work(starts.len() as u64, OPERATION)?;
        ctx.stable_sort_by(
            &mut starts,
            |left, right| left.0.cmp(&right.0),
            |_| 0,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            for parameter in parameters.iter_mut() {
                ctx.charge_work(1, OPERATION)?;
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
                    ctx.charge_work(1, OPERATION)?;
                    let owned = match scalar.feature_ref.as_deref() {
                        Some(owner) => owner == native_feature.id,
                        None => scalar.offset > start && scalar.offset < end,
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
                        ctx.reserve_collection_vec(&mut scalars, 1, OPERATION)?;
                        scalars.push(scalar);
                    }
                }
                let mut driving = Vec::new();
                for scalar in &scalars {
                    ctx.charge_work(1, OPERATION)?;
                    if scalar.role == FeatureInputScalarRole::Driving {
                        ctx.reserve_collection_vec(&mut driving, 1, OPERATION)?;
                        driving.push(*scalar);
                    }
                }
                let mut candidates = Vec::new();
                if driving.is_empty() {
                    for scalar in scalars {
                        ctx.charge_work(1, OPERATION)?;
                        if scalar.role == FeatureInputScalarRole::Native {
                            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push(scalar);
                        }
                    }
                } else {
                    candidates = driving;
                }
                let mut compatible = Vec::new();
                for scalar in candidates {
                    ctx.charge_work(1, OPERATION)?;
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
                        ctx.reserve_collection_vec(&mut compatible, 1, OPERATION)?;
                        compatible.push(scalar);
                    }
                }
                if let [scalar] = compatible.as_slice() {
                    parameter.native_ref = Some(crate::text_admission::format_retained(
                        ctx,
                        format_args!("{}", scalar.id),
                        OPERATION,
                    )?);
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    features: &[cadmpeg_ir::features::Feature],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "synthesize SLDPRT display relation parameter";
    let mut lane_refs = Vec::new();
    for lane in lanes {
        ctx.reserve_collection_vec(&mut lane_refs, 1, "collect SLDPRT display relation lanes")?;
        lane_refs.push(lane);
    }
    let owned = owned_relation_parameters(ctx, features, parameters, lane_refs.iter().copied())?;
    let mut features_by_native_ref = HashMap::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !features_by_native_ref.contains_key(native_ref) {
            ctx.charge_collection_items(1, OPERATION)?;
            features_by_native_ref
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        features_by_native_ref.insert(native_ref, feature);
    }
    let mut relation_ids = HashSet::new();
    let mut parameter_ids = HashSet::new();
    let mut names_by_owner = HashSet::new();
    let mut next_ordinals = HashMap::<cadmpeg_ir::features::FeatureId, u32>::new();
    for parameter in parameters.iter() {
        ctx.charge_work(1, OPERATION)?;
        if let Some(relation_id) = parameter.properties.get(RELATION_PARAMETER_ID_PROPERTY) {
            if !relation_ids.contains(relation_id.as_str()) {
                let id = crate::text_admission::format_retained(
                    ctx,
                    format_args!("{relation_id}"),
                    OPERATION,
                )?;
                ctx.charge_collection_items(1, OPERATION)?;
                relation_ids
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                relation_ids.insert(id);
            }
        }
        if !parameter_ids.contains(&parameter.id) {
            let id_text = crate::text_admission::format_retained(
                ctx,
                format_args!("{}", parameter.id.as_str()),
                OPERATION,
            )?;
            let id = ParameterId::mint(id_text)
                .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT parameter ID"))?;
            ctx.charge_collection_items(1, OPERATION)?;
            parameter_ids
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            parameter_ids.insert(id);
        }
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        ctx.charge_work(names_by_owner.len() as u64, OPERATION)?;
        if !names_by_owner
            .iter()
            .any(|(known_owner, known_name)| known_owner == owner && known_name == &parameter.name)
        {
            let owner_copy = copy_projection_feature_id(ctx, owner, OPERATION)?;
            let name_copy = crate::text_admission::format_retained(
                ctx,
                format_args!("{}", parameter.name),
                OPERATION,
            )?;
            ctx.charge_collection_items(1, OPERATION)?;
            names_by_owner
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            names_by_owner.insert((owner_copy, name_copy));
        }
        let next = if parameter.ordinal == u32::MAX {
            parameter.ordinal
        } else {
            parameter.ordinal + 1
        };
        if let Some(current) = next_ordinals.get_mut(owner) {
            *current = (*current).max(next);
        } else {
            let owner_copy = copy_projection_feature_id(ctx, owner, OPERATION)?;
            ctx.charge_collection_items(1, OPERATION)?;
            next_ordinals
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            next_ordinals.insert(owner_copy, next);
        }
    }

    for lane in lane_refs {
        for relation in &lane.relation_instances {
            ctx.charge_work(1, OPERATION)?;
            if relation.parameter_scalar_ref().is_some()
                || owned.get(&relation.id).is_some_and(Option::is_some)
                || relation_ids.contains(&relation.id)
            {
                continue;
            }
            let Some(scalar) = relation_display_scalar_for_parameter(ctx, relation, lane)? else {
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
                let owner_copy = copy_projection_feature_id(ctx, owner, OPERATION)?;
                ctx.charge_collection_items(1, OPERATION)?;
                next_ordinals
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                next_ordinals.insert(owner_copy, next_ordinal);
            }
            let mut candidate = scoped_reference_name(ctx, source_name, None, None, OPERATION)?;
            ctx.charge_work(names_by_owner.len() as u64, OPERATION)?;
            if names_by_owner
                .iter()
                .any(|(known_owner, known_name)| known_owner == owner && known_name == &candidate.0)
            {
                candidate = scoped_reference_name(
                    ctx,
                    source_name,
                    Some(relation.offset),
                    None,
                    OPERATION,
                )?;
                let mut suffix = 0u32;
                loop {
                    ctx.charge_work(names_by_owner.len() as u64, OPERATION)?;
                    if !names_by_owner.iter().any(|(known_owner, known_name)| {
                        known_owner == owner && known_name == &candidate.0
                    }) {
                        break;
                    }
                    suffix = suffix
                        .checked_add(1)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    candidate = scoped_reference_name(
                        ctx,
                        source_name,
                        Some(relation.offset),
                        Some(suffix),
                        OPERATION,
                    )?;
                }
            }
            let relation_key = relation
                .id
                .rsplit_once('#')
                .map_or(relation.id.as_str(), |(_, key)| key);
            let (mut key_text, _key_reservation) =
                crate::text_admission::reserve_scoped_string(ctx, relation_key.len(), OPERATION)?;
            key_text.push_str(relation_key);
            let Ok(relation_key) = cadmpeg_ir::ids::IdentityKey::try_new(key_text) else {
                continue;
            };
            let id_text = crate::text_admission::format_retained(
                ctx,
                format_args!("sldprt:model:parameter#reference:{relation_key}"),
                OPERATION,
            )?;
            let Ok(id) = ParameterId::mint(id_text) else {
                continue;
            };
            if parameter_ids.contains(&id) {
                continue;
            }
            let id_copy = ParameterId::mint(crate::text_admission::format_retained(
                ctx,
                format_args!("{}", id.as_str()),
                OPERATION,
            )?)
            .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT parameter ID"))?;
            ctx.charge_collection_items(1, OPERATION)?;
            parameter_ids
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            parameter_ids.insert(id_copy);
            let mut properties = BTreeMap::new();
            ctx.charge_collection_items(1, OPERATION)?;
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ID_PROPERTY),
                crate::text_admission::format_retained(
                    ctx,
                    format_args!("{}", relation.id),
                    OPERATION,
                )?,
            );
            ctx.charge_collection_items(1, OPERATION)?;
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_DISPLAY_SCALAR_ID_PROPERTY),
                crate::text_admission::format_retained(
                    ctx,
                    format_args!("{}", scalar.id),
                    OPERATION,
                )?,
            );
            ctx.charge_collection_items(1, OPERATION)?;
            properties.insert(
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ROLE_PROPERTY),
                RELATION_PARAMETER_ROLE_REFERENCE.into(),
            );
            ctx.charge_collection_items(1, OPERATION)?;
            properties.insert(
                cadmpeg_core::nonblank_literal!("source_name"),
                crate::text_admission::format_retained(
                    ctx,
                    format_args!("{source_name}"),
                    OPERATION,
                )?,
            );
            let name = candidate.0;
            let parameter_name =
                crate::text_admission::format_retained(ctx, format_args!("{name}"), OPERATION)?;
            let owner_for_parameter = copy_projection_feature_id(ctx, owner, OPERATION)?;
            ctx.reserve_collection_vec(parameters, 1, OPERATION)?;
            parameters.push(DesignParameter {
                id,
                owner: Some(owner_for_parameter),
                ordinal: current_ordinal,
                name: parameter_name,
                expression,
                display,
                value: Some(value),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties,
                pmi: None,
                native_ref: None,
            });
            let owner_for_index = copy_projection_feature_id(ctx, owner, OPERATION)?;
            let name_for_index =
                crate::text_admission::format_retained(ctx, format_args!("{name}"), OPERATION)?;
            ctx.charge_collection_items(1, OPERATION)?;
            names_by_owner
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            names_by_owner.insert((owner_for_index, name_for_index));
            if !relation_ids.contains(relation.id.as_str()) {
                let relation_id = crate::text_admission::format_retained(
                    ctx,
                    format_args!("{}", relation.id),
                    OPERATION,
                )?;
                ctx.charge_collection_items(1, OPERATION)?;
                relation_ids
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                relation_ids.insert(relation_id);
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "group SLDPRT display relation families";

    let ownership = owned_relation_parameters(ctx, features, parameters, lanes)?;
    let mut families = HashMap::<&cadmpeg_ir::features::ParameterId, HashSet<_>>::new();
    for relation in lanes.iter().flat_map(|lane| &lane.relation_instances) {
        ctx.charge_work(1, OPERATION)?;
        if let Some(Some(parameter)) = ownership.get(&relation.id) {
            if !families.contains_key(parameter) {
                ctx.charge_collection_items(1, OPERATION)?;
                families
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
            let family_set = families.entry(parameter).or_default();
            if !family_set.contains(&relation.family) {
                ctx.charge_collection_items(1, OPERATION)?;
                family_set
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
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
    const OPERATION: &str = "project SLDPRT compact body selections";
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut selected = None;
        let mut duplicate = false;
        for selection in lanes.iter().flat_map(|lane| &lane.body_selections) {
            ctx.charge_work(1, OPERATION)?;
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
                    ctx.reserve_collection_vec(
                        &mut ids,
                        selection.local_body_ids.len(),
                        OPERATION,
                    )?;
                    for id in &selection.local_body_ids {
                        let digits = id.to_string();
                        let mut text = String::new();
                        crate::text_admission::reserve_retained_string(
                            ctx,
                            &mut text,
                            digits.len(),
                            OPERATION,
                        )?;
                        text.push_str(&digits);
                        ids.push(text);
                    }
                    let Ok(selection) = cadmpeg_ir::features::BodySelection::local_charged(
                        ids,
                        compact_body_selection_value_charged(ctx, &selection.local_body_ids)?,
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
    const INDEX_OPERATION: &str = "index SLDPRT compact edge selections";
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_ref() else {
            continue;
        };
        ctx.charge_work(1, INDEX_OPERATION)?;
        let mut id_text = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut id_text,
            feature.id.as_str().len(),
            INDEX_OPERATION,
        )?;
        id_text.push_str(feature.id.as_str());
        let id = cadmpeg_ir::features::FeatureId::mint(id_text)
            .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT feature id"))?;
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        ctx.charge_collection_items(1, INDEX_OPERATION)?;
        feature_ids_by_native
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
        let mut native = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut native,
            native_ref.len(),
            INDEX_OPERATION,
        )?;
        native.push_str(native_ref);
        feature_ids_by_native.insert(native, id);
    }
    let mut selections = HashMap::<&str, Vec<&FeatureInputEdgeSelection>>::new();
    for selection in lanes.iter().flat_map(|lane| &lane.edge_selections) {
        ctx.charge_work(1, INDEX_OPERATION)?;
        if !selections.contains_key(selection.feature_ref.as_str()) {
            ctx.charge_collection_items(1, INDEX_OPERATION)?;
            selections
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        let group = selections
            .entry(selection.feature_ref.as_str())
            .or_default();
        ctx.reserve_collection_vec(group, 1, INDEX_OPERATION)?;
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
                const OPERATION: &str = "project SLDPRT compact generated edges";
                let native = compact_edge_selection_set_value_charged(ctx, selections)?;
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
                    let mut id_text = String::new();
                    crate::text_admission::reserve_retained_string(ctx, &mut id_text, feature_id.as_str().len(), OPERATION)?;
                    id_text.push_str(feature_id.as_str());
                    let feature = cadmpeg_ir::features::FeatureId::mint(id_text)
                        .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT generated edge feature id"))?;
                    let local_id = compact_edge_path_value_charged(ctx, selection)?;
                    let Ok(edge) = cadmpeg_ir::features::GeneratedEdgeRef::new(feature, local_id) else {
                        complete = false;
                        break;
                    };
                    ctx.charge_work(generated.len() as u64, OPERATION)?;
                    if !generated.contains(&edge) {
                        ctx.reserve_collection_vec(&mut generated, 1, OPERATION)?;
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
                    variable_fillet_radius_groups(ctx, native_ref, histories, lanes, edge_selections)?
                {
                    let unresolved_edges = matches!(existing_edges, EdgeSelection::Unresolved);
                    if unresolved_edges || radius_groups.len() == 1 {
                        const GROUP_OPERATION: &str = "collect SLDPRT variable fillet groups";
                        let mut replacement_groups = Vec::new();
                        ctx.reserve_collection_vec(&mut replacement_groups, radius_groups.len(), GROUP_OPERATION)?;
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
                    ctx.charge_work(dependencies.len() as u64, DEPENDENCY_OPERATION)?;
                    if dependencies.contains(dependency) {
                        continue;
                    }
                    let mut id_text = String::new();
                    crate::text_admission::reserve_retained_string(ctx, &mut id_text, dependency.as_str().len(), DEPENDENCY_OPERATION)?;
                    id_text.push_str(dependency.as_str());
                    let id = cadmpeg_ir::features::FeatureId::mint(id_text)
                        .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT edge dependency id"))?;
                    ctx.charge_work(dependencies.len() as u64, DEPENDENCY_OPERATION)?;
                    dependencies.try_insert_charged(id, ctx, DEPENDENCY_OPERATION)?;
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
    ctx: &DecodeContext<'_>,
    feature_ref: &str,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    selections: &[&'a FeatureInputEdgeSelection],
) -> Result<Option<Vec<RadiusSelectionGroup<'a>>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "project SLDPRT variable fillet radii";
    let charge_sort = |len: usize| {
        let levels = if len > 1 { len.ilog2() + 1 } else { 1 };
        let count = u64::try_from(len)
            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        let units = count
            .checked_mul(u64::from(levels))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(units, OPERATION)
    };
    let Some(history) = histories.iter().find(|history| {
        history
            .features
            .iter()
            .any(|feature| feature.id == feature_ref)
    }) else {
        return Ok(None);
    };
    let Some(feature) = history.features.iter().find(|feature| {
        feature.id == feature_ref && feature.kind.eq_ignore_ascii_case("VarFillet")
    }) else {
        return Ok(None);
    };
    let mut parameter_names = HashSet::new();
    for name in feature.parameters.keys() {
        ctx.charge_work(1, OPERATION)?;
        if variable_fillet_dimension_index_for_feature(feature, name.as_str()).is_some() {
            ctx.charge_collection_items(1, OPERATION)?;
            parameter_names
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            parameter_names.insert(name);
        }
    }
    if parameter_names.len() != feature.parameters.len() || parameter_names.len() < 2 {
        return Ok(None);
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
        ctx.reserve_collection_vec(&mut ordered_parameters, parameter_names.len(), OPERATION)?;
        for name in &parameter_names {
            let Some(parameter) =
                variable_fillet_dimension_index_for_feature(feature, name.as_str()).zip(
                    feature.parameters.get(*name).and_then(|value| {
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    }),
                )
            else {
                return Ok(None);
            };
            ordered_parameters.push(parameter);
        }
        charge_sort(ordered_parameters.len())?;
        ordered_parameters.sort_unstable_by_key(|(index, _)| *index);
        if ordered_parameters
            .iter()
            .enumerate()
            .all(|(expected, (actual, _))| expected == *actual)
        {
            let mut selections_copy = Vec::new();
            ctx.reserve_collection_vec(&mut selections_copy, selections.len(), OPERATION)?;
            selections_copy.extend_from_slice(selections);
            let mut selections = selections_copy;
            charge_sort(selections.len())?;
            selections.sort_unstable_by_key(|selection| selection.ordinal);
            let points = ordered_parameters
                .into_iter()
                .enumerate()
                .map(|(parameter, (_, radius))| {
                    Some(VariableRadius {
                        parameter: parameter as f64,
                        radius: Length::from(radius),
                    })
                })
                .collect::<Option<Vec<_>>>();
            let Some(points) = points else {
                return Ok(None);
            };
            let Some(points) =
                cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok()
            else {
                return Ok(None);
            };
            return Ok(Some(vec![RadiusSelectionGroup(
                RadiusSpec::Variable { points },
                selections,
            )]));
        }
    }

    let mut vertex_radii = HashMap::<[u8; 12], PositiveLength>::new();
    let mut control_names = HashSet::<String>::new();
    let mut non_vertex_control_names = HashSet::<String>::new();
    let mut non_vertex_control_references = Vec::new();
    for lane in lanes {
        let mut objects = Vec::new();
        for candidate in &history.features {
            ctx.charge_work(lane.names.len() as u64, OPERATION)?;
            ctx.charge_work(lane.names.len() as u64, OPERATION)?;
            if let Some(name) = feature_object_name(candidate, lane) {
                ctx.reserve_collection_vec(&mut objects, 1, OPERATION)?;
                objects.push((name.offset, candidate));
            }
        }
        charge_sort(objects.len())?;
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
        let Some(controls) = variable_fillet_control_references(ctx, feature, lane, object_end)?
        else {
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
                        return Ok(None);
                    }
                    ctx.charge_collection_items(1, OPERATION)?;
                    control_names
                        .try_reserve(1)
                        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    let mut retained_name = String::new();
                    crate::text_admission::reserve_retained_string(
                        ctx,
                        &mut retained_name,
                        name.len(),
                        OPERATION,
                    )?;
                    retained_name.push_str(&name);
                    control_names.insert(retained_name);
                    let Some(radius) = feature.parameters.get(name.as_str()).and_then(|value| {
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    }) else {
                        return Ok(None);
                    };
                    if !vertex_radii.contains_key(&vertex.type_signature) {
                        ctx.charge_collection_items(1, OPERATION)?;
                        vertex_radii.try_reserve(1).map_err(|_| {
                            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                        })?;
                    }
                    match vertex_radii.entry(vertex.type_signature) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(radius);
                        }
                        std::collections::hash_map::Entry::Occupied(entry)
                            if !same_dimension_length(entry.get().get(), radius.get()) =>
                        {
                            return Ok(None);
                        }
                        std::collections::hash_map::Entry::Occupied(_) => {}
                    }
                }
                (None, None) => {
                    if non_vertex_control_names.contains(&name) {
                        return Ok(None);
                    }
                    ctx.charge_collection_items(1, OPERATION)?;
                    non_vertex_control_names
                        .try_reserve(1)
                        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    non_vertex_control_names.insert(name);
                    ctx.reserve_collection_vec(
                        &mut non_vertex_control_references,
                        references.len(),
                        OPERATION,
                    )?;
                    non_vertex_control_references.extend(references);
                }
                _ => return Ok(None),
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
            return Ok(None);
        }
        let mut ordered_parameters = Vec::new();
        ctx.reserve_collection_vec(&mut ordered_parameters, parameter_names.len(), OPERATION)?;
        for name in &parameter_names {
            let Some(parameter) =
                variable_fillet_dimension_index_for_feature(feature, name.as_str()).zip(
                    feature.parameters.get(*name).and_then(|value| {
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    }),
                )
            else {
                return Ok(None);
            };
            ordered_parameters.push(parameter);
        }
        charge_sort(ordered_parameters.len())?;
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
            return Ok(None);
        }
        for reference in &non_vertex_control_references {
            let mut present = false;
            for selected in selections
                .iter()
                .flat_map(|selection| selection.references.iter())
            {
                ctx.charge_work(1, OPERATION)?;
                if selected == reference {
                    present = true;
                    break;
                }
            }
            if !present {
                return Ok(None);
            }
        }
        let mut selections_copy = Vec::new();
        ctx.reserve_collection_vec(&mut selections_copy, selections.len(), OPERATION)?;
        selections_copy.extend_from_slice(selections);
        let mut selections = selections_copy;
        charge_sort(selections.len())?;
        selections.sort_unstable_by_key(|selection| selection.ordinal);
        let points = ordered_parameters
            .into_iter()
            .enumerate()
            .map(|(parameter, (_, radius))| {
                Some(VariableRadius {
                    parameter: parameter as f64,
                    radius: Length::from(radius),
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(points) = points else {
            return Ok(None);
        };
        let Some(points) = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok()
        else {
            return Ok(None);
        };
        return Ok(Some(vec![RadiusSelectionGroup(
            RadiusSpec::Variable { points },
            selections,
        )]));
    }
    if control_names.len() != parameter_names.len()
        || !parameter_names
            .iter()
            .all(|name| control_names.contains(name.as_str()))
    {
        return Ok(None);
    }

    for signature in vertex_radii.keys() {
        let mut found = false;
        for component in selections
            .iter()
            .flat_map(|selection| selection.references.iter())
            .flat_map(|reference| reference.iter())
        {
            ctx.charge_work(1, OPERATION)?;
            if component.instance == Some(0x8083) && component.type_signature == *signature {
                found = true;
                break;
            }
        }
        if !found {
            return Ok(None);
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
                    (vertex_radii.get(&first), vertex_radii.get(&second))
                else {
                    return Ok(None);
                };
                let pair = (*first_radius, *second_radius);
                ctx.charge_work(groups.len() as u64, OPERATION)?;
                if let Some((_, grouped)) =
                    groups.iter_mut().find(|(candidate, _)| *candidate == pair)
                {
                    ctx.reserve_collection_vec(grouped, 1, OPERATION)?;
                    grouped.push(selection);
                } else {
                    let mut grouped = Vec::new();
                    ctx.reserve_collection_vec(&mut grouped, 1, OPERATION)?;
                    grouped.push(selection);
                    ctx.reserve_collection_vec(&mut groups, 1, OPERATION)?;
                    groups.push((pair, grouped));
                }
            }
            (None, None, None) => {
                ctx.reserve_collection_vec(&mut unassigned, 1, OPERATION)?;
                unassigned.push(selection);
            }
            _ => return Ok(None),
        }
    }
    if groups.len() == 1 {
        ctx.reserve_precharged_vec(&mut groups[0].1, unassigned.len(), OPERATION)?;
        groups[0].1.append(&mut unassigned);
        charge_sort(groups[0].1.len())?;
        groups[0]
            .1
            .sort_unstable_by_key(|selection| selection.ordinal);
    } else if !unassigned.is_empty() {
        return Ok(None);
    }
    if groups.is_empty() {
        return Ok(None);
    }
    let mut result = Vec::new();
    ctx.reserve_collection_vec(&mut result, groups.len(), OPERATION)?;
    for ((first, second), selections) in groups {
        let Some(points) = cadmpeg_ir::features::edge_treatments::VariableRadii::new(vec![
            VariableRadius {
                parameter: 0.0,
                radius: Length::from(first),
            },
            VariableRadius {
                parameter: 1.0,
                radius: Length::from(second),
            },
        ])
        .ok() else {
            return Ok(None);
        };
        result.push(RadiusSelectionGroup(
            RadiusSpec::Variable { points },
            selections,
        ));
    }
    Ok(Some(result))
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
    const INDEX_OPERATION: &str = "index SLDPRT compact surface selections";
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_ref() else {
            continue;
        };
        ctx.charge_work(1, INDEX_OPERATION)?;
        let mut id_text = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut id_text,
            feature.id.as_str().len(),
            INDEX_OPERATION,
        )?;
        id_text.push_str(feature.id.as_str());
        let id = cadmpeg_ir::features::FeatureId::mint(id_text)
            .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT feature id"))?;
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        ctx.charge_collection_items(1, INDEX_OPERATION)?;
        feature_ids_by_native
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
        let mut native = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut native,
            native_ref.len(),
            INDEX_OPERATION,
        )?;
        native.push_str(native_ref);
        feature_ids_by_native.insert(native, id);
    }
    let mut history_features = Vec::new();
    for history in histories {
        for feature in &history.features {
            ctx.reserve_collection_vec(&mut history_features, 1, INDEX_OPERATION)?;
            history_features.push(feature);
        }
    }
    let mut selections = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in lanes.iter().flat_map(|lane| &lane.surface_selections) {
        ctx.charge_work(1, INDEX_OPERATION)?;
        if !selections.contains_key(selection.feature_ref.as_str()) {
            ctx.charge_collection_items(1, INDEX_OPERATION)?;
            selections
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        let group = selections
            .entry(selection.feature_ref.as_str())
            .or_default();
        ctx.reserve_collection_vec(group, 1, INDEX_OPERATION)?;
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
                        ctx.charge_work(u64::try_from(seeds.len())
                            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                        if seeds
                            .iter()
                            .any(|seed| matches!(seed, PatternSeed::Feature(_)))
                        {
                            break 'feature_edit;
                        }
                        for selection in feature_selections {
                            let history_work = u64::try_from(history_features.len())
                                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                            ctx.charge_work(history_work.checked_add(1)
                                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                            let native = compact_surface_selection_value(ctx, &selection.components)?;
                            let generated = component_path_feature(
                                ctx,
                                &selection.components,
                                &history_features,
                                &selection.feature_ref,
                                ComponentPathEnd::Trailing,
                            )?
                            .and_then(|(component, producer)| {
                                feature_ids_by_native
                                    .get(producer.id.as_str())
                                    .zip(component.local_id.as_ref())
                            });
                            let seed = match generated {
                                Some((producer, local_id)) => {
                                    let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                    let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                                    let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(
                                        producer_id,
                                        local_id_text,
                                    ) else {
                                        let seed = PatternSeed::Faces(
                                            cadmpeg_ir::features::FaceSelection::Native(native),
                                        );
                                        ctx.charge_work(u64::try_from(seeds.len())
                                            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                                        if !seeds.contains(&seed) {
                                            ctx.reserve_collection_vec(seeds, 1, OPERATION)?;
                                            seeds.push(seed);
                                        }
                                        continue;
                                    };
                                    ctx.charge_work(u64::try_from(seeds.len())
                                        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
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
                                        let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                        dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
                                    }
                                    let mut faces = Vec::new();
                                    ctx.reserve_collection_vec(&mut faces, 1, OPERATION)?;
                                    faces.push(face);
                                    let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
                                    PatternSeed::Faces(
                                        cadmpeg_ir::features::FaceSelection::generated(
                                            faces,
                                            native_copy,
                                        )
                                        .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native)),
                                    )
                                }
                                None => {
                                    PatternSeed::Faces(cadmpeg_ir::features::FaceSelection::Native(native))
                                }
                            };
                            ctx.charge_work(u64::try_from(seeds.len())
                                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                            if !seeds.contains(&seed) {
                                ctx.reserve_collection_vec(seeds, 1, OPERATION)?;
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
                        let native = compact_surface_selection_set_value(ctx, feature_selections)?;
                        let mut faces = Vec::new();
                        let mut complete = true;
                        for selection in feature_selections {
                            ctx.charge_work(1, OPERATION)?;
                            let generated = selection
                                .terminal_feature_ref
                                .as_ref()
                                .and_then(|producer| feature_ids_by_native.get(producer))
                                .zip(selection.components.last())
                                .and_then(|(producer, component)| Some((producer, component.local_id?)));
                            if let Some((producer, local_id)) = generated {
                                let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                let local_id_text = crate::text_admission::format_retained(ctx,
                                    format_args!("{local_id}"), OPERATION,
                                )?;
                                let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(
                                    producer_id,
                                    local_id_text,
                                ) else {
                                    complete = false;
                                    continue;
                                };
                                ctx.charge_work(u64::try_from(faces.len())
                                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                                if !faces.contains(&face) {
                                    ctx.reserve_collection_vec(&mut faces, 1, OPERATION)?;
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
                                    let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                    dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
                                }
                            }
                        }
                        *targets = if complete && !faces.is_empty() {
                            let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
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
                        let Some((target, tool)) = cut_with_surface_selection_pair(ctx, feature_selections)?
                        else {
                            break 'feature_edit;
                        };
                        let target_native = compact_surface_selection_value(ctx, &target.components)?;
                        let target_producer = target
                            .terminal_feature_ref
                            .as_ref()
                            .and_then(|producer| feature_ids_by_native.get(producer));
                        if let Some(producer) = target_producer {
                            let mut local_id_bytes = 0usize;
                            let mut local_id_count = 0usize;
                            for component in &target.components {
                                ctx.charge_work(1, OPERATION)?;
                                let Some(id) = component.local_id else {
                                    continue;
                                };
                                let digits = if id == 0 { 1 } else {
                                    usize::try_from(id.ilog10()).ok()
                                        .and_then(|log| log.checked_add(1))
                                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?
                                };
                                local_id_bytes = local_id_bytes
                                    .checked_add(usize::from(local_id_count != 0))
                                    .and_then(|sum| sum.checked_add(digits))
                                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                                local_id_count = local_id_count.checked_add(1)
                                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                            }
                            let mut local_id = String::new();
                            crate::text_admission::reserve_retained_string(ctx, &mut local_id, local_id_bytes, OPERATION)?;
                            for id in target.components.iter().filter_map(|component| component.local_id) {
                                if !local_id.is_empty() {
                                    local_id.push(',');
                                }
                                write!(local_id, "{id}").map_err(|_| {
                                    cadmpeg_core::CodecError::malformed("cannot format SLDPRT surface cut body id")
                                })?;
                            }
                            let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
                            let Ok(body) =
                                cadmpeg_ir::features::GeneratedBodyRef::new(producer_id, local_id)
                            else {
                                break 'feature_edit;
                            };
                            let mut bodies = Vec::new();
                            ctx.reserve_collection_vec(&mut bodies, 1, OPERATION)?;
                            bodies.push(body);
                            let native_copy = crate::text_admission::format_retained(ctx, format_args!("{target_native}"), OPERATION)?;
                            *targets = BodySelection::generated(bodies, native_copy)
                                .unwrap_or(BodySelection::Native(target_native));
                            if !dependencies.contains(producer) {
                                let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
                            }
                        }
                        let tool_native = compact_surface_selection_value(ctx, &tool.components)?;
                        let tool_generated = tool
                            .terminal_feature_ref
                            .as_ref()
                            .and_then(|producer| feature_ids_by_native.get(producer))
                            .zip(tool.components.last())
                            .and_then(|(producer, component)| {
                                component.local_id.map(|local_id| (producer, local_id))
                            });
                        if let Some((producer, local_id)) = tool_generated {
                            let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
                            let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                            let generated_face = cadmpeg_ir::features::GeneratedFaceRef::new(
                                producer_id,
                                local_id_text,
                            );
                            *tools = if let Ok(face) = generated_face {
                                let mut faces = Vec::new();
                                ctx.reserve_collection_vec(&mut faces, 1, OPERATION)?;
                                faces.push(face);
                                let native_copy = crate::text_admission::format_retained(ctx, format_args!("{tool_native}"), OPERATION)?;
                                FaceSelection::generated(faces, native_copy)
                                    .unwrap_or(FaceSelection::Native(tool_native))
                            } else {
                                FaceSelection::Native(tool_native)
                            };
                            if !dependencies.contains(producer) {
                                let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
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
                            full_round_fillet_selection_triple(ctx, feature_selections)?
                        else {
                            break 'feature_edit;
                        };
                        let [center_faces, side_one_faces, side_two_faces] =
                            [center_faces, side_one_faces, side_two_faces].map(|selection| -> Result<FaceSelection, cadmpeg_core::CodecError> {
                                let native = compact_surface_selection_value(ctx, &selection.components)?;
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
                                            dependencies.try_insert_charged(copy_projection_feature_id(ctx, producer, OPERATION)?, ctx, OPERATION)?;
                                        }
                                        let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(
                                            copy_projection_feature_id(ctx, producer, OPERATION)?, local_id_text,
                                        ) {
                                            Ok(face) => {
                                                let mut faces = Vec::new();
                                                ctx.reserve_collection_vec(&mut faces, 1, OPERATION)?;
                                                faces.push(face);
                                                let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
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
                                        dependencies.try_insert_charged(copy_projection_feature_id(ctx, producer, OPERATION)?, ctx, OPERATION)?;
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
                                    ),
                                )
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
                            ctx.charge_work(u64::try_from(selection.producer_feature_refs.len())
                                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                            for producer in selection
                                .producer_feature_refs
                                .iter()
                                .filter_map(|producer| feature_ids_by_native.get(producer))
                                .filter(|producer| *producer != feature_id)
                            {
                                if !dependencies.contains(producer) {
                                    let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                    dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
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
                        let native = compact_surface_selection_value(ctx, &selection.components)?;
                        let generated = selection
                            .terminal_feature_ref
                            .as_ref()
                            .and_then(|producer| feature_ids_by_native.get(producer))
                            .zip(selection.components.last())
                            .and_then(|(feature, component)| Some((feature, component.local_id?)));
                        let face = match generated {
                            Some((producer, local_id)) => {
                                if !dependencies.contains(producer) {
                                    let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                    dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
                                }
                                let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
                                let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                                match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) {
                                    Ok(face) => {
                                        let mut faces = Vec::new();
                                        ctx.reserve_collection_vec(&mut faces, 1, OPERATION)?;
                                        faces.push(face);
                                        let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
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
                    let native = compact_surface_selection_value(ctx, &selection.components)?;
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
                                        let producer_id = copy_projection_feature_id(ctx, feature, OPERATION)?;
                                        let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text) {
                                            Ok(face) => {
                                                let mut generated_faces = Vec::new();
                                                ctx.reserve_collection_vec(&mut generated_faces, 1, OPERATION)?;
                                                generated_faces.push(face);
                                                let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
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
                                        let producer_id = copy_projection_feature_id(ctx, feature, OPERATION)?;
                                        let local_id_text = crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedVertexRef::new(producer_id, local_id_text) {
                                            Ok(generated_vertex) => {
                                                let native_copy = crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
                                                cadmpeg_ir::features::VertexSelection::generated(generated_vertex, native_copy)
                                                    .unwrap_or_else(|_| {
                                                        cadmpeg_ir::features::VertexSelection::native(native).unwrap_or(
                                                            cadmpeg_ir::features::VertexSelection::Unresolved,
                                                        )
                                                    })
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
                            let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
                            dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
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
        ctx.charge_work(1, ALIAS_OPERATION)?;
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
        let native_key =
            crate::text_admission::format_retained(ctx, format_args!("{native}"), ALIAS_OPERATION)?;
        let face_copy = face.try_clone_charged(ctx, ALIAS_OPERATION)?;
        if !face_aliases.contains_key(native) {
            ctx.charge_collection_items(1, ALIAS_OPERATION)?;
            face_aliases
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(ALIAS_OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        face_aliases.insert(native_key, face_copy);
    }
    for feature in features {
        ctx.charge_work(1, ALIAS_OPERATION)?;
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
        let face = face.try_clone_charged(ctx, ALIAS_OPERATION)?;
        if let FaceSelection::Generated { faces, .. } = &face {
            for producer in faces.iter().map(|face| &face.feature) {
                ctx.charge_work(1, ALIAS_OPERATION)?;
                if producer != &feature.id && !feature.dependencies.contains(producer) {
                    let copy = copy_projection_feature_id(ctx, producer, ALIAS_OPERATION)?;
                    feature
                        .dependencies
                        .try_insert_charged(copy, ctx, ALIAS_OPERATION)?;
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
    ctx: &DecodeContext<'_>,
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Result<Option<[&'a FeatureInputSurfaceSelection; 3]>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "group SLDPRT full round fillet selections";
    let by_lane = surface_selections_by_lane(ctx, selections, OPERATION)?;
    let mut consensus: Option<[&'a FeatureInputSurfaceSelection; 3]> = None;
    for mut lane_selections in by_lane.into_values() {
        let len = lane_selections.len();
        let count = u64::try_from(len)
            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        let levels = if len > 1 { len.ilog2() + 1 } else { 1 };
        let work = count
            .checked_mul(u64::from(levels))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        lane_selections.sort_unstable_by_key(|selection| selection.offset);
        let [center, side_one, side_two] = lane_selections.as_slice() else {
            return Ok(None);
        };
        if let Some([expected_center, expected_side_one, expected_side_two]) = consensus {
            if !same_surface_selection_semantics(expected_center, center)
                || !same_surface_selection_semantics(expected_side_one, side_one)
                || !same_surface_selection_semantics(expected_side_two, side_two)
            {
                return Ok(None);
            }
        } else {
            consensus = Some([*center, *side_one, *side_two]);
        }
    }
    Ok(consensus)
}

fn surface_selections_by_lane<'a>(
    ctx: &DecodeContext<'_>,
    selections: &[&'a FeatureInputSurfaceSelection],
    operation: &'static str,
) -> Result<HashMap<&'a str, Vec<&'a FeatureInputSurfaceSelection>>, cadmpeg_core::CodecError> {
    let mut by_lane = HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in selections {
        ctx.charge_work(1, operation)?;
        if !by_lane.contains_key(selection.parent.as_str()) {
            ctx.charge_collection_items(1, operation)?;
            by_lane
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        let group = by_lane.entry(selection.parent.as_str()).or_default();
        ctx.reserve_collection_vec(group, 1, operation)?;
        group.push(*selection);
    }
    Ok(by_lane)
}

pub(crate) fn project_draft_operands(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const INDEX_OPERATION: &str = "index SLDPRT draft feature identities";
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(1, INDEX_OPERATION)?;
        let mut id = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut id,
            feature.id.as_str().len(),
            INDEX_OPERATION,
        )?;
        id.push_str(feature.id.as_str());
        let id = cadmpeg_ir::features::FeatureId::mint(id)
            .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT draft feature id"))?;
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        ctx.charge_collection_items(1, INDEX_OPERATION)?;
        feature_ids_by_native
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
        let mut native = String::new();
        crate::text_admission::reserve_retained_string(
            ctx,
            &mut native,
            native_ref.len(),
            INDEX_OPERATION,
        )?;
        native.push_str(native_ref);
        feature_ids_by_native.insert(native, id);
    }
    let mut candidates = HashMap::<String, Vec<DraftOperands>>::new();
    for lane in lanes {
        for (feature, operands) in draft_operand_candidates(ctx, histories, lane)? {
            const OPERATION: &str = "group SLDPRT draft operand candidates";
            if !candidates.contains_key(&feature) {
                ctx.charge_collection_items(1, OPERATION)?;
                candidates
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
            let by_feature = candidates.entry(feature).or_default();
            ctx.reserve_collection_vec(by_feature, 1, OPERATION)?;
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
        ctx,
        paths.len(),
        |index| &paths[index],
        "sldprt:feature-input:draft-surface-vectors:",
        "format SLDPRT draft surface selection set",
    )?;
    let mut generated = Vec::new();
    let mut generated_dependencies = Vec::new();
    for path in paths {
        let history_count = histories
            .iter()
            .try_fold(0usize, |count, history| {
                count.checked_add(history.features.len())
            })
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(
            u64::try_from(history_count)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some((producer, local_id)) = component_path_terminal_feature(
            ctx,
            path,
            histories.iter().flat_map(|history| &history.features),
        )?
        .filter(|producer| producer != consumer_ref)
        .and_then(|producer| {
            feature_ids_by_native
                .get(&producer)
                .zip(path.last()?.local_id.as_ref())
        }) else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        let producer_id = copy_projection_feature_id(ctx, producer, OPERATION)?;
        let local_id_text =
            crate::text_admission::format_retained(ctx, format_args!("{local_id}"), OPERATION)?;
        let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text)
        else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        ctx.charge_work(
            u64::try_from(generated.len())
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if !generated.contains(&face) {
            ctx.reserve_collection_vec(&mut generated, 1, OPERATION)?;
            generated.push(face);
        }
        ctx.charge_work(
            u64::try_from(generated_dependencies.len())
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if !generated_dependencies.contains(producer) {
            let dependency = copy_projection_feature_id(ctx, producer, OPERATION)?;
            ctx.reserve_collection_vec(&mut generated_dependencies, 1, OPERATION)?;
            generated_dependencies.push(dependency);
        }
    }
    if generated.is_empty() {
        Ok(cadmpeg_ir::features::FaceSelection::Native(native))
    } else {
        for dependency in generated_dependencies {
            if !dependencies.contains(&dependency) {
                dependencies.try_insert_charged(dependency, ctx, OPERATION)?;
            }
        }
        let native_copy =
            crate::text_admission::format_retained(ctx, format_args!("{native}"), OPERATION)?;
        Ok(
            cadmpeg_ir::features::FaceSelection::generated(generated, native_copy)
                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native)),
        )
    }
}

fn compact_surface_selection_set_value(
    ctx: &DecodeContext<'_>,
    selections: &[&FeatureInputSurfaceSelection],
) -> Result<String, cadmpeg_core::CodecError> {
    format_surface_path_set(
        ctx,
        selections.len(),
        |index| &selections[index].components,
        "sldprt:feature-input:surface-selection-vectors:",
        "format SLDPRT surface selection set",
    )
}

fn format_surface_path_set<'a>(
    ctx: &DecodeContext<'_>,
    path_count: usize,
    path_at: impl Fn(usize) -> &'a [crate::records::FeatureInputComponentPathEntry],
    set_prefix: &'static str,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    const PATH_PREFIX: &str = "sldprt:feature-input:surface-component-ids:";
    let same_ids = |left: &[crate::records::FeatureInputComponentPathEntry],
                    right: &[crate::records::FeatureInputComponentPathEntry]|
     -> Result<bool, cadmpeg_core::CodecError> {
        ctx.charge_work(1, operation)?;
        if left.len() != right.len() {
            return Ok(false);
        }
        for (left, right) in left.iter().zip(right) {
            ctx.charge_work(1, operation)?;
            if left.local_id != right.local_id {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let mut unique_count = 0usize;
    let mut path_bytes = 0usize;
    for index in 0..path_count {
        let components = path_at(index);
        let mut duplicate = false;
        for previous_index in 0..index {
            if same_ids(path_at(previous_index), components)? {
                duplicate = true;
                break;
            }
        }
        if duplicate {
            continue;
        }
        unique_count = unique_count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        let mut bytes = PATH_PREFIX.len();
        for (component_index, component) in components.iter().enumerate() {
            ctx.charge_work(1, operation)?;
            let digits = match component.local_id {
                Some(0) | None => 1,
                Some(local_id) => usize::try_from(local_id.ilog10())
                    .ok()
                    .and_then(|log| log.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
            };
            let separator = usize::from(component_index != 0);
            bytes = bytes
                .checked_add(separator)
                .and_then(|sum| sum.checked_add(digits))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        path_bytes = path_bytes
            .checked_add(bytes)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
    let set_bytes = if unique_count == 1 {
        0
    } else {
        let separators = if unique_count == 0 {
            0
        } else {
            unique_count - 1
        };
        set_prefix
            .len()
            .checked_add(separators)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?
    };
    let total_bytes = path_bytes
        .checked_add(set_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    let mut value = String::new();
    crate::text_admission::reserve_retained_string(ctx, &mut value, total_bytes, operation)?;
    if unique_count != 1 {
        value.push_str(set_prefix);
    }
    let mut emitted = 0usize;
    for index in 0..path_count {
        let components = path_at(index);
        let mut duplicate = false;
        for previous_index in 0..index {
            if same_ids(path_at(previous_index), components)? {
                duplicate = true;
                break;
            }
        }
        if duplicate {
            continue;
        }
        if emitted != 0 {
            value.push(';');
        }
        value.push_str(PATH_PREFIX);
        for (component_index, component) in components.iter().enumerate() {
            if component_index != 0 {
                value.push(',');
            }
            match component.local_id {
                Some(local_id) => write!(value, "{local_id}").map_err(|_| {
                    cadmpeg_core::CodecError::malformed("cannot format SLDPRT surface selection")
                })?,
                None => value.push('_'),
            }
        }
        emitted += 1;
    }
    Ok(value)
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
    ctx: &DecodeContext<'_>,
    selections: &[&'a FeatureInputSurfaceSelection],
) -> Result<
    Option<(
        &'a FeatureInputSurfaceSelection,
        &'a FeatureInputSurfaceSelection,
    )>,
    cadmpeg_core::CodecError,
> {
    const OPERATION: &str = "group SLDPRT surface cut selections";
    let by_lane = surface_selections_by_lane(ctx, selections, OPERATION)?;
    let mut consensus = None;
    for mut lane_selections in by_lane.into_values() {
        ctx.charge_work(1, OPERATION)?;
        if lane_selections.len() != 2 {
            return Ok(None);
        }
        lane_selections.sort_unstable_by_key(|selection| selection.offset);
        let pair = (lane_selections[0], lane_selections[1]);
        if let Some((target, tool)) = consensus {
            if !same_surface_selection_semantics(target, pair.0)
                || !same_surface_selection_semantics(tool, pair.1)
            {
                return Ok(None);
            }
        } else {
            consensus = Some(pair);
        }
    }
    Ok(consensus)
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
    const OPERATION: &str = "index SLDPRT cosmetic thread history features";
    const ID_OPERATION: &str = "index SLDPRT cosmetic thread feature IDs";

    let input_work = [features.len(), histories.len(), faces.len(), surfaces.len()]
        .into_iter()
        .try_fold(0u64, |work, count| {
            work.checked_add(cadmpeg_core::decode::u64_from_index(count))
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "find unique SLDPRT cylindrical face",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(input_work, "find unique SLDPRT cylindrical face")?;
    let mut native_features = HashMap::new();
    let mut history_features = Vec::new();
    for native_feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        if !native_features.contains_key(native_feature.id.as_str()) {
            ctx.charge_collection_items(1, OPERATION)?;
            native_features
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        native_features.insert(native_feature.id.as_str(), native_feature);
        ctx.reserve_collection_vec(&mut history_features, 1, OPERATION)?;
        history_features.push(native_feature);
    }
    let mut feature_ids_by_native = HashMap::new();
    let mut scoped_ids = Vec::new();
    for feature in features.iter() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(1, ID_OPERATION)?;
        let (mut id_text, id_reservation) = crate::text_admission::reserve_scoped_string(
            ctx,
            feature.id.as_str().len(),
            ID_OPERATION,
        )?;
        id_text.push_str(feature.id.as_str());
        let id = cadmpeg_ir::features::FeatureId::mint(id_text).map_err(|_| {
            cadmpeg_core::CodecError::malformed("invalid SLDPRT cosmetic thread feature id")
        })?;
        ctx.reserve_collection_vec(&mut scoped_ids, 1, ID_OPERATION)?;
        scoped_ids.push(id_reservation);
        if let Some(previous) = feature_ids_by_native.get_mut(native_ref) {
            *previous = id;
            continue;
        }
        ctx.charge_collection_items(1, ID_OPERATION)?;
        feature_ids_by_native
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(ID_OPERATION, u64::MAX - 1, u64::MAX))?;
        let (mut native_key, key_reservation) =
            crate::text_admission::reserve_scoped_string(ctx, native_ref.len(), ID_OPERATION)?;
        native_key.push_str(native_ref);
        ctx.reserve_collection_vec(&mut scoped_ids, 1, ID_OPERATION)?;
        scoped_ids.push(key_reservation);
        feature_ids_by_native.insert(native_key, id);
    }
    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                const REFERENCE_OPERATION: &str = "collect SLDPRT cosmetic thread references";
                const NATIVE_OPERATION: &str = "format SLDPRT cosmetic thread cylinder references";
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
                let format_reference_key = |lane_key: &str, offset: u64| {
                    let digits = if offset == 0 {
                        1
                    } else {
                        usize::try_from(offset.ilog10())
                            .ok()
                            .and_then(|digits| digits.checked_add(1))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(REFERENCE_OPERATION, u64::MAX - 1, u64::MAX)
                            })?
                    };
                    let bytes = lane_key
                        .len()
                        .checked_add(1)
                        .and_then(|size| size.checked_add(digits))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(REFERENCE_OPERATION, u64::MAX - 1, u64::MAX)
                        })?;
                    let (mut key, reservation) = crate::text_admission::reserve_scoped_string(
                        ctx,
                        bytes,
                        REFERENCE_OPERATION,
                    )?;
                    write!(key, "{lane_key}:{offset}").map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "cannot format SLDPRT cylinder reference key",
                        )
                    })?;
                    Ok::<_, cadmpeg_core::CodecError>((key, reservation))
                };
                let mut references = Vec::<(
                    String,
                    Option<std::borrow::Cow<'_, [crate::records::FeatureInputComponentPathEntry]>>,
                    Option<&str>,
                )>::new();
                let mut key_reservations = Vec::new();
                for lane in lanes {
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    for selection in &lane.surface_selections {
                        ctx.charge_work(1, REFERENCE_OPERATION)?;
                        if selection.feature_ref != native_feature.id {
                            continue;
                        }
                        let (key, reservation) = format_reference_key(lane_key, selection.offset)?;
                        ctx.reserve_collection_vec(&mut key_reservations, 1, REFERENCE_OPERATION)?;
                        key_reservations.push(reservation);
                        ctx.reserve_collection_vec(&mut references, 1, REFERENCE_OPERATION)?;
                        references.push((
                            key,
                            Some(std::borrow::Cow::Borrowed(selection.components.as_slice())),
                            selection.producer_feature_refs.first().map(String::as_str),
                        ));
                    }
                }
                for lane in lanes {
                    const TOKEN_OPERATION: &str = "collect SLDPRT cosmetic thread cylinder tokens";

                    let Some((_, start, end)) = feature_object_byte_ranges(ctx, histories, lane)?
                        .get(native_feature.id.as_str())
                        .copied()
                    else {
                        continue;
                    };
                    let mut cylinder_tokens = HashSet::new();
                    for class in &lane.classes {
                        ctx.charge_work(1, TOKEN_OPERATION)?;
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
                        if !cylinder_tokens.contains(&token) {
                            ctx.charge_collection_items(1, TOKEN_OPERATION)?;
                            cylinder_tokens.try_reserve(1).map_err(|_| {
                                ctx.refuse_codec_limit(TOKEN_OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                        }
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
                        let offset = u64::try_from(marker).map_err(|_| {
                            ctx.refuse_codec_limit(REFERENCE_OPERATION, u64::MAX - 1, u64::MAX)
                        })?;
                        let (key, reservation) = format_reference_key(lane_key, offset)?;
                        ctx.reserve_collection_vec(&mut key_reservations, 1, REFERENCE_OPERATION)?;
                        key_reservations.push(reservation);
                        ctx.reserve_collection_vec(&mut references, 1, REFERENCE_OPERATION)?;
                        references.push((key, components.map(std::borrow::Cow::Owned), None));
                    }
                }
                let count = u64::try_from(references.len()).map_err(|_| {
                    ctx.refuse_codec_limit(NATIVE_OPERATION, u64::MAX - 1, u64::MAX)
                })?;
                let levels = if references.len() > 1 {
                    references.len().ilog2() + 1
                } else {
                    1
                };
                let sort_work = count.checked_mul(u64::from(levels)).ok_or_else(|| {
                    ctx.refuse_codec_limit(NATIVE_OPERATION, u64::MAX - 1, u64::MAX)
                })?;
                ctx.charge_work(sort_work, NATIVE_OPERATION)?;
                references.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                let native = if references.is_empty() {
                    None
                } else {
                    const PREFIX: &str = "sldprt:feature-input:cylinder-reference:";
                    let mut bytes = PREFIX.len();
                    let mut last = None;
                    let mut distinct = 0usize;
                    for (reference, _, _) in &references {
                        ctx.charge_work(1, NATIVE_OPERATION)?;
                        if last == Some(reference.as_str()) {
                            continue;
                        }
                        bytes = bytes
                            .checked_add(reference.len())
                            .and_then(|size| size.checked_add(usize::from(distinct != 0)))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(NATIVE_OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                        distinct += 1;
                        last = Some(reference.as_str());
                    }
                    let mut native = String::new();
                    crate::text_admission::reserve_retained_string(
                        ctx,
                        &mut native,
                        bytes,
                        NATIVE_OPERATION,
                    )?;
                    native.push_str(PREFIX);
                    last = None;
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
                    ctx.charge_work(1, GENERATED_OPERATION)?;
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
                                    ctx,
                                    components,
                                    &history_features,
                                    native_feature.id.as_str(),
                                    ComponentPathEnd::Leading,
                                )?,
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
                    let producer_id =
                        copy_projection_feature_id(ctx, producer, GENERATED_OPERATION)?;
                    let local_id_text = crate::text_admission::format_retained(
                        ctx,
                        format_args!("{local_id}"),
                        GENERATED_OPERATION,
                    )?;
                    *face = match cadmpeg_ir::features::GeneratedFaceRef::new(
                        producer_id,
                        local_id_text,
                    ) {
                        Ok(generated_face) => {
                            let mut faces = Vec::new();
                            ctx.reserve_collection_vec(&mut faces, 1, GENERATED_OPERATION)?;
                            faces.push(generated_face);
                            let native_copy = crate::text_admission::format_retained(
                                ctx,
                                format_args!("{native}"),
                                GENERATED_OPERATION,
                            )?;
                            cadmpeg_ir::features::FaceSelection::generated(faces, native_copy)
                                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                        }
                        Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                    if producer != feature_id && !dependencies.contains(producer) {
                        let dependency =
                            copy_projection_feature_id(ctx, producer, GENERATED_OPERATION)?;
                        dependencies.try_insert_charged(dependency, ctx, GENERATED_OPERATION)?;
                    }
                    return Ok(());
                }
                let Some(diameter) = diameter else {
                    return Ok(());
                };
                let diameter = diameter.get();

                let selected = match unique_cylindrical_face(ctx, diameter * 0.5, faces, surfaces)?
                {
                    Some(selected) => Some(selected),
                    None if native.is_some() => {
                        unique_topological_cylindrical_face(ctx, faces, surfaces)?
                    }
                    None => None,
                };
                let Some(selected) = selected else {
                    return Ok(());
                };
                let mut selected_faces = Vec::new();
                ctx.reserve_collection_vec(
                    &mut selected_faces,
                    1,
                    "project SLDPRT unbound cosmetic thread face",
                )?;
                selected_faces.push(selected);
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

fn unique_cylindrical_face(
    ctx: &DecodeContext<'_>,
    radius: f64,
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
    if !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let tolerance = (radius.abs() * EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9)
        .max(EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9);
    unique_matching_face(
        ctx,
        faces,
        surfaces,
        "find unique SLDPRT cylindrical face",
        |surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                (cylinder_surface.radius().get() - radius).abs() <= tolerance
            }
            _ => false,
        },
    )
}

fn unique_matching_face(
    ctx: &DecodeContext<'_>,
    faces: &[Face],
    surfaces: &[Surface],
    operation: &'static str,
    mut matches_surface: impl FnMut(&Surface) -> bool,
) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
    let mut selected = None;
    for face in faces {
        for surface in surfaces {
            ctx.charge_work(1, operation)?;
            if face.surface != surface.id || !matches_surface(surface) {
                continue;
            }
            if selected.is_some() {
                return Ok(None);
            }
            selected = Some(&face.id);
            break;
        }
    }
    selected
        .map(|id| {
            let text = crate::text_admission::format_retained(
                ctx,
                format_args!("{}", id.as_str()),
                operation,
            )?;
            FaceId::mint(text)
                .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT face id"))
        })
        .transpose()
}

fn unique_topological_cylindrical_face(
    ctx: &DecodeContext<'_>,
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
    unique_matching_face(
        ctx,
        faces,
        surfaces,
        "find unique SLDPRT topological cylinder face",
        |surface| {
            matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
            )
        },
    )
}

/// Resolve frame-only offset-plane supports when exactly one B-rep face lies
/// on the serialized support plane.
pub(crate) fn project_unbound_offset_plane_faces(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<(), cadmpeg_core::CodecError> {
    for feature in features {
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference,
                    ..
                }) = definition
                else {
                    return Ok(());
                };
                let (origin, normal) = match reference.as_ref() {
                    Some(cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { frame }) => {
                        (frame.origin(), frame.normal().get())
                    }
                    _ => return Ok(()),
                };
                let Some(selected) =
                    unique_planar_face(ctx, origin.get(), normal, faces, surfaces)?
                else {
                    return Ok(());
                };
                let mut selected_faces = Vec::new();
                ctx.reserve_collection_vec(
                    &mut selected_faces,
                    1,
                    "project SLDPRT unbound offset plane face",
                )?;
                selected_faces.push(selected);
                *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                    face: cadmpeg_ir::features::FaceSelection::Faces(selected_faces),
                });
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn unique_planar_face(
    ctx: &DecodeContext<'_>,
    origin: Point3,
    normal: Vector3,
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "find unique SLDPRT planar face";
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return Ok(None);
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
    unique_matching_face(ctx, faces, surfaces, OPERATION, |surface| {
        match surface.geometry {
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
                let distance = displacement.x * normal.x
                    + displacement.y * normal.y
                    + displacement.z * normal.z;
                (alignment.abs() - 1.0).abs() <= EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E9
                    && distance.abs() <= tolerance
            }
            _ => false,
        }
    })
}

#[cfg(test)]
mod projections_tests;
