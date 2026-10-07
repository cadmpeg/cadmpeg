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
use super::profiles::{first_positions, group_by_owner, mint_formatted, owned_members};
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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;

const EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9: f64 = 1.0e-9;
const EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E8: f64 = 1.0e-8;
const EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E9: f64 = 1.0e-9;

/// Adds the model feature of each native producer, other than `consumer`, as a dependency.
fn add_producer_dependencies(
    ctx: &DecodeContext<'_>,
    feature_ids_by_native: &HashMap<String, cadmpeg_ir::features::FeatureId>,
    producers: &[String],
    consumer: &cadmpeg_ir::features::FeatureId,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    for producer in ctx.admit_iter(producers, operation)? {
        let Some(producer) =
            ctx.get_hash_map(feature_ids_by_native, producer.as_str(), operation)?
        else {
            continue;
        };
        add_dependency(ctx, producer, consumer, dependencies, operation)?;
    }
    Ok(())
}

/// Adds `producer` as a dependency of `consumer` unless it is the consumer itself.
fn add_dependency(
    ctx: &DecodeContext<'_>,
    producer: &cadmpeg_ir::features::FeatureId,
    consumer: &cadmpeg_ir::features::FeatureId,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if ctx.equal(producer, consumer, operation)?
        || ctx.contains(dependencies.as_slice(), producer, operation)?
    {
        return Ok(());
    }
    dependencies.insert(
        ctx,
        producer.try_clone_for_decode(ctx, operation)?,
        operation,
    )?;
    Ok(())
}

fn scoped_reference_name<'a>(
    ctx: &'a DecodeContext<'_>,
    source_name: &str,
    offset: Option<u64>,
    suffix: Option<u32>,
    operation: &'static str,
) -> Result<(String, cadmpeg_core::decode::ScopedReservation<'a>), cadmpeg_core::CodecError> {
    match (offset, suffix) {
        (None, None) => ctx.format_scoped(format_args!("{source_name}@reference"), operation),
        (Some(offset), None) => {
            ctx.format_scoped(format_args!("{source_name}@reference:{offset}"), operation)
        }
        (None, Some(suffix)) => {
            ctx.format_scoped(format_args!("{source_name}@reference:{suffix}"), operation)
        }
        (Some(offset), Some(suffix)) => ctx.format_scoped(
            format_args!("{source_name}@reference:{offset}:{suffix}"),
            operation,
        ),
    }
}

/// The expected circle radius a radius or diameter dimension states.
fn dimension_radius(parameter: &DesignParameter) -> Option<f64> {
    let Some(ParameterValue::Length(value)) = &parameter.value else {
        return None;
    };
    match parameter.display {
        Some(DimensionDisplay::Radius) => Some(value.get()),
        Some(DimensionDisplay::Diameter) => Some(value.get() * 0.5),
        None => None,
    }
}

pub(super) fn bind_circular_profile_by_dimension(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "bind SLDPRT circular profile by dimension";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    // Proposals as (sketch, feature, first sketch with that identity).
    let mut proposals = Vec::new();
    // The sketch features bound to each sketch identity class, and each feature's current class.
    let mut bound_by_class = HashMap::<usize, Vec<usize>>::new();
    let mut feature_class: Vec<Option<usize>>;
    {
        let mut first_sketch = HashMap::<&str, usize>::new();
        for (position, sketch) in ctx.admit_iter(&sketches[..], OPERATION)?.enumerate() {
            if !ctx.contains_key_hash_map(&first_sketch, sketch.id.as_str(), OPERATION)? {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut first_sketch, sketch.id.as_str(), position, OPERATION)
                })?;
            }
        }
        // The last entity with each identity.
        let mut geometry_by_entity = HashMap::<&str, &cadmpeg_ir::SketchGeometry>::new();
        for entity in ctx.admit_iter(sketch_entities, OPERATION)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut geometry_by_entity,
                    entity.id().as_str(),
                    &entity.geometry,
                    OPERATION,
                )
            })?;
        }
        let mut sketch_features = HashMap::<&str, Vec<usize>>::new();
        feature_class =
            storage.with_storage(|| ctx.alloc_filled(features.len(), None::<usize>, OPERATION))?;
        for (index, feature) in ctx.admit_iter(&features[..], OPERATION)?.enumerate() {
            let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut sketch_features,
                    feature.id.as_str(),
                    index,
                    OPERATION,
                    OPERATION,
                )
            })?;
            let Some(bound) = sketch.id() else {
                continue;
            };
            let Some(&class) = ctx.get_hash_map(&first_sketch, bound.as_str(), OPERATION)? else {
                continue;
            };
            feature_class[index] = Some(class);
            storage.with_storage(|| {
                ctx.push_hash_group(&mut bound_by_class, class, index, OPERATION, OPERATION)
            })?;
        }
        // Every sketch feature's dimension radii, ordered by radius.
        let mut radii = Vec::new();
        for parameter in ctx.admit_iter(parameters, OPERATION)? {
            let (Some(owner), Some(expected)) =
                (parameter.owner.as_ref(), dimension_radius(parameter))
            else {
                continue;
            };
            let Some(owners) = ctx.get_hash_map(&sketch_features, owner.as_str(), OPERATION)?
            else {
                continue;
            };
            for feature_index in ctx.admit_iter(owners, OPERATION)? {
                storage.with_storage(|| {
                    ctx.push_vec(&mut radii, (expected, *feature_index), OPERATION)
                })?;
            }
        }
        ctx.sort_unstable_by_key(
            &mut radii,
            |value| *value,
            |left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)),
            OPERATION,
        )?;
        for (sketch_index, sketch) in ctx.admit_iter(&sketches[..], OPERATION)?.enumerate() {
            let [profile] = sketch.profiles.as_slice() else {
                continue;
            };
            let [entity_use] = profile.as_slice() else {
                continue;
            };
            let Some(SketchGeometryDefinition::Circle { radius, .. }) = ctx
                .get_hash_map(&geometry_by_entity, entity_use.entity.as_str(), OPERATION)?
                .map(|geometry| geometry.definition())
            else {
                continue;
            };
            let radius = radius.get();
            // A matching radius lies within this window; `same_dimension_length` decides.
            let reach = 4.0e-9 * radius.abs().max(1.0);
            let lower = ctx.partition_point(
                &radii,
                |(expected, _)| Ok(*expected < radius - reach),
                OPERATION,
            )?;
            let mut matched = None;
            let mut ambiguous = false;
            ctx.position_by(
                &radii[lower..],
                |(expected, feature_index)| {
                    if *expected > radius + reach {
                        return Ok(true);
                    }
                    if !same_dimension_length(*expected, radius) {
                        return Ok(false);
                    }
                    match matched {
                        None => matched = Some(*feature_index),
                        Some(previous) if previous != *feature_index => ambiguous = true,
                        Some(_) => {}
                    }
                    Ok(ambiguous)
                },
                OPERATION,
            )?;
            let (Some(feature_index), false) = (matched, ambiguous) else {
                continue;
            };
            let Some(&class) = ctx.get_hash_map(&first_sketch, sketch.id.as_str(), OPERATION)?
            else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut proposals,
                    (sketch_index, feature_index, class),
                    OPERATION,
                )
            })?;
        }
    }
    let mut feature_counts = HashMap::<usize, usize>::new();
    for &(_, feature_index, _) in ctx.admit_iter(&proposals, OPERATION)? {
        if let Some(count) = ctx.get_mut_hash_map(&mut feature_counts, &feature_index, OPERATION)? {
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            continue;
        }
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut feature_counts, feature_index, 1, OPERATION)
        })?;
    }
    for (sketch_index, feature_index, class) in ctx.admit_iter(proposals, OPERATION)? {
        if ctx.get_hash_map(&feature_counts, &feature_index, OPERATION)? != Some(&1) {
            continue;
        }
        let sketch_id = sketches[sketch_index]
            .id
            .try_clone_for_decode(ctx, OPERATION)?;
        // Unbind every sketch feature still bound to this sketch identity.
        if let Some(bound) = ctx.get_mut_hash_map(&mut bound_by_class, &class, OPERATION)? {
            for bound_index in ctx.admit_iter(std::mem::take(bound), OPERATION)? {
                if feature_class[bound_index] != Some(class) {
                    continue;
                }
                feature_class[bound_index] = None;
                features[bound_index].evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: bound,
                        ..
                    }) = definition
                    {
                        *bound = cadmpeg_ir::features::SketchFeatureBinding::Planar(None);
                    }
                });
            }
        }
        let name = features[feature_index]
            .name
            .as_deref()
            .map(|name| ctx.copy_retained_text(name, OPERATION))
            .transpose()?;
        let mut rebound = false;
        features[feature_index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
                definition
            {
                *sketch = cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id));
                rebound = true;
            }
        });
        if rebound {
            feature_class[feature_index] = Some(class);
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut bound_by_class,
                    class,
                    feature_index,
                    OPERATION,
                    OPERATION,
                )
            })?;
        }
        sketches[class].name = name;
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut neutral_owners = HashMap::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut neutral_owners, &feature.id, native_ref, OPERATION)
        })?;
    }
    // Parameter positions grouped by the native record that owns them, in parameter order.
    let mut parameters_by_native = HashMap::<&str, Vec<usize>>::new();
    for (index, parameter) in ctx.admit_iter(&parameters[..], OPERATION)?.enumerate() {
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        let Some(native) = ctx.get_hash_map(&neutral_owners, owner, OPERATION)? else {
            continue;
        };
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut parameters_by_native,
                *native,
                index,
                OPERATION,
                OPERATION,
            )
        })?;
    }
    if parameters_by_native.is_empty() {
        return Ok(());
    }
    let mut native_features = BTreeMap::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    OPERATION,
                )
            })?;
        }
    }
    let mut lanes = lanes.into_iter();
    while let Some(lane) = ctx.next_charged(&mut lanes, OPERATION)? {
        let mut lane_storage = ctx.reserve_scoped(0, OPERATION)?;
        let (scalar_positions, _scalar_positions_storage) =
            first_positions(ctx, &lane.scalars, |scalar| scalar.id.as_str(), OPERATION)?;
        let mut length_scalars = HashSet::new();
        let mut angle_scalars = HashSet::new();
        let mut detached_scalars = HashSet::new();
        for relation in ctx.admit_iter(&lane.relation_instances, OPERATION)? {
            let Some(id) = relation.parameter_scalar_ref() else {
                continue;
            };
            let family = if relation.family == FeatureInputRelationFamily::Angle {
                &mut angle_scalars
            } else {
                &mut length_scalars
            };
            lane_storage.with_storage(|| ctx.insert_hash_set(family, id, OPERATION))?;
            let detached = ctx
                .get_hash_map(&scalar_positions, id, OPERATION)?
                .is_some_and(|position| lane.scalars[*position].operands.is_empty());
            if detached {
                lane_storage
                    .with_storage(|| ctx.insert_hash_set(&mut detached_scalars, id, OPERATION))?;
            }
        }
        let mut names_by_id = HashMap::new();
        for name in ctx.admit_iter(&lane.names, OPERATION)? {
            lane_storage.with_storage(|| {
                ctx.insert_hash_map(&mut names_by_id, name.id.as_str(), name, OPERATION)
            })?;
        }
        // Scalars by owning feature, and the scalars with no owner in offset order.
        let (scalars_by_feature, _scalars_by_feature_storage) = group_by_owner(
            ctx,
            &lane.scalars,
            |scalar| scalar.feature_ref.as_deref(),
            OPERATION,
        )?;
        let mut unowned = Vec::new();
        for scalar in ctx.admit_iter(&lane.scalars, OPERATION)? {
            if scalar.feature_ref.is_none() {
                lane_storage.with_storage(|| ctx.push_vec(&mut unowned, scalar, OPERATION))?;
            }
        }
        ctx.stable_sort_by_key(&mut unowned, |scalar| scalar.offset, Ord::cmp, OPERATION)?;
        let mut starts = Vec::<(Option<u64>, &crate::records::Feature)>::new();
        for feature in ctx
            .admit_iter(&native_features, OPERATION)?
            .map(|(_, feature)| *feature)
        {
            let start = feature_object_name(feature, lane).map(|name| name.offset);
            lane_storage.with_storage(|| ctx.push_vec(&mut starts, (start, feature), OPERATION))?;
        }
        ctx.stable_sort_by_key(
            &mut starts,
            |value| (value.0.is_none(), value.0),
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, native_feature)) in ctx.admit_iter(&starts, OPERATION)?.enumerate() {
            let Some(owned_parameters) =
                ctx.get_hash_map(&parameters_by_native, native_feature.id.as_str(), OPERATION)?
            else {
                continue;
            };
            let end = starts.get(index + 1).and_then(|next| next.0);
            let unowned_range = match start {
                Some(start) => {
                    let lower = ctx.partition_point(
                        &unowned,
                        |scalar| Ok(scalar.offset <= start),
                        OPERATION,
                    )?;
                    let upper = match end {
                        Some(end) => ctx.partition_point(
                            &unowned,
                            |scalar| Ok(scalar.offset < end),
                            OPERATION,
                        )?,
                        None => unowned.len(),
                    };
                    &unowned[lower..upper.max(lower)]
                }
                None => &[],
            };
            let owned_scalars = owned_members(
                ctx,
                &scalars_by_feature,
                native_feature.id.as_str(),
                OPERATION,
            )?;
            for &parameter_index in ctx.admit_iter(owned_parameters, OPERATION)? {
                let parameter = &mut parameters[parameter_index];
                if parameter.native_ref.is_some() {
                    continue;
                }
                let mut scalars_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut scalars = Vec::new();
                for scalar in ctx
                    .admit_iter(owned_scalars, OPERATION)?
                    .chain(ctx.admit_iter(unowned_range, OPERATION)?)
                {
                    let Some(name) =
                        ctx.get_hash_map(&names_by_id, scalar.name.as_str(), OPERATION)?
                    else {
                        continue;
                    };
                    if ctx.equal(name.value.as_str(), parameter.name.as_str(), OPERATION)?
                        && value_only_scalar_offset(ctx, &lane.native_payload, name)?
                            != usize::try_from(scalar.offset).ok()
                    {
                        scalars_storage
                            .with_storage(|| ctx.push_vec(&mut scalars, *scalar, OPERATION))?;
                    }
                }
                let driving = ctx.any_by(
                    &scalars,
                    |scalar| Ok(scalar.role == FeatureInputScalarRole::Driving),
                    OPERATION,
                )?;
                let role = if driving {
                    FeatureInputScalarRole::Driving
                } else {
                    FeatureInputScalarRole::Native
                };
                let mut compatible = None;
                let mut compatible_count = 0_usize;
                for scalar in ctx.admit_iter(&scalars, OPERATION)? {
                    if scalar.role != role {
                        continue;
                    }
                    let measured =
                        ctx.contains_hash_set(&length_scalars, scalar.id.as_str(), OPERATION)?
                            || ctx.contains_hash_set(
                                &angle_scalars,
                                scalar.id.as_str(),
                                OPERATION,
                            )?;
                    let expected = match parameter.value.as_ref() {
                        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
                            let Some(expected) =
                                crate::history::parameters::eval::exact_integer_f64(*expected)
                            else {
                                continue;
                            };
                            Some(expected)
                        }
                        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
                            Some(if *expected { 1.0 } else { 0.0 })
                        }
                        _ => None,
                    };
                    let matches_value = match expected {
                        Some(expected) if measured => {
                            same_dimension_length(scalar.value.get() * 1000.0, expected)
                        }
                        Some(expected) => scalar.value.get() == expected,
                        None => true,
                    };
                    if matches_value {
                        compatible = Some(*scalar);
                        compatible_count += 1;
                    }
                }
                let (Some(scalar), 1) = (compatible, compatible_count) else {
                    continue;
                };
                let is_length =
                    ctx.contains_hash_set(&length_scalars, scalar.id.as_str(), OPERATION)?;
                let is_angle =
                    ctx.contains_hash_set(&angle_scalars, scalar.id.as_str(), OPERATION)?;
                let scalar_is_detached =
                    ctx.contains_hash_set(&detached_scalars, scalar.id.as_str(), OPERATION)?;
                parameter.native_ref = Some(ctx.copy_retained_text(&scalar.id, OPERATION)?);
                let scalar_is_untyped_real = matches!(
                    parameter.value,
                    Some(cadmpeg_ir::features::ParameterValue::Real(_))
                ) && !scalar_is_detached;
                let length = || {
                    cadmpeg_ir::scalar::Length::new(scalar.value.get() * 1000.0).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })
                };
                if scalar_is_detached && is_length {
                    parameter.expression = crate::history::literals::format_length_mm(length()?);
                } else if scalar_is_detached && is_angle {
                    parameter.expression = crate::history::literals::format_angle_rad(
                        cadmpeg_ir::scalar::Angle::from_assigned_real(scalar.value),
                    );
                }
                let evaluated = if is_length && !scalar_is_untyped_real {
                    Some(cadmpeg_ir::features::ParameterValue::Length(length()?))
                } else if is_angle && !scalar_is_untyped_real {
                    Some(cadmpeg_ir::features::ParameterValue::Angle(
                        cadmpeg_ir::scalar::Angle::from_assigned_real(scalar.value),
                    ))
                } else {
                    match parameter.value.as_ref() {
                        Some(cadmpeg_ir::features::ParameterValue::Length(_)) => {
                            Some(cadmpeg_ir::features::ParameterValue::Length(length()?))
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut lane_refs = Vec::new();
    let mut lanes = lanes.into_iter();
    while let Some(lane) = ctx.next_charged(&mut lanes, "collect SLDPRT display relation lanes")? {
        storage.with_storage(|| {
            ctx.push_vec(
                &mut lane_refs,
                lane,
                "collect SLDPRT display relation lanes",
            )
        })?;
    }
    let (owned, _owned_storage) = ctx
        .with_scoped_storage("SLDPRT relation ownership index", || {
            owned_relation_parameters(ctx, features, parameters, &lane_refs)
        })?;
    let mut features_by_native_ref = HashMap::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut features_by_native_ref, native_ref, feature, OPERATION)
        })?;
    }
    // Relation and parameter identities already bound, each owner's parameter names, and
    // each owner's next free ordinal, kept current as parameters are appended.
    let mut relation_ids = HashSet::<String>::new();
    let mut parameter_ids = HashSet::<String>::new();
    let mut names_by_owner = HashMap::<String, HashSet<String>>::new();
    let mut next_ordinals = HashMap::<String, u32>::new();
    let record_name = |names_by_owner: &mut HashMap<String, HashSet<String>>,
                       storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
                       owner: &str,
                       name: &str|
     -> Result<(), cadmpeg_core::CodecError> {
        storage.with_storage(|| {
            if let Some(names) = ctx.get_mut_hash_map(names_by_owner, owner, OPERATION)? {
                if !ctx.contains_hash_set(names, name, OPERATION)? {
                    ctx.insert_hash_set(
                        names,
                        ctx.copy_retained_text(name, OPERATION)?,
                        OPERATION,
                    )?;
                }
                return Ok(());
            }
            let mut names = HashSet::new();
            ctx.insert_hash_set(
                &mut names,
                ctx.copy_retained_text(name, OPERATION)?,
                OPERATION,
            )?;
            ctx.insert_hash_map(
                names_by_owner,
                ctx.copy_retained_text(owner, OPERATION)?,
                names,
                OPERATION,
            )?;
            Ok(())
        })
    };
    let record_text = |set: &mut HashSet<String>,
                       storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
                       text: &str|
     -> Result<(), cadmpeg_core::CodecError> {
        if ctx.contains_hash_set(set, text, OPERATION)? {
            return Ok(());
        }
        storage.with_storage(|| {
            ctx.insert_hash_set(set, ctx.copy_retained_text(text, OPERATION)?, OPERATION)
        })?;
        Ok(())
    };
    for parameter in ctx.admit_iter(&parameters[..], OPERATION)? {
        if let Some(relation_id) = ctx.get_btree_map(
            &parameter.properties,
            RELATION_PARAMETER_ID_PROPERTY,
            OPERATION,
        )? {
            record_text(&mut relation_ids, &mut storage, relation_id)?;
        }
        record_text(&mut parameter_ids, &mut storage, parameter.id.as_str())?;
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        record_name(
            &mut names_by_owner,
            &mut storage,
            owner.as_str(),
            &parameter.name,
        )?;
        let next = if parameter.ordinal == u32::MAX {
            parameter.ordinal
        } else {
            parameter.ordinal + 1
        };
        if let Some(current) = ctx.get_mut_hash_map(
            &mut next_ordinals,
            owner.as_str(),
            "lookup SLDPRT existing parameter ordinal",
        )? {
            *current = (*current).max(next);
        } else {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut next_ordinals,
                    ctx.copy_retained_text(owner.as_str(), OPERATION)?,
                    next,
                    OPERATION,
                )
            })?;
        }
    }

    for lane in ctx.admit_iter(lane_refs, OPERATION)? {
        let (names_by_id, _names_by_id_storage) =
            first_positions(ctx, &lane.names, |name| name.id.as_str(), OPERATION)?;
        for relation in ctx.admit_iter(&lane.relation_instances, OPERATION)? {
            if relation.parameter_scalar_ref().is_some()
                || ctx
                    .get_hash_map(&owned, &relation.id, OPERATION)?
                    .is_some_and(Option::is_some)
                || ctx.contains_hash_set(&relation_ids, relation.id.as_str(), OPERATION)?
            {
                continue;
            }
            let Some(scalar) = relation_display_scalar_for_parameter(ctx, relation, lane)? else {
                continue;
            };
            let Some(feature) = ctx
                .get_hash_map(
                    &features_by_native_ref,
                    relation.feature_ref.as_str(),
                    OPERATION,
                )?
                .copied()
            else {
                continue;
            };
            let Some(source_name) = ctx
                .get_hash_map(&names_by_id, scalar.name.as_str(), OPERATION)?
                .map(|position| lane.names[*position].value.as_str())
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let Some((value, display, expression)) =
                relation_display_parameter_value(ctx, relation.family, scalar.value.get())?
            else {
                continue;
            };
            let owner = feature.id.as_str();
            let current_ordinal = ctx
                .get_hash_map(&next_ordinals, owner, OPERATION)?
                .copied()
                .unwrap_or(0);
            let Some(next_ordinal) = current_ordinal.checked_add(1) else {
                continue;
            };
            if let Some(ordinal) = ctx.get_mut_hash_map(
                &mut next_ordinals,
                owner,
                "lookup SLDPRT display parameter ordinal",
            )? {
                *ordinal = next_ordinal;
            } else {
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut next_ordinals,
                        ctx.copy_retained_text(owner, OPERATION)?,
                        next_ordinal,
                        OPERATION,
                    )
                })?;
            }
            let owner_names = ctx.get_hash_map(&names_by_owner, owner, OPERATION)?;
            let taken = |name: &str| -> Result<bool, cadmpeg_core::CodecError> {
                match owner_names {
                    Some(names) => ctx.contains_hash_set(names, name, OPERATION),
                    None => Ok(false),
                }
            };
            let mut candidate = scoped_reference_name(ctx, source_name, None, None, OPERATION)?;
            if taken(&candidate.0)? {
                candidate = scoped_reference_name(
                    ctx,
                    source_name,
                    Some(relation.offset),
                    None,
                    OPERATION,
                )?;
                let mut suffix = 0u32;
                while taken(&candidate.0)? {
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
            let relation_key = ctx
                .rsplit_once(&relation.id, "#", "split SLDPRT relation key")?
                .map_or(relation.id.as_str(), |(_, key)| key);
            if relation_key.is_empty() {
                continue;
            }
            let Some(id) = mint_formatted::<ParameterId>(
                ctx,
                format_args!("sldprt:model:parameter#reference:{relation_key}"),
                OPERATION,
            )?
            else {
                continue;
            };
            if ctx.contains_hash_set(&parameter_ids, id.as_str(), OPERATION)? {
                continue;
            }
            record_text(&mut parameter_ids, &mut storage, id.as_str())?;
            let mut properties = BTreeMap::new();
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ID_PROPERTY),
                ctx.copy_retained_text(&relation.id, OPERATION)?,
                OPERATION,
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_const!(RELATION_DISPLAY_SCALAR_ID_PROPERTY),
                ctx.copy_retained_text(&scalar.id, OPERATION)?,
                OPERATION,
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ROLE_PROPERTY),
                RELATION_PARAMETER_ROLE_REFERENCE.into(),
                OPERATION,
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!("source_name"),
                ctx.copy_retained_text(source_name, OPERATION)?,
                OPERATION,
            )?;
            let name = candidate.0;
            record_name(&mut names_by_owner, &mut storage, owner, &name)?;
            let parameter_name = ctx.copy_retained_text(&name, OPERATION)?;
            ctx.push_vec(
                parameters,
                DesignParameter {
                    id,
                    owner: Some(feature.id.try_clone_for_decode(ctx, OPERATION)?),
                    ordinal: current_ordinal,
                    name: parameter_name,
                    expression,
                    display,
                    value: Some(value),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties,
                    pmi: None,
                    native_ref: None,
                },
                OPERATION,
            )?;
            record_text(&mut relation_ids, &mut storage, relation.id.as_str())?;
        }
    }
    Ok(())
}

fn relation_display_parameter_value(
    ctx: &DecodeContext<'_>,
    family: FeatureInputRelationFamily,
    value: f64,
) -> Result<Option<(ParameterValue, Option<DimensionDisplay>, String)>, cadmpeg_core::CodecError> {
    Ok(Some(match family {
        FeatureInputRelationFamily::Angle => {
            let Some(angle) = Angle::new(value) else {
                return Ok(None);
            };
            (
                ParameterValue::Angle(angle),
                None,
                crate::history::literals::format_angle_rad(angle),
            )
        }
        FeatureInputRelationFamily::CircleDiameter => {
            let Some(millimetres) = Length::new(value * 1000.0) else {
                return Ok(None);
            };
            (
                ParameterValue::Length(millimetres),
                Some(DimensionDisplay::Diameter),
                ctx.format_retained(
                    format_args!(
                        "<MOD-DIAM>{}",
                        crate::history::literals::LengthLiteral(millimetres)
                    ),
                    "format SLDPRT relation display parameter",
                )?,
            )
        }
        FeatureInputRelationFamily::LineLineDistance
        | FeatureInputRelationFamily::PointPointDistance
        | FeatureInputRelationFamily::PointLineDistance
        | FeatureInputRelationFamily::PointPointHorizontalDistance
        | FeatureInputRelationFamily::PointPointVerticalDistance => {
            let Some(millimetres) = Length::new(value * 1000.0) else {
                return Ok(None);
            };
            (
                ParameterValue::Length(millimetres),
                None,
                crate::history::literals::format_length_mm(millimetres),
            )
        }
    }))
}

/// Apply relation-defined units and display semantics to parameters named by display scalars.
pub(crate) fn type_display_relation_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "group SLDPRT display relation families";

    let (ownership, _ownership_storage) = ctx
        .with_scoped_storage("SLDPRT relation ownership index", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    // Each owned parameter's relation family; `None` once its relations disagree.
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut families =
        HashMap::<&cadmpeg_ir::features::ParameterId, Option<FeatureInputRelationFamily>>::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        for relation in ctx.admit_iter(&lane.relation_instances, OPERATION)? {
            let Some(Some(parameter)) = ctx.get_hash_map(&ownership, &relation.id, OPERATION)?
            else {
                continue;
            };
            if let Some(family) = ctx.get_mut_hash_map(&mut families, parameter, OPERATION)? {
                if *family != Some(relation.family) {
                    *family = None;
                }
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut families, parameter, Some(relation.family), OPERATION)
            })?;
        }
    }
    for parameter in ctx.admit_iter(parameters, OPERATION)? {
        let Some(&Some(family)) = ctx.get_hash_map(&families, &parameter.id, OPERATION)? else {
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
                        ctx.format_retained(
                            format_args!(
                                "<MOD-DIAM>{}",
                                crate::history::literals::LengthLiteral(value)
                            ),
                            "format SLDPRT relation display parameter",
                        )?
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
                        ctx.format_retained(
                            format_args!(
                                "<MOD-DIAM>{}",
                                crate::history::literals::LengthLiteral(value)
                            ),
                            "format SLDPRT relation display parameter",
                        )?
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    // Each feature's body selection; `None` when a feature owns several.
    let mut selections = HashMap::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        for selection in ctx.admit_iter(&lane.body_selections, OPERATION)? {
            if let Some(selected) =
                ctx.get_mut_hash_map(&mut selections, selection.feature_ref.as_str(), OPERATION)?
            {
                *selected = None;
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut selections,
                    selection.feature_ref.as_str(),
                    Some(selection),
                    OPERATION,
                )
            })?;
        }
    }
    if selections.is_empty() {
        return Ok(());
    }
    for feature in ctx.admit_iter(features, OPERATION)? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&Some(selection)) = ctx.get_hash_map(&selections, native_ref, OPERATION)? else {
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
                    let ids = ctx.try_collect_vec(
                        ctx.admit_iter(&selection.local_body_ids, OPERATION)?
                            .map(|id| ctx.format_retained(format_args!("{id}"), OPERATION)),
                        OPERATION,
                    )?;
                    let Ok(selection) = cadmpeg_ir::features::BodySelection::local(
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

/// Each model feature identity keyed by the native record it names; a later feature replaces an
/// earlier one with the same native reference. Keys and identities are copies so the model
/// features stay editable.
fn model_feature_ids_by_native<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    operation: &'static str,
) -> Result<
    (
        HashMap<String, cadmpeg_ir::features::FeatureId>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut ids = HashMap::new();
    for feature in ctx.admit_iter(features, operation)? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        storage.with_storage(|| {
            let id = feature.id.try_clone_for_decode(ctx, operation)?;
            if let Some(previous) = ctx.get_mut_hash_map(&mut ids, native_ref, operation)? {
                *previous = id;
                return Ok(());
            }
            ctx.insert_hash_map(
                &mut ids,
                ctx.copy_retained_text(native_ref, operation)?,
                id,
                operation,
            )?;
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }
    Ok((ids, storage))
}

/// Selections of every lane grouped by the feature that owns them, in lane order.
fn selections_by_feature<'a, 'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    selections: impl Fn(&'a FeatureInputLane) -> &'a [T],
    feature_ref: impl Fn(&'a T) -> &'a str,
    operation: &'static str,
) -> Result<
    (
        HashMap<&'a str, Vec<&'a T>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut groups = HashMap::new();
    for lane in ctx.admit_iter(lanes, operation)? {
        for selection in ctx.admit_iter(selections(lane), operation)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut groups,
                    feature_ref(selection),
                    selection,
                    operation,
                    operation,
                )
            })?;
        }
    }
    Ok((groups, storage))
}

pub(crate) fn project_compact_edge_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const INDEX_OPERATION: &str = "index SLDPRT compact edge selections";
    let (selections, _selections_storage) = selections_by_feature(
        ctx,
        lanes,
        |lane| &lane.edge_selections,
        |selection| selection.feature_ref.as_str(),
        INDEX_OPERATION,
    )?;
    if selections.is_empty() {
        return Ok(());
    }
    let (feature_ids_by_native, _feature_ids_storage) =
        model_feature_ids_by_native(ctx, features, INDEX_OPERATION)?;
    for feature in ctx.admit_iter(features, INDEX_OPERATION)? {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let native_ref = feature.native_ref.as_deref();
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), cadmpeg_core::CodecError> {
            let Some(native_ref) = native_ref else {
                return Ok(());
            };
            let Some(edge_selections) = ctx
                .get_hash_map(&selections, native_ref, INDEX_OPERATION)?
                .filter(|selections| !selections.is_empty())
            else {
                return Ok(());
            };
            let projected_edges = |selections: &[&FeatureInputEdgeSelection]| -> Result<_, cadmpeg_core::CodecError> {
                const OPERATION: &str = "project SLDPRT compact generated edges";
                let native = compact_edge_selection_set_value_charged(ctx, selections)?;
                let mut generated = Vec::<cadmpeg_ir::features::GeneratedEdgeRef>::new();
                let mut complete = true;
                for selection in ctx.admit_iter(selections, OPERATION)? {
                    let Some(native_feature) = selection.terminal_feature_ref.as_deref() else {
                        complete = false;
                        break;
                    };
                    let Some(feature_id) =
                        ctx.get_hash_map(&feature_ids_by_native, native_feature, OPERATION)?
                    else {
                        complete = false;
                        break;
                    };
                    let feature = feature_id.try_clone_for_decode(ctx, OPERATION)?;
                    let local_id = compact_edge_path_value_charged(ctx, selection)?;
                    let Ok(edge) = cadmpeg_ir::features::GeneratedEdgeRef::new(feature, local_id, ctx,)? else {
                        complete = false;
                        break;
                    };
                    if !ctx.contains(&generated, &edge, OPERATION)? {
                        ctx.push_vec(&mut generated, edge, OPERATION)?;
                    }
                }
                if complete && !generated.is_empty() {
                    EdgeSelection::generated(generated, native, ctx,)?
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
                        ctx.reserve_vec(&mut replacement_groups, radius_groups.len(), GROUP_OPERATION)?;
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
            const DEPENDENCY_OPERATION: &str = "add SLDPRT compact edge dependency";
            for selection in ctx.admit_iter(edge_selections, DEPENDENCY_OPERATION)? {
                add_producer_dependencies(
                    ctx,
                    &feature_ids_by_native,
                    &selection.producer_feature_refs,
                    feature_id,
                    dependencies,
                    DEPENDENCY_OPERATION,
                )?;
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

/// Native edge-endpoint component instance tag.
const EDGE_ENDPOINT_INSTANCE: u16 = 0x8083;

/// The type signatures of the first `limit` endpoint components of `references`, in order.
fn endpoint_signatures(
    ctx: &DecodeContext<'_>,
    references: &[Vec<crate::records::FeatureInputComponentPathEntry>],
    limit: usize,
    operation: &'static str,
) -> Result<Vec<[u8; 12]>, cadmpeg_core::CodecError> {
    let mut signatures = Vec::new();
    ctx.position_by(
        references,
        |reference| {
            ctx.position_by(
                reference,
                |component| {
                    if component.instance == Some(EDGE_ENDPOINT_INSTANCE) {
                        ctx.push_vec(&mut signatures, component.type_signature, operation)?;
                    }
                    Ok(signatures.len() == limit)
                },
                operation,
            )?;
            Ok(signatures.len() == limit)
        },
        operation,
    )?;
    Ok(signatures)
}

/// The `(index, radius)` dimensions of a variable fillet in index order, and whether the indices
/// are exactly `0..n`; `None` when a name is not a radius dimension.
fn ordered_fillet_dimensions(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    parameter_names: &BTreeSet<&cadmpeg_core::text::NonBlankString>,
    operation: &'static str,
) -> Result<Option<(Vec<(usize, PositiveLength)>, bool)>, cadmpeg_core::CodecError> {
    let mut ordered = Vec::new();
    for name in ctx.admit_iter(parameter_names, operation)? {
        let Some(parameter) =
            variable_fillet_dimension_index_for_feature(ctx, feature, name.as_str())?.zip(
                ctx.get_btree_map(&feature.parameters, *name, operation)?
                    .and_then(|value| {
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    }),
            )
        else {
            return Ok(None);
        };
        ctx.push_vec(&mut ordered, parameter, operation)?;
    }
    ctx.sort_unstable_by(&mut ordered, |value| &value.0, Ord::cmp, operation)?;
    let consecutive = ctx.all_by(
        ordered.iter().enumerate(),
        |(expected, (actual, _))| Ok(expected == *actual),
        operation,
    )?;
    Ok(Some((ordered, consecutive)))
}

/// One variable radius profile over the ordered dimensions, applied to every selection.
fn whole_feature_radius_group<'a>(
    ctx: &DecodeContext<'_>,
    ordered: Vec<(usize, PositiveLength)>,
    selections: &[&'a FeatureInputEdgeSelection],
) -> Result<Option<Vec<RadiusSelectionGroup<'a>>>, cadmpeg_core::CodecError> {
    let mut selections = ctx.copy_slice(selections, "SLDPRT variable radius source samples")?;
    ctx.sort_unstable_by(
        &mut selections,
        |value| &value.ordinal,
        Ord::cmp,
        "SLDPRT variable radius source samples",
    )?;
    let mut sample_storage = ctx.reserve_scoped(0, "SLDPRT variable radius source samples")?;
    let mut points = Vec::new();
    for (parameter, (_, radius)) in ctx
        .admit_iter(ordered, "SLDPRT variable radius source conversion")?
        .enumerate()
    {
        let Some(parameter) = f64_from_index(parameter) else {
            return Ok(None);
        };
        ctx.push_scoped_vec(
            &mut sample_storage,
            &mut points,
            VariableRadius {
                parameter,
                radius: Length::from(radius),
            },
            "SLDPRT variable radius source samples",
        )?;
    }
    let Some(points) = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points, ctx)?.ok()
    else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    ctx.push_vec(
        &mut groups,
        RadiusSelectionGroup(RadiusSpec::Variable { points }, selections),
        "SLDPRT variable radius source samples",
    )?;
    Ok(Some(groups))
}

fn variable_fillet_radius_groups<'a>(
    ctx: &DecodeContext<'_>,
    feature_ref: &str,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    selections: &[&'a FeatureInputEdgeSelection],
) -> Result<Option<Vec<RadiusSelectionGroup<'a>>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "project SLDPRT variable fillet radii";
    let Some(history) = ctx.find_by(
        histories,
        |history| {
            ctx.any_by(
                &history.features,
                |feature| ctx.equal(feature.id.as_str(), feature_ref, OPERATION),
                OPERATION,
            )
        },
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let Some(feature) = ctx.find_by(
        &history.features,
        |feature| {
            Ok(ctx.equal(feature.id.as_str(), feature_ref, OPERATION)?
                && ctx.eq_ignore_ascii_case(&feature.kind, "VarFillet", OPERATION)?)
        },
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let mut names_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut parameter_names = BTreeSet::new();
    for name in ctx
        .admit_iter(&feature.parameters, OPERATION)?
        .map(|(name, _)| name)
    {
        if variable_fillet_dimension_index_for_feature(ctx, feature, name.as_str())?.is_some() {
            names_storage
                .with_storage(|| ctx.insert_btree_set(&mut parameter_names, name, OPERATION))?;
        }
    }
    if parameter_names.len() != feature.parameters.len() || parameter_names.len() < 2 {
        return Ok(None);
    }
    let has_endpoint_reference = ctx.any_by(
        selections,
        |selection| Ok(!endpoint_signatures(ctx, &selection.references, 1, OPERATION)?.is_empty()),
        OPERATION,
    )?;

    // A legacy VarFillet with exactly the two ordered controls 0 and 1 may
    // omit endpoint markers entirely. Its three-reference edge-control
    // roster supplies one feature-wide radius profile for every selected edge.
    // Require that roster shape and reject any endpoint-bearing reference so a
    // feature with several endpoint-specific profiles cannot enter this path.
    let has_legacy_edge_control_roster = ctx.any_by(
        selections,
        |selection| {
            Ok(selection.references.len() == 3
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
                    }))
        },
        OPERATION,
    )?;
    if parameter_names.len() == 2 && has_legacy_edge_control_roster && !has_endpoint_reference {
        let Some((ordered, consecutive)) =
            ordered_fillet_dimensions(ctx, feature, &parameter_names, OPERATION)?
        else {
            return Ok(None);
        };
        if consecutive {
            return whole_feature_radius_group(ctx, ordered, selections);
        }
    }

    let mut controls_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut vertex_radii = BTreeMap::<[u8; 12], PositiveLength>::new();
    let mut control_names = HashSet::<String>::new();
    let mut non_vertex_control_names = HashSet::<String>::new();
    let mut non_vertex_control_references = Vec::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        let mut objects = Vec::new();
        for candidate in ctx.admit_iter(&history.features, OPERATION)? {
            if let Some(name) = feature_object_name(candidate, lane) {
                controls_storage.with_storage(|| {
                    ctx.push_vec(&mut objects, (name.offset, candidate), OPERATION)
                })?;
            }
        }
        ctx.sort_unstable_by(&mut objects, |value| &value.0, Ord::cmp, OPERATION)?;
        let Some(index) = ctx.position_by(
            &objects,
            |(_, candidate)| ctx.equal(candidate.id.as_str(), feature_ref, OPERATION),
            OPERATION,
        )?
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
        for super::selections::VariableFilletControl(name, references) in
            ctx.admit_iter(controls, OPERATION)?
        {
            match endpoint_signatures(ctx, &references, 2, OPERATION)?.as_slice() {
                [vertex] => {
                    if ctx.contains_hash_set(&control_names, name.as_str(), OPERATION)? {
                        return Ok(None);
                    }
                    let Some(radius) = ctx
                        .get_btree_map(&feature.parameters, name.as_str(), OPERATION)?
                        .and_then(|value| {
                            crate::history::literals::parse_positive_dimension_length_mm(value)
                        })
                    else {
                        return Ok(None);
                    };
                    controls_storage.with_storage(|| {
                        ctx.insert_hash_set(&mut control_names, name, OPERATION)
                    })?;
                    match ctx.get_btree_map(&vertex_radii, vertex, OPERATION)? {
                        Some(existing) if !same_dimension_length(existing.get(), radius.get()) => {
                            return Ok(None);
                        }
                        Some(_) => {}
                        None => {
                            controls_storage.with_storage(|| {
                                ctx.insert_btree_map(&mut vertex_radii, *vertex, radius, OPERATION)
                            })?;
                        }
                    }
                }
                [] => {
                    if ctx.contains_hash_set(&non_vertex_control_names, name.as_str(), OPERATION)? {
                        return Ok(None);
                    }
                    controls_storage.with_storage(|| {
                        ctx.insert_hash_set(&mut non_vertex_control_names, name, OPERATION)
                    })?;
                    controls_storage.with_storage(|| {
                        ctx.extend_vec(&mut non_vertex_control_references, references, OPERATION)
                    })?;
                }
                _ => return Ok(None),
            }
        }
    }
    if vertex_radii.is_empty() {
        if parameter_names.len() != 2
            || !control_names.is_empty()
            || non_vertex_control_names.len() != parameter_names.len()
            || !ctx.all_by(
                &parameter_names,
                |name| ctx.contains_hash_set(&non_vertex_control_names, name.as_str(), OPERATION),
                OPERATION,
            )?
            || non_vertex_control_references.is_empty()
            || has_endpoint_reference
        {
            return Ok(None);
        }
        let Some((ordered, true)) =
            ordered_fillet_dimensions(ctx, feature, &parameter_names, OPERATION)?
        else {
            return Ok(None);
        };
        if !ctx.all_by(
            &non_vertex_control_references,
            |reference| {
                ctx.any_by(
                    selections,
                    |selection| ctx.contains(&selection.references, reference, OPERATION),
                    OPERATION,
                )
            },
            OPERATION,
        )? {
            return Ok(None);
        }
        return whole_feature_radius_group(ctx, ordered, selections);
    }
    if control_names.len() != parameter_names.len()
        || !ctx.all_by(
            &parameter_names,
            |name| ctx.contains_hash_set(&control_names, name.as_str(), OPERATION),
            OPERATION,
        )?
    {
        return Ok(None);
    }

    // Every endpoint signature the selections reference.
    let mut selected_endpoints = BTreeSet::new();
    for selection in ctx.admit_iter(selections, OPERATION)? {
        for reference in ctx.admit_iter(&selection.references, OPERATION)? {
            for component in ctx.admit_iter(reference, OPERATION)? {
                if component.instance == Some(EDGE_ENDPOINT_INSTANCE) {
                    controls_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut selected_endpoints,
                            component.type_signature,
                            OPERATION,
                        )
                    })?;
                }
            }
        }
    }
    if !ctx.all_by(
        &vertex_radii,
        |(signature, _)| ctx.contains_btree_set(&selected_endpoints, signature, OPERATION),
        OPERATION,
    )? {
        return Ok(None);
    }

    let mut groups = Vec::<(
        (PositiveLength, PositiveLength),
        Vec<&FeatureInputEdgeSelection>,
    )>::new();
    let mut unassigned = Vec::new();
    for &selection in ctx.admit_iter(selections, OPERATION)? {
        match endpoint_signatures(ctx, &selection.references, 3, OPERATION)?.as_slice() {
            [first, second] => {
                let (Some(first_radius), Some(second_radius)) = (
                    ctx.get_btree_map(&vertex_radii, first, OPERATION)?,
                    ctx.get_btree_map(&vertex_radii, second, OPERATION)?,
                ) else {
                    return Ok(None);
                };
                let pair = (*first_radius, *second_radius);
                if let Some(position) =
                    ctx.position_by(&groups, |(candidate, _)| Ok(*candidate == pair), OPERATION)?
                {
                    ctx.push_vec(&mut groups[position].1, selection, OPERATION)?;
                } else {
                    let mut grouped = Vec::new();
                    ctx.push_vec(&mut grouped, selection, OPERATION)?;
                    ctx.push_vec(&mut groups, (pair, grouped), OPERATION)?;
                }
            }
            [] => ctx.push_vec(&mut unassigned, selection, OPERATION)?,
            _ => return Ok(None),
        }
    }
    if groups.len() == 1 {
        ctx.append_vec(&mut groups[0].1, &mut unassigned, OPERATION)?;
        ctx.sort_unstable_by(
            &mut groups[0].1,
            |value| &value.ordinal,
            Ord::cmp,
            OPERATION,
        )?;
    } else if !unassigned.is_empty() {
        return Ok(None);
    }
    if groups.is_empty() {
        return Ok(None);
    }
    let mut result = Vec::new();
    for ((first, second), selections) in ctx.admit_iter(groups, OPERATION)? {
        let mut sample_storage = ctx.reserve_scoped(0, "SLDPRT variable radius source samples")?;
        let raw_points = sample_storage.with_storage(|| {
            ctx.collect_retained_vec(
                [
                    VariableRadius {
                        parameter: 0.0,
                        radius: Length::from(first),
                    },
                    VariableRadius {
                        parameter: 1.0,
                        radius: Length::from(second),
                    },
                ],
                "SLDPRT variable radius source samples",
            )
        })?;
        let Some(points) =
            cadmpeg_ir::features::edge_treatments::VariableRadii::new(raw_points, ctx)?.ok()
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut result,
            RadiusSelectionGroup(RadiusSpec::Variable { points }, selections),
            OPERATION,
        )?;
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
    let (selections, _selections_storage) = selections_by_feature(
        ctx,
        lanes,
        |lane| &lane.surface_selections,
        |selection| selection.feature_ref.as_str(),
        INDEX_OPERATION,
    )?;
    let (feature_ids_by_native, _feature_ids_storage) =
        model_feature_ids_by_native(ctx, features, INDEX_OPERATION)?;
    let mut history_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut history_features = Vec::new();
    for history in ctx.admit_iter(histories, INDEX_OPERATION)? {
        for feature in ctx.admit_iter(&history.features, INDEX_OPERATION)? {
            history_storage
                .with_storage(|| ctx.push_vec(&mut history_features, feature, INDEX_OPERATION))?;
        }
    }
    // The model feature identity a native producer reference names.
    let producer_id =
        |native: &str| ctx.get_hash_map(&feature_ids_by_native, native, INDEX_OPERATION);
    for feature in ctx.admit_iter(&mut features[..], INDEX_OPERATION)? {
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
                    let Some(feature_selections) =
                        ctx.get_hash_map(&selections, native_ref, OPERATION)?.map(Vec::as_slice)
                    else {
                        break 'feature_edit;
                    };
                    if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
                        definition
                    {
                        const OPERATION: &str = "project SLDPRT pattern face seeds";
                        if ctx.any_by(
                            &seeds[..],
                            |seed| Ok(matches!(seed, PatternSeed::Feature(_))),
                            OPERATION,
                        )? {
                            break 'feature_edit;
                        }
                        for selection in ctx.admit_iter(feature_selections, OPERATION)? {
                            let native = compact_surface_selection_value(ctx, &selection.components)?;
                            let generated = match component_path_feature(
                                ctx,
                                &selection.components,
                                &history_features,
                                &selection.feature_ref,
                                ComponentPathEnd::Trailing,
                            )? {
                                Some((component, producer)) => producer_id(producer.id.as_str())?
                                    .zip(component.local_id.as_ref()),
                                None => None,
                            };
                            let seed = match generated {
                                Some((producer, local_id)) => {
                                    let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
                                    let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                    let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(
                                        producer_id,
                                        local_id_text, ctx,
                                    )? else {
                                        let seed = PatternSeed::Faces(
                                            cadmpeg_ir::features::FaceSelection::Native(native),
                                        );
                                        if !ctx.contains(seeds, &seed, OPERATION)? {
                                            ctx.push_vec(seeds, seed, OPERATION)?;
                                        }
                                        continue;
                                    };
                                    if ctx.any_by(
                                        &seeds[..],
                                        |seed| match seed {
                                            PatternSeed::Faces(
                                                cadmpeg_ir::features::FaceSelection::Generated { faces, .. },
                                            ) => ctx.contains(faces, &face, OPERATION),
                                            _ => Ok(false),
                                        },
                                        OPERATION,
                                    )? {
                                        continue;
                                    }
                                    if !ctx.contains(dependencies.as_slice(), producer, OPERATION)? {
 dependencies.insert(ctx, producer.try_clone_for_decode(ctx, OPERATION)?, OPERATION)?;
}
                                    let mut faces = Vec::new();
                                    ctx.reserve_vec(&mut faces, 1, OPERATION)?;
                                    faces.push(face);
                                    let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                                    PatternSeed::Faces(
                                        cadmpeg_ir::features::FaceSelection::generated(
                                            faces,
                                            native_copy, ctx,
                                        )?
                                        .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native)),
                                    )
                                }
                                None => {
                                    PatternSeed::Faces(cadmpeg_ir::features::FaceSelection::Native(native))
                                }
                            };
                            if !ctx.contains(seeds, &seed, OPERATION)? {
                                ctx.push_vec(seeds, seed, OPERATION)?;
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
                            let generated = match selection.terminal_feature_ref.as_deref() {
 Some(producer) => producer_id(producer)?,
 None => None,
 }
                                .zip(selection.components.last())
                                .and_then(|(producer, component)| Some((producer, component.local_id?)));
                            if let Some((producer, local_id)) = generated {
                                let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
                                let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                let Ok(face) = cadmpeg_ir::features::GeneratedFaceRef::new(
                                    producer_id,
                                    local_id_text, ctx,
                                )? else {
                                    complete = false;
                                    continue;
                                };
                                ctx.charge_work(u64::try_from(faces.len())
                                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                                if !ctx.contains(&faces, &face, OPERATION)? {
                                    ctx.reserve_vec(&mut faces, 1, OPERATION)?;
                                    faces.push(face);
                                }
                            } else {
                                complete = false;
                            }
                            add_producer_dependencies(ctx, &feature_ids_by_native, &selection.producer_feature_refs, feature_id, dependencies, OPERATION)?;
                        }
                        *targets = if complete && !faces.is_empty() {
                            let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                            cadmpeg_ir::features::FaceSelection::generated(faces, native_copy, ctx,)?
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
                        let target_producer = match target.terminal_feature_ref.as_deref() {
 Some(producer) => producer_id(producer)?,
 None => None,
 };
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
                            ctx.try_reserve_retained_text(&mut local_id, local_id_bytes, OPERATION)?;
                            for id in target.components.iter().filter_map(|component| component.local_id) {
                                if !local_id.is_empty() {
                                    ctx.push_retained_char(&mut local_id, ',', "format SLDPRT surface cut body separator")?;
                                }
                                write!(local_id, "{id}").map_err(|_| {
                                    cadmpeg_core::CodecError::malformed("cannot format SLDPRT surface cut body id")
                                })?;
                            }
                            let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
                            let Ok(body) =
                                cadmpeg_ir::features::GeneratedBodyRef::new(producer_id, local_id, ctx,)?
                            else {
                                break 'feature_edit;
                            };
                            let mut bodies = Vec::new();
                            ctx.reserve_vec(&mut bodies, 1, OPERATION)?;
                            bodies.push(body);
                            let native_copy = ctx.copy_retained_text(&target_native, OPERATION)?;
                            *targets = BodySelection::generated(bodies, native_copy, ctx)?
                                .unwrap_or(BodySelection::Native(target_native));
                            if !ctx.contains(dependencies.as_slice(), producer, OPERATION)? {
 dependencies.insert(ctx, producer.try_clone_for_decode(ctx, OPERATION)?, OPERATION)?;
}
                        }
                        let tool_native = compact_surface_selection_value(ctx, &tool.components)?;
                        let tool_generated = match tool.terminal_feature_ref.as_deref() {
 Some(producer) => producer_id(producer)?,
 None => None,
 }
                            .zip(tool.components.last())
                            .and_then(|(producer, component)| {
                                component.local_id.map(|local_id| (producer, local_id))
                            });
                        if let Some((producer, local_id)) = tool_generated {
                            let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
                            let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                            let generated_face = cadmpeg_ir::features::GeneratedFaceRef::new(
                                producer_id,
                                local_id_text, ctx,
                            )?;
                            *tools = if let Ok(face) = generated_face {
                                let mut faces = Vec::new();
                                ctx.reserve_vec(&mut faces, 1, OPERATION)?;
                                faces.push(face);
                                let native_copy = ctx.copy_retained_text(&tool_native, OPERATION)?;
                                FaceSelection::generated(faces, native_copy, ctx,)?
                                    .unwrap_or(FaceSelection::Native(tool_native))
                            } else {
                                FaceSelection::Native(tool_native)
                            };
                            if !ctx.contains(dependencies.as_slice(), producer, OPERATION)? {
 dependencies.insert(ctx, producer.try_clone_for_decode(ctx, OPERATION)?, OPERATION)?;
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
                                let generated = match selection.terminal_feature_ref.as_deref() {
 Some(producer) => producer_id(producer)?,
 None => None,
 }
                                    .zip(selection.components.last())
                                    .and_then(|(producer, component)| {
                                        Some((producer, component.local_id?))
                                    });
                                let face = match generated {
                                    Some((producer, local_id)) => {
                                        if producer != feature_id
 && !ctx.contains(dependencies.as_slice(), producer, OPERATION)?
                                        {
                                            dependencies.insert(ctx, producer.try_clone_for_decode(ctx, OPERATION)?, OPERATION)?;
                                        }
                                        let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(
                                            producer.try_clone_for_decode(ctx, OPERATION)?, local_id_text, ctx,
                                        )? {
                                            Ok(face) => {
                                                let mut faces = Vec::new();
                                                ctx.reserve_vec(&mut faces, 1, OPERATION)?;
                                                faces.push(face);
                                                let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                                                cadmpeg_ir::features::FaceSelection::generated(faces, native_copy, ctx,)?
                                                    .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                                            }
                                            Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                                        }
                                    }
                                    None => cadmpeg_ir::features::FaceSelection::Native(native),
                                };
                                add_producer_dependencies(ctx, &feature_ids_by_native, &selection.producer_feature_refs, feature_id, dependencies, OPERATION)?;
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
                            ctx.charge_work(u64::try_from(selection.producer_feature_refs.len())
                                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                            add_producer_dependencies(ctx, &feature_ids_by_native, &selection.producer_feature_refs, feature_id, dependencies, OPERATION)?;
                        }
                        break 'feature_edit;
                    }
                    let first_component = matches!(
                        definition,
                        FeatureDefinition::Operation(FeatureOperation::CosmeticThread { .. })
                    );
                    let Some(selection) = (if first_component {
                        cosmetic_thread_surface_selection_consensus(ctx, feature_selections, OPERATION)?
                    } else {
                        surface_selection_consensus(ctx, feature_selections, OPERATION)?
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
                        let generated = match selection.terminal_feature_ref.as_deref() {
 Some(producer) => producer_id(producer)?,
 None => None,
 }
                            .zip(selection.components.last())
                            .and_then(|(feature, component)| Some((feature, component.local_id?)));
                        let face = match generated {
                            Some((producer, local_id)) => {
                                if !ctx.contains(dependencies.as_slice(), producer, OPERATION)? {
 dependencies.insert(ctx, producer.try_clone_for_decode(ctx, OPERATION)?, OPERATION)?;
}
                                let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
                                let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text, ctx,)? {
                                    Ok(face) => {
                                        let mut faces = Vec::new();
                                        ctx.reserve_vec(&mut faces, 1, OPERATION)?;
                                        faces.push(face);
                                        let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                                        cadmpeg_ir::features::FaceSelection::generated(faces, native_copy, ctx,)?
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
                    let generated = match producer {
 Some(producer) => producer_id(producer)?,
 None => None,
 }
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
                                        let producer_id = feature.try_clone_for_decode(ctx, OPERATION)?;
                                        let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text, ctx,)? {
                                            Ok(face) => {
                                                let mut generated_faces = Vec::new();
                                                ctx.reserve_vec(&mut generated_faces, 1, OPERATION)?;
                                                generated_faces.push(face);
                                                let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                                                cadmpeg_ir::features::FaceSelection::generated(generated_faces, native_copy, ctx,)?
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
                                        let producer_id = feature.try_clone_for_decode(ctx, OPERATION)?;
                                        let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
                                        match cadmpeg_ir::features::GeneratedVertexRef::new(producer_id, local_id_text, ctx,)? {
                                            Ok(generated_vertex) => {
                                                let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
                                                match cadmpeg_ir::features::VertexSelection::generated(generated_vertex, native_copy, ctx,)? {
                                                    Ok(selection) => selection,
                                                    Err(_) => cadmpeg_ir::features::VertexSelection::native(native, ctx,)?.unwrap_or(
                                                            cadmpeg_ir::features::VertexSelection::Unresolved,
                                                        ),
                                                }
                                            }
                                            Err(_) => cadmpeg_ir::features::VertexSelection::native(native, ctx,)?
                                                .unwrap_or(cadmpeg_ir::features::VertexSelection::Unresolved),
                                        }
                                    }
                                    None => cadmpeg_ir::features::VertexSelection::native(native, ctx,)?
                                        .unwrap_or(cadmpeg_ir::features::VertexSelection::Unresolved),
                                };
                            }
                        }
                    }
                    add_producer_dependencies(ctx, &feature_ids_by_native, &selection.producer_feature_refs, feature_id, dependencies, OPERATION)?;
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    // Each resolved cosmetic thread face, keyed by its feature's native reference.
    let mut alias_storage = ctx.reserve_scoped(0, ALIAS_OPERATION)?;
    let mut face_aliases = HashMap::new();
    for feature in ctx.admit_iter(&features[..], ALIAS_OPERATION)? {
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
        let face_copy = face.try_clone_for_decode(ctx, ALIAS_OPERATION)?;
        alias_storage.with_storage(|| {
            if let Some(previous) =
                ctx.get_mut_hash_map(&mut face_aliases, native, ALIAS_OPERATION)?
            {
                *previous = face_copy;
                return Ok(());
            }
            let native_key = ctx.copy_retained_text(native, ALIAS_OPERATION)?;
            ctx.insert_hash_map(&mut face_aliases, native_key, face_copy, ALIAS_OPERATION)?;
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }
    if face_aliases.is_empty() {
        return Ok(());
    }
    for feature in ctx.admit_iter(features, ALIAS_OPERATION)? {
        let Some(target) = ctx.get_btree_map(
            &feature.source_properties,
            "ReferenceFaceFeature",
            ALIAS_OPERATION,
        )?
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. })
        ) {
            continue;
        }
        let Some(face) = ctx.get_hash_map(&face_aliases, target.as_str(), ALIAS_OPERATION)? else {
            continue;
        };
        let face = face.try_clone_for_decode(ctx, ALIAS_OPERATION)?;
        if let FaceSelection::Generated { faces, .. } = &face {
            for generated in ctx.admit_iter(faces.as_slice(), ALIAS_OPERATION)? {
                add_dependency(
                    ctx,
                    &generated.feature,
                    &feature.id,
                    &mut feature.dependencies,
                    ALIAS_OPERATION,
                )?;
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
    for mut lane_selections in ctx.admit_iter(by_lane, OPERATION)?.map(|(_, group)| group) {
        ctx.sort_unstable_by(
            &mut lane_selections,
            |value| &value.offset,
            Ord::cmp,
            OPERATION,
        )?;
        let [center, side_one, side_two] = lane_selections.as_slice() else {
            return Ok(None);
        };
        if let Some([expected_center, expected_side_one, expected_side_two]) = consensus {
            if !same_surface_selection_semantics(ctx, expected_center, center, OPERATION)?
                || !same_surface_selection_semantics(ctx, expected_side_one, side_one, OPERATION)?
                || !same_surface_selection_semantics(ctx, expected_side_two, side_two, OPERATION)?
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
) -> Result<BTreeMap<&'a str, Vec<&'a FeatureInputSurfaceSelection>>, cadmpeg_core::CodecError> {
    let mut by_lane = BTreeMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new();
    for selection in ctx.admit_iter(selections, operation)? {
        ctx.push_btree_group(
            &mut by_lane,
            selection.parent.as_str(),
            *selection,
            operation,
            operation,
        )?;
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
    const GROUP_OPERATION: &str = "group SLDPRT draft operand candidates";
    let mut candidates_storage = ctx.reserve_scoped(0, GROUP_OPERATION)?;
    let mut candidates = HashMap::<String, Vec<DraftOperands>>::new();
    for lane in ctx.admit_iter(lanes, GROUP_OPERATION)? {
        for (feature, operands) in ctx.admit_iter(
            draft_operand_candidates(ctx, histories, lane)?,
            GROUP_OPERATION,
        )? {
            candidates_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut candidates,
                    feature,
                    operands,
                    GROUP_OPERATION,
                    GROUP_OPERATION,
                )
            })?;
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let (feature_ids_by_native, _feature_ids_storage) =
        model_feature_ids_by_native(ctx, features, INDEX_OPERATION)?;
    for feature in ctx.admit_iter(features, INDEX_OPERATION)? {
        let native_ref = feature.native_ref.as_deref();
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                let Some(native_ref) = native_ref else {
                    return Ok(());
                };
                let Some(operands) = ctx.get_hash_map(&candidates, native_ref, GROUP_OPERATION)?
                else {
                    return Ok(());
                };
                let Some(first) = operands.first() else {
                    return Ok(());
                };
                if !ctx.all_by(
                    operands,
                    |item| same_draft_operands(ctx, first, item),
                    GROUP_OPERATION,
                )? {
                    return Ok(());
                }
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
    for path in ctx.admit_iter(paths, OPERATION)? {
        let Some(terminal) = component_path_terminal_feature(
            ctx,
            path,
            histories.iter().flat_map(|history| &history.features),
        )?
        else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        if ctx.equal(terminal.as_str(), consumer_ref, OPERATION)? {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        }
        let Some((producer, local_id)) = ctx
            .get_hash_map(feature_ids_by_native, terminal.as_str(), OPERATION)?
            .zip(
                path.last()
                    .and_then(|component| component.local_id.as_ref()),
            )
        else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        let producer_id = producer.try_clone_for_decode(ctx, OPERATION)?;
        let local_id_text = ctx.format_retained(format_args!("{local_id}"), OPERATION)?;
        let Ok(face) =
            cadmpeg_ir::features::GeneratedFaceRef::new(producer_id, local_id_text, ctx)?
        else {
            return Ok(cadmpeg_ir::features::FaceSelection::Native(native));
        };
        if !ctx.contains(&generated, &face, OPERATION)? {
            ctx.push_vec(&mut generated, face, OPERATION)?;
        }
        if !ctx.contains(&generated_dependencies, producer, OPERATION)? {
            let dependency = producer.try_clone_for_decode(ctx, OPERATION)?;
            ctx.push_vec(&mut generated_dependencies, dependency, OPERATION)?;
        }
    }
    if generated.is_empty() {
        Ok(cadmpeg_ir::features::FaceSelection::Native(native))
    } else {
        for dependency in ctx.admit_iter(generated_dependencies, OPERATION)? {
            if !ctx.contains(dependencies.as_slice(), &dependency, OPERATION)? {
                dependencies.insert(ctx, dependency, OPERATION)?;
            }
        }
        let native_copy = ctx.copy_retained_text(&native, OPERATION)?;
        Ok(
            cadmpeg_ir::features::FaceSelection::generated(generated, native_copy, ctx)?
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
    // The first path with each local identifier sequence, in path order.
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut seen = HashSet::<Vec<Option<u32>>>::new();
    let mut unique = Vec::new();
    for index in ctx.admit_iter(0..path_count, operation)? {
        let components = path_at(index);
        let key = storage.with_storage(|| {
            ctx.collect_vec(
                ctx.admit_iter(components, operation)?
                    .map(|component| component.local_id),
                operation,
            )
        })?;
        if storage.with_storage(|| ctx.insert_hash_set(&mut seen, key, operation))? {
            storage.with_storage(|| ctx.push_vec(&mut unique, components, operation))?;
        }
    }
    drop(seen);
    let set = unique.len() != 1;
    let mut value = String::new();
    if set {
        ctx.append_retained(&mut value, set_prefix, operation)?;
    }
    for (emitted, components) in ctx.admit_iter(&unique, operation)?.enumerate() {
        if emitted != 0 {
            ctx.push_retained_char(&mut value, ';', "format SLDPRT surface path separator")?;
        }
        ctx.append_retained(&mut value, PATH_PREFIX, operation)?;
        for (component_index, component) in ctx.admit_iter(*components, operation)?.enumerate() {
            if component_index != 0 {
                ctx.push_retained_char(
                    &mut value,
                    ',',
                    "format SLDPRT surface component separator",
                )?;
            }
            match component.local_id {
                Some(local_id) => ctx.append_formatted_retained(
                    &mut value,
                    format_args!("{local_id}"),
                    operation,
                )?,
                None => ctx.push_retained_char(
                    &mut value,
                    '_',
                    "format SLDPRT absent surface component",
                )?,
            }
        }
    }
    Ok(value)
}

fn surface_selection_consensus<'a>(
    ctx: &DecodeContext<'_>,
    selections: &[&'a FeatureInputSurfaceSelection],
    operation: &'static str,
) -> Result<Option<&'a FeatureInputSurfaceSelection>, cadmpeg_core::CodecError> {
    let Some(first) = selections.first().copied() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(
            selections,
            |selection| same_surface_selection_semantics(ctx, first, selection, operation),
            operation,
        )?
        .then_some(first))
}

/// Configuration lanes repeat a cosmetic-thread cylinder reference, but only
/// its first typed component identifies the attached face.  The remaining
/// components retain the owning path and can vary with the lane's instance
/// path.  Reject only when the attached-face component itself disagrees.
fn cosmetic_thread_surface_selection_consensus<'a>(
    ctx: &DecodeContext<'_>,
    selections: &[&'a FeatureInputSurfaceSelection],
    operation: &'static str,
) -> Result<Option<&'a FeatureInputSurfaceSelection>, cadmpeg_core::CodecError> {
    let Some(first) = selections.first().copied() else {
        return Ok(None);
    };
    let Some(first_component) = first.components.first() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(
            selections,
            |selection| {
                Ok(selection.components.first().is_some_and(|component| {
                    component.local_id == first_component.local_id
                        && component.type_signature[4..8] == first_component.type_signature[4..8]
                }))
            },
            operation,
        )?
        .then_some(first))
}

fn same_surface_selection_semantics(
    ctx: &DecodeContext<'_>,
    left: &FeatureInputSurfaceSelection,
    right: &FeatureInputSurfaceSelection,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(left.components.len() == right.components.len()
        && ctx.equal(
            &left.producer_feature_refs,
            &right.producer_feature_refs,
            operation,
        )?
        && ctx.equal(
            &left.terminal_feature_ref,
            &right.terminal_feature_ref,
            operation,
        )?
        && ctx.all_by(
            left.components.iter().zip(&right.components),
            |(left, right)| {
                Ok(left.local_id == right.local_id
                    && left.type_signature[4..8] == right.type_signature[4..8])
            },
            operation,
        )?)
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
    for mut lane_selections in ctx.admit_iter(by_lane, OPERATION)?.map(|(_, group)| group) {
        if lane_selections.len() != 2 {
            return Ok(None);
        }
        ctx.sort_unstable_by(
            &mut lane_selections,
            |value| &value.offset,
            Ord::cmp,
            OPERATION,
        )?;
        let pair = (lane_selections[0], lane_selections[1]);
        if let Some((target, tool)) = consensus {
            if !same_surface_selection_semantics(ctx, target, pair.0, OPERATION)?
                || !same_surface_selection_semantics(ctx, tool, pair.1, OPERATION)?
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
    const REFERENCE_OPERATION: &str = "collect SLDPRT cosmetic thread references";
    const TOKEN_OPERATION: &str = "collect SLDPRT cosmetic thread cylinder tokens";
    const NATIVE_OPERATION: &str = "format SLDPRT cosmetic thread cylinder references";
    const GENERATED_OPERATION: &str = "resolve SLDPRT cosmetic thread generated face";

    let unresolved_thread = |feature: &cadmpeg_ir::features::Feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved
                    | cadmpeg_ir::features::FaceSelection::Native(_),
                ..
            })
        ) && feature.native_ref.is_some()
    };
    if !ctx.any_by(
        &features[..],
        |feature| Ok(unresolved_thread(feature)),
        OPERATION,
    )? {
        return Ok(());
    }
    let mut lookup_storage = ctx.reserve_scoped(
        0,
        "SLDPRT project_unbound_cosmetic_thread_faces lookup storage",
    )?;
    let mut native_features = HashMap::new();
    let mut history_features = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for native_feature in ctx.admit_iter(&history.features, OPERATION)? {
            lookup_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut native_features,
                    native_feature.id.as_str(),
                    native_feature,
                    OPERATION,
                )
            })?;
            lookup_storage
                .with_storage(|| ctx.push_vec(&mut history_features, native_feature, OPERATION))?;
        }
    }
    let (first_history_feature, _first_history_storage) = first_positions(
        ctx,
        &history_features,
        |feature| feature.id.as_str(),
        OPERATION,
    )?;
    let (feature_ids_by_native, _feature_ids_storage) =
        model_feature_ids_by_native(ctx, features, ID_OPERATION)?;
    let face_surfaces = FaceSurfaces::new(ctx, faces, surfaces)?;
    // Each lane's key, feature byte ranges and cylinder reference tokens.
    let mut lane_contexts = Vec::new();
    for lane in ctx.admit_iter(lanes, TOKEN_OPERATION)? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "split SLDPRT feature-input lane key")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        let ranges =
            lookup_storage.with_storage(|| feature_object_byte_ranges(ctx, histories, lane))?;
        let mut cylinder_tokens = HashSet::new();
        for class in ctx.admit_iter(&lane.classes, TOKEN_OPERATION)? {
            if class.name != "moCylinderRef_w" {
                continue;
            }
            let token = usize::try_from(class.offset)
                .ok()
                .and_then(|offset| offset.checked_add("moCylinderRef_w".len() + 6))
                .and_then(|body| View::u16_le_at(&lane.native_payload, body))
                .filter(|token| is_class_token(*token));
            let Some(token) = token else {
                continue;
            };
            lookup_storage.with_storage(|| {
                ctx.insert_hash_set(&mut cylinder_tokens, token, TOKEN_OPERATION)
            })?;
        }
        lookup_storage.with_storage(|| {
            ctx.push_vec(
                &mut lane_contexts,
                (lane, lane_key, ranges, cylinder_tokens),
                TOKEN_OPERATION,
            )
        })?;
    }
    // Each feature's surface selections with the key of the lane that holds them.
    let mut selections = HashMap::<&str, Vec<(&str, &FeatureInputSurfaceSelection)>>::new();
    for (lane, lane_key, _, _) in ctx.admit_iter(&lane_contexts, REFERENCE_OPERATION)? {
        for selection in ctx.admit_iter(&lane.surface_selections, REFERENCE_OPERATION)? {
            lookup_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut selections,
                    selection.feature_ref.as_str(),
                    (*lane_key, selection),
                    REFERENCE_OPERATION,
                    REFERENCE_OPERATION,
                )
            })?;
        }
    }
    for feature in ctx.admit_iter(features, OPERATION)? {
        if !unresolved_thread(feature) {
            continue;
        }
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut edit_result: Result<(), cadmpeg_core::CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                let Some(native_ref) = native_ref else {
                    return Ok(());
                };
                let Some(native_feature) = ctx
                    .get_hash_map(&native_features, native_ref, OPERATION)?
                    .copied()
                else {
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
                let mut references_storage = ctx.reserve_scoped(0, REFERENCE_OPERATION)?;
                let mut references = Vec::<(
                    String,
                    Option<std::borrow::Cow<'_, [crate::records::FeatureInputComponentPathEntry]>>,
                    Option<&str>,
                )>::new();
                for (lane_key, selection) in ctx.admit_iter(
                    ctx.get_hash_map(&selections, native_feature.id.as_str(), REFERENCE_OPERATION)?
                        .map_or(&[][..], Vec::as_slice),
                    REFERENCE_OPERATION,
                )? {
                    references_storage.with_storage(|| {
                        let key = ctx.format_retained(
                            format_args!("{lane_key}:{}", selection.offset),
                            REFERENCE_OPERATION,
                        )?;
                        ctx.push_vec(
                            &mut references,
                            (
                                key,
                                Some(std::borrow::Cow::Borrowed(selection.components.as_slice())),
                                selection.producer_feature_refs.first().map(String::as_str),
                            ),
                            REFERENCE_OPERATION,
                        )
                    })?;
                }
                for (lane, lane_key, ranges, cylinder_tokens) in
                    ctx.admit_iter(&lane_contexts, TOKEN_OPERATION)?
                {
                    let Some((_, start, end)) = ctx
                        .get_hash_map(ranges, native_feature.id.as_str(), TOKEN_OPERATION)?
                        .copied()
                    else {
                        continue;
                    };
                    for super::selections::CylinderMarkerReference(marker, components) in ctx
                        .admit_iter(
                            cosmetic_thread_cylinder_marker_reference(
                                ctx,
                                native_feature,
                                lane,
                                start,
                                end,
                                cylinder_tokens,
                            )?,
                            REFERENCE_OPERATION,
                        )?
                    {
                        references_storage.with_storage(|| {
                            let key = ctx.format_retained(
                                format_args!("{lane_key}:{marker}"),
                                REFERENCE_OPERATION,
                            )?;
                            ctx.push_vec(
                                &mut references,
                                (key, components.map(std::borrow::Cow::Owned), None),
                                REFERENCE_OPERATION,
                            )
                        })?;
                    }
                }
                ctx.sort_unstable_by(
                    &mut references,
                    |value| &value.0,
                    Ord::cmp,
                    NATIVE_OPERATION,
                )?;
                let native = if references.is_empty() {
                    None
                } else {
                    const PREFIX: &str = "sldprt:feature-input:cylinder-reference:";
                    let mut native = String::new();
                    ctx.append_retained(&mut native, PREFIX, NATIVE_OPERATION)?;
                    let mut last: Option<&str> = None;
                    for (reference, _, _) in ctx.admit_iter(&references, NATIVE_OPERATION)? {
                        if let Some(last) = last {
                            if ctx.equal(last, reference.as_str(), NATIVE_OPERATION)? {
                                continue;
                            }
                            ctx.push_retained_char(
                                &mut native,
                                ',',
                                "format SLDPRT cylinder reference separator",
                            )?;
                        }
                        ctx.append_retained(&mut native, reference, NATIVE_OPERATION)?;
                        last = Some(reference.as_str());
                    }
                    Some(native)
                };
                let mut generated = None;
                let mut complete = true;
                for (_, components, explicit_producer) in
                    ctx.admit_iter(&references, GENERATED_OPERATION)?
                {
                    let candidate = match components.as_deref() {
                        None => None,
                        Some(components) => {
                            let explicit = match explicit_producer {
                                Some(producer_ref) => {
                                    match ctx
                                        .get_hash_map(
                                            &first_history_feature,
                                            *producer_ref,
                                            GENERATED_OPERATION,
                                        )?
                                        .map(|position| history_features[*position])
                                    {
                                        Some(producer) => components
                                            .first()
                                            .filter(|component| component.local_id.is_some())
                                            .map(|component| (component, producer)),
                                        None => None,
                                    }
                                }
                                None => None,
                            };
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
                            match selected {
                                Some((component, producer)) => ctx
                                    .get_hash_map(
                                        &feature_ids_by_native,
                                        producer.id.as_str(),
                                        GENERATED_OPERATION,
                                    )?
                                    .zip(component.local_id),
                                None => None,
                            }
                        }
                    };
                    let Some(candidate) = candidate else {
                        complete = false;
                        break;
                    };
                    match generated {
                        Some(previous) if previous != candidate => {
                            complete = false;
                            break;
                        }
                        Some(_) => {}
                        None => generated = Some(candidate),
                    }
                }
                if let Some((producer, local_id)) = generated.filter(|_| complete) {
                    let Some(native) = native else {
                        return Ok(());
                    };
                    let producer_id = producer.try_clone_for_decode(ctx, GENERATED_OPERATION)?;
                    let local_id_text =
                        ctx.format_retained(format_args!("{local_id}"), GENERATED_OPERATION)?;
                    *face = match cadmpeg_ir::features::GeneratedFaceRef::new(
                        producer_id,
                        local_id_text,
                        ctx,
                    )? {
                        Ok(generated_face) => {
                            let mut faces = Vec::new();
                            ctx.push_vec(&mut faces, generated_face, GENERATED_OPERATION)?;
                            let native_copy =
                                ctx.copy_retained_text(&native, GENERATED_OPERATION)?;
                            cadmpeg_ir::features::FaceSelection::generated(faces, native_copy, ctx)?
                                .unwrap_or(cadmpeg_ir::features::FaceSelection::Native(native))
                        }
                        Err(_) => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                    add_dependency(ctx, producer, feature_id, dependencies, GENERATED_OPERATION)?;
                    return Ok(());
                }
                let Some(diameter) = diameter else {
                    return Ok(());
                };
                let selected = match face_surfaces.unique_cylinder(ctx, diameter.get() * 0.5)? {
                    Some(selected) => Some(selected),
                    None if native.is_some() => face_surfaces.unique_cylinder_face(ctx)?,
                    None => None,
                };
                let Some(selected) = selected else {
                    return Ok(());
                };
                let mut selected_faces = Vec::new();
                ctx.push_vec(
                    &mut selected_faces,
                    selected,
                    "project SLDPRT unbound cosmetic thread face",
                )?;
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

/// Every B-rep face paired with its cylinder or plane surface, indexed once so a face search
/// reads only the faces whose surface can match. A face counts once however many surfaces
/// share its surface identity.
struct FaceSurfaces<'a, 'ctx> {
    faces: &'a [Face],
    /// (radius, face position) of each face on a cylinder, ordered by radius.
    cylinders: Vec<(f64, usize)>,
    /// (face position, plane) of each face on a plane, in face order.
    planes: Vec<(usize, &'a cadmpeg_ir::geometry::analytic::PlaneSurface)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> FaceSurfaces<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        faces: &'a [Face],
        surfaces: &'a [Surface],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        const OPERATION: &str = "index SLDPRT face surfaces";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut by_id = HashMap::new();
        for surface in ctx.admit_iter(surfaces, OPERATION)? {
            storage.with_storage(|| {
                ctx.push_hash_group(&mut by_id, &surface.id, surface, OPERATION, OPERATION)
            })?;
        }
        let mut cylinders = Vec::new();
        let mut planes = Vec::new();
        for (index, face) in ctx.admit_iter(faces, OPERATION)?.enumerate() {
            let Some(group) = ctx.get_hash_map(&by_id, &face.surface, OPERATION)? else {
                continue;
            };
            for surface in ctx.admit_iter(group, OPERATION)? {
                match &surface.geometry {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
                        storage.with_storage(|| {
                            ctx.push_vec(
                                &mut cylinders,
                                (cylinder.radius().get(), index),
                                OPERATION,
                            )
                        })?;
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
                        storage.with_storage(|| {
                            ctx.push_vec(&mut planes, (index, plane), OPERATION)
                        })?;
                    }
                    _ => {}
                }
            }
        }
        ctx.sort_unstable_by_key(
            &mut cylinders,
            |value| *value,
            |left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)),
            OPERATION,
        )?;
        Ok(Self {
            faces,
            cylinders,
            planes,
            _storage: storage,
        })
    }

    /// The face of the one matching entry, or `None` when no face or several faces match.
    fn unique(
        &self,
        ctx: &DecodeContext<'_>,
        entries: impl IntoIterator<Item = Result<Option<usize>, cadmpeg_core::CodecError>>,
        operation: &'static str,
    ) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
        let mut selected = None;
        let mut ambiguous = false;
        let mut entries = entries.into_iter();
        while let Some(entry) = ctx.next_charged(&mut entries, operation)? {
            let Some(index) = entry? else {
                continue;
            };
            match selected {
                Some(previous) if previous != index => {
                    ambiguous = true;
                    break;
                }
                Some(_) => {}
                None => selected = Some(index),
            }
        }
        match selected.filter(|_| !ambiguous) {
            Some(index) => Ok(Some(
                self.faces[index].id.try_clone_for_decode(ctx, operation)?,
            )),
            None => Ok(None),
        }
    }

    /// The one face on a cylinder of `radius`.
    fn unique_cylinder(
        &self,
        ctx: &DecodeContext<'_>,
        radius: f64,
    ) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "find unique SLDPRT cylindrical face";
        if !radius.is_finite() || radius <= 0.0 {
            return Ok(None);
        }
        let tolerance = (radius.abs() * EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9)
            .max(EPS_PROJECTIONS_UNIQUE_CYLINDRICAL_FACE_E9);
        // Every match lies inside this doubled window; the tolerance test decides.
        let lower = ctx.partition_point(
            &self.cylinders,
            |(candidate, _)| Ok(*candidate < radius - 2.0 * tolerance),
            OPERATION,
        )?;
        let upper = ctx.partition_point(
            &self.cylinders,
            |(candidate, _)| Ok(*candidate <= radius + 2.0 * tolerance),
            OPERATION,
        )?;
        self.unique(
            ctx,
            self.cylinders[lower..upper.max(lower)]
                .iter()
                .map(|(candidate, index)| {
                    Ok(((candidate - radius).abs() <= tolerance).then_some(*index))
                }),
            OPERATION,
        )
    }

    /// The one face on any cylinder.
    fn unique_cylinder_face(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<FaceId>, cadmpeg_core::CodecError> {
        self.unique(
            ctx,
            self.cylinders.iter().map(|(_, index)| Ok(Some(*index))),
            "find unique SLDPRT topological cylinder face",
        )
    }

    /// The one face on a plane through `origin` with normal `normal`.
    fn unique_plane(
        &self,
        ctx: &DecodeContext<'_>,
        origin: Point3,
        normal: Vector3,
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
        self.unique(
            ctx,
            self.planes.iter().map(|(index, plane_surface)| {
                let candidate_origin = plane_surface.origin().get();
                let candidate_normal = *plane_surface.frame().axis().as_raw();
                let candidate_length = candidate_normal.norm();
                if !candidate_length.is_finite() || candidate_length <= f64::EPSILON {
                    return Ok(None);
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
                Ok(
                    ((alignment.abs() - 1.0).abs() <= EPS_PROJECTIONS_UNIQUE_PLANAR_FACE_E9
                        && distance.abs() <= tolerance)
                        .then_some(*index),
                )
            }),
            OPERATION,
        )
    }
}

/// Resolve frame-only offset-plane supports when exactly one B-rep face lies
/// on the serialized support plane.
pub(crate) fn project_unbound_offset_plane_faces(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "project SLDPRT unbound offset plane face";
    let resolved_plane = |feature: &cadmpeg_ir::features::Feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. }),
                ..
            })
        )
    };
    if !ctx.any_by(
        &features[..],
        |feature| Ok(resolved_plane(feature)),
        OPERATION,
    )? {
        return Ok(());
    }
    let face_surfaces = FaceSurfaces::new(ctx, faces, surfaces)?;
    for feature in ctx.admit_iter(features, OPERATION)? {
        if !resolved_plane(feature) {
            continue;
        }
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
                let Some(selected) = face_surfaces.unique_plane(ctx, origin.get(), normal)? else {
                    return Ok(());
                };
                let mut selected_faces = Vec::new();
                ctx.push_vec(&mut selected_faces, selected, OPERATION)?;
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

#[cfg(test)]
mod projections_tests;
