//! Sketch profile projection from marker and compact records.

use super::assembly::is_supplemental_config_lane;
use super::bindings::history_metadata_ids;
use super::compact_reference_planes::CompactReferencePlaneIndex;
use super::curves::{
    closed_marker_profiles, closed_marker_profiles_allowing_shared_endpoints,
    compact_bounded_curve_tangent, compact_legacy_rectangle_line_endpoints,
    compact_line_chain_addresses, compact_line_region_addresses,
    complete_ordered_compact_line_profile, current_compact_rectangle_line_endpoints,
    current_wide_rectangle_line_endpoints, indexed_rectangle_from_line_cycle,
    legacy_extended_rectangle_diagonal_endpoint, legacy_extended_rectangle_line_endpoints,
    ordered_rectangle_corners, resolve_connected_marker_arcs, resolve_slot_marker_arcs,
    resolve_two_center_semicircle_profile, tangent_bounded_curve,
    unique_dimensioned_rectangle_markers, LaneFrameIndex,
};
use super::endpoints::arc_centers::unique_arc_center_marker;
use super::endpoints::geometry_index::{MarkerGeometryIndex, MarkerPrefixIndex};
use super::endpoints::{
    auxiliary_profile_record, compact_legacy_code_one_line_endpoint_indices,
    compact_legacy_curve_endpoint_indices, compact_legacy_profile_full_circle,
    compact_legacy_terminal_diameter_circle, compact_profile_full_circle, coordinate_circle_radius,
    coordinate_ellipse_axes, coordinate_roster_arc_center, coordinate_roster_full_circle,
    current_compact_roster_selected_axis, current_indexed_arc_reverses_center_sweep,
    current_profile_circle_dimension, equal_index_coordinate_roster_full_circle,
    extended_declared_inline_line_endpoints, extended_geometry_full_circle,
    extended_identity_inline_line_endpoints, extended_linked_inline_line_endpoints,
    extended_wide_construction_line_roster_indices, implicit_coordinate_roster_curve_endpoints,
    implicit_profile_chain_closure_endpoints, indexed_arc_uses_coordinate_center,
    inferred_point_coordinates_by_index, legacy_compact_diameter_arc_center,
    legacy_coordinate_circle_radius, legacy_direct_compact_selected_axis_endpoint_indices,
    legacy_marker104_arc_center, legacy_profile_radial_circle, legacy_undetailed_profile_line,
    legacy_unlocated_geometry_handle, marker_is_selected_construction_line,
    marker_profile_curve_role, minor_arc_geometry, output_curve_endpoint_markers,
    packed_compact_legacy_curve_endpoint_indices, relation_reference_curve_record,
    terminal_relation_class_offset, wide_coordinate_roster_full_circle,
};
use super::grid::quantize;
use super::holes::{feature_input_sketch_frame, sketch_feature_frames};
use super::markers::{
    compact_legacy_142_profile_curve_endpoints, inline_arc_coordinates,
    legacy_140_profile_point_variant_coordinates, marker_is_geometry_locus,
};
use super::projections::bind_circular_profile_by_dimension;
use super::reference_geometry::reference_plane_frame_key;
use super::relation_geometry::{declared_entity_handle_circular_marker, owned_relation_parameters};
use super::relation_loci::same_dimension_length;
use super::scalars::ObjectNames;
use super::transforms::sketch_frame_marker_transform;
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputLane, FeatureInputRelationFamily, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{
    features::{FeatureDefinition, FeatureOperation},
    scalar::{Angle, Length},
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_core::decode::index_from_u64;
use cadmpeg_core::decode::u64_from_index;

/// The sketch arenas and their annotations, updated together by one binding.
pub(crate) struct SketchArenas<'a> {
    pub(crate) sketches: &'a mut Vec<Sketch>,
    pub(crate) sketch_entities: &'a mut Vec<SketchEntity>,
    pub(crate) sketch_constraints: &'a mut Vec<SketchConstraint>,
    pub(crate) annotations: &'a mut Annotations,
}

/// Formats an identity and admits it under the identity grammar, charging the grammar's scan
/// of the formatted text. Text the grammar rejects yields `None`.
pub(super) fn mint_formatted<T: TryFrom<String>>(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<Option<T>, CodecError> {
    let text = ctx.format_retained(args, operation)?;
    ctx.charge_work(u64_from_index(text.len()), operation)?;
    Ok(T::try_from(text).ok())
}

/// Native feature records of every history, keyed by record identity.
fn index_native_features<'history, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    histories: &'history [crate::records::FeatureHistory],
    operation: &'static str,
) -> Result<
    (
        BTreeMap<&'history str, &'history crate::records::Feature>,
        ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut native_features = BTreeMap::new();
    for history in ctx.admit_iter(histories, operation)? {
        for feature in ctx.admit_iter(&history.features, operation)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    operation,
                )
            })?;
        }
    }
    Ok((native_features, storage))
}

/// Model feature positions grouped by the native record their `native_ref` names, in model
/// order. Keys borrow the native record identity, so the model features stay editable.
fn model_features_by_native<'native, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    native_features: &BTreeMap<&'native str, &crate::records::Feature>,
    operation: &'static str,
) -> Result<(HashMap<&'native str, Vec<usize>>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut groups = HashMap::new();
    for (index, feature) in ctx.admit_iter(features, operation)?.enumerate() {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some((native, _)) =
            ctx.get_key_value_btree_map(native_features, native_ref, operation)?
        else {
            continue;
        };
        storage.with_storage(|| {
            ctx.push_hash_group(&mut groups, *native, index, operation, operation)
        })?;
    }
    Ok((groups, storage))
}

/// The first model feature, in model order, that names `native` and that `accept` admits.
fn first_model_feature(
    ctx: &DecodeContext<'_>,
    groups: &HashMap<&str, Vec<usize>>,
    native: &str,
    mut accept: impl FnMut(usize) -> bool,
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    let Some(group) = ctx.get_hash_map(groups, native, operation)? else {
        return Ok(None);
    };
    ctx.find_by(group.iter().copied(), |index| Ok(accept(*index)), operation)
}

/// Every sketch identity, copied once so the sketch arena stays growable while new
/// identities are tested against it.
pub(super) fn index_sketch_ids<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    sketches: &[Sketch],
    operation: &'static str,
) -> Result<(HashMap<String, usize>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut positions = HashMap::new();
    for (index, sketch) in ctx.admit_iter(sketches, operation)?.enumerate() {
        if ctx.contains_key_hash_map(&positions, sketch.id.as_str(), operation)? {
            continue;
        }
        storage.with_storage(|| {
            let id = ctx.copy_retained_text(sketch.id.as_str(), operation)?;
            ctx.insert_hash_map(&mut positions, id, index, operation)
        })?;
    }
    Ok((positions, storage))
}

/// Appends `sketch` and records its identity in `positions`.
fn push_indexed_sketch(
    ctx: &DecodeContext<'_>,
    sketches: &mut Vec<Sketch>,
    positions: &mut HashMap<String, usize>,
    storage: &mut ScopedReservation<'_>,
    sketch: Sketch,
    operation: &'static str,
) -> Result<(), CodecError> {
    let index = sketches.len();
    if !ctx.contains_key_hash_map(positions, sketch.id.as_str(), operation)? {
        storage.with_storage(|| {
            let id = ctx.copy_retained_text(sketch.id.as_str(), operation)?;
            ctx.insert_hash_map(positions, id, index, operation)
        })?;
    }
    ctx.push_vec(sketches, sketch, operation)
}

/// Values grouped by the feature identity that owns them, in source order.
pub(super) fn group_by_owner<'a, 'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    values: &'a [T],
    owner: impl Fn(&'a T) -> Option<&'a str>,
    operation: &'static str,
) -> Result<(HashMap<&'a str, Vec<&'a T>>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut groups = HashMap::new();
    for value in ctx.admit_iter(values, operation)? {
        let Some(key) = owner(value) else {
            continue;
        };
        storage
            .with_storage(|| ctx.push_hash_group(&mut groups, key, value, operation, operation))?;
    }
    Ok((groups, storage))
}

/// The members `owner` has in a grouping; empty when it owns none.
pub(super) fn owned_members<'groups, K: std::borrow::Borrow<str> + Eq + std::hash::Hash, T>(
    ctx: &DecodeContext<'_>,
    groups: &'groups HashMap<K, Vec<T>>,
    owner: &str,
    operation: &'static str,
) -> Result<&'groups [T], CodecError> {
    Ok(ctx
        .get_hash_map(groups, owner, operation)?
        .map_or(&[][..], Vec::as_slice))
}

/// The position of the first value with each key.
pub(super) fn first_positions<'a, 'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    values: &'a [T],
    key: impl Fn(&'a T) -> &'a str,
    operation: &'static str,
) -> Result<(HashMap<&'a str, usize>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut positions = HashMap::new();
    for (index, value) in ctx.admit_iter(values, operation)?.enumerate() {
        let key = key(value);
        if ctx.contains_key_hash_map(&positions, key, operation)? {
            continue;
        }
        storage.with_storage(|| ctx.insert_hash_map(&mut positions, key, index, operation))?;
    }
    Ok((positions, storage))
}

/// The ascending offsets of the lane classes named `name`.
pub(super) fn class_offsets<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    lane: &FeatureInputLane,
    name: &'static str,
    operation: &'static str,
) -> Result<(Vec<u64>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut offsets = Vec::new();
    for class in ctx.admit_iter(&lane.classes, operation)? {
        if class.name == name {
            storage.with_storage(|| ctx.push_vec(&mut offsets, class.offset, operation))?;
        }
    }
    ctx.sort_unstable_by_key(&mut offsets, |offset| *offset, Ord::cmp, operation)?;
    Ok((offsets, storage))
}

/// The smallest offset among a feature's markers.
fn first_owned_marker_offset(
    ctx: &DecodeContext<'_>,
    markers: &[&SketchInputEntity],
    operation: &'static str,
) -> Result<Option<u64>, CodecError> {
    ctx.fold(
        markers,
        None,
        |start: Option<u64>, marker| {
            Ok(Some(start.map_or(marker.offset(), |value| {
                value.min(marker.offset())
            })))
        },
        operation,
    )
}

/// The positions of each sketch's entities, keyed by a copy of the sketch identity so the
/// entity arena stays growable.
fn index_entity_groups<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    entities: &[SketchEntity],
    operation: &'static str,
) -> Result<(HashMap<String, Vec<usize>>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut groups = HashMap::new();
    for (position, entity) in ctx.admit_iter(entities, operation)?.enumerate() {
        push_entity_group(ctx, &mut groups, &mut storage, entity, position, operation)?;
    }
    Ok((groups, storage))
}

/// Records the entity at `position` in its sketch's group.
fn push_entity_group(
    ctx: &DecodeContext<'_>,
    groups: &mut HashMap<String, Vec<usize>>,
    storage: &mut ScopedReservation<'_>,
    entity: &SketchEntity,
    position: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    storage.with_storage(|| {
        if let Some(group) = ctx.get_mut_hash_map(groups, entity.sketch.as_str(), operation)? {
            return ctx.push_vec(group, position, operation);
        }
        let key = ctx.copy_retained_text(entity.sketch.as_str(), operation)?;
        ctx.push_hash_group(groups, key, position, operation, operation)
    })
}

/// Reconcile profile streams with uniquely enclosing sketch feature records.
pub(crate) fn bind_sketch_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    arenas: SketchArenas<'_>,
    parameters: &[cadmpeg_ir::features::DesignParameter],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind SLDPRT sketch profiles";

    let SketchArenas {
        sketches,
        sketch_entities,
        sketch_constraints,
        annotations,
    } = arenas;

    let mut declared_carriers_storage =
        ctx.reserve_scoped(0, "collect SLDPRT declared circular carriers")?;
    let declared_carriers = declared_carriers_storage.with_storage(|| {
        declared_entity_handle_circular_carriers(ctx, features, parameters, lanes)
    })?;
    let mut superseded_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut superseded = BTreeSet::new();
    let mut metadata_ids_storage = ctx.reserve_scoped(0, "index SLDPRT profile metadata")?;
    let metadata_ids =
        metadata_ids_storage.with_storage(|| history_metadata_ids(ctx, histories))?;
    let (native_features, _native_features_storage) =
        index_native_features(ctx, histories, "index SLDPRT profile source features")?;
    let (features_by_native, _features_by_native_storage) =
        model_features_by_native(ctx, features, &native_features, OPERATION)?;
    // Each lane's sketches with the source offset their provenance records.
    let mut lane_keys_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut lane_keys = HashSet::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        lane_keys_storage
            .with_storage(|| ctx.insert_hash_set(&mut lane_keys, lane.id.as_str(), OPERATION))?;
    }
    let mut lane_sketches_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut lane_sketches = HashMap::<&str, Vec<(u64, usize)>>::new();
    for (sketch_index, sketch) in ctx.admit_iter(&sketches[..], OPERATION)?.enumerate() {
        let Some(native_ref) = sketch.native_ref.as_deref() else {
            continue;
        };
        let Some(lane_key) = ctx.get_hash_set(&lane_keys, native_ref, OPERATION)? else {
            continue;
        };
        let Some(source) =
            ctx.get_btree_map(&annotations.provenance, sketch.id.as_str(), OPERATION)?
        else {
            continue;
        };
        lane_sketches_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut lane_sketches,
                *lane_key,
                (source.offset, sketch_index),
                OPERATION,
                OPERATION,
            )
        })?;
    }
    // The entities of each sketch, needed only to test declared circular carriers.
    let mut sketch_entity_groups_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut sketch_entity_groups = HashMap::<&str, Vec<&SketchEntity>>::new();
    if !declared_carriers.is_empty() {
        for entity in ctx.admit_iter(&sketch_entities[..], OPERATION)? {
            sketch_entity_groups_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut sketch_entity_groups,
                    entity.sketch.as_str(),
                    entity,
                    OPERATION,
                    OPERATION,
                )
            })?;
        }
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT profiles records")? {
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut starts_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut starts = Vec::<(u64, usize, &crate::records::Feature)>::new();
        for feature in ctx
            .admit_iter(&native_features, "scan SLDPRT profiles records")?
            .map(|(_, value)| value)
        {
            if ctx.contains_hash_set(
                &metadata_ids,
                feature.id.as_str(),
                "resolve SLDPRT profiles keys",
            )? {
                continue;
            }
            let Some(name) = object_names.of(ctx, feature)? else {
                continue;
            };
            let ordinal = starts.len();
            starts_storage.with_storage(|| {
                ctx.push_vec(&mut starts, (name.offset, ordinal, *feature), OPERATION)
            })?;
        }
        ctx.sort_unstable_by_key(&mut starts, |value| (value.0, value.1), Ord::cmp, OPERATION)?;
        let enclosed =
            match ctx.get_mut_hash_map(&mut lane_sketches, lane.id.as_str(), OPERATION)? {
                Some(candidates) => {
                    ctx.sort_unstable_by_key(candidates, |value| *value, Ord::cmp, OPERATION)?;
                    &candidates[..]
                }
                None => &[],
            };
        for (index, &(start, _, native_feature)) in ctx
            .admit_iter(&starts[..], "scan SLDPRT profiles records")?
            .enumerate()
        {
            let Some(feature_index) = first_model_feature(
                ctx,
                &features_by_native,
                native_feature.id.as_str(),
                |_| true,
                OPERATION,
            )?
            else {
                continue;
            };
            let feature = &mut features[feature_index];
            let end = starts.get(index + 1).map(|next| next.0);
            let lower =
                ctx.partition_point(enclosed, |(offset, _)| Ok(*offset <= start), OPERATION)?;
            let upper = match end {
                Some(end) => {
                    ctx.partition_point(enclosed, |(offset, _)| Ok(*offset < end), OPERATION)?
                }
                None => enclosed.len(),
            };
            let Some([(_, sketch_index)]) = enclosed.get(lower..upper) else {
                continue;
            };
            let sketch = &mut sketches[*sketch_index];
            if let Some(carriers) =
                ctx.get_hash_map(&declared_carriers, native_feature.id.as_str(), OPERATION)?
            {
                let entities = ctx
                    .get_hash_map(&sketch_entity_groups, sketch.id.as_str(), OPERATION)?
                    .map_or(&[][..], Vec::as_slice);
                if !nested_profile_contains_declared_circular_carriers(
                    ctx,
                    sketch,
                    entities.iter().copied(),
                    carriers,
                )? {
                    superseded_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut superseded,
                            ctx.copy_retained_text(sketch.id.as_str(), OPERATION)?,
                            OPERATION,
                        )
                    })?;
                    continue;
                }
            }
            let replace = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. }) => true,
                FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                    shape.section_is_unresolved()
                }
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    profile:
                        cadmpeg_ir::features::ProfileRef::Planar(
                            cadmpeg_ir::features::PlanarProfileRef::Unresolved(owner),
                        ),
                    ..
                }) => ctx.equal(owner, &native_feature.id, OPERATION)?,
                _ => false,
            };
            if !replace {
                continue;
            }
            let sketch_id = sketch.id.try_clone_for_decode(ctx, OPERATION)?;
            if matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ) {
                sketch.name = Some(ctx.copy_retained_text(&native_feature.name, OPERATION)?);
            }
            feature
                .evaluation
                .edit(|definition, _outputs| match definition {
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: feature_sketch,
                    }) => {
                        *feature_sketch =
                            cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id));
                    }
                    FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                        shape.set_referenced_profile(sketch_id.into());
                    }
                    FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                        *profile = cadmpeg_ir::features::ProfileRef::Planar(
                            cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch_id),
                        );
                    }
                    _ => {}
                });
        }
    }
    drop(sketch_entity_groups);
    drop(sketch_entity_groups_storage);
    if !superseded.is_empty() {
        let mut removed_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut removed = HashSet::new();
        for id in ctx.admit_iter(&superseded, OPERATION)? {
            removed_storage
                .with_storage(|| ctx.insert_hash_set(&mut removed, id.as_str(), OPERATION))?;
        }
        for entity in ctx.admit_iter(&sketch_entities[..], OPERATION)? {
            if ctx.contains_btree_set(&superseded, entity.sketch.as_str(), OPERATION)? {
                removed_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut removed, entity.id().as_str(), OPERATION)
                })?;
            }
        }
        for constraint in ctx.admit_iter(&sketch_constraints[..], OPERATION)? {
            if ctx.contains_btree_set(&superseded, constraint.sketch.as_str(), OPERATION)? {
                removed_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut removed, constraint.id.as_str(), OPERATION)
                })?;
            }
        }
        let mut keep = |id: &str| Ok(!ctx.contains_hash_set(&removed, id, OPERATION)?);
        annotations.retain_provenance(ctx, &mut keep)?;
        let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
        builder.retain_exactness(ctx, keep)?;
        *annotations = builder.build();
        drop(removed);
        drop(removed_storage);
        ctx.retain_vec(
            sketches,
            |sketch| Ok(!ctx.contains_btree_set(&superseded, sketch.id.as_str(), OPERATION)?),
            OPERATION,
        )?;
        ctx.retain_vec(
            sketch_entities,
            |entity| Ok(!ctx.contains_btree_set(&superseded, entity.sketch.as_str(), OPERATION)?),
            OPERATION,
        )?;
        ctx.retain_vec(
            sketch_constraints,
            |constraint| {
                Ok(!ctx.contains_btree_set(&superseded, constraint.sketch.as_str(), OPERATION)?)
            },
            OPERATION,
        )?;
    }
    bind_circular_profile_by_dimension(ctx, features, sketches, sketch_entities, parameters)?;
    Ok(())
}

#[derive(Debug)]
pub(super) struct CircleCarrier(pub [f64; 2], pub f64);

fn declared_entity_handle_circular_carriers(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<String, Vec<CircleCarrier>>, CodecError> {
    const OPERATION: &str = "collect SLDPRT declared circular carriers";

    let mut ownership_storage = ctx.reserve_scoped(0, OPERATION)?;
    let ownership = ownership_storage
        .with_storage(|| owned_relation_parameters(ctx, features, parameters, lanes))?;
    let mut parameters_by_id_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "scan SLDPRT profiles records")? {
        parameters_by_id_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_id, &parameter.id, parameter, OPERATION)
        })?;
    }
    let mut carriers = HashMap::<String, Vec<CircleCarrier>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT profiles records")? {
        for relation in ctx
            .admit_iter(&lane.relation_instances, OPERATION)?
            .filter(|relation| relation.family == FeatureInputRelationFamily::CircleDiameter)
        {
            let [operand] = relation.operands.as_slice() else {
                continue;
            };
            let Some(parameter_id) = ctx
                .get_hash_map(&ownership, &relation.id, "resolve SLDPRT profiles keys")?
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(parameter) = ctx.get_hash_map(
                &parameters_by_id,
                &parameter_id,
                "resolve SLDPRT profiles keys",
            )?
            else {
                continue;
            };
            let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = &parameter.value else {
                continue;
            };
            let radius = match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
                None => continue,
            };
            let Some((center, encoded_radius)) = declared_entity_handle_circular_marker(
                ctx,
                lanes,
                relation.feature_ref.as_str(),
                operand,
                radius,
            )?
            else {
                continue;
            };
            let Some(coordinates) = center.coordinates_m else {
                continue;
            };
            let carrier = CircleCarrier(coordinates.get(), encoded_radius);
            if let Some(votes) = ctx.get_mut_hash_map(
                &mut carriers,
                relation.feature_ref.as_str(),
                "resolve SLDPRT profiles keys",
            )? {
                ctx.push_vec(votes, carrier, OPERATION)?;
                continue;
            }
            let key = ctx.copy_retained_text(&relation.feature_ref, OPERATION)?;
            ctx.push_hash_group(&mut carriers, key, carrier, OPERATION, OPERATION)?;
        }
    }
    drop(ownership);
    drop(ownership_storage);
    Ok(carriers)
}

/// Whether every declared circular carrier has a circle or arc of `sketch` at the same center
/// and radius. The sketch's circles are indexed by grid cell once, so each carrier probes only
/// the circles that share its center.
pub(super) fn nested_profile_contains_declared_circular_carriers<'a>(
    ctx: &DecodeContext<'_>,
    sketch: &Sketch,
    entities: impl IntoIterator<Item = &'a SketchEntity>,
    declared: &[CircleCarrier],
) -> Result<bool, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    const OPERATION: &str = "match declared circular profile carriers";

    let Some(transform) = sketch_frame_marker_transform(sketch, QUANTUM) else {
        return Ok(true);
    };
    let mut circles_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut circles = HashMap::<(i64, i64), Vec<f64>>::new();
    let mut entities = entities.into_iter();
    while let Some(entity) = ctx.next_charged(&mut entities, OPERATION)? {
        if !ctx.equal(entity.sketch.as_str(), sketch.id.as_str(), OPERATION)? {
            continue;
        }
        let (SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. }) = entity.geometry.definition()
        else {
            continue;
        };
        let Some(cells) = quantize(center.get(), QUANTUM).cells() else {
            continue;
        };
        circles_storage.with_storage(|| {
            ctx.push_hash_group(&mut circles, cells, radius.get(), OPERATION, OPERATION)
        })?;
    }
    let mut visited = declared.iter();
    while let Some(CircleCarrier([u, v], radius)) = ctx.next_charged(&mut visited, OPERATION)? {
        let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
        let Some(center) = transform.apply(native) else {
            continue;
        };
        let Some(radii) = ctx.get_hash_map(&circles, &center, OPERATION)? else {
            return Ok(false);
        };
        if !ctx.any_by(
            radii,
            |existing| Ok(same_dimension_length(*existing, *radius)),
            OPERATION,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn project_compact_sketch_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    const OPERATION: &str = "project SLDPRT compact sketch profiles";
    let mut metadata_ids_storage = ctx.reserve_scoped(0, "index SLDPRT profile metadata")?;
    let metadata_ids =
        metadata_ids_storage.with_storage(|| history_metadata_ids(ctx, histories))?;

    let (native_features, _native_features_storage) =
        index_native_features(ctx, histories, "index SLDPRT compact profile features")?;
    let (features_by_native, _features_by_native_storage) =
        model_features_by_native(ctx, features, &native_features, OPERATION)?;
    let (mut sketch_positions, mut sketch_positions_storage) =
        index_sketch_ids(ctx, sketches, OPERATION)?;
    let mut lane_frames_storage = ctx.reserve_scoped(0, OPERATION)?;
    let lane_frames =
        lane_frames_storage.with_storage(|| LaneFrameIndex::new(ctx, features, histories))?;
    for lane in ctx.admit_iter(lanes, "scan SLDPRT profiles records")? {
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut plane_frames_storage = ctx.reserve_scoped(0, OPERATION)?;
        let plane_frames = plane_frames_storage
            .with_storage(|| lane_frames.lane_frames(ctx, histories, &object_names))?;
        let plane_index = CompactReferencePlaneIndex::new(ctx, &lane.native_payload)?;
        let (markers_by_owner, _markers_by_owner_storage) = group_by_owner(
            ctx,
            &lane.sketch_entities,
            |marker| marker.feature_ref.as_deref(),
            OPERATION,
        )?;
        let (relations_by_owner, _relations_by_owner_storage) = group_by_owner(
            ctx,
            &lane.relation_instances,
            |relation| Some(relation.feature_ref.as_str()),
            OPERATION,
        )?;
        let (scalar_positions, _scalar_positions_storage) =
            first_positions(ctx, &lane.scalars, |scalar| scalar.id.as_str(), OPERATION)?;
        let (line_class_offsets, _line_class_offsets_storage) =
            class_offsets(ctx, lane, "sgLineHandle", OPERATION)?;
        let (arc_class_offsets, _arc_class_offsets_storage) =
            class_offsets(ctx, lane, "sgArcHandle", OPERATION)?;
        let mut objects_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut objects = Vec::new();
        for (_, feature) in ctx.admit_iter(&native_features, OPERATION)? {
            let feature = *feature;
            if ctx.contains_hash_set(&metadata_ids, feature.id.as_str(), OPERATION)? {
                continue;
            }
            let start = match object_names.of(ctx, feature)? {
                Some(name) => Some(name.offset),
                None => first_owned_marker_offset(
                    ctx,
                    owned_members(ctx, &markers_by_owner, feature.id.as_str(), OPERATION)?,
                    OPERATION,
                )?,
            };
            let Some(start) = start else {
                continue;
            };
            let ordinal = objects.len();
            objects_storage.with_storage(|| {
                ctx.push_vec(&mut objects, (start, ordinal, feature), OPERATION)
            })?;
        }
        ctx.sort_unstable_by_key(
            &mut objects,
            |value| {
                let (left_offset, left_ordinal, _) = value;
                (*left_offset, *left_ordinal)
            },
            Ord::cmp,
            OPERATION,
        )?;
        for (object_index, &(start, _, native_feature)) in ctx
            .admit_iter(&objects[..], "scan SLDPRT profiles records")?
            .enumerate()
        {
            let Some(feature_index) = first_model_feature(
                ctx,
                &features_by_native,
                native_feature.id.as_str(),
                |index| {
                    matches!(
                        features[index].evaluation.definition(),
                        FeatureDefinition::Operation(FeatureOperation::Sketch {
                            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                            ..
                        })
                    )
                },
                OPERATION,
            )?
            else {
                continue;
            };
            let end = objects.get(object_index + 1).map_or(
                u64_from_index(lane.native_payload.len()),
                |(offset, _, _)| *offset,
            );
            let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
                continue;
            };
            let Some(interval) = lane.native_payload.get(start..end) else {
                continue;
            };
            let (region_addresses, _region_storage) = ctx
                .with_scoped_storage(OPERATION, || compact_line_region_addresses(ctx, interval))?;
            let (chain_addresses, _chain_storage) =
                ctx.with_scoped_storage(OPERATION, || compact_line_chain_addresses(ctx, interval))?;
            let addresses = region_addresses.as_ref().or(chain_addresses.as_ref());
            let owned_markers = owned_members(
                ctx,
                &markers_by_owner,
                native_feature.id.as_str(),
                OPERATION,
            )?;
            let mut dimensions_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut dimensions = Vec::new();
            for relation in ctx.admit_iter(
                owned_members(
                    ctx,
                    &relations_by_owner,
                    native_feature.id.as_str(),
                    OPERATION,
                )?,
                OPERATION,
            )? {
                if matches!(
                    relation.family,
                    FeatureInputRelationFamily::Angle | FeatureInputRelationFamily::CircleDiameter
                ) {
                    continue;
                }
                let Some(scalar_ref) = relation.parameter_scalar_ref() else {
                    continue;
                };
                let Some(&scalar_index) =
                    ctx.get_hash_map(&scalar_positions, scalar_ref, OPERATION)?
                else {
                    continue;
                };
                let Some(scalar) = lane.scalars.get(scalar_index) else {
                    continue;
                };
                dimensions_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut dimensions,
                        scalar.value.get() * NATIVE_TO_IR,
                        OPERATION,
                    )
                })?;
            }
            let dimensioned_rectangle = if addresses.is_none() {
                unique_dimensioned_rectangle_markers(ctx, owned_markers, &dimensions)?
            } else {
                None
            };
            let mut markers_storage = ctx.reserve_scoped(0, OPERATION)?;
            let markers = if let Some(rectangle) = dimensioned_rectangle {
                markers_storage.with_storage(|| ctx.collect_vec(rectangle, OPERATION))?
            } else if region_addresses.is_some() {
                let interval_offsets =
                    |offsets: &[u64]| -> Result<std::ops::Range<usize>, CodecError> {
                        let lower = ctx.partition_point(
                            offsets,
                            |offset| Ok(*offset < u64_from_index(start)),
                            OPERATION,
                        )?;
                        let upper = ctx.partition_point(
                            offsets,
                            |offset| Ok(*offset < u64_from_index(end)),
                            OPERATION,
                        )?;
                        Ok(lower..upper.max(lower))
                    };
                let Some(&[line_class_offset]) =
                    line_class_offsets.get(interval_offsets(&line_class_offsets)?)
                else {
                    continue;
                };
                if !interval_offsets(&arc_class_offsets)?.is_empty() {
                    continue;
                }
                let Some(first_marker) = ctx
                    .max_by_key(
                        owned_markers,
                        |marker| Ok((marker.offset() <= line_class_offset, marker.offset())),
                        |left, right| ctx.compare(left, right, OPERATION),
                        OPERATION,
                    )?
                    .filter(|marker| marker.offset() <= line_class_offset)
                    .copied()
                else {
                    continue;
                };
                let begin = ctx
                    .position_by(
                        owned_markers,
                        |marker| Ok(marker.offset() >= first_marker.offset()),
                        OPERATION,
                    )?
                    .unwrap_or(owned_markers.len());
                let run = &owned_markers[begin..];
                let run_end = ctx
                    .position_by(run, |marker| Ok(marker.coordinates_m.is_none()), OPERATION)?
                    .unwrap_or(run.len());
                markers_storage
                    .with_storage(|| ctx.collect_vec(run[..run_end].iter().copied(), OPERATION))?
            } else {
                let Some(addresses) = addresses else {
                    continue;
                };
                let mut matching_run = None;
                let mut matching_runs = 0usize;
                let mut run_start = 0;
                for (index, marker) in ctx
                    .admit_iter(owned_markers, OPERATION)?
                    .map(Some)
                    .chain(std::iter::once(None))
                    .enumerate()
                {
                    let separates = marker.is_none_or(|marker| {
                        marker.coordinates_m.is_none()
                            || !matches!(
                                marker.kind(),
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    });
                    if !separates {
                        continue;
                    }
                    let run = &owned_markers[run_start..index];
                    if run.len() == addresses.len() {
                        matching_run = Some(run);
                        matching_runs += 1;
                    }
                    run_start = index + 1;
                }
                let (Some(run), 1) = (matching_run, matching_runs) else {
                    continue;
                };
                markers_storage.with_storage(|| ctx.collect_vec(run.iter().copied(), OPERATION))?
            };
            if addresses.is_some_and(|addresses| markers.len() != addresses.len())
                || markers.len() < 3
            {
                continue;
            }
            let context_start = object_index
                .checked_sub(1)
                .and_then(|index| objects.get(index))
                .and_then(|(offset, _, _)| usize::try_from(*offset).ok())
                .unwrap_or(0);
            let Some((origin, normal, u_axis)) = feature_input_sketch_frame(
                ctx,
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            )?
            else {
                continue;
            };
            let lane_key = ctx
                .rsplit_once(&lane.id, "#", "resolve SLDPRT profiles keys")?
                .map_or(lane.id.as_str(), |(_, key)| key);
            let (sketch_id, sketch_id_storage) = ctx.with_scoped_storage(OPERATION, || {
                let sketch_text = ctx.format_retained(
                    format_args!(
                        "sldprt:model:sketch#compact:{lane_key}:{}",
                        native_feature.ordinal
                    ),
                    OPERATION,
                )?;
                ctx.charge_work(u64_from_index(sketch_text.len()), OPERATION)?;
                Ok::<_, CodecError>(SketchId::mint(sketch_text))
            })?;
            let Ok(sketch_id) = sketch_id else {
                continue;
            };
            if ctx.contains_key_hash_map(
                &sketch_positions,
                sketch_id.as_str(),
                "find SLDPRT profile sketch",
            )? {
                sketch_id_storage.commit()?;
                features[feature_index].evaluation.set_definition(
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch {
                            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                sketch_id,
                            )),
                        },
                    ),
                );
                continue;
            }
            let Ok(placement) =
                cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
            else {
                continue;
            };
            let (sketch, sketch_fields_storage) = ctx.with_scoped_storage(OPERATION, || {
                let sketch = Sketch {
                    id: sketch_id.try_clone_for_decode(ctx, OPERATION)?,
                    name: Some(ctx.copy_retained_text(&native_feature.name, OPERATION)?),
                    configuration: lane
                        .configuration
                        .as_deref()
                        .map(|value| ctx.copy_retained_text(value, OPERATION))
                        .transpose()?,
                    visible: None,
                    placement,
                    profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                    native_ref: Some(ctx.copy_retained_text(&lane.id, OPERATION)?),
                };
                Ok::<_, CodecError>(sketch)
            })?;
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let entity_start = sketch_entities.len();
            if dimensioned_rectangle.is_some() {
                let mut points_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut points = Vec::new();
                for marker in ctx.admit_iter(&markers, OPERATION)? {
                    let Some([u, v]) = marker
                        .coordinates_m
                        .map(cadmpeg_ir::units::FiniteVector::get)
                    else {
                        continue;
                    };
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let Some(point) = transform.apply(native).and_then(|point| {
                        Some(Point2::new(
                            f64_from_i64(point.0)? * QUANTUM,
                            f64_from_i64(point.1)? * QUANTUM,
                        ))
                    }) else {
                        continue;
                    };
                    points_storage.with_storage(|| ctx.push_vec(&mut points, point, OPERATION))?;
                }
                let Some(corners) = ordered_rectangle_corners(ctx, &points)? else {
                    continue;
                };
                let mut corner_markers_storage = ctx.reserve_scoped(0, OPERATION)?;
                let corner_markers = corner_markers_storage.with_storage(|| {
                    ctx.collect_fallible_options(
                        corners.iter().map(|corner| {
                            let position = ctx.position_by(
                                &points[..],
                                |point| ctx.equal(point, corner, OPERATION),
                                OPERATION,
                            )?;
                            Ok::<_, CodecError>(
                                position.and_then(|index| markers.get(index).copied()),
                            )
                        }),
                        OPERATION,
                    )
                })?;
                let Some(corner_markers) = corner_markers else {
                    continue;
                };
                let mut profile = Vec::new();
                for (index, start) in corners.iter().enumerate() {
                    let end = corners[(index + 1) % corners.len()];
                    let start_marker = corner_markers[index];
                    let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                    let entity_text = ctx.format_retained(
                        format_args!(
                            "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                            native_feature.ordinal
                        ),
                        OPERATION,
                    )?;
                    ctx.charge_work(u64_from_index(entity_text.len()), OPERATION)?;
                    let Ok(entity_id) = SketchEntityId::mint(entity_text) else {
                        continue;
                    };
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: *start,
                        end,
                    }) else {
                        continue;
                    };
                    ctx.push_vec(
                        &mut profile,
                        SketchEntityUse {
                            entity: entity_id.try_clone_for_decode(ctx, OPERATION)?,
                            reversed: false,
                        },
                        OPERATION,
                    )?;
                    let entity = profile_entity(
                        ctx,
                        entity_id,
                        &sketch_id,
                        geometry,
                        Some(start_marker.id()),
                        Some([start_marker.id(), end_marker.id()]),
                        OPERATION,
                    )?;
                    ctx.reserve_vec(sketch_entities, 1, OPERATION)?;
                    sketch_entities.push(entity);
                }
                let mut sketch = sketch;
                if profile.is_empty() {
                    let error = "sketch profile chain must be nonempty";
                    ctx.truncate_vec(sketch_entities, entity_start, OPERATION)?;
                    ctx.push_vec(
                        losses,
                        crate::loss::SldprtLossCode::SketchProfileRejected.note(
                            ctx.format_retained(
                                format_args!(
                                    "Sketch {sketch_id} profile was not transferred: {error}"
                                ),
                                OPERATION,
                            )?,
                        ),
                        OPERATION,
                    )?;
                    continue;
                }
                let mut profiles = Vec::new();
                ctx.reserve_vec(&mut profiles, 1, OPERATION)?;
                profiles.push(profile);
                sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
                push_indexed_sketch(
                    ctx,
                    sketches,
                    &mut sketch_positions,
                    &mut sketch_positions_storage,
                    sketch,
                    OPERATION,
                )?;
                sketch_fields_storage.commit()?;
                sketch_id_storage.commit()?;
                features[feature_index].evaluation.set_definition(
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch {
                            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                sketch_id,
                            )),
                        },
                    ),
                );
                continue;
            }
            if let (Some(curves), Some(vertices)) =
                (region_addresses.as_deref(), chain_addresses.as_deref())
            {
                let project = |marker: &SketchInputEntity| {
                    let [u, v] = marker.coordinates_m?.get();
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some(Point2::new(
                        f64_from_i64(point.0)? * QUANTUM,
                        f64_from_i64(point.1)? * QUANTUM,
                    ))
                };
                let mut lines_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut lines = Vec::new();
                let mut line_fields_storage = ctx.reserve_scoped(0, OPERATION)?;
                let paired_count = curves.len().min(vertices.len());
                let admitted_curves = ctx.admit_iter(&curves[..paired_count], OPERATION)?;
                let admitted_vertices = ctx.admit_iter(&vertices[..paired_count], OPERATION)?;
                for (index, (curve, vertex)) in admitted_curves.zip(admitted_vertices).enumerate() {
                    let Some((curve, vertex, start, end)) = (|| {
                        let curve = markers.get(usize::from(*curve).checked_sub(1)?)?;
                        let vertex = markers.get(usize::from(*vertex).checked_sub(1)?)?;
                        let start = project(curve)?;
                        let end = project(vertex)?;
                        (start != end).then_some((*curve, *vertex, start, end))
                    })() else {
                        continue;
                    };
                    let (entity_id, id_storage) = ctx.with_scoped_storage(OPERATION, || {
                        let entity_text = ctx.format_retained(
                            format_args!(
                                "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                                native_feature.ordinal,
                            ),
                            OPERATION,
                        )?;
                        ctx.charge_work(u64_from_index(entity_text.len()), OPERATION)?;
                        Ok::<_, CodecError>(SketchEntityId::mint(entity_text))
                    })?;
                    let Ok(entity_id) = entity_id else {
                        continue;
                    };
                    line_fields_storage.with_storage(|| id_storage.commit())?;
                    lines_storage.with_storage(|| {
                        ctx.reserve_vec(&mut lines, 1, OPERATION)?;
                        lines.push((entity_id, curve, vertex, start, end));
                        Ok::<(), CodecError>(())
                    })?;
                }
                let (ordered_profile, ordered_profile_storage) = ctx
                    .with_scoped_storage(OPERATION, || {
                        complete_ordered_compact_line_profile(ctx, &lines, markers.len())
                    })?;
                let profile = if let Some(profile) = ordered_profile {
                    let mut projected_storage = ctx.reserve_scoped(0, OPERATION)?;
                    let ((projected, complete), projected_fields_storage) = ctx
                        .with_scoped_storage(OPERATION, || {
                            let mut projected = Vec::new();
                            let mut complete = true;
                            let mut visited = lines.into_iter();
                            while let Some((entity_id, marker, vertex, start, end)) =
                                ctx.next_charged(&mut visited, OPERATION)?
                            {
                                let Ok(geometry) =
                                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                        start,
                                        end,
                                    })
                                else {
                                    complete = false;
                                    break;
                                };
                                let entity = profile_entity(
                                    ctx,
                                    entity_id,
                                    &sketch_id,
                                    geometry,
                                    Some(marker.id()),
                                    Some([marker.id(), vertex.id()]),
                                    OPERATION,
                                )?;
                                projected_storage.with_storage(|| {
                                    ctx.reserve_vec(&mut projected, 1, OPERATION)?;
                                    projected.push(entity);
                                    Ok::<(), CodecError>(())
                                })?;
                            }
                            Ok::<_, CodecError>((projected, complete))
                        })?;
                    if !complete {
                        continue;
                    }
                    ctx.extend_vec(sketch_entities, projected, OPERATION)?;
                    projected_fields_storage.commit()?;
                    line_fields_storage.commit()?;
                    ordered_profile_storage.commit()?;
                    profile
                } else {
                    drop((lines, line_fields_storage, ordered_profile_storage));
                    let mut points_storage = ctx.reserve_scoped(0, OPERATION)?;
                    let mut points = Vec::new();
                    let mut complete = true;
                    let mut visited = markers.iter();
                    while let Some(marker) = ctx.next_charged(&mut visited, OPERATION)? {
                        let Some(point) = project(marker) else {
                            complete = false;
                            break;
                        };
                        points_storage
                            .with_storage(|| ctx.push_vec(&mut points, point, OPERATION))?;
                    }
                    if !complete {
                        continue;
                    }
                    let Some(corners) = ordered_rectangle_corners(ctx, &points)? else {
                        continue;
                    };
                    let mut corner_markers_storage = ctx.reserve_scoped(0, OPERATION)?;
                    let corner_markers = corner_markers_storage.with_storage(|| {
                        ctx.collect_fallible_options(
                            corners.iter().map(|corner| {
                                let position = ctx.position_by(
                                    &points[..],
                                    |point| ctx.equal(point, corner, OPERATION),
                                    OPERATION,
                                )?;
                                Ok::<_, CodecError>(
                                    position.and_then(|index| markers.get(index).copied()),
                                )
                            }),
                            OPERATION,
                        )
                    })?;
                    let Some(corner_markers) = corner_markers else {
                        continue;
                    };
                    let mut profile = Vec::new();
                    for (index, start) in corners.iter().enumerate() {
                        let end = corners[(index + 1) % corners.len()];
                        let start_marker = corner_markers[index];
                        let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                        let entity_text = ctx.format_retained(
                            format_args!(
                                "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                                native_feature.ordinal
                            ),
                            OPERATION,
                        )?;
                        ctx.charge_work(u64_from_index(entity_text.len()), OPERATION)?;
                        let Ok(entity_id) = SketchEntityId::mint(entity_text) else {
                            continue;
                        };
                        let Ok(geometry) =
                            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                start: *start,
                                end,
                            })
                        else {
                            continue;
                        };
                        ctx.push_vec(
                            &mut profile,
                            SketchEntityUse {
                                entity: entity_id.try_clone_for_decode(ctx, OPERATION)?,
                                reversed: false,
                            },
                            OPERATION,
                        )?;
                        let entity = profile_entity(
                            ctx,
                            entity_id,
                            &sketch_id,
                            geometry,
                            Some(start_marker.id()),
                            Some([start_marker.id(), end_marker.id()]),
                            OPERATION,
                        )?;
                        ctx.reserve_vec(sketch_entities, 1, OPERATION)?;
                        sketch_entities.push(entity);
                    }
                    profile
                };
                let mut sketch = sketch;
                if profile.is_empty() {
                    let error = "sketch profile chain must be nonempty";
                    ctx.truncate_vec(sketch_entities, entity_start, OPERATION)?;
                    ctx.push_vec(
                        losses,
                        crate::loss::SldprtLossCode::SketchProfileRejected.note(
                            ctx.format_retained(
                                format_args!(
                                    "Sketch {sketch_id} profile was not transferred: {error}"
                                ),
                                OPERATION,
                            )?,
                        ),
                        OPERATION,
                    )?;
                    continue;
                }
                let mut profiles = Vec::new();
                ctx.reserve_vec(&mut profiles, 1, OPERATION)?;
                profiles.push(profile);
                sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
                push_indexed_sketch(
                    ctx,
                    sketches,
                    &mut sketch_positions,
                    &mut sketch_positions_storage,
                    sketch,
                    OPERATION,
                )?;
                sketch_fields_storage.commit()?;
                sketch_id_storage.commit()?;
                features[feature_index].evaluation.set_definition(
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch {
                            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                sketch_id,
                            )),
                        },
                    ),
                );
                continue;
            }
            let Some(addresses) = addresses else {
                continue;
            };
            let mut points_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut points = Vec::new();
            for address in ctx.admit_iter(addresses, OPERATION)? {
                let Some(marker) = usize::from(*address)
                    .checked_sub(1)
                    .and_then(|index| markers.get(index))
                else {
                    continue;
                };
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let Some(point) = transform.apply(native).and_then(|point| {
                    Some(Point2::new(
                        f64_from_i64(point.0)? * QUANTUM,
                        f64_from_i64(point.1)? * QUANTUM,
                    ))
                }) else {
                    continue;
                };
                points_storage
                    .with_storage(|| ctx.push_vec(&mut points, (*marker, point), OPERATION))?;
            }
            let duplicate_adjacent_points = points.len() == addresses.len()
                && ctx.any_by(
                    0..points.len(),
                    |index| {
                        ctx.equal(
                            &points[index].1,
                            &points[(index + 1) % points.len()].1,
                            "compare SLDPRT profile adjacent points",
                        )
                    },
                    OPERATION,
                )?;
            if points.len() != addresses.len() || duplicate_adjacent_points {
                continue;
            }
            let mut profile = Vec::new();
            for (index, (marker, start)) in ctx
                .admit_iter(&points[..], "scan SLDPRT profiles records")?
                .enumerate()
            {
                let end = points[(index + 1) % points.len()].1;
                let entity_text = ctx.format_retained(
                    format_args!(
                        "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                        native_feature.ordinal
                    ),
                    OPERATION,
                )?;
                ctx.charge_work(u64_from_index(entity_text.len()), OPERATION)?;
                let Ok(entity_id) = SketchEntityId::mint(entity_text) else {
                    continue;
                };
                let Ok(geometry) =
                    SketchGeometry::try_from(SketchGeometryDefinition::Line { start: *start, end })
                else {
                    continue;
                };
                ctx.push_vec(
                    &mut profile,
                    SketchEntityUse {
                        entity: entity_id.try_clone_for_decode(ctx, OPERATION)?,
                        reversed: false,
                    },
                    OPERATION,
                )?;
                let entity = profile_entity(
                    ctx,
                    entity_id,
                    &sketch_id,
                    geometry,
                    Some(marker.id()),
                    None,
                    OPERATION,
                )?;
                ctx.reserve_vec(sketch_entities, 1, OPERATION)?;
                sketch_entities.push(entity);
            }
            let mut sketch = sketch;
            if profile.is_empty() {
                let error = "sketch profile chain must be nonempty";
                ctx.truncate_vec(sketch_entities, entity_start, OPERATION)?;
                ctx.push_vec(
                    losses,
                    crate::loss::SldprtLossCode::SketchProfileRejected.note(ctx.format_retained(
                        format_args!("Sketch {sketch_id} profile was not transferred: {error}"),
                        OPERATION,
                    )?),
                    OPERATION,
                )?;
                continue;
            }
            let mut profiles = Vec::new();
            ctx.reserve_vec(&mut profiles, 1, OPERATION)?;
            profiles.push(profile);
            sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
            push_indexed_sketch(
                ctx,
                sketches,
                &mut sketch_positions,
                &mut sketch_positions_storage,
                sketch,
                OPERATION,
            )?;
            sketch_fields_storage.commit()?;
            sketch_id_storage.commit()?;
            features[feature_index].evaluation.set_definition(
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    },
                ),
            );
        }
    }
    Ok(())
}

fn profile_entity(
    ctx: &DecodeContext<'_>,
    id: SketchEntityId,
    sketch: &SketchId,
    geometry: SketchGeometry,
    native: Option<&str>,
    endpoints: Option<[&str; 2]>,
    operation: &'static str,
) -> Result<SketchEntity, CodecError> {
    let sketch = sketch.try_clone_for_decode(ctx, operation)?;
    let native = native
        .map(|text| ctx.copy_retained_text(text, operation))
        .transpose()?;
    let mut entity = SketchEntity::new(id, sketch, geometry).with_native_ref(native);
    if let Some(endpoints) = endpoints {
        let mut references = Vec::new();
        ctx.reserve_vec(&mut references, endpoints.len(), operation)?;
        for endpoint in endpoints {
            references.push(ctx.copy_retained_text(endpoint, operation)?);
        }
        entity = entity.with_endpoint_refs(references);
    }
    Ok(entity)
}

fn copy_profile_sketch(
    ctx: &DecodeContext<'_>,
    sketch: &Sketch,
    operation: &'static str,
) -> Result<Sketch, CodecError> {
    let mut profiles = Vec::new();
    for profile in ctx.admit_iter(sketch.profiles.as_slice(), "scan SLDPRT profiles records")? {
        let mut copied = Vec::new();
        for member in ctx.admit_iter(profile, operation)? {
            ctx.push_vec(
                &mut copied,
                SketchEntityUse {
                    entity: member.entity.try_clone_for_decode(ctx, operation)?,
                    reversed: member.reversed,
                },
                operation,
            )?;
        }
        ctx.push_vec(&mut profiles, copied, operation)?;
    }
    Ok(Sketch {
        id: sketch.id.try_clone_for_decode(ctx, operation)?,
        name: sketch
            .name
            .as_deref()
            .map(|text| ctx.copy_retained_text(text, operation))
            .transpose()?,
        configuration: sketch
            .configuration
            .as_deref()
            .map(|text| ctx.copy_retained_text(text, operation))
            .transpose()?,
        visible: sketch.visible,
        placement: sketch.placement,
        profiles: profiles.try_into().map_err(CodecError::malformed)?,
        native_ref: sketch
            .native_ref
            .as_deref()
            .map(|text| ctx.copy_retained_text(text, operation))
            .transpose()?,
    })
}

enum MarkerGeometryFailure {
    Absent,
    Codec(CodecError),
}

impl From<CodecError> for MarkerGeometryFailure {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

/// A lane's classes by offset and its relations by owning feature and class, built once per
/// lane so each marker tests for a terminal display carrier with keyed lookups.
struct TerminalCarriers<'lane, 'ctx> {
    classes: HashMap<u64, &'lane str>,
    relations: HashSet<(&'lane str, &'lane str)>,
    _storage: ScopedReservation<'ctx>,
}

impl<'lane, 'ctx> TerminalCarriers<'lane, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'lane FeatureInputLane,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT terminal relation carriers";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut classes = HashMap::new();
        for class in ctx.admit_iter(&lane.classes, OPERATION)? {
            if !ctx.contains_key_hash_map(&classes, &class.offset, OPERATION)? {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut classes, class.offset, class.id.as_str(), OPERATION)
                })?;
            }
        }
        let mut relations = HashSet::new();
        for relation in ctx.admit_iter(&lane.relation_instances, OPERATION)? {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut relations,
                    (relation.feature_ref.as_str(), relation.class_ref.as_str()),
                    OPERATION,
                )
            })?;
        }
        Ok(Self {
            classes,
            relations,
            _storage: storage,
        })
    }

    /// Whether `marker` is the display carrier of a terminal relation of its own feature.
    fn carries(
        &self,
        ctx: &DecodeContext<'_>,
        lane: &FeatureInputLane,
        marker: &SketchInputEntity,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "find SLDPRT terminal display carrier";
        if !matches!(
            marker.kind(),
            SketchInputKind::LineOrCircle | SketchInputKind::Arc
        ) || marker.coordinates_m.is_some()
        {
            return Ok(false);
        }
        let Some(feature_ref) = marker.feature_ref.as_deref() else {
            return Ok(false);
        };
        let Some(offset) = index_from_u64(marker.offset()) else {
            return Ok(false);
        };
        let Some(class_offset) = terminal_relation_class_offset(&lane.native_payload, offset)
        else {
            return Ok(false);
        };
        let Some(class) =
            ctx.get_hash_map(&self.classes, &u64_from_index(class_offset), OPERATION)?
        else {
            return Ok(false);
        };
        ctx.contains_hash_set(&self.relations, &(feature_ref, *class), OPERATION)
    }
}

pub(crate) fn project_marker_backed_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    let mut metadata_ids_storage = ctx.reserve_scoped(0, "index SLDPRT marker profile metadata")?;
    let metadata_ids =
        metadata_ids_storage.with_storage(|| history_metadata_ids(ctx, histories))?;

    let (native_features, _native_features_storage) = index_native_features(
        ctx,
        histories,
        "index SLDPRT marker profile native features",
    )?;
    let (features_by_native, _features_by_native_storage) = model_features_by_native(
        ctx,
        features,
        &native_features,
        "find SLDPRT marker profile feature",
    )?;
    let mut marker_owners_storage = ctx.reserve_scoped(0, "index SLDPRT marker profile owners")?;
    let mut marker_owners = HashSet::new();
    for lane in ctx.admit_iter(lanes, "index SLDPRT marker profile owners")? {
        for marker in ctx.admit_iter(&lane.sketch_entities, "index SLDPRT marker profile owners")? {
            if let Some(owner) = marker.feature_ref.as_deref() {
                marker_owners_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut marker_owners,
                        owner,
                        "index SLDPRT marker profile owners",
                    )
                })?;
            }
        }
    }
    let mut feature_frames_storage = ctx.reserve_scoped(0, "resolve SLDPRT feature frames")?;
    let feature_frames = feature_frames_storage
        .with_storage(|| sketch_feature_frames(ctx, features, histories, lanes))?;
    project_detached_legacy_config_sketches(
        ctx,
        features,
        sketches,
        sketch_entities,
        &native_features,
        lanes,
        &feature_frames,
    )?;
    let (mut sketch_positions, mut sketch_positions_storage) =
        index_sketch_ids(ctx, sketches, "compare SLDPRT profile sketch identities")?;
    let mut lane_frames_storage = ctx.reserve_scoped(0, "resolve SLDPRT feature frames")?;
    let lane_frames =
        lane_frames_storage.with_storage(|| LaneFrameIndex::new(ctx, features, histories))?;
    // Compact sketches that a marker sketch replaces; removed together once every lane is read.
    let mut replaced_storage = ctx.reserve_scoped(0, "remove prior SLDPRT marker sketches")?;
    let mut replaced = HashSet::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT profiles records")? {
        let slots = super::curves::SlotReferences::new(ctx, &lane.native_payload)?;
        let prefixes = MarkerPrefixIndex::new(ctx, &lane.native_payload)?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut plane_frames_storage = ctx.reserve_scoped(0, "resolve SLDPRT feature frames")?;
        let plane_frames = plane_frames_storage
            .with_storage(|| lane_frames.lane_frames(ctx, histories, &object_names))?;
        let plane_index = CompactReferencePlaneIndex::new(ctx, &lane.native_payload)?;
        let mut markers_by_id_storage = ctx.reserve_scoped(0, "index SLDPRT profile markers")?;
        let mut markers_by_id = HashMap::new();
        for marker in ctx.admit_iter(&(lane.sketch_entities)[..], "scan SLDPRT profiles records")? {
            markers_by_id_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut markers_by_id,
                    marker.id(),
                    marker,
                    "index SLDPRT profile markers",
                )
            })?;
        }
        let (markers_by_owner, _markers_by_owner_storage) = group_by_owner(
            ctx,
            &lane.sketch_entities,
            |marker| marker.feature_ref.as_deref(),
            "scan SLDPRT profiles records",
        )?;
        let terminal_carriers = TerminalCarriers::new(ctx, lane)?;
        let mut marker_offsets_storage = ctx.reserve_scoped(0, "scan SLDPRT profiles records")?;
        let mut marker_offsets = marker_offsets_storage.with_storage(|| {
            ctx.collect_vec(
                lane.sketch_entities.iter().map(SketchInputEntity::offset),
                "scan SLDPRT profiles records",
            )
        })?;
        ctx.sort_unstable_by_key(
            &mut marker_offsets,
            |offset| *offset,
            Ord::cmp,
            "scan SLDPRT profiles records",
        )?;
        let mut objects_storage = ctx.reserve_scoped(0, "collect SLDPRT marker profile objects")?;
        let mut objects = Vec::new();
        for (_, native_feature) in
            ctx.admit_iter(&native_features, "scan SLDPRT profiles records")?
        {
            let feature = *native_feature;
            if ctx.contains_hash_set(
                &metadata_ids,
                feature.id.as_str(),
                "resolve SLDPRT profiles keys",
            )? {
                continue;
            }
            let start = match object_names.of(ctx, feature)? {
                Some(name) => Some(name.offset),
                None => first_owned_marker_offset(
                    ctx,
                    owned_members(
                        ctx,
                        &markers_by_owner,
                        feature.id.as_str(),
                        "resolve SLDPRT profiles keys",
                    )?,
                    "resolve SLDPRT profiles keys",
                )?,
            };
            let Some(start) = start else {
                continue;
            };
            objects_storage.with_storage(|| {
                ctx.push_vec(
                    &mut objects,
                    (start, feature),
                    "collect SLDPRT marker profile objects",
                )
            })?;
        }
        ctx.stable_sort_by(
            &mut objects,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT profile objects",
        )?;
        for (object_index, &(start, native_feature)) in ctx
            .admit_iter(&objects[..], "scan SLDPRT profiles records")?
            .enumerate()
        {
            let Some(feature_index) = first_model_feature(
                ctx,
                &features_by_native,
                native_feature.id.as_str(),
                |index| {
                    matches!(
                        features[index].evaluation.definition(),
                        FeatureDefinition::Operation(
                            FeatureOperation::Sketch { .. }
                                | FeatureOperation::SketchBlockDefinition { .. }
                        )
                    )
                },
                "find SLDPRT marker profile feature",
            )?
            else {
                continue;
            };
            let (bound_sketch, block_definition) =
                match features[feature_index].evaluation.definition() {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch { sketch, .. },
                    ) => (sketch.id(), false),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SketchBlockDefinition { sketch },
                    ) => (sketch.as_ref(), true),
                    _ => continue,
                };
            let end = objects
                .get(object_index + 1)
                .map_or(u64_from_index(lane.native_payload.len()), |(offset, _)| {
                    *offset
                });
            let mut object_markers_storage =
                ctx.reserve_scoped(0, "collect SLDPRT profile object markers")?;
            let mut object_markers = Vec::new();
            for marker in ctx.admit_iter(
                owned_members(
                    ctx,
                    &markers_by_owner,
                    native_feature.id.as_str(),
                    "compare SLDPRT profile object feature identity",
                )?,
                "scan SLDPRT profiles records",
            )? {
                let marker = *marker;
                if marker.offset() < end {
                    object_markers_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut object_markers,
                            1,
                            "collect SLDPRT profile object markers",
                        )?;
                        object_markers.push(marker);
                        Ok::<(), CodecError>(())
                    })?;
                }
            }
            let geometry_index =
                MarkerGeometryIndex::new(ctx, &object_markers, std::rc::Rc::clone(&prefixes))?;
            let context_start = object_index
                .checked_sub(1)
                .and_then(|index| objects.get(index))
                .map_or(0, |(offset, _)| *offset);
            let (Ok(context_start), Ok(start), Ok(end)) = (
                usize::try_from(context_start),
                usize::try_from(start),
                usize::try_from(end),
            ) else {
                continue;
            };
            let frame = feature_input_sketch_frame(
                ctx,
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            )?;
            let frame = match frame {
                Some(frame) => Some(frame),
                None => ctx
                    .get_hash_map(
                        &feature_frames,
                        native_feature.id.as_str(),
                        "resolve SLDPRT feature frame",
                    )?
                    .copied()
                    .or_else(|| {
                        block_definition.then_some((
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        ))
                    }),
            };
            let lane_key = ctx
                .rsplit_once(&lane.id, "#", "resolve SLDPRT profiles keys")?
                .map_or(lane.id.as_str(), |(_, key)| key);
            let (sketch_id, sketch_id_storage) =
                ctx.with_scoped_storage("build SLDPRT marker sketch identity", || {
                    let sketch_text = ctx.format_retained(
                        format_args!(
                            "sldprt:model:sketch#markers:{lane_key}:{}",
                            native_feature.ordinal
                        ),
                        "format SLDPRT marker profile sketch identity",
                    )?;
                    ctx.charge_work(
                        u64_from_index(sketch_text.len()),
                        "validate SLDPRT marker profile sketch identity",
                    )?;
                    Ok::<_, CodecError>(SketchId::mint(sketch_text))
                })?;
            let Ok(sketch_id) = sketch_id else {
                continue;
            };
            let mut markers_storage =
                ctx.reserve_scoped(0, "collect SLDPRT profile geometry markers")?;
            let mut markers = Vec::new();
            for marker in ctx
                .admit_iter(&object_markers[..], "scan SLDPRT profiles records")?
                .copied()
            {
                if !matches!(
                    marker.kind(),
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                ) {
                    continue;
                }
                if let Some(offset) = index_from_u64(marker.offset()) {
                    if legacy_unlocated_geometry_handle(&lane.native_payload, offset)
                        || auxiliary_profile_record(&lane.native_payload, offset)
                        || relation_reference_curve_record(
                            ctx,
                            &lane.native_payload,
                            marker,
                            &geometry_index,
                        )?
                        || terminal_carriers.carries(ctx, lane, marker)?
                    {
                        continue;
                    }
                }
                markers_storage.with_storage(|| {
                    ctx.reserve_vec(&mut markers, 1, "collect SLDPRT profile geometry markers")?;
                    markers.push(marker);
                    Ok::<(), CodecError>(())
                })?;
            }
            if markers.is_empty() {
                let first_after_start = ctx.partition_point(
                    &marker_offsets,
                    |offset| Ok(*offset <= u64_from_index(start)),
                    "scan SLDPRT profiles records",
                )?;
                let has_unbound_marker = marker_offsets
                    .get(first_after_start)
                    .is_some_and(|offset| *offset < u64_from_index(end));
                if object_markers.is_empty()
                    && !has_unbound_marker
                    && !ctx.contains_hash_set(
                        &marker_owners,
                        native_feature.id.as_str(),
                        "resolve SLDPRT profiles keys",
                    )?
                    && bound_sketch.is_none()
                    && !block_definition
                {
                    if !ctx.contains_key_hash_map(
                        &sketch_positions,
                        sketch_id.as_str(),
                        "compare SLDPRT profile sketch identities",
                    )? {
                        let placement = match frame {
                            Some((origin, normal, u_axis)) => {
                                match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                                    origin, normal, u_axis,
                                ) {
                                    Ok(placement) => placement,
                                    Err(_) => continue,
                                }
                            }
                            None => cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
                        };
                        let (sketch, empty_sketch_storage) =
                            ctx.with_scoped_storage("build SLDPRT marker sketch fields", || {
                                let id = sketch_id.try_clone_for_decode(
                                    ctx,
                                    "copy SLDPRT empty marker sketch identity",
                                )?;
                                let name = ctx.copy_retained_text(
                                    &native_feature.name,
                                    "copy SLDPRT empty marker sketch name",
                                )?;
                                let configuration = lane
                                    .configuration
                                    .as_deref()
                                    .map(|value| {
                                        ctx.copy_retained_text(
                                            value,
                                            "copy SLDPRT empty marker sketch configuration",
                                        )
                                    })
                                    .transpose()?;
                                let native_ref = ctx.copy_retained_text(
                                    &lane.id,
                                    "copy SLDPRT empty marker sketch native reference",
                                )?;
                                let sketch = Sketch {
                                    id,
                                    name: Some(name),
                                    configuration,
                                    visible: None,
                                    placement,
                                    profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                                    native_ref: Some(native_ref),
                                };
                                Ok::<_, CodecError>(sketch)
                            })?;
                        push_indexed_sketch(
                            ctx,
                            sketches,
                            &mut sketch_positions,
                            &mut sketch_positions_storage,
                            sketch,
                            "append SLDPRT empty marker sketch",
                        )?;
                        empty_sketch_storage.commit()?;
                    }
                    sketch_id_storage.commit()?;
                    features[feature_index].evaluation.set_definition(
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Sketch {
                                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                    sketch_id,
                                )),
                            },
                        ),
                    );
                }
                continue;
            }
            if ctx.contains_key_hash_map(
                &sketch_positions,
                sketch_id.as_str(),
                "compare SLDPRT profile sketch identities",
            )? {
                sketch_id_storage.commit()?;
                features[feature_index]
                    .evaluation
                    .set_definition(if block_definition {
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::SketchBlockDefinition {
                                sketch: Some(sketch_id),
                            },
                        )
                    } else {
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Sketch {
                                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                    sketch_id,
                                )),
                            },
                        )
                    });
                continue;
            }
            if let Some(bound_sketch) = bound_sketch.as_ref() {
                if !ctx.contains_text(
                    bound_sketch.as_str(),
                    "sketch#compact:",
                    "check SLDPRT bound marker sketch identity",
                )? {
                    continue;
                }
            }
            let placement = match frame {
                Some((origin, normal, u_axis)) => {
                    match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                        origin, normal, u_axis,
                    ) {
                        Ok(placement) => placement,
                        Err(_) => continue,
                    }
                }
                None => cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
            };
            let (mut sketch, sketch_fields_storage) =
                ctx.with_scoped_storage("build SLDPRT marker sketch fields", || {
                    let sketch_copy = sketch_id
                        .try_clone_for_decode(ctx, "copy SLDPRT marker sketch identity")?;
                    let name = ctx.copy_retained_text(
                        &native_feature.name,
                        "copy SLDPRT marker sketch name",
                    )?;
                    let configuration = lane
                        .configuration
                        .as_deref()
                        .map(|value| {
                            ctx.copy_retained_text(value, "copy SLDPRT marker sketch configuration")
                        })
                        .transpose()?;
                    let native_ref = ctx.copy_retained_text(
                        &lane.id,
                        "copy SLDPRT marker sketch native reference",
                    )?;
                    let sketch = Sketch {
                        id: sketch_copy,
                        name: Some(name),
                        configuration,
                        visible: None,
                        placement,
                        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                        native_ref: Some(native_ref),
                    };
                    Ok::<_, CodecError>(sketch)
                })?;
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let encoded_rectangle =
                indexed_rectangle_from_line_cycle(ctx, &lane.native_payload, &object_markers)?;
            let mut inferred_points_storage =
                ctx.reserve_scoped(0, "solve SLDPRT omitted point coordinates")?;
            let inferred_points = std::cell::OnceCell::new();
            let mut projected_storage =
                ctx.reserve_scoped(0, "collect SLDPRT projected marker entities")?;
            let mut projected = Vec::new();
            let mut entity_storage_slots = Vec::new();
            let mut entity_storage_slots_storage =
                ctx.reserve_scoped(0, "track SLDPRT projected entity storage")?;
            for marker in ctx
                .admit_iter(&markers[..], "scan SLDPRT profiles records")?
                .copied()
            {
                let mut native_geometry_storage =
                    ctx.reserve_scoped(0, "build SLDPRT marker native geometry")?;
                let mut endpoint_refs_storage =
                    ctx.reserve_scoped(0, "build SLDPRT projected endpoint references")?;
                let (entity, entity_storage) = ctx.with_scoped_storage("build SLDPRT projected marker entity", || -> Result<_, CodecError> {
                let mut native_geometry = || -> Result<_, MarkerGeometryFailure> {
                    let kind = native_geometry_storage.with_storage(|| cadmpeg_core::nonblank_literal!(
                        ctx, "sldprt:marker-geometry:{}", marker.kind().native_code()
                    ))?;
                    Ok(SketchGeometry::native(kind))
                };
                let entity = (|| -> Result<_, MarkerGeometryFailure> {
                    let project = |endpoint: &SketchInputEntity| {
                        let [u, v] = endpoint.coordinates_m?.get();
                        let point = transform.apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))?;
                        Some(Point2::new(
                            f64_from_i64(point.0)? * QUANTUM,
                            f64_from_i64(point.1)? * QUANTUM,
                        ))
                    };
                    let project_coordinates = |[u, v]: [f64; 2]| {
                        let point = transform.apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))?;
                        Some(Point2::new(
                            f64_from_i64(point.0)? * QUANTUM,
                            f64_from_i64(point.1)? * QUANTUM,
                        ))
                    };
                    let is_recovered_legacy_profile_point = |endpoint: &SketchInputEntity| {
                        usize::try_from(endpoint.offset())
                            .ok()
                            .is_some_and(|offset| {
                                legacy_140_profile_point_variant_coordinates(
                                    &lane.native_payload,
                                    offset,
                                )
                                .is_some()
                            })
                    };
                    let geometry = match marker.kind() {
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                            let point = project(marker).ok_or(MarkerGeometryFailure::Absent)?;
                            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                                position: point,
                            })
                            .ok()
                            .ok_or(MarkerGeometryFailure::Absent)?
                        }
                        SketchInputKind::LineOrCircle => {
                            let mut circle_geometry = legacy_profile_radial_circle(&lane.native_payload,
marker,
&geometry_index,
)?;
                            if circle_geometry.is_none() {
                                circle_geometry = compact_profile_full_circle(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = current_profile_circle_dimension(&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = compact_legacy_terminal_diameter_circle(
&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = compact_legacy_profile_full_circle(
                                    ctx,
                                    &lane.native_payload,
                                    marker,
                                    &geometry_index,
                                    &prefixes,
                                )?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = extended_geometry_full_circle(&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = coordinate_roster_full_circle(&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = wide_coordinate_roster_full_circle(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if let Some((center, radius)) = circle_geometry {
                                let point = transform
                                    .apply(quantize(
                                        Point2::new(
                                            center[0] * NATIVE_TO_IR,
                                            center[1] * NATIVE_TO_IR,
                                        ),
                                        QUANTUM,
                                    ))
                                    .ok_or(MarkerGeometryFailure::Absent)?;
                                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                                    center: Point2::new(
                                        f64_from_i64(point.0)
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            * QUANTUM,
                                        f64_from_i64(point.1)
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            * QUANTUM,
                                    ),
                                    radius: Length::new(radius * NATIVE_TO_IR)
                                        .ok_or(MarkerGeometryFailure::Absent)?,
                                })
                                .ok()
                                .ok_or(MarkerGeometryFailure::Absent)?
                            } else {
                                let (endpoints, _endpoints_storage) = ctx.with_scoped_storage("resolve SLDPRT projected marker endpoints", || output_curve_endpoint_markers(ctx, &lane.native_payload, marker, &markers_by_id, &object_markers, &geometry_index))?;
                                if let [start_marker, end_marker] = endpoints.as_slice() {
                                    let (Some(start), Some(end)) =
                                        (project(start_marker), project(end_marker))
                                    else {
                                        return Err(MarkerGeometryFailure::Absent);
                                    };
                                    if start == end {
                                        if is_recovered_legacy_profile_point(start_marker)
                                            || is_recovered_legacy_profile_point(end_marker)
                                        {
                                            // A zero-length line is not valid IR geometry. Preserve
                                            // the marker whose endpoint collapsed because a newly
                                            // recognized profile point supplied its coordinates.
                                            native_geometry()?
                                        } else {
                                            return Err(MarkerGeometryFailure::Absent);
                                        }
                                    } else {
                                        SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                            start,
                                            end,
                                        })
                                        .ok()
                                        .ok_or(MarkerGeometryFailure::Absent)?
                                    }
                                } else if let Some([start, end]) = {
                                    let mut inline = extended_declared_inline_line_endpoints(
                                        ctx, &lane.native_payload, marker, &geometry_index,
                                    )?;
                                    if inline.is_none() {
                                        inline = extended_linked_inline_line_endpoints(
                                            ctx, &lane.native_payload, marker, &geometry_index,
                                        )?;
                                    }
                                    if inline.is_none() {
                                        inline = extended_identity_inline_line_endpoints(
                                            ctx, &lane.native_payload, marker, &geometry_index,
                                        )?;
                                    }
                                    let mut endpoints = inline.map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get));
                                    if endpoints.is_none() {
                                        let inferred = match inferred_points.get() {
                                            Some(inferred) => inferred,
                                            None => {
                                                let inferred = inferred_points_storage.with_storage(|| inferred_point_coordinates_by_index(
                                                    ctx,
                                                    lane,
                                                    native_feature.id.as_str(),
                                                ))?;
                                                inferred_points.get_or_init(|| inferred)
                                            }
                                        };
                                        endpoints = implicit_coordinate_roster_curve_endpoints(ctx,
&lane.native_payload,
marker,
inferred,
&geometry_index,
)?;
                                    }
                                    if endpoints.is_none() {
                                        endpoints = implicit_profile_chain_closure_endpoints(ctx, &lane.native_payload, marker, &object_markers, &geometry_index)?;
                                    }
                                    endpoints.or_else(|| {
                                        compact_legacy_142_profile_curve_endpoints(
                                            &lane.native_payload,
                                            index_from_u64(marker.offset())?,
                                        )
                                        .map(|endpoints| {
                                            endpoints.map(cadmpeg_ir::units::FiniteVector::get)
                                        })
                                    })
                                } {
                                    let (Some(start), Some(end)) =
                                        (project_coordinates(start), project_coordinates(end))
                                    else {
                                        return Err(MarkerGeometryFailure::Absent);
                                    };
                                    if start == end {
                                        return Err(MarkerGeometryFailure::Absent);
                                    }
                                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                        start,
                                        end,
                                    })
                                    .ok()
                                    .ok_or(MarkerGeometryFailure::Absent)?
                                } else {
                                    native_geometry()?
                                }
                            }
                        }
                        SketchInputKind::Arc => {
                            let (endpoints, _endpoints_storage) = ctx.with_scoped_storage("resolve SLDPRT projected marker endpoints", || marker_curve_endpoint_markers(ctx, &lane.native_payload, marker, &markers_by_id, &object_markers, &geometry_index))?;
                            let mut circle_geometry = equal_index_coordinate_roster_full_circle(&lane.native_payload,
marker,
&geometry_index,
)?;
                            if circle_geometry.is_none() {
                                circle_geometry = compact_profile_full_circle(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = coordinate_roster_full_circle(&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = wide_coordinate_roster_full_circle(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?;
                            }
                            if let Some((center, radius)) = circle_geometry {
                                let point = transform
                                    .apply(quantize(
                                        Point2::new(
                                            center[0] * NATIVE_TO_IR,
                                            center[1] * NATIVE_TO_IR,
                                        ),
                                        QUANTUM,
                                    ))
                                    .ok_or(MarkerGeometryFailure::Absent)?;
                                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                                    center: Point2::new(
                                        f64_from_i64(point.0)
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            * QUANTUM,
                                        f64_from_i64(point.1)
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            * QUANTUM,
                                    ),
                                    radius: Length::new(radius * NATIVE_TO_IR)
                                        .ok_or(MarkerGeometryFailure::Absent)?,
                                })
                                .ok()
                                .ok_or(MarkerGeometryFailure::Absent)?
                            } else if let (Some(point), Some(radius)) =
                                (marker.coordinates_m.and_then(|_| project(marker)), {
                                    let radius = coordinate_circle_radius(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?;
                                    if radius.is_some() {
                                        radius
                                    } else {
                                        legacy_coordinate_circle_radius(
                                            ctx,
                                            &lane.native_payload,
                                            marker,
                                            &geometry_index,
                                        )?
                                    }
                                })
                            {
                                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                                    center: point,
                                    radius: Length::new(radius * NATIVE_TO_IR)
                                        .ok_or(MarkerGeometryFailure::Absent)?,
                                })
                                .ok()
                                .ok_or(MarkerGeometryFailure::Absent)?
                            } else if let (Some(center), Some((major_axis, major, minor))) = (
                                marker.coordinates_m.and_then(|_| project(marker)),
                                coordinate_ellipse_axes(ctx,
&lane.native_payload,
marker,
&geometry_index,
)?,
                            ) {
                                let axis = transform
                                    .apply_axes(quantize(
                                        Point2::new(major_axis[0], major_axis[1]),
                                        QUANTUM,
                                    ))
                                    .ok_or(MarkerGeometryFailure::Absent)?;
                                SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                                    center,
                                    major_angle: Angle::new(
                                        (f64_from_i64(axis.1)
                                            .ok_or(MarkerGeometryFailure::Absent)?)
                                        .atan2(
                                            f64_from_i64(axis.0)
                                                .ok_or(MarkerGeometryFailure::Absent)?,
                                        ),
                                    )
                                    .ok_or(MarkerGeometryFailure::Absent)?,
                                    radii: cadmpeg_ir::sketches::EllipseRadii {
                                        major_radius: Length::new(major * NATIVE_TO_IR)
                                            .ok_or(MarkerGeometryFailure::Absent)?,
                                        minor_radius: Length::new(minor * NATIVE_TO_IR)
                                            .ok_or(MarkerGeometryFailure::Absent)?,
                                    },
                                    bounds: None,
                                })
                                .ok()
                                .ok_or(MarkerGeometryFailure::Absent)?
                            } else if let Some([center, start, end]) =
                                index_from_u64(marker.offset()).and_then(|offset| {
                                    inline_arc_coordinates(&lane.native_payload, offset)
                                })
                            {
                                let (Some(center), Some(start), Some(end)) = (
                                    project_coordinates(center.get()),
                                    project_coordinates(start.get()),
                                    project_coordinates(end.get()),
                                ) else {
                                    return Err(MarkerGeometryFailure::Absent);
                                };
                                minor_arc_geometry(start, end, center, QUANTUM)
                                    .ok_or(MarkerGeometryFailure::Absent)?
                            } else if let ([start, end], Some(point)) = (
                                endpoints.as_slice(),
                                marker.coordinates_m.and_then(|_| project(marker)),
                            ) {
                                let (Some(start), Some(end)) = (project(start), project(end))
                                else {
                                    return Err(MarkerGeometryFailure::Absent);
                                };
                                match minor_arc_geometry(start, end, point, QUANTUM) {
                                    Some(geometry) => geometry,
                                    None => native_geometry()?,
                                }
                            } else {
                                (|| -> Result<SketchGeometry, MarkerGeometryFailure> {
                                    let [start, end] = endpoints.as_slice() else {
                                        return Err(MarkerGeometryFailure::Absent);
                                    };
                                    let (start, end) = (
                                        project(start).ok_or(MarkerGeometryFailure::Absent)?,
                                        project(end).ok_or(MarkerGeometryFailure::Absent)?,
                                    );
                                    let offset = index_from_u64(marker.offset())
                                        .ok_or(MarkerGeometryFailure::Absent)?;
                                    if extended_wide_construction_line_roster_indices(
                                        &lane.native_payload,
                                        offset,
                                    )
                                    .is_some()
                                        || packed_compact_legacy_curve_endpoint_indices(
                                            &lane.native_payload,
                                            offset,
                                        )
                                        .is_some_and(
                                            |_| {
                                                marker_profile_curve_role(
                                                    &lane.native_payload,
                                                    offset,
                                                ) == Some(2)
                                            },
                                        )
                                        || legacy_direct_compact_selected_axis_endpoint_indices(
                                            &lane.native_payload,
                                            offset,
                                        )
                                        .is_some()
                                    {
                                        return SketchGeometry::try_from(
                                            SketchGeometryDefinition::Line { start, end },
                                        )
                                        .map_err(|_| MarkerGeometryFailure::Absent);
                                    }
                                    let legacy_center = match legacy_marker104_arc_center(
                                        ctx,
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                        [endpoints[0], endpoints[1]],
                                        &geometry_index,
                                    )? {
                                        Some(center) => Some(center),
                                        None => legacy_compact_diameter_arc_center(
                                            ctx,
                                            &lane.native_payload,
                                            marker,
                                            &geometry_index,
                                            [endpoints[0], endpoints[1]],
                                        )?,
                                    };
                                    if let Some([u, v]) = legacy_center {
                                        let center = transform
                                            .apply(quantize(
                                                Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                                                QUANTUM,
                                            ))
                                            .ok_or(MarkerGeometryFailure::Absent)?;
                                        let center = Point2::new(
                                            f64_from_i64(center.0)
                                                .ok_or(MarkerGeometryFailure::Absent)?
                                                * QUANTUM,
                                            f64_from_i64(center.1)
                                                .ok_or(MarkerGeometryFailure::Absent)?
                                                * QUANTUM,
                                        );
                                        return minor_arc_geometry(start, end, center, QUANTUM)
                                            .ok_or(MarkerGeometryFailure::Absent);
                                    }
                                    if indexed_arc_uses_coordinate_center(
                                        &lane.native_payload,
                                        offset,
                                    ) {
                                        let [start_u, start_v] = endpoints[0]
                                            .coordinates_m
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            .get();
                                        let [end_u, end_v] = endpoints[1]
                                            .coordinates_m
                                            .ok_or(MarkerGeometryFailure::Absent)?
                                            .get();
                                        let roster_center = coordinate_roster_arc_center(ctx, &lane.native_payload, marker, [endpoints[0], endpoints[1]], &geometry_index)?
                                        .map(|[u, v]| {
                                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR)
                                        });
                                        let roster_center_witness = roster_center.is_some();

                                        let (center_start, center_end) =
                                            if current_indexed_arc_reverses_center_sweep(
                                                &lane.native_payload,
                                                offset,
                                            ) {
                                                (
                                                    Point2::new(
                                                        end_u * NATIVE_TO_IR,
                                                        end_v * NATIVE_TO_IR,
                                                    ),
                                                    Point2::new(
                                                        start_u * NATIVE_TO_IR,
                                                        start_v * NATIVE_TO_IR,
                                                    ),
                                                )
                                            } else {
                                                (
                                                    Point2::new(
                                                        start_u * NATIVE_TO_IR,
                                                        start_v * NATIVE_TO_IR,
                                                    ),
                                                    Point2::new(
                                                        end_u * NATIVE_TO_IR,
                                                        end_v * NATIVE_TO_IR,
                                                    ),
                                                )
                                            };
                                        let center = match roster_center {
                                            Some(center) => Some(center),
                                            None => match geometry_index.arc_centers(marker, true, QUANTUM)? {
                                                Some(candidates) => unique_arc_center_marker(ctx, center_start, center_end, candidates, QUANTUM,
                                                    [Some(endpoints[0].id()), Some(endpoints[1].id())])?,
                                                None => None,
                                            },
                                        };
                                        if let Some(center) = center {
                                            let center = transform
                                                .apply(quantize(center, QUANTUM))
                                                .ok_or(MarkerGeometryFailure::Absent)?;
                                            let center = Point2::new(
                                                f64_from_i64(center.0)
                                                    .ok_or(MarkerGeometryFailure::Absent)?
                                                    * QUANTUM,
                                                f64_from_i64(center.1)
                                                    .ok_or(MarkerGeometryFailure::Absent)?
                                                    * QUANTUM,
                                            );
                                            // The three transformed points are quantized independently.
                                            // Allow two quanta when the record supplies the center directly.
                                            let tolerance = if roster_center_witness {
                                                QUANTUM * 2.0
                                            } else {
                                                QUANTUM
                                            };
                                            let geometry =
                                                minor_arc_geometry(start, end, center, tolerance);
                                            return geometry.ok_or(MarkerGeometryFailure::Absent);
                                        }
                                    }
                                    let [start_marker, end_marker] = endpoints.as_slice() else {
                                        return Err(MarkerGeometryFailure::Absent);
                                    };
                                    let [start_u, start_v] = start_marker
                                        .coordinates_m
                                        .ok_or(MarkerGeometryFailure::Absent)?
                                        .get();
                                    let [end_u, end_v] = end_marker
                                        .coordinates_m
                                        .ok_or(MarkerGeometryFailure::Absent)?
                                        .get();

                                    let center = match geometry_index.arc_centers(marker, true, QUANTUM)? {
                                        Some(candidates) => unique_arc_center_marker(ctx,
                                            Point2::new(start_u * NATIVE_TO_IR, start_v * NATIVE_TO_IR),
                                            Point2::new(end_u * NATIVE_TO_IR, end_v * NATIVE_TO_IR),
                                            candidates, QUANTUM, [Some(start_marker.id()), Some(end_marker.id())])?,
                                        None => None,
                                    };
                                    if let Some(center) = center {
                                        let center = transform
                                            .apply(quantize(center, QUANTUM))
                                            .ok_or(MarkerGeometryFailure::Absent)?;
                                        let center = Point2::new(
                                            f64_from_i64(center.0)
                                                .ok_or(MarkerGeometryFailure::Absent)?
                                                * QUANTUM,
                                            f64_from_i64(center.1)
                                                .ok_or(MarkerGeometryFailure::Absent)?
                                                * QUANTUM,
                                        );
                                        return minor_arc_geometry(start, end, center, QUANTUM)
                                            .ok_or(MarkerGeometryFailure::Absent);
                                    }
                                    if let Some([tu, tv]) =
                                        compact_bounded_curve_tangent(&lane.native_payload, offset)
                                    {
                                        let (tu, tv) = transform
                                            .apply_axes(quantize(Point2::new(tu, tv), QUANTUM))
                                            .ok_or(MarkerGeometryFailure::Absent)?;
                                        return tangent_bounded_curve(
                                            start,
                                            end,
                                            [
                                                f64_from_i64(tu)
                                                    .ok_or(MarkerGeometryFailure::Absent)?
                                                    * QUANTUM,
                                                f64_from_i64(tv)
                                                    .ok_or(MarkerGeometryFailure::Absent)?
                                                    * QUANTUM,
                                            ],
                                            QUANTUM,
                                        )
                                        .ok_or(MarkerGeometryFailure::Absent);
                                    }
                                    (current_undetailed_bounded_curve_is_line(
                                        &lane.native_payload,
                                        offset,
                                    ) || legacy_undetailed_profile_line(
                                        &lane.native_payload,
                                        offset,
                                    ))
                                    .then_some(
                                        SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                            start,
                                            end,
                                        })
                                        .ok()
                                        .ok_or(MarkerGeometryFailure::Absent)?,
                                    )
                                    .ok_or(MarkerGeometryFailure::Absent)
                                })()
                                .or_else(|failure| {
                                    match failure {
                                        MarkerGeometryFailure::Absent => {
                                            native_geometry()
                                        }
                                        MarkerGeometryFailure::Codec(error) => {
                                            Err(MarkerGeometryFailure::Codec(error))
                                        }
                                    }
                                })?
                            }
                        }
                        SketchInputKind::Relation(_)
                        | SketchInputKind::Native(_)
                        | SketchInputKind::NativeHandle(_) => {
                            return Err(MarkerGeometryFailure::Absent)
                        }
                    };
                    if matches!(
                        geometry.definition(),
                        SketchGeometryDefinition::Native { .. }
                    ) && marker.coordinates_m.is_some()
                        && index_from_u64(marker.offset()).is_none_or(|offset| {
                            !marker_is_geometry_locus(&lane.native_payload, offset)
                        })
                    {
                        return Err(MarkerGeometryFailure::Absent);
                    }
                    let construction = index_from_u64(marker.offset()).is_some_and(|offset| {
                        (marker_is_selected_construction_line(&lane.native_payload, offset)
                            || current_compact_roster_selected_axis(&lane.native_payload, offset))
                            && !(matches!(
                                geometry.definition(),
                                SketchGeometryDefinition::Circle { .. }
                            ) && marker_profile_curve_role(&lane.native_payload, offset)
                                == Some(1))
                    });
                    Ok((geometry, construction))
                })();
                let entity = match entity {
                    Ok(entity) => Some(entity),
                    Err(MarkerGeometryFailure::Absent) => None,
                    Err(MarkerGeometryFailure::Codec(error)) => return Err(error),
                };
                if let Some((geometry, construction)) = entity {
                    let mut endpoint_refs = Vec::new();
                    if matches!(
                        marker.kind(),
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    ) {
                        let (endpoints, _endpoints_storage) = ctx.with_scoped_storage("resolve SLDPRT projected marker endpoints", || output_curve_endpoint_markers(ctx, &lane.native_payload, marker, &markers_by_id, &object_markers, &geometry_index))?;
                        for endpoint in
                            ctx.admit_iter(&endpoints, "collect SLDPRT marker endpoint references")?
                        {
                            let reference = endpoint_refs_storage.with_storage(|| ctx.copy_retained_text(
                                endpoint.id(),
                                "copy SLDPRT marker endpoint reference",
                            ))?;
                            endpoint_refs_storage.with_storage(|| ctx.reserve_vec(
                                &mut endpoint_refs,
                                1,
                                "collect SLDPRT marker endpoint references",
                            ))?;
                            endpoint_refs.push(reference);
                        }
                    }
                    let id_text = ctx.format_retained(
                        format_args!(
                            "sldprt:model:sketch-entity#markers:{lane_key}:{}:{}",
                            native_feature.ordinal,
                            marker.ordinal()
                        ),
                        "format SLDPRT marker entity identity",
                    )?;
                    ctx.charge_work(
                        u64_from_index(id_text.len()),
                        "validate SLDPRT marker entity identity",
                    )?;
                    let Ok(entity_id) = SketchEntityId::mint(id_text) else { return Ok(None); };
                    let owner = sketch_id
                        .try_clone_for_decode(ctx, "copy SLDPRT marker entity sketch identity")?;
                    let native_ref = ctx
                        .copy_retained_text(marker.id(), "copy SLDPRT marker native reference")?;
                    let entity = SketchEntity::new(entity_id, owner, geometry)
                        .with_construction(construction)
                        .with_native_ref(Some(native_ref))
                        .with_endpoint_refs(endpoint_refs);
                    Ok(Some(entity))
                } else {
                    Ok(None)
                }
                })?;
                if let Some(entity) = entity {
                    let endpoint_refs_pointer = entity.endpoint_refs.as_ptr();
                    projected_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut projected,
                            1,
                            "collect SLDPRT projected marker entities",
                        )?;
                        projected.push(entity);
                        Ok::<(), CodecError>(())
                    })?;
                    entity_storage_slots_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut entity_storage_slots,
                            Some((
                                entity_storage,
                                native_geometry_storage,
                                endpoint_refs_storage,
                                endpoint_refs_pointer,
                            )),
                            "track SLDPRT projected entity storage",
                        )
                    })?;
                }
            }
            if let Some(rectangle) = encoded_rectangle {
                let mut rectangle_marker_refs_storage =
                    ctx.reserve_scoped(0, "index SLDPRT rectangle marker references")?;
                let mut rectangle_marker_refs = HashSet::new();
                for marker in
                    ctx.admit_iter(&(object_markers)[..], "scan SLDPRT profiles records")?
                {
                    let Some(marker_id) = (|| {
                        let offset = index_from_u64(marker.offset())?;
                        compact_legacy_curve_endpoint_indices(&lane.native_payload, offset)
                            .or_else(|| {
                                compact_legacy_code_one_line_endpoint_indices(
                                    &lane.native_payload,
                                    offset,
                                )
                            })
                            .or_else(|| {
                                legacy_extended_rectangle_line_endpoints(
                                    &lane.native_payload,
                                    offset,
                                )
                            })
                            .or_else(|| {
                                current_compact_rectangle_line_endpoints(
                                    &lane.native_payload,
                                    offset,
                                )
                            })
                            .or_else(|| {
                                compact_legacy_rectangle_line_endpoints(
                                    &lane.native_payload,
                                    offset,
                                )
                            })
                            .or_else(|| {
                                current_wide_rectangle_line_endpoints(&lane.native_payload, offset)
                            })?;
                        Some(marker.id())
                    })() else {
                        continue;
                    };
                    rectangle_marker_refs_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut rectangle_marker_refs,
                            marker_id,
                            "index SLDPRT rectangle marker references",
                        )
                    })?;
                }
                let mut storage_position = 0;
                ctx.retain_vec(
                    &mut projected,
                    |entity| {
                        let keep = match entity.native_ref.as_deref() {
                            Some(native) => !ctx.contains_hash_set(
                                &rectangle_marker_refs,
                                native,
                                "filter SLDPRT rectangle marker references",
                            )?,
                            None => true,
                        };
                        if !keep {
                            drop(entity_storage_slots[storage_position].take());
                        }
                        storage_position += 1;
                        Ok(keep)
                    },
                    "filter SLDPRT rectangle marker references",
                )?;
                ctx.retain_vec(
                    &mut entity_storage_slots,
                    |storage| Ok(storage.is_some()),
                    "compact SLDPRT projected entity storage",
                )?;
                let corners = rectangle.map(|point| {
                    let point = transform.apply(quantize(
                        Point2::new(point.u * NATIVE_TO_IR, point.v * NATIVE_TO_IR),
                        QUANTUM,
                    ))?;
                    Some(Point2::new(
                        f64_from_i64(point.0)? * QUANTUM,
                        f64_from_i64(point.1)? * QUANTUM,
                    ))
                });
                let [Some(first), Some(second), Some(third), Some(fourth)] = corners else {
                    continue;
                };
                let corners = [first, second, third, fourth];
                let point_matches_corner = |point: Point2, corner: Point2| {
                    same_dimension_length(point.u, corner.u)
                        && same_dimension_length(point.v, corner.v)
                };
                // `Some(Some(index))` for one misplaced point, `Some(None)` for several.
                let mut misplaced_point = None;
                let mut matched_corners = [false; 4];
                for (index, entity) in ctx
                    .admit_iter(&projected[..], "match SLDPRT rectangle corner points")?
                    .enumerate()
                {
                    let SketchGeometryDefinition::Point { position } =
                        *entity.geometry.definition()
                    else {
                        continue;
                    };
                    let mut placed = false;
                    for (matched, corner) in matched_corners.iter_mut().zip(corners) {
                        if point_matches_corner(position.get(), corner) {
                            *matched = true;
                            placed = true;
                        }
                    }
                    if !placed {
                        misplaced_point = Some(match misplaced_point {
                            None => Some(index),
                            Some(_) => None,
                        });
                    }
                }
                let mut missing_corners = corners
                    .iter()
                    .zip(matched_corners)
                    .filter_map(|(corner, matched)| (!matched).then_some(*corner));
                if let (Some(Some(point)), Some(corner), None) = (
                    misplaced_point,
                    missing_corners.next(),
                    missing_corners.next(),
                ) {
                    projected[point].geometry =
                        match SketchGeometry::try_from(SketchGeometryDefinition::Point {
                            position: corner,
                        }) {
                            Ok(geometry) => geometry,
                            Err(_) => continue,
                        };
                }
                for entity_index in ctx.admit_iter(
                    &(0..projected.len()),
                    "scan SLDPRT projected profile markers",
                )? {
                    let entity = &mut projected[entity_index];
                    let SketchGeometryDefinition::Native { .. } = *entity.geometry.definition()
                    else {
                        continue;
                    };
                    let marker = match entity.native_ref.as_deref() {
                        Some(native) => ctx
                            .get_hash_map(
                                &markers_by_id,
                                native,
                                "resolve SLDPRT profile native marker",
                            )?
                            .copied(),
                        None => None,
                    };
                    let Some(marker) = marker else {
                        continue;
                    };
                    let Some([u, v]) =
                        legacy_extended_rectangle_diagonal_endpoint(&lane.native_payload, marker)
                            .map(cadmpeg_ir::units::FiniteVector::get)
                    else {
                        continue;
                    };
                    let Some(endpoint) = transform
                        .apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))
                        .and_then(|point| {
                            Some(Point2::new(
                                f64_from_i64(point.0)? * QUANTUM,
                                f64_from_i64(point.1)? * QUANTUM,
                            ))
                        })
                    else {
                        continue;
                    };
                    let mut matching = corners
                        .iter()
                        .enumerate()
                        .filter(|(_, corner)| {
                            same_dimension_length(corner.u, endpoint.u)
                                && same_dimension_length(corner.v, endpoint.v)
                        })
                        .map(|(index, _)| index);
                    let (Some(index), None) = (matching.next(), matching.next()) else {
                        continue;
                    };
                    entity.geometry =
                        match SketchGeometry::try_from(SketchGeometryDefinition::Line {
                            start: endpoint,
                            end: corners[(index + 2) % corners.len()],
                        }) {
                            Ok(geometry) => geometry,
                            Err(_) => continue,
                        };
                }
                for (index, start) in corners.iter().enumerate() {
                    let (entity, entity_storage) =
                        ctx.with_scoped_storage("build SLDPRT rectangle edge", || {
                            let id_text = ctx.format_retained(
                                format_args!(
                            "sldprt:model:sketch-entity#markers:{lane_key}:{}:rectangle:{index}",
                            native_feature.ordinal
                        ),
                                "format SLDPRT rectangle edge identity",
                            )?;
                            ctx.charge_work(
                                u64_from_index(id_text.len()),
                                "validate SLDPRT rectangle edge identity",
                            )?;
                            let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                                return Ok(None);
                            };
                            let owner = sketch_id.try_clone_for_decode(
                                ctx,
                                "copy SLDPRT rectangle sketch identity",
                            )?;
                            let Ok(geometry) =
                                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                                    start: *start,
                                    end: corners[(index + 1) % corners.len()],
                                })
                            else {
                                return Ok(None);
                            };
                            Ok::<_, CodecError>(Some(SketchEntity::new(entity_id, owner, geometry)))
                        })?;
                    let Some(entity) = entity else {
                        continue;
                    };
                    let endpoint_refs_pointer = entity.endpoint_refs.as_ptr();
                    projected_storage.with_storage(|| {
                        ctx.reserve_vec(&mut projected, 1, "append SLDPRT rectangle edges")?;
                        projected.push(entity);
                        Ok::<(), CodecError>(())
                    })?;
                    let native_storage =
                        ctx.reserve_scoped(0, "build SLDPRT marker native geometry")?;
                    let endpoint_refs_storage =
                        ctx.reserve_scoped(0, "build SLDPRT projected endpoint references")?;
                    entity_storage_slots_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut entity_storage_slots,
                            Some((
                                entity_storage,
                                native_storage,
                                endpoint_refs_storage,
                                endpoint_refs_pointer,
                            )),
                            "track SLDPRT projected entity storage",
                        )
                    })?;
                }
            }
            let ((), resolved_fields_storage) =
                ctx.with_scoped_storage("build SLDPRT resolved marker fields", || {
                    resolve_two_center_semicircle_profile(
                        ctx,
                        &lane.native_payload,
                        &object_markers,
                        &mut projected,
                        &mut projected_storage,
                        QUANTUM,
                    )?;
                    resolve_slot_marker_arcs(
                        ctx,
                        &slots,
                        &object_markers,
                        &mut projected,
                        QUANTUM,
                    )?;
                    resolve_connected_marker_arcs(ctx, &mut projected, QUANTUM)?;
                    Ok::<_, CodecError>(())
                })?;
            let (profiles, profiles_storage) =
                ctx.with_scoped_storage("build SLDPRT marker sketch profiles", || {
                    Ok::<_, CodecError>(cadmpeg_ir::sketches::SketchProfiles::try_from(
                        closed_marker_profiles(ctx, &projected)?,
                    ))
                })?;
            let Ok(profiles) = profiles else {
                continue;
            };
            sketch.profiles = profiles;
            if projected.is_empty() || (bound_sketch.is_some() && sketch.profiles.is_empty()) {
                continue;
            }
            if let Some(bound_sketch) = &bound_sketch {
                replaced_storage.with_storage(|| {
                    let id = ctx.copy_retained_text(
                        bound_sketch.as_str(),
                        "remove prior SLDPRT marker sketches",
                    )?;
                    ctx.insert_hash_set(&mut replaced, id, "remove prior SLDPRT marker sketches")
                })?;
            }
            for (entity, storage) in ctx
                .admit_iter(
                    &projected[..entity_storage_slots.len()],
                    "keep SLDPRT projected entity fields",
                )?
                .zip(ctx.admit_iter(
                    &mut entity_storage_slots,
                    "keep SLDPRT projected entity fields",
                )?)
            {
                if let Some((fields, native, endpoints, endpoints_pointer)) = storage.take() {
                    fields.commit()?;
                    // Resolvers allocate replacements before dropping the previous endpoint list.
                    if entity.endpoint_refs.as_ptr() == endpoints_pointer {
                        endpoints.commit()?;
                    }
                    if matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Native { .. }
                    ) {
                        native.commit()?;
                    }
                }
            }
            ctx.extend_vec(
                sketch_entities,
                projected,
                "append SLDPRT projected marker entities",
            )?;
            push_indexed_sketch(
                ctx,
                sketches,
                &mut sketch_positions,
                &mut sketch_positions_storage,
                sketch,
                "append SLDPRT marker sketch",
            )?;
            sketch_fields_storage.commit()?;
            sketch_id_storage.commit()?;
            profiles_storage.commit()?;
            resolved_fields_storage.commit()?;
            features[feature_index]
                .evaluation
                .set_definition(if block_definition {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SketchBlockDefinition {
                            sketch: Some(sketch_id),
                        },
                    )
                } else {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch {
                            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                                sketch_id,
                            )),
                        },
                    )
                });
        }
    }
    drop(sketch_positions);
    drop(sketch_positions_storage);
    if !replaced.is_empty() {
        ctx.retain_vec(
            sketch_entities,
            |entity| {
                Ok(!ctx.contains_hash_set(
                    &replaced,
                    entity.sketch.as_str(),
                    "remove prior SLDPRT marker sketch entities",
                )?)
            },
            "remove prior SLDPRT marker sketch entities",
        )?;
        ctx.retain_vec(
            sketches,
            |sketch| {
                Ok(!ctx.contains_hash_set(
                    &replaced,
                    sketch.id.as_str(),
                    "remove prior SLDPRT marker sketches",
                )?)
            },
            "remove prior SLDPRT marker sketches",
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SketchBlockAssemblyFrame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

struct SketchBlockInstancePlacement {
    feature_id: String,
    block_source: u32,
    transform: Transform,
}

struct AssembledSketchBlockProfile<'a> {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
    _entities_storage: ScopedReservation<'a>,
    output_storage: ScopedReservation<'a>,
}

struct SketchBlockProfileInput<'a> {
    sketch_id: &'a SketchId,
    native_profile: &'a crate::records::Feature,
    native_ref: &'a str,
    configuration: Option<&'a str>,
    block_sketches: &'a HashMap<u32, SketchId>,
    instances: &'a [SketchBlockInstancePlacement],
    sketches: &'a [Sketch],
    /// The position of each sketch in `sketches`, by identity.
    sketch_positions: &'a HashMap<String, usize>,
    sketch_entities: &'a [SketchEntity],
    /// The positions of each sketch's entities in `sketch_entities`, by sketch identity.
    entity_groups: &'a HashMap<String, Vec<usize>>,
}

/// Resolve a profile feature that owns a reusable sketch-block sequence.
///
/// A block definition stores geometry in its reusable local sketch coordinates.
/// An instance placement maps those coordinates into the owning profile plane;
/// the definition's own sketch frame is not applied a second time. This keeps
/// the assembled geometry planar when the reusable definition frame is a
/// source-local construction frame rather than the consuming profile plane.
pub(crate) fn project_sketch_block_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "project SLDPRT sketch block profiles";
    // A block profile assembles block definitions; without one there is nothing to assemble.
    if !ctx.any_by(
        histories,
        |history| {
            ctx.any_by(
                &history.features,
                |feature| {
                    Ok(
                        native_object_class(feature.input_class.as_deref().unwrap_or_default())
                            == NativeClassKind::SketchBlockDefinition,
                    )
                },
                OPERATION,
            )
        },
        OPERATION,
    )? {
        return Ok(());
    }
    let (native_features, _native_features_storage) =
        index_native_features(ctx, histories, OPERATION)?;
    let (features_by_native, _features_by_native_storage) =
        model_features_by_native(ctx, features, &native_features, OPERATION)?;
    let (mut sketch_positions, mut sketch_positions_storage) =
        index_sketch_ids(ctx, sketches, OPERATION)?;
    let (mut entity_groups, mut entity_groups_storage) =
        index_entity_groups(ctx, sketch_entities, OPERATION)?;
    // Whether each history record is metadata, decided on first use.
    let mut metadata_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut metadata = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        metadata_storage.with_storage(|| {
            let decisions = ctx.alloc_filled(history.features.len(), None, OPERATION)?;
            ctx.push_vec(&mut metadata, decisions, OPERATION)
        })?;
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT profiles records")? {
        let object_names = ObjectNames::new(ctx, lane)?;
        for (history_index, history) in ctx
            .admit_iter(histories, "scan SLDPRT profiles records")?
            .enumerate()
        {
            let mut objects_storage =
                ctx.reserve_scoped(0, "collect SLDPRT sketch block history objects")?;
            let mut objects = Vec::new();
            for (ordinal, feature) in ctx
                .admit_iter(&history.features[..], "scan SLDPRT profiles records")?
                .enumerate()
            {
                let Some(name) = object_names.of(ctx, feature)? else {
                    continue;
                };
                let is_metadata = match metadata[history_index][ordinal] {
                    Some(decision) => decision,
                    None => {
                        let decision = crate::history::classify::is_history_metadata_record(
                            ctx,
                            feature,
                            &history.features,
                        )?;
                        metadata[history_index][ordinal] = Some(decision);
                        decision
                    }
                };
                if !is_metadata {
                    objects_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut objects,
                            (name.offset, feature, ordinal),
                            "collect SLDPRT sketch block history objects",
                        )
                    })?;
                }
            }
            ctx.sort_unstable_by_key(
                &mut objects,
                |value| {
                    let (left_offset, _, left_ordinal) = value;
                    (*left_offset, *left_ordinal)
                },
                Ord::cmp,
                "sort SLDPRT sketch block objects",
            )?;

            for (profile_position, (_, native_profile, _)) in ctx
                .admit_iter(&objects[..], "scan SLDPRT profiles records")?
                .enumerate()
            {
                if !super::component_paths::is_profile_feature_object(native_profile) {
                    continue;
                }
                let mut explicit_children_storage =
                    ctx.reserve_scoped(0, "collect SLDPRT dissectable child sources")?;
                let explicit_children = ctx
                    .get_btree_map(
                        &native_profile.properties,
                        "DissectableChildren",
                        "collect SLDPRT dissectable child sources",
                    )?
                    .map(|value| {
                        explicit_children_storage
                            .with_storage(|| dissectable_child_sources(ctx, value))
                    })
                    .transpose()?;
                if explicit_children.as_ref().is_some_and(Option::is_none) {
                    continue;
                }
                let search_start = profile_position.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "find SLDPRT sketch block profile end",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
                let first_non_block = ctx.position_by(
                    &objects[search_start..],
                    |(_, feature, _)| Ok(!is_sketch_block_object(feature)),
                    "find SLDPRT sketch block profile end",
                )?;
                let end = match first_non_block {
                    Some(position) => search_start.checked_add(position).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "find SLDPRT sketch block profile end",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?,
                    None => objects.len(),
                };
                let intervening = &objects[profile_position + 1..end];
                if !super::component_paths::profile_owns_intervening_sketch_blocks(
                    ctx,
                    native_profile,
                    ctx.admit_iter(intervening, "scan SLDPRT sketch block ownership")?
                        .map(|(_, feature, _)| *feature),
                )? {
                    continue;
                }
                let mut inferred_children_storage =
                    ctx.reserve_scoped(0, "collect SLDPRT sketch block children")?;
                let inferred_children = if explicit_children.is_none() {
                    let mut children = HashSet::new();
                    for (_, feature, _) in ctx
                        .admit_iter(intervening, "collect SLDPRT sketch block children")?
                        .filter(|(_, feature, _)| {
                            native_object_class(feature.input_class.as_deref().unwrap_or_default())
                                == NativeClassKind::SketchBlockDefinition
                        })
                    {
                        if let Some(source) = feature.source_value() {
                            inferred_children_storage.with_storage(|| {
                                ctx.insert_hash_set(
                                    &mut children,
                                    source,
                                    "collect SLDPRT sketch block children",
                                )
                            })?;
                        }
                    }
                    (!children.is_empty()).then_some(children)
                } else {
                    None
                };
                let Some(children) = explicit_children.flatten().or(inferred_children) else {
                    continue;
                };
                let Some(profile_index) = first_model_feature(
                    ctx,
                    &features_by_native,
                    native_profile.id.as_str(),
                    |_| true,
                    "find SLDPRT sketch block profile feature",
                )?
                else {
                    continue;
                };
                if !matches!(
                    features[profile_index].evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                            | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                        ..
                    })
                ) {
                    continue;
                }

                let mut block_sketches_storage =
                    ctx.reserve_scoped(0, "index SLDPRT sketch block definitions")?;
                let mut block_sketches = HashMap::<u32, SketchId>::new();
                let mut block_feature_ids_storage =
                    ctx.reserve_scoped(0, "index SLDPRT sketch block feature identities")?;
                let mut block_feature_ids = HashMap::<u32, String>::new();
                let mut definitions_complete = true;
                let mut visited = intervening.iter();
                while let Some((_, native_definition, _)) =
                    ctx.next_charged(&mut visited, "scan SLDPRT sketch block definitions")?
                {
                    if native_object_class(
                        native_definition.input_class.as_deref().unwrap_or_default(),
                    ) != NativeClassKind::SketchBlockDefinition
                    {
                        continue;
                    }
                    let Some(source) = native_definition.source_value() else {
                        definitions_complete = false;
                        break;
                    };
                    if !ctx.contains_hash_set(
                        &children,
                        &source,
                        "find SLDPRT sketch block child",
                    )? {
                        definitions_complete = false;
                        break;
                    }
                    let Some(definition_index) = first_model_feature(
                        ctx,
                        &features_by_native,
                        native_definition.id.as_str(),
                        |_| true,
                        "find SLDPRT sketch block definition feature",
                    )?
                    else {
                        definitions_complete = false;
                        break;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition {
                        sketch: Some(sketch_id),
                    }) = features[definition_index].evaluation.definition()
                    else {
                        definitions_complete = false;
                        break;
                    };
                    let Some(sketch) = ctx
                        .get_hash_map(
                            &sketch_positions,
                            sketch_id.as_str(),
                            "find SLDPRT block sketch",
                        )?
                        .map(|position| &sketches[*position])
                    else {
                        definitions_complete = false;
                        break;
                    };
                    let belongs_to_lane = match sketch.native_ref.as_deref() {
                        Some(native_ref) => ctx.equal(
                            native_ref,
                            lane.id.as_str(),
                            "compare SLDPRT block sketch lane",
                        )?,
                        None => false,
                    };
                    if !belongs_to_lane {
                        definitions_complete = false;
                        break;
                    }
                    block_sketches_storage.with_storage(|| {
                        let sketch_copy = sketch_id.try_clone_for_decode(
                            ctx,
                            "copy SLDPRT sketch block definition identity",
                        )?;
                        ctx.insert_hash_map(
                            &mut block_sketches,
                            source,
                            sketch_copy,
                            "index SLDPRT sketch block definitions",
                        )
                    })?;
                    block_feature_ids_storage.with_storage(|| {
                        let feature_id = ctx.copy_retained_text(
                            features[definition_index].id.as_str(),
                            "copy SLDPRT sketch block feature identity",
                        )?;
                        ctx.insert_hash_map(
                            &mut block_feature_ids,
                            source,
                            feature_id,
                            "index SLDPRT sketch block feature identities",
                        )
                    })?;
                }
                if !definitions_complete || block_sketches.len() != children.len() {
                    continue;
                }

                let mut instances_storage =
                    ctx.reserve_scoped(0, "collect SLDPRT sketch block instances")?;
                let mut instances = Vec::new();
                let mut instances_complete = true;
                let mut visited = intervening.iter();
                while let Some((_, native_instance, _)) =
                    ctx.next_charged(&mut visited, "scan SLDPRT sketch block instances")?
                {
                    if native_object_class(
                        native_instance.input_class.as_deref().unwrap_or_default(),
                    ) != NativeClassKind::SketchBlockInstance
                    {
                        continue;
                    }
                    let Some(instance_index) = first_model_feature(
                        ctx,
                        &features_by_native,
                        native_instance.id.as_str(),
                        |_| true,
                        "find SLDPRT sketch block instance feature",
                    )?
                    else {
                        instances_complete = false;
                        break;
                    };
                    let source_text = match ctx.get_btree_map(
                        &features[instance_index].source_properties,
                        "BlockDefinition",
                        "find SLDPRT sketch block source",
                    )? {
                        Some(source) => Some(source),
                        None => ctx.get_btree_map(
                            &native_instance.properties,
                            "BlockDefinition",
                            "find SLDPRT sketch block source",
                        )?,
                    };
                    let Some(source_text) = source_text else {
                        instances_complete = false;
                        break;
                    };
                    let Ok(block_source_number) =
                        ctx.parse_text::<u32>(source_text, "parse SLDPRT sketch block source")?
                    else {
                        instances_complete = false;
                        break;
                    };
                    if !ctx.contains_hash_set(
                        &children,
                        &block_source_number,
                        "find SLDPRT sketch block child",
                    )? {
                        instances_complete = false;
                        break;
                    }
                    let FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance {
                        block: Some(block),
                        placement: Some(transform),
                    }) = features[instance_index].evaluation.definition()
                    else {
                        instances_complete = false;
                        break;
                    };
                    let has_block_sketch = ctx.contains_key_hash_map(
                        &block_sketches,
                        &block_source_number,
                        "find SLDPRT sketch block source",
                    )?;
                    let matches_block = if has_block_sketch {
                        match ctx.get_hash_map(
                            &block_feature_ids,
                            &block_source_number,
                            "find SLDPRT sketch block feature",
                        )? {
                            Some(feature_id) => ctx.equal(
                                feature_id.as_str(),
                                block.as_str(),
                                "compare SLDPRT sketch block feature",
                            )?,
                            None => false,
                        }
                    } else {
                        false
                    };
                    if !has_block_sketch || !matches_block {
                        instances_complete = false;
                        break;
                    }
                    instances_storage.with_storage(|| {
                        let feature_id = ctx.copy_retained_text(
                            features[instance_index].id.as_str(),
                            "copy SLDPRT sketch block instance identity",
                        )?;
                        ctx.reserve_vec(
                            &mut instances,
                            1,
                            "collect SLDPRT sketch block instances",
                        )?;
                        instances.push(SketchBlockInstancePlacement {
                            feature_id,
                            block_source: block_source_number,
                            transform: *transform,
                        });
                        Ok::<(), CodecError>(())
                    })?;
                }
                if !instances_complete || instances.is_empty() {
                    continue;
                }

                let lane_key = ctx
                    .rsplit_once(&lane.id, "#", "resolve SLDPRT profiles keys")?
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let (sketch_id, sketch_id_storage) =
                    ctx.with_scoped_storage("build SLDPRT block profile identity", || {
                        let sketch_text = ctx.format_retained(
                            format_args!(
                                "sldprt:model:sketch#block-profile:{lane_key}:{}",
                                native_profile.ordinal
                            ),
                            "format SLDPRT sketch block profile identity",
                        )?;
                        ctx.charge_work(
                            u64_from_index(sketch_text.len()),
                            "validate SLDPRT sketch block profile identity",
                        )?;
                        Ok::<_, CodecError>(SketchId::mint(sketch_text))
                    })?;
                let Ok(sketch_id) = sketch_id else {
                    continue;
                };
                let Some(assembled) = assemble_sketch_block_profile(
                    ctx,
                    &SketchBlockProfileInput {
                        sketch_id: &sketch_id,
                        native_profile,
                        native_ref: &lane.id,
                        configuration: lane.configuration.as_deref(),
                        block_sketches: &block_sketches,
                        instances: &instances,
                        sketches,
                        sketch_positions: &sketch_positions,
                        sketch_entities,
                        entity_groups: &entity_groups,
                    },
                )?
                else {
                    continue;
                };
                if !ctx.contains_key_hash_map(
                    &sketch_positions,
                    sketch_id.as_str(),
                    "find SLDPRT assembled sketch",
                )? {
                    assembled.output_storage.commit()?;
                    let first_entity = sketch_entities.len();
                    ctx.extend_vec(
                        sketch_entities,
                        assembled.entities,
                        "append SLDPRT sketch block entities",
                    )?;
                    for position in ctx.admit_iter(
                        &(first_entity..sketch_entities.len()),
                        "append SLDPRT sketch block entities",
                    )? {
                        push_entity_group(
                            ctx,
                            &mut entity_groups,
                            &mut entity_groups_storage,
                            &sketch_entities[position],
                            position,
                            "append SLDPRT sketch block entities",
                        )?;
                    }
                    push_indexed_sketch(
                        ctx,
                        sketches,
                        &mut sketch_positions,
                        &mut sketch_positions_storage,
                        assembled.sketch,
                        "append SLDPRT assembled sketch block",
                    )?;
                }
                sketch_id_storage.commit()?;
                features[profile_index]
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }));
            }
        }
    }
    Ok(())
}

fn dissectable_child_sources(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<Option<HashSet<u32>>, CodecError> {
    const OPERATION: &str = "collect SLDPRT dissectable child sources";
    let (result, storage) = ctx.with_scoped_storage(OPERATION, || {
        let mut values = HashSet::new();
        let mut parts = 0_usize;
        let mut start = 0_usize;
        let mut visited = value
            .char_indices()
            .map(|(at, ch)| (at, Some(ch)))
            .chain(std::iter::once((value.len(), None)));
        while let Some((position, character)) = ctx.next_charged(&mut visited, OPERATION)? {
            if character.is_some_and(|character| character != ',') {
                continue;
            }
            let Some(part) = value.get(start..position) else {
                return Err(CodecError::malformed(
                    "invalid SLDPRT child source text boundary",
                ));
            };
            let Ok(source) = ctx.parse_text::<u32>(ctx.trim_text(part, OPERATION)?, OPERATION)?
            else {
                return Ok(None);
            };
            ctx.insert_hash_set(&mut values, source, OPERATION)?;
            parts += 1;
            start = position + 1;
        }
        if values.is_empty() || ctx.contains_hash_set(&values, &0, OPERATION)? {
            return Ok(None);
        }
        Ok::<_, CodecError>((values.len() == parts).then_some(values))
    })?;
    if result.is_some() {
        storage.commit()?;
    }
    Ok(result)
}

fn is_sketch_block_object(feature: &crate::records::Feature) -> bool {
    matches!(
        native_object_class(feature.input_class.as_deref().unwrap_or_default()),
        NativeClassKind::SketchBlockDefinition | NativeClassKind::SketchBlockInstance
    )
}

fn assemble_sketch_block_profile<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    input: &SketchBlockProfileInput<'_>,
) -> Result<Option<AssembledSketchBlockProfile<'ctx>>, CodecError> {
    let (assembled, output_storage) =
        ctx.with_scoped_storage("build SLDPRT sketch block output", || {
            let mut rotations_storage =
                ctx.reserve_scoped(0, "collect SLDPRT sketch block rotations")?;
            let Some((placement, rotations)) = rotations_storage
                .with_storage(|| sketch_block_assembly_frame(ctx, input.instances))?
            else {
                return Ok(None);
            };
            let mut assembled_profiles = Vec::new();
            let mut assembled_entities_storage =
                ctx.reserve_scoped(0, "collect SLDPRT assembled sketch block entities")?;
            let mut assembled_entities = Vec::new();
            let sketch_key = ctx
                .rsplit_once(
                    input.sketch_id.as_str(),
                    "#",
                    "read SLDPRT sketch identity key",
                )?
                .map_or(input.sketch_id.as_str(), |(_, key)| key);
            let mut visited = input.instances.iter().zip(&rotations);
            while let Some((instance, rotation)) =
                ctx.next_charged(&mut visited, "scan SLDPRT sketch block instances")?
            {
                let Some(source_sketch_id) = ctx.get_hash_map(
                    input.block_sketches,
                    &instance.block_source,
                    "resolve SLDPRT sketch block definition",
                )?
                else {
                    return Ok(None);
                };
                let Some(source_sketch) = ctx
                    .get_hash_map(
                        input.sketch_positions,
                        source_sketch_id.as_str(),
                        "find SLDPRT sketch block definition",
                    )?
                    .map(|position| &input.sketches[*position])
                else {
                    return Ok(None);
                };
                let mut source_entities_storage =
                    ctx.reserve_scoped(0, "collect SLDPRT sketch block source entities")?;
                let source_entities = source_entities_storage.with_storage(|| {
                    ctx.collect_vec(
                        owned_members(
                            ctx,
                            input.entity_groups,
                            source_sketch.id.as_str(),
                            "scan SLDPRT sketch block source entities",
                        )?
                        .iter()
                        .map(|position| &input.sketch_entities[*position]),
                        "collect SLDPRT sketch block source entities",
                    )
                })?;
                let instance_key = ctx
                    .rsplit_once(&instance.feature_id, "#", "read SLDPRT sketch identity key")?
                    .map_or(instance.feature_id.as_str(), |(_, key)| key);
                let mut entity_ids_storage =
                    ctx.reserve_scoped(0, "index SLDPRT sketch block entity identities")?;
                let mut entity_ids = HashMap::new();
                let mut visited = source_entities.iter();
                while let Some(entity) =
                    ctx.next_charged(&mut visited, "scan SLDPRT profiles records")?
                {
                    let inserted =
                        entity_ids_storage.with_storage(|| -> Result<bool, CodecError> {
                            let id_text = ctx.format_retained(
                                format_args!(
                        "sldprt:model:sketch-entity#{sketch_key}:instance:{instance_key}:entity:{}",
                        ctx.rsplit_once(
                            entity.id().as_str(),
                            "#",
                            "read SLDPRT sketch identity key"
                        )?
                        .map_or(entity.id().as_str(), |(_, key)| key)
                    ),
                                "format SLDPRT sketch block entity identity",
                            )?;
                            ctx.charge_work(
                                u64_from_index(id_text.len()),
                                "validate SLDPRT sketch block entity identity",
                            )?;
                            let Ok(id) = SketchEntityId::mint(id_text) else {
                                return Ok(false);
                            };
                            ctx.insert_hash_map(
                                &mut entity_ids,
                                entity.id(),
                                id,
                                "index SLDPRT sketch block entity identities",
                            )?;
                            Ok(true)
                        })?;
                    if !inserted {
                        return Ok(None);
                    }
                }
                let mut visited = source_entities.iter();
                while let Some(source_entity) =
                    ctx.next_charged(&mut visited, "scan SLDPRT profiles records")?
                {
                    let Some(id) = ctx.get_hash_map(
                        &entity_ids,
                        source_entity.id(),
                        "resolve SLDPRT profiles keys",
                    )?
                    else {
                        return Ok(None);
                    };
                    let id =
                        id.try_clone_for_decode(ctx, "copy SLDPRT sketch block entity identity")?;
                    let sketch_id = input
                        .sketch_id
                        .try_clone_for_decode(ctx, "copy SLDPRT sketch block sketch identity")?;
                    let Some(geometry) = transform_sketch_block_geometry(
                        ctx,
                        &source_entity.geometry,
                        instance.transform,
                        placement,
                        *rotation,
                    )?
                    else {
                        return Ok(None);
                    };
                    let native_ref = ctx.format_retained(
                        format_args!(
                            "{}:{}",
                            instance.feature_id,
                            source_entity
                                .native_ref
                                .as_deref()
                                .unwrap_or(source_entity.id().as_str())
                        ),
                        "format SLDPRT sketch block native reference",
                    )?;
                    let geometry_ref = match source_entity.geometry_ref.as_deref() {
                        Some(value) => Some(ctx.copy_retained_text(
                            value,
                            "copy SLDPRT sketch block geometry reference",
                        )?),
                        None => None,
                    };
                    let mut endpoint_refs = Vec::new();
                    for reference in ctx.admit_iter(
                        &(source_entity.endpoint_refs)[..],
                        "scan SLDPRT profiles records",
                    )? {
                        let reference = ctx.copy_retained_text(
                            reference,
                            "copy SLDPRT sketch block endpoint reference",
                        )?;
                        ctx.reserve_vec(
                            &mut endpoint_refs,
                            1,
                            "collect SLDPRT sketch block endpoint references",
                        )?;
                        endpoint_refs.push(reference);
                    }
                    assembled_entities_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut assembled_entities,
                            1,
                            "collect SLDPRT assembled sketch block entities",
                        )?;
                        assembled_entities.push(
                            SketchEntity::new(id, sketch_id, geometry)
                                .with_construction(source_entity.construction)
                                .with_native_ref(Some(native_ref))
                                .with_geometry_ref(geometry_ref)
                                .with_endpoint_refs(endpoint_refs),
                        );
                        Ok::<(), CodecError>(())
                    })?;
                }
                let mut recovered_profiles_storage =
                    ctx.reserve_scoped(0, "recover SLDPRT sketch block source profiles")?;
                let recovered_profiles = if source_sketch.profiles.is_empty() {
                    Some(recovered_profiles_storage.with_storage(|| {
                        closed_marker_profiles_allowing_shared_endpoints(ctx, &source_entities)
                    })?)
                } else {
                    None
                };
                let source_profiles = recovered_profiles
                    .as_deref()
                    .unwrap_or(source_sketch.profiles.as_slice());
                let mut visited_profiles = source_profiles.iter();
                while let Some(profile) =
                    ctx.next_charged(&mut visited_profiles, "scan SLDPRT sketch block profiles")?
                {
                    let mut assembled_profile = Vec::new();
                    let mut visited = profile.iter();
                    while let Some(use_) =
                        ctx.next_charged(&mut visited, "scan SLDPRT sketch block profile members")?
                    {
                        let Some(id) = ctx.get_hash_map(
                            &entity_ids,
                            &use_.entity,
                            "resolve SLDPRT profiles keys",
                        )?
                        else {
                            return Ok(None);
                        };
                        let id = id.try_clone_for_decode(
                            ctx,
                            "copy SLDPRT sketch block profile entity identity",
                        )?;
                        ctx.reserve_vec(
                            &mut assembled_profile,
                            1,
                            "collect SLDPRT assembled sketch block profile",
                        )?;
                        assembled_profile.push(SketchEntityUse {
                            entity: id,
                            reversed: use_.reversed,
                        });
                    }
                    ctx.reserve_vec(
                        &mut assembled_profiles,
                        1,
                        "collect SLDPRT assembled sketch block profiles",
                    )?;
                    assembled_profiles.push(assembled_profile);
                }
            }
            if assembled_profiles.is_empty() {
                return Ok(None);
            }
            let Some(placement) =
                SketchPlacement::try_resolved(placement.origin, placement.normal, placement.u_axis)
                    .ok()
            else {
                return Ok(None);
            };
            let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(assembled_profiles)
            else {
                return Ok(None);
            };
            let sketch_id = input
                .sketch_id
                .try_clone_for_decode(ctx, "copy SLDPRT assembled sketch block identity")?;
            let name = ctx.copy_retained_text(
                &input.native_profile.name,
                "copy SLDPRT assembled sketch block name",
            )?;
            let configuration = match input.configuration {
                Some(value) => Some(ctx.copy_retained_text(
                    value,
                    "copy SLDPRT assembled sketch block configuration",
                )?),
                None => None,
            };
            let native_ref = ctx.copy_retained_text(
                input.native_ref,
                "copy SLDPRT assembled sketch block native reference",
            )?;
            Ok::<_, CodecError>(Some((
                Sketch {
                    id: sketch_id,
                    name: Some(name),
                    configuration,
                    visible: None,
                    placement,
                    profiles,
                    native_ref: Some(native_ref),
                },
                assembled_entities,
                assembled_entities_storage,
            )))
        })?;
    Ok(assembled.map(
        |(sketch, entities, entities_storage)| AssembledSketchBlockProfile {
            sketch,
            entities,
            _entities_storage: entities_storage,
            output_storage,
        },
    ))
}

fn sketch_block_assembly_frame(
    ctx: &DecodeContext<'_>,
    instances: &[SketchBlockInstancePlacement],
) -> Result<Option<(SketchBlockAssemblyFrame, Vec<f64>)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage(
        "build SLDPRT profile candidate",
        || -> Result<_, CodecError> {
            const TOLERANCE: f64 = 1.0e-8;
            let Some(first) = instances.first().map(|instance| instance.transform) else {
                return Ok(None);
            };
            if !first.is_proper_rigid() {
                return Ok(None);
            }
            let Some((frame, v_axis)) = (|| {
                let origin = first.apply_point(Point3::new(0.0, 0.0, 0.0))?.get();
                let u_axis = first.apply_vector(Vector3::new(1.0, 0.0, 0.0))?.unit()?;
                let first_v = first.apply_vector(Vector3::new(0.0, 1.0, 0.0))?.unit()?;
                let normal = u_axis.cross(first_v).unit()?;
                let v_axis = normal.cross(u_axis).unit()?;
                Some((
                    SketchBlockAssemblyFrame {
                        origin,
                        normal,
                        u_axis,
                    },
                    v_axis,
                ))
            })() else {
                return Ok(None);
            };
            let SketchBlockAssemblyFrame {
                origin,
                normal,
                u_axis,
            } = frame;
            let mut rotations = Vec::new();
            let mut visited = instances.iter();
            while let Some(instance) =
                ctx.next_charged(&mut visited, "scan SLDPRT profiles records")?
            {
                let placement = instance.transform;
                if !placement.is_proper_rigid() {
                    return Ok(None);
                }
                let Some(rotation) = (|| {
                    let instance_origin = placement.apply_point(Point3::new(0.0, 0.0, 0.0))?.get();
                    let origin_delta = instance_origin.vector_from(origin);
                    if origin_delta.dot(normal).abs()
                        > TOLERANCE * (1.0 + origin.distance(Point3::new(0.0, 0.0, 0.0)))
                    {
                        return None;
                    }
                    let instance_u = placement
                        .apply_vector(Vector3::new(1.0, 0.0, 0.0))?
                        .unit()?;
                    let instance_v = placement
                        .apply_vector(Vector3::new(0.0, 1.0, 0.0))?
                        .unit()?;
                    if instance_u.cross(instance_v).dot(normal) < 1.0 - TOLERANCE {
                        return None;
                    }
                    let projected_u = Point2::new(instance_u.dot(u_axis), instance_u.dot(v_axis));
                    let projected_v = Point2::new(instance_v.dot(u_axis), instance_v.dot(v_axis));
                    if (projected_u.u * projected_u.u + projected_u.v * projected_u.v - 1.0).abs()
                        > TOLERANCE
                        || (projected_v.u * projected_v.u + projected_v.v * projected_v.v - 1.0)
                            .abs()
                            > TOLERANCE
                        || (projected_u.u * projected_v.v - projected_u.v * projected_v.u - 1.0)
                            .abs()
                            > TOLERANCE
                    {
                        return None;
                    }
                    Some(projected_u.v.atan2(projected_u.u))
                })() else {
                    return Ok(None);
                };
                ctx.reserve_vec(&mut rotations, 1, "collect SLDPRT sketch block rotations")?;
                rotations.push(rotation);
            }
            Ok(Some((frame, rotations)))
        },
    )?;
    if result.is_some() {
        storage.commit()?;
    }
    Ok(result)
}

fn transform_sketch_block_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
    transform: Transform,
    frame: SketchBlockAssemblyFrame,
    rotation: f64,
) -> Result<Option<SketchGeometry>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage(
        "build SLDPRT profile candidate",
        || -> Result<_, CodecError> {
            const OPERATION: &str = "transform SLDPRT sketch block geometry";
            let point = |point| transform_sketch_block_point(point, transform, frame);
            let finite_point = |value| {
                transform_sketch_block_point(value, transform, frame).and_then(FinitePoint2::new)
            };
            let direction = |direction| {
                transform_sketch_block_direction(direction, transform, frame)
                    .and_then(FinitePoint2::new)
            };
            let angle = |value: Angle| Angle::new(value.get() + rotation);
            match geometry.definition() {
                SketchGeometryDefinition::Nurbs { curve } => {
                    let mut copied = curve.try_clone_for_decode(ctx, OPERATION)?;
                    if copied
                        .try_map_control_points_in_place(
                            |pole| point(pole.get()).and_then(FinitePoint2::new).ok_or(()),
                            ctx,
                        )?
                        .is_err()
                    {
                        return Ok(None);
                    }
                    return Ok(Some(SketchGeometry::nurbs(copied)));
                }
                SketchGeometryDefinition::Text {
                    text,
                    font_family,
                    font_weight,
                    height,
                    width_factor,
                    placement,
                    horizontal_alignment,
                    vertical_alignment,
                } => {
                    let prepared = (|| {
                        Some((match placement {
                            Some(placement) => Some(cadmpeg_ir::sketches::TextPlacement {
                                anchor: finite_point(placement.anchor.get())?,
                                rotation: angle(placement.rotation)?,
                            }),
                            None => None,
                        },))
                    })();
                    let Some((placement,)) = prepared else {
                        return Ok(None);
                    };
                    return Ok(SketchGeometry::from_parts(SketchGeometryDefinition::Text {
                        text: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.copy_retained_text(text.as_str(), OPERATION)?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| CodecError::malformed("blank decoded sketch text"))?,
                        font_family: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.copy_retained_text(font_family.as_str(), OPERATION)?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| CodecError::malformed("blank decoded sketch font"))?,
                        font_weight: *font_weight,
                        height: *height,
                        width_factor: *width_factor,
                        placement,
                        horizontal_alignment: *horizontal_alignment,
                        vertical_alignment: *vertical_alignment,
                    })
                    .ok());
                }
                _ => {}
            }
            Ok((|| {
                Some(match geometry.definition() {
                    SketchGeometryDefinition::Point { position } => {
                        SketchGeometry::from_parts(SketchGeometryDefinition::Point {
                            position: finite_point(position.get())?,
                        })
                        .ok()?
                    }
                    SketchGeometryDefinition::Line { start, end } => {
                        SketchGeometry::from_parts(SketchGeometryDefinition::Line {
                            start: finite_point(start.get())?,
                            end: finite_point(end.get())?,
                        })
                        .ok()?
                    }
                    SketchGeometryDefinition::ReferenceLine {
                        origin,
                        direction: axis,
                    } => SketchGeometry::from_parts(SketchGeometryDefinition::ReferenceLine {
                        origin: finite_point(origin.get())?,
                        direction: direction(axis.get())?,
                    })
                    .ok()?,
                    SketchGeometryDefinition::Circle { center, radius } => {
                        SketchGeometry::from_parts(SketchGeometryDefinition::Circle {
                            center: finite_point(center.get())?,
                            radius: *radius,
                        })
                        .ok()?
                    }
                    SketchGeometryDefinition::Arc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                    } => SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
                        center: finite_point(center.get())?,
                        radius: *radius,
                        start_angle: angle(*start_angle)?,
                        end_angle: angle(*end_angle)?,
                    })
                    .ok()?,
                    SketchGeometryDefinition::Ellipse {
                        center,
                        major_angle,
                        radii,
                        bounds,
                    } => SketchGeometry::from_parts(SketchGeometryDefinition::Ellipse {
                        center: finite_point(center.get())?,
                        major_angle: angle(*major_angle)?,
                        radii: cadmpeg_ir::sketches::EllipseRadii {
                            major_radius: radii.major(),
                            minor_radius: radii.minor(),
                        },
                        bounds: match bounds {
                            Some([start, end]) => Some([angle(*start)?, angle(*end)?]),
                            None => None,
                        },
                    })
                    .ok()?,
                    SketchGeometryDefinition::Hyperbola {
                        center,
                        major_angle,
                        major_radius,
                        minor_radius,
                        bounds,
                    } => SketchGeometry::from_parts(SketchGeometryDefinition::Hyperbola {
                        center: finite_point(center.get())?,
                        major_angle: angle(*major_angle)?,
                        major_radius: *major_radius,
                        minor_radius: *minor_radius,
                        bounds: *bounds,
                    })
                    .ok()?,
                    SketchGeometryDefinition::Parabola {
                        vertex,
                        axis_angle,
                        focal_length,
                        bounds,
                    } => SketchGeometry::from_parts(SketchGeometryDefinition::Parabola {
                        vertex: finite_point(vertex.get())?,
                        axis_angle: angle(*axis_angle)?,
                        focal_length: *focal_length,
                        bounds: *bounds,
                    })
                    .ok()?,
                    SketchGeometryDefinition::Nurbs { .. }
                    | SketchGeometryDefinition::Text { .. }
                    | SketchGeometryDefinition::ExternalReference { .. }
                    | SketchGeometryDefinition::Native { .. } => return None,
                })
            })())
        },
    )?;
    if result.is_some() {
        storage.commit()?;
    }
    Ok(result)
}

fn transform_sketch_block_point(
    point: Point2,
    transform: Transform,
    frame: SketchBlockAssemblyFrame,
) -> Option<Point2> {
    const TOLERANCE: f64 = 1.0e-8;
    let transformed = transform.apply_point(Point3::new(point.u, point.v, 0.0))?;
    let delta = transformed.vector_from(frame.origin);
    (delta.dot(frame.normal).abs()
        <= TOLERANCE * (1.0 + frame.origin.distance(Point3::new(0.0, 0.0, 0.0))))
    .then(|| {
        Point2::new(
            delta.dot(frame.u_axis),
            delta.dot(frame.normal.cross(frame.u_axis)),
        )
    })
}

fn transform_sketch_block_direction(
    direction: Point2,
    transform: Transform,
    frame: SketchBlockAssemblyFrame,
) -> Option<Point2> {
    let transformed = transform.apply_vector(Vector3::new(direction.u, direction.v, 0.0))?;
    let v_axis = frame.normal.cross(frame.u_axis);
    let result = Point2::new(transformed.dot(frame.u_axis), transformed.dot(v_axis));
    result.is_finite().then_some(result)
}

fn project_detached_legacy_config_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    native_features: &BTreeMap<&str, &crate::records::Feature>,
    lanes: &[FeatureInputLane],
    feature_frames: &HashMap<String, (Point3, Vector3, Vector3)>,
) -> Result<(), CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    const OPERATION: &str = "project SLDPRT detached configuration sketches";

    for lane in ctx
        .admit_iter(lanes, OPERATION)?
        .filter(|lane| is_supplemental_config_lane(lane))
    {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "resolve SLDPRT profiles keys")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        let detached_frame = {
            let mut frames_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut frames = Vec::new();
            for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                let Some(feature) = marker.feature_ref.as_deref() else {
                    continue;
                };
                if let Some(frame) = ctx.get_hash_map(feature_frames, feature, OPERATION)? {
                    frames_storage.with_storage(|| ctx.push_vec(&mut frames, *frame, OPERATION))?;
                }
            }
            ctx.sort_unstable_by_key(
                &mut frames,
                reference_plane_frame_key,
                Ord::cmp,
                "sort SLDPRT legacy config sketch frames",
            )?;
            ctx.dedup_vec(&mut frames, OPERATION)?;
            let [frame] = frames.as_slice() else {
                continue;
            };
            *frame
        };
        let (markers_by_owner, _markers_by_owner_storage) = group_by_owner(
            ctx,
            &lane.sketch_entities,
            |marker| marker.feature_ref.as_deref(),
            OPERATION,
        )?;
        for feature_index in ctx.admit_iter(&(0..features.len()), OPERATION)? {
            let feature = &mut features[feature_index];
            'feature_edit: {
                let Some(native_ref) = feature.native_ref.as_deref() else {
                    break 'feature_edit;
                };
                if !matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                            | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                        ..
                    })
                ) {
                    break 'feature_edit;
                }
                let Some(native_feature) = ctx
                    .get_btree_map(native_features, native_ref, "resolve SLDPRT profiles keys")?
                    .copied()
                else {
                    break 'feature_edit;
                };
                let markers = owned_members(ctx, &markers_by_owner, native_ref, OPERATION)?;
                if markers.is_empty() {
                    break 'feature_edit;
                }
                let (origin, normal, u_axis) = ctx
                    .get_hash_map(feature_frames, native_ref, "resolve SLDPRT profiles keys")?
                    .copied()
                    .unwrap_or(detached_frame);
                let Ok(placement) =
                    cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
                else {
                    break 'feature_edit;
                };
                let (sketch, _base_storage) = ctx.with_scoped_storage(OPERATION, || {
                    let sketch_text = ctx.format_retained(
                        format_args!(
                            "sldprt:model:sketch#legacy-config:{lane_key}:{}",
                            native_feature.ordinal
                        ),
                        OPERATION,
                    )?;
                    ctx.charge_work(u64_from_index(sketch_text.len()), OPERATION)?;
                    let Ok(sketch_id) = SketchId::mint(sketch_text) else {
                        return Ok(None);
                    };
                    let sketch = Sketch {
                        id: sketch_id,
                        name: Some(ctx.copy_retained_text(&native_feature.name, OPERATION)?),
                        configuration: lane
                            .configuration
                            .as_deref()
                            .map(|text| ctx.copy_retained_text(text, OPERATION))
                            .transpose()?,
                        visible: None,
                        placement,
                        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                        native_ref: Some(ctx.copy_retained_text(&lane.id, OPERATION)?),
                    };
                    Ok::<_, CodecError>(Some(sketch))
                })?;
                let Some(sketch) = sketch else {
                    break 'feature_edit;
                };
                let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                    break 'feature_edit;
                };
                let project = |coordinates: [f64; 2]| {
                    let native = quantize(
                        Point2::new(coordinates[0] * NATIVE_TO_IR, coordinates[1] * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let point = transform.apply(native)?;
                    Some(Point2::new(
                        f64_from_i64(point.0)? * QUANTUM,
                        f64_from_i64(point.1)? * QUANTUM,
                    ))
                };

                let (entities_storage, projected, output_storage) = {
                    let mut hex_entities_storage = ctx.reserve_scoped(0, OPERATION)?;
                    let (projected, output_storage) = ctx.with_scoped_storage(OPERATION, || {
                        legacy_config_hex_sketch(
                            ctx,
                            native_feature,
                            &sketch,
                            markers,
                            &project,
                            &mut hex_entities_storage,
                        )
                    })?;
                    match projected {
                        Some(projected) => (hex_entities_storage, Some(projected), output_storage),
                        None => {
                            drop(hex_entities_storage);
                            drop(output_storage);
                            let mut collinear_entities_storage =
                                ctx.reserve_scoped(0, OPERATION)?;
                            let (projected, output_storage) =
                                ctx.with_scoped_storage(OPERATION, || {
                                    legacy_config_collinear_sketch(
                                        ctx,
                                        lane,
                                        native_feature,
                                        &sketch,
                                        markers,
                                        &project,
                                        &mut collinear_entities_storage,
                                    )
                                })?;
                            (collinear_entities_storage, projected, output_storage)
                        }
                    }
                };
                let Some((sketch, mut entities)) = projected else {
                    break 'feature_edit;
                };
                if ctx.any_by(
                    &entities,
                    |entity| {
                        Ok(matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Native { .. }
                        ))
                    },
                    OPERATION,
                )? {
                    break 'feature_edit;
                }
                let sketch_id = sketch.id.try_clone_for_decode(ctx, OPERATION)?;
                ctx.append_vec(sketch_entities, &mut entities, OPERATION)?;
                drop(entities);
                drop(entities_storage);
                ctx.reserve_vec(sketches, 1, OPERATION)?;
                sketches.push(sketch);
                output_storage.commit()?;
                feature
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }));
            }
        }
    }
    Ok(())
}

fn legacy_config_hex_sketch(
    ctx: &DecodeContext<'_>,
    native_feature: &crate::records::Feature,
    sketch: &Sketch,
    markers: &[&SketchInputEntity],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
    entities_storage: &mut ScopedReservation<'_>,
) -> Result<Option<(Sketch, Vec<SketchEntity>)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("build SLDPRT profile candidate", || -> Result<_, CodecError> {
    const OPERATION: &str = "project SLDPRT legacy hex sketch";
    // The located marker with each object index the grammar names, and the located marker with
    // no object index at the origin; `Err(())` once a slot holds two.
    const OBJECT_SLOTS: usize = 18;
    fn fill<'marker>(
        slot: &mut Option<Result<&'marker SketchInputEntity, ()>>,
        marker: &'marker SketchInputEntity,
    ) {
        *slot = Some(match slot {
            None => Ok(marker),
            Some(_) => Err(()),
        });
    }
    let mut objects: [Option<Result<&SketchInputEntity, ()>>; OBJECT_SLOTS] = [None; OBJECT_SLOTS];
    let mut unindexed_origin: Option<Result<&SketchInputEntity, ()>> = None;
    for marker in ctx.admit_iter(markers, OPERATION)?.copied() {
        let Some(coordinates) = marker.coordinates_m else {
            continue;
        };
        match marker.object_index() {
            Some(index) => {
                if let Some(slot) = usize::try_from(index)
                    .ok()
                    .and_then(|index| objects.get_mut(index))
                {
                    fill(slot, marker);
                }
            }
            None => {
                if same_dimension_length(coordinates[0], 0.0)
                    && same_dimension_length(coordinates[1], 0.0)
                {
                    fill(&mut unindexed_origin, marker);
                }
            }
        }
    }
    let mut curves_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut curves = curves_storage.with_storage(|| {
        let mut curves = Vec::new();
        for &marker in ctx.admit_iter(markers, OPERATION)? {
            if marker.coordinates_m.is_none() && marker.kind() == SketchInputKind::LineOrCircle {
                ctx.push_vec(&mut curves, marker, OPERATION)?;
            }
        }
        Ok::<_, CodecError>(curves)
    })?;
    ctx.sort_unstable_by_key(
        &mut curves,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT legacy hex sketch curves",
    )?;
    let prepared = (|| {
        let unique_object = |object_index: usize| objects[object_index]?.ok();
        let vertices = [
            unique_object(9)?,
            unique_object(10)?,
            unique_object(11)?,
            unique_object(12)?,
            unique_object(13)?,
            unique_object(14)?,
        ];
        let origin = unique_object(3).or_else(|| unindexed_origin?.ok())?;
        let horizontal = [unique_object(4)?, unique_object(5)?];
        let vertical = [unique_object(7)?, unique_object(8)?];
        let circle = unique_object(15)?;
        let circle_radial = unique_object(17)?;
        let construction_radial = unique_object(16)?;
        let [horizontal_curve, vertical_curve, construction_circle, line0, line1, line2, line3, line4, line5] =
            curves.as_slice()
        else {
            return None;
        };
        if [horizontal_curve, vertical_curve, construction_circle]
            .iter()
            .any(|marker| marker.offset() >= line0.offset())
            || vertices
                .windows(2)
                .any(|pair| pair[0].offset() >= pair[1].offset())
        {
            return None;
        }
        let point = |marker: &SketchInputEntity| project(marker.coordinates_m?.get());
        let center = point(circle)?;
        let circle_radius = {
            let radial = point(circle_radial)?;
            (radial.u - center.u).hypot(radial.v - center.v)
        };
        let construction_center = point(origin)?;
        let construction_radius = {
            let radial = point(construction_radial)?;
            (radial.u - construction_center.u).hypot(radial.v - construction_center.v)
        };
        if !circle_radius.is_finite()
            || circle_radius <= SKETCH_POINT_TOLERANCE
            || !construction_radius.is_finite()
            || construction_radius <= SKETCH_POINT_TOLERANCE
        {
            return None;
        }
        Some((
            vertices,
            origin,
            horizontal,
            vertical,
            circle,
            circle_radial,
            construction_radial,
            [
                *horizontal_curve,
                *vertical_curve,
                *construction_circle,
                *line0,
                *line1,
                *line2,
                *line3,
                *line4,
                *line5,
            ],
            center,
            construction_center,
            circle_radius,
            construction_radius,
        ))
    })();
    let Some((
        vertices,
        origin,
        horizontal,
        vertical,
        circle,
        circle_radial,
        construction_radial,
        [horizontal_curve, vertical_curve, construction_circle, line0, line1, line2, line3, line4, line5],
        center,
        construction_center,
        circle_radius,
        construction_radius,
    )) = prepared
    else {
        return Ok(None);
    };
    let point = |marker: &SketchInputEntity| project(marker.coordinates_m?.get());
    let sketch_key = ctx
        .rsplit_once(sketch.id.as_str(), "#", "resolve SLDPRT profiles keys")?
        .map_or(sketch.id.as_str(), |(_, key)| key);
    let entity_id = |kind: &str, index: usize| -> Result<Option<SketchEntityId>, CodecError> {
        let identity = ctx.format_retained(
            format_args!(
                "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
                native_feature.ordinal,
            ),
            OPERATION,
        )?;
        ctx.charge_work(u64_from_index(identity.len()), OPERATION)?;
        Ok(SketchEntityId::mint(identity).ok())
    };
    let mut entities = Vec::new();
    for (index, (curve, endpoints)) in [(horizontal_curve, horizontal), (vertical_curve, vertical)]
        .into_iter()
        .enumerate()
    {
        let Some(geometry) = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: point(endpoints[0])?,
                end: point(endpoints[1])?,
            })
            .ok()
        })() else {
            return Ok(None);
        };
        let Some(id) = entity_id("axis", index)? else {
            return Ok(None);
        };
        let entity = profile_entity(
            ctx,
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([endpoints[0].id(), endpoints[1].id()]),
            OPERATION,
        )?
        .with_construction(true);
        entities_storage.with_storage(|| ctx.push_vec(&mut entities, entity, OPERATION))?;
    }
    for (index, center, radius, curve, endpoints, construction) in [
        (
            0,
            center,
            circle_radius,
            circle,
            [circle, circle_radial],
            false,
        ),
        (
            1,
            construction_center,
            construction_radius,
            construction_circle,
            [origin, construction_radial],
            true,
        ),
    ] {
        let Some(geometry) = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center,
                radius: Length::new(radius)?,
            })
            .ok()
        })() else {
            return Ok(None);
        };
        let Some(id) = entity_id("circle", index)? else {
            return Ok(None);
        };
        let mut entity = profile_entity(
            ctx,
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([endpoints[0].id(), endpoints[1].id()]),
            OPERATION,
        )?;
        if construction {
            entity = entity.with_construction(true);
        }
        entities_storage.with_storage(|| ctx.push_vec(&mut entities, entity, OPERATION))?;
    }
    let mut outer_profile = Vec::new();
    for (index, curve) in [line0, line1, line2, line3, line4, line5]
        .into_iter()
        .enumerate()
    {
        let start = vertices[index];
        let end = vertices[(index + 1) % vertices.len()];
        let Some(id) = entity_id("profile", index)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut outer_profile, 1, OPERATION)?;
        outer_profile.push(SketchEntityUse {
            entity: id.try_clone_for_decode(ctx, OPERATION)?,
            reversed: false,
        });
        let Some(geometry) = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: point(start)?,
                end: point(end)?,
            })
            .ok()
        })() else {
            return Ok(None);
        };
        let entity = profile_entity(
            ctx,
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([start.id(), end.id()]),
            OPERATION,
        )?;
        entities_storage.with_storage(|| ctx.push_vec(&mut entities, entity, OPERATION))?;
    }
    let mut copied = copy_profile_sketch(ctx, sketch, OPERATION)?;
    let Some(circle_id) = entity_id("circle", 0)? else {
        return Ok(None);
    };
    let mut circle_profile = Vec::new();
    ctx.reserve_vec(&mut circle_profile, 1, OPERATION)?;
    circle_profile.push(SketchEntityUse {
        entity: circle_id,
        reversed: false,
    });
    let mut profiles = Vec::new();
    ctx.reserve_vec(&mut profiles, 2, OPERATION)?;
    profiles.push(outer_profile);
    profiles.push(circle_profile);
    let Ok(profiles) = profiles.try_into() else {
        return Ok(None);
    };
    copied.profiles = profiles;
    Ok(Some((copied, entities)))
    })?;
    if result.is_some() {
        storage.commit()?;
    }
    Ok(result)
}

fn legacy_config_collinear_sketch(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    native_feature: &crate::records::Feature,
    sketch: &Sketch,
    markers: &[&SketchInputEntity],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
    entities_storage: &mut ScopedReservation<'_>,
) -> Result<Option<(Sketch, Vec<SketchEntity>)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage(
        "build SLDPRT profile candidate",
        || -> Result<_, CodecError> {
            const OPERATION: &str = "project SLDPRT legacy collinear sketch";
            let mut curves_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut curves = curves_storage.with_storage(|| {
                let mut curves = Vec::new();
                for &marker in ctx.admit_iter(markers, OPERATION)? {
                    if marker.coordinates_m.is_none()
                        && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        ctx.push_vec(&mut curves, marker, OPERATION)?;
                    }
                }
                Ok::<_, CodecError>(curves)
            })?;
            ctx.sort_unstable_by_key(
                &mut curves,
                |value| value.offset(),
                Ord::cmp,
                "sort SLDPRT legacy collinear sketch curves",
            )?;
            let prepared = (|| {
                let [negative_curve, first_curve, second_curve, third_curve] = curves.as_slice()
                else {
                    return None;
                };
                let negative_offset = usize::try_from(negative_curve.offset()).ok()?;
                if lane
                    .native_payload
                    .get(negative_offset + 56..negative_offset + 58)
                    != Some(&[0x1e, 0x00])
                    || lane
                        .native_payload
                        .get(negative_offset + 66..negative_offset + 74)
                        != Some(&0.0f64.to_le_bytes())
                {
                    return None;
                }
                let negative_u = View::f64_le_at(&lane.native_payload, negative_offset + 58)?;
                if !negative_u.is_finite() || negative_u >= 0.0 {
                    return None;
                }
                Some((
                    negative_u,
                    [*negative_curve, *first_curve, *second_curve, *third_curve],
                ))
            })();
            let Some((negative_u, line_curves)) = prepared else {
                return Ok(None);
            };
            let mut chain_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut chain = chain_storage.with_storage(|| {
                let mut chain = Vec::new();
                for &marker in ctx.admit_iter(markers, OPERATION)? {
                    if !matches!(marker.object_index(), Some(18 | 19 | 21)) {
                        continue;
                    }
                    let Some(coordinates) = marker.coordinates_m else {
                        continue;
                    };
                    let ordinal = chain.len();
                    ctx.push_vec(&mut chain, (marker, coordinates.get(), ordinal), OPERATION)?;
                }
                Ok::<_, CodecError>(chain)
            })?;
            let Some(origin) = ctx
                .admit_iter(markers, OPERATION)?
                .copied()
                .filter(|marker| marker.object_index().is_none())
                .filter_map(|marker| Some((marker, marker.coordinates_m?.get())))
                .min_by_key(|(marker, _)| marker.offset())
            else {
                return Ok(None);
            };
            let ordinal = chain.len();
            chain_storage.with_storage(|| {
                ctx.push_vec(&mut chain, (origin.0, origin.1, ordinal), OPERATION)
            })?;
            ctx.sort_unstable_by_key(
                &mut chain,
                |value| (value.1[0], value.2),
                |left, right| {
                    left.0
                        .total_cmp(&right.0)
                        .then_with(|| left.1.cmp(&right.1))
                },
                "sort SLDPRT legacy collinear sketch chain",
            )?;
            ctx.dedup_by(
                &mut chain,
                |left, right| {
                    Ok(same_dimension_length(left.1[0], right.1[0])
                        && same_dimension_length(left.1[1], right.1[1]))
                },
                "deduplicate SLDPRT legacy collinear sketch chain",
            )?;
            if chain.len() != 4
                || ctx.any_by(
                    &chain,
                    |value| Ok(!same_dimension_length(value.1[1], origin.1[1]) || value.1[0] < 0.0),
                    OPERATION,
                )?
            {
                return Ok(None);
            }
            let negative = [negative_u, origin.1[1]];
            let sketch_key = ctx
                .rsplit_once(sketch.id.as_str(), "#", "resolve SLDPRT profiles keys")?
                .map_or(sketch.id.as_str(), |(_, key)| key);
            let entity_id =
                |kind: &str, index: usize| -> Result<Option<SketchEntityId>, CodecError> {
                    let identity = ctx.format_retained(
                        format_args!(
                "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
                native_feature.ordinal,
            ),
                        OPERATION,
                    )?;
                    ctx.charge_work(u64_from_index(identity.len()), OPERATION)?;
                    Ok(SketchEntityId::mint(identity).ok())
                };
            let segments = [
                (negative, origin.1),
                (chain[0].1, chain[1].1),
                (chain[1].1, chain[2].1),
                (chain[2].1, chain[3].1),
            ];
            let mut entities = Vec::new();
            for (index, (curve, (start, end))) in line_curves.into_iter().zip(segments).enumerate()
            {
                let Some(id) = entity_id("line", index)? else {
                    return Ok(None);
                };
                let Some(geometry) = (|| {
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: project(start)?,
                        end: project(end)?,
                    })
                    .ok()
                })() else {
                    return Ok(None);
                };
                let entity = profile_entity(
                    ctx,
                    id,
                    &sketch.id,
                    geometry,
                    Some(curve.id()),
                    None,
                    OPERATION,
                )?;
                entities_storage.with_storage(|| ctx.push_vec(&mut entities, entity, OPERATION))?;
            }
            let mut points_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut points = points_storage.with_storage(|| {
                let mut points = Vec::new();
                for &marker in ctx.admit_iter(markers, OPERATION)? {
                    let Some(coordinates) = marker.coordinates_m else {
                        continue;
                    };
                    let ordinal = points.len();
                    ctx.push_vec(
                        &mut points,
                        (Some(marker), coordinates.get(), ordinal),
                        OPERATION,
                    )?;
                }
                let ordinal = points.len();
                ctx.push_vec(&mut points, (None, negative, ordinal), OPERATION)?;
                Ok::<_, CodecError>(points)
            })?;
            ctx.sort_unstable_by_key(
                &mut points,
                |value| (value.1[0], value.1[1], value.2),
                |left, right| {
                    left.0
                        .total_cmp(&right.0)
                        .then_with(|| left.1.total_cmp(&right.1))
                        .then_with(|| left.2.cmp(&right.2))
                },
                "sort SLDPRT legacy collinear sketch points",
            )?;
            ctx.dedup_by(
                &mut points,
                |left, right| {
                    Ok(same_dimension_length(left.1[0], right.1[0])
                        && same_dimension_length(left.1[1], right.1[1]))
                },
                "deduplicate SLDPRT legacy collinear sketch points",
            )?;
            for (index, point) in ctx.admit_iter(&points[..], OPERATION)?.enumerate() {
                let Some(id) = entity_id("point", index)? else {
                    return Ok(None);
                };
                let Some(geometry) = (|| {
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: project(point.1)?,
                    })
                    .ok()
                })() else {
                    return Ok(None);
                };
                let entity = profile_entity(
                    ctx,
                    id,
                    &sketch.id,
                    geometry,
                    point.0.map(SketchInputEntity::id),
                    None,
                    OPERATION,
                )?;
                entities_storage.with_storage(|| ctx.push_vec(&mut entities, entity, OPERATION))?;
            }
            Ok(Some((
                copy_profile_sketch(ctx, sketch, OPERATION)?,
                entities,
            )))
        },
    )?;
    if result.is_some() {
        storage.commit()?;
    }
    Ok(result)
}

#[cfg(test)]
mod detached_legacy_sketch_tests {
    use super::super::bindings::bind_detached_legacy_sketch_objects;
    use super::{
        assemble_sketch_block_profile, index_entity_groups, index_sketch_ids,
        legacy_config_collinear_sketch, legacy_config_hex_sketch, project_marker_backed_sketches,
        project_sketch_block_profiles, sketch_block_assembly_frame, SketchBlockInstancePlacement,
        SketchBlockProfileInput, TerminalCarriers,
    };
    use crate::layout::current_terminal_relation_carrier as terminal;
    use crate::records::FeatureInputLane;
    use crate::records::FeatureSource;
    use crate::records::ObjectId;
    use crate::records::SketchInputEntity;
    use crate::records::SketchInputKind;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputRelationFamily,
        FeatureInputRelationInstance,
    };
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::Sketch;
    use cadmpeg_ir::sketches::SketchEntity;
    use cadmpeg_ir::sketches::SketchEntityId;
    use cadmpeg_ir::sketches::SketchEntityUse;
    use cadmpeg_ir::sketches::SketchGeometry;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    use cadmpeg_ir::sketches::SketchId;
    use cadmpeg_ir::sketches::SketchPlacement;
    use cadmpeg_ir::transform::Transform;
    use std::collections::BTreeMap;
    use std::collections::HashMap;
    use std::collections::HashSet;

    fn feature() -> Feature {
        Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: FeatureSource::from_value(30),
            ordinal: 30,
            name: "profile".into(),
            kind: "ProfileFeature".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    fn sketch() -> Sketch {
        Sketch {
            id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
            name: Some("profile".into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some("lane".into()),
        }
    }

    fn profile_binding_error(
        policy: cadmpeg_core::decode::DecodePolicy,
    ) -> cadmpeg_core::CodecError {
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: vec![0; 32],
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: ObjectId::from_value(30),
                value: "profile".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let feature = cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#profile-feature").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            native_ref: Some("feature".into()),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                }),
            ),
        };
        let sketch = sketch();
        let mut builder = cadmpeg_ir::AnnotationBuilder::new();
        let stream = cadmpeg_ir::annotations::StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            cadmpeg_ir::stream_name!("test:profile"),
            "fixture stream handle",
        )
        .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                sketch.id.as_str(),
                &stream,
                1,
                Some("profile"),
            )
            .unwrap();
        let annotations = builder.build();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (service, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut admitted = [feature.clone()];
        super::bind_sketch_profiles(
            &service,
            &mut admitted,
            super::SketchArenas {
                sketches: &mut vec![sketch.clone()],
                sketch_entities: &mut Vec::new(),
                sketch_constraints: &mut Vec::new(),
                annotations: &mut annotations.clone(),
            },
            &[],
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
        .unwrap();
        assert!(matches!(admitted[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)),
            }) if id == &sketch.id));
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &policy,
        )
        .unwrap();
        super::bind_sketch_profiles(
            &ctx,
            &mut [feature],
            super::SketchArenas {
                sketches: &mut vec![sketch],
                sketch_entities: &mut Vec::new(),
                sketch_constraints: &mut Vec::new(),
                annotations: &mut annotations.clone(),
            },
            &[],
            &[history],
            std::slice::from_ref(&lane),
        )
        .unwrap_err()
    }

    #[test]
    fn sketch_profile_binding_refuses_collection_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(
            matches!(profile_binding_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn sketch_profile_binding_refuses_retained_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        assert!(
            matches!(profile_binding_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn sketch_profile_binding_refuses_work_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        assert!(
            matches!(profile_binding_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    pub(super) fn compact_profile_projection_fixture() -> (
        FeatureHistory,
        FeatureInputLane,
        cadmpeg_ir::features::Feature,
    ) {
        use crate::layout::constructed_reference_plane_matrix_frame;
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        };
        let mut payload = vec![0; constructed_reference_plane_matrix_frame::LEN];
        payload[constructed_reference_plane_matrix_frame::NORMAL + 16
            ..constructed_reference_plane_matrix_frame::NORMAL + 24]
            .copy_from_slice(&1.0f64.to_le_bytes());
        payload[constructed_reference_plane_matrix_frame::FRAME_MARKER] = 1;
        for (index, value) in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .enumerate()
        {
            let offset = constructed_reference_plane_matrix_frame::BASIS_MATRIX + index * 8;
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        payload.extend(4u16.to_le_bytes());
        for address in [3u32, 2, 1, 4] {
            payload.extend(address.to_le_bytes());
        }
        payload.extend(1u32.to_le_bytes());
        payload.extend(0u16.to_le_bytes());
        payload.extend(6u32.to_le_bytes());
        payload.extend([0xff; 4]);
        payload.extend([0; 8]);
        payload.extend(5u32.to_le_bytes());
        payload.extend(5u32.to_le_bytes());
        payload.extend([0xff, 0xfe, 0xff, 0, 0, 0]);
        payload.extend([0xff; 4]);
        let lane = FeatureInputLane {
            id: "lane#1".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: "lane#1".into(),
                ordinal: 0,
                offset: 0,
                object_id: ObjectId::from_value(30),
                value: "profile".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: [[0.0, 0.0], [0.001, 0.0], [0.001, 0.001], [0.0, 0.001]]
                .into_iter()
                .enumerate()
                .map(|(index, point)| {
                    marker(
                        u32::try_from(index).unwrap(),
                        None,
                        SketchInputKind::Point,
                        Some(point),
                    )
                })
                .collect(),
        };
        let feature = cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#profile-feature").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            native_ref: Some("feature".into()),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                }),
            ),
        };
        (history, lane, feature)
    }

    fn compact_profile_projection_error(
        policy: cadmpeg_core::decode::DecodePolicy,
    ) -> cadmpeg_core::CodecError {
        let (history, lane, feature) = compact_profile_projection_fixture();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (service, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut sketches = Vec::new();
        let mut entities = Vec::new();
        let mut losses = Vec::new();
        let mut admitted = [feature.clone()];
        super::project_compact_sketch_profiles(
            &service,
            &mut admitted,
            &mut sketches,
            &mut entities,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
            &mut losses,
        )
        .unwrap();
        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].profiles.len(), 1);
        assert_eq!(sketches[0].profiles[0].len(), 4);
        assert_eq!(entities.len(), 4);
        assert!(losses.is_empty());
        assert!(matches!(admitted[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)),
            }) if id == &sketches[0].id));
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &policy,
        )
        .unwrap();
        super::project_compact_sketch_profiles(
            &ctx,
            &mut [feature],
            &mut Vec::new(),
            &mut Vec::new(),
            &[history],
            std::slice::from_ref(&lane),
            &mut Vec::new(),
        )
        .unwrap_err()
    }

    #[test]
    fn compact_profile_projection_refuses_collection_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(
            matches!(compact_profile_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn compact_profile_projection_refuses_retained_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        assert!(
            matches!(compact_profile_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn compact_profile_projection_refuses_work_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        assert!(
            matches!(compact_profile_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    fn marker(
        ordinal: u32,
        object_index: Option<u32>,
        kind: SketchInputKind,
        coordinates_m: Option<[f64; 2]>,
    ) -> SketchInputEntity {
        {
            let marker_id: String = format!("marker-{ordinal}");
            let marker_parent: String = "sldprt:feature-input:config-objects#1".into();
            let mut constructed_marker = crate::records::SketchInputEntity::new(
                marker_id,
                marker_parent,
                ordinal,
                u64::from(ordinal) * 100,
                kind,
            );
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(object_index, None);
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m =
                coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
            constructed_marker.links = None;
            constructed_marker
        }
    }

    fn current_terminal_relation_payload() -> Vec<u8> {
        const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
        const CLASS: &[u8] = b"sgCircleDim";
        let mut payload = vec![0; terminal::LEN];
        payload[terminal::MARKER..terminal::MARKER + super::super::SKETCH_MARKER.len()]
            .copy_from_slice(super::super::SKETCH_MARKER);
        payload[terminal::NATIVE_KIND..terminal::NATIVE_KIND + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        payload[terminal::GEOMETRY_LOCUS..terminal::GEOMETRY_LOCUS + 4]
            .copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[terminal::ROLE..terminal::ROLE + 2].copy_from_slice(&1u16.to_le_bytes());
        payload[terminal::STATE..terminal::STATE + 2].copy_from_slice(&1u16.to_le_bytes());
        payload[terminal::SELECTOR..terminal::SELECTOR + 8]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[terminal::STATE_VALUE..terminal::STATE_VALUE + 8]
            .copy_from_slice(&1.0f64.to_le_bytes());
        payload[terminal::TERMINAL_HEADER..terminal::TERMINAL_HEADER + 4]
            .copy_from_slice(&[1, 0, 1, 0]);
        payload[terminal::ENDPOINT_SELECTOR..terminal::ENDPOINT_SELECTOR + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        payload[terminal::SIGNED_SELECTOR..terminal::SIGNED_SELECTOR + 8]
            .copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[terminal::TERMINAL_SELECTOR..terminal::TERMINAL_SELECTOR + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        for relative in
            (terminal::REFERENCE_SENTINELS..terminal::REFERENCE_SENTINELS + 16).step_by(4)
        {
            payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
        }
        payload[terminal::TERMINAL_TAG..terminal::TERMINAL_TAG + 2]
            .copy_from_slice(&3u16.to_le_bytes());
        let class_offset = payload.len();
        payload.resize(class_offset + 6 + CLASS.len(), 0);
        payload[class_offset..class_offset + CLASS_MARKER.len()].copy_from_slice(CLASS_MARKER);
        payload[class_offset + 4..class_offset + 6]
            .copy_from_slice(&u16::try_from(CLASS.len()).unwrap().to_le_bytes());
        payload[class_offset + 6..class_offset + 6 + CLASS.len()].copy_from_slice(CLASS);
        payload
    }

    #[test]
    fn terminal_relation_display_carrier_requires_same_feature_and_class() {
        let lane_id = "lane";
        let feature_id = "feature";
        let class_id = "class";
        let mut carrier = marker(0, None, SketchInputKind::LineOrCircle, None);
        carrier = carrier.with_test_position(carrier.ordinal(), 0);
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: current_terminal_relation_payload(),
            classes: vec![FeatureInputClass {
                id: class_id.into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: cadmpeg_core::decode::u64_from_index(terminal::LEN),
                name: "sgCircleDim".into(),
            }],
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: vec![FeatureInputRelationInstance {
                id: "relation".into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 200,
                family: FeatureInputRelationFamily::CircleDiameter,
                class_ref: class_id.into(),
                feature_ref: feature_id.into(),
                scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                    vec!["sldprt:test:scalar#unselected-1".into()],
                    None,
                    None,
                )
                .unwrap(),
                operands: Vec::new(),
            }],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        let ctx = cadmpeg_test_support::service_decode_context();
        let carries = |lane: &FeatureInputLane| {
            TerminalCarriers::new(&ctx, lane)
                .and_then(|carriers| carriers.carries(&ctx, lane, &carrier))
                .unwrap()
        };
        assert!(carries(&lane));

        let mut wrong_feature = lane.relation_instances[0].clone();
        wrong_feature.feature_ref = "other-feature".into();
        let mut wrong_feature_lane = lane.clone();
        wrong_feature_lane.relation_instances = vec![wrong_feature];
        assert!(!carries(&wrong_feature_lane));

        let mut wrong_class = lane.relation_instances[0].clone();
        wrong_class.class_ref = "other-class".into();
        let mut wrong_class_lane = lane;
        wrong_class_lane.relation_instances = vec![wrong_class];
        assert!(!carries(&wrong_class_lane));
    }

    #[test]
    fn detached_object_without_legacy_dimension_handle_binds_to_unique_sketch() {
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        };
        let mut detached = marker(1, Some(1), SketchInputKind::Point, Some([1.0, 2.0]));
        detached.feature_ref = None;
        detached = detached.with_test_position(detached.ordinal(), 100);
        let mut lane = FeatureInputLane {
            id: "sldprt:feature-input:config-objects#1".into(),
            configuration: None,
            native_payload: vec![0; 512],
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![detached],
        };

        bind_detached_legacy_sketch_objects(
            &cadmpeg_test_support::service_decode_context(),
            std::slice::from_ref(&history),
            &HashSet::new(),
            &mut lane,
        )
        .unwrap();

        assert_eq!(
            lane.sketch_entities[0].feature_ref.as_deref(),
            Some("feature")
        );
    }

    #[test]
    fn empty_named_sketch_is_projected_without_geometry_markers() {
        let mut native_feature = feature();
        native_feature.kind = "Sketch".into();
        native_feature.input_class = Some("moProfileFeature_c".into());
        native_feature.name = "empty".into();
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native_feature],
        };
        let lane_id = "sldprt:feature-input:resolved-features#1";
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: vec![0; 64],
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 8,
                object_id: ObjectId::from_value(30),
                value: "empty".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let expected_sketch = SketchId::mint("sldprt:model:sketch#markers:1:30").unwrap();
        let mut neutral_feature = cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#neutral")
                .expect("identity grammar"),
            ordinal: 30,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }),
            ),
            native_ref: None,
        };
        neutral_feature.name = Some("empty".into());
        neutral_feature.native_ref = Some("feature".into());
        let mut features = vec![neutral_feature];
        let mut sketches = Vec::new();
        let mut sketch_entities = Vec::new();

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("profile fixture fits service policy");
        project_marker_backed_sketches(
            &ctx,
            &mut features,
            &mut sketches,
            &mut sketch_entities,
            &[history],
            &[lane],
        )
        .expect("marker profile projection fits service policy");

        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].id, expected_sketch);
        assert_eq!(
            sketches[0].profiles.as_slice(),
            Vec::<Vec<SketchEntityUse>>::new()
        );
        assert_eq!(sketches[0].placement, SketchPlacement::Unresolved {});
        assert!(sketch_entities.is_empty());
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                ..
            }) if sketch == &expected_sketch
        ));
    }

    #[test]
    fn unbound_marker_keeps_empty_sketch_unresolved() {
        let mut native_feature = feature();
        native_feature.kind = "Sketch".into();
        native_feature.input_class = Some("moProfileFeature_c".into());
        native_feature.name = "empty".into();
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native_feature],
        };
        let lane_id = "sldprt:feature-input:resolved-features#1";
        let mut unbound = marker(0, Some(1), SketchInputKind::Point, Some([0.0, 0.0]));
        unbound.feature_ref = None;
        unbound = unbound.with_test_position(unbound.ordinal(), 20);
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: vec![0; 64],
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 8,
                object_id: ObjectId::from_value(30),
                value: "empty".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![unbound],
        };
        let mut neutral_feature = cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#neutral")
                .expect("identity grammar"),
            ordinal: 30,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }),
            ),
            native_ref: None,
        };
        neutral_feature.name = Some("empty".into());
        neutral_feature.native_ref = Some("feature".into());
        let mut features = vec![neutral_feature];
        let mut sketches = Vec::new();
        let mut sketch_entities = Vec::new();

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("profile fixture fits service policy");
        project_marker_backed_sketches(
            &ctx,
            &mut features,
            &mut sketches,
            &mut sketch_entities,
            &[history],
            &[lane],
        )
        .expect("marker profile projection fits service policy");

        assert!(sketches.is_empty());
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                ..
            })
        ));
    }

    #[test]
    fn hex_profile_accepts_explicit_and_omitted_origin_indices() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        for origin_index in [Some(3), None] {
            let mut markers = vec![
                marker(0, origin_index, SketchInputKind::Point, Some([0.0, 0.0])),
                marker(1, Some(4), SketchInputKind::Point, Some([-2.0, 0.0])),
                marker(2, Some(5), SketchInputKind::Point, Some([2.0, 0.0])),
                marker(3, Some(7), SketchInputKind::Point, Some([0.0, -2.0])),
                marker(4, Some(8), SketchInputKind::Point, Some([0.0, 2.0])),
            ];
            for (index, coordinates) in [
                [1.0, 0.0],
                [0.5, 0.866],
                [-0.5, 0.866],
                [-1.0, 0.0],
                [-0.5, -0.866],
                [0.5, -0.866],
            ]
            .into_iter()
            .enumerate()
            {
                markers.push(marker(
                    5 + u32::try_from(index).unwrap(),
                    Some(9 + u32::try_from(index).unwrap()),
                    SketchInputKind::Point,
                    Some(coordinates),
                ));
            }
            markers.extend([
                marker(11, Some(15), SketchInputKind::Arc, Some([0.0, 0.0])),
                marker(12, Some(16), SketchInputKind::Point, Some([1.5, 0.0])),
                marker(13, Some(17), SketchInputKind::Point, Some([0.5, 0.0])),
            ]);
            for ordinal in 14..23 {
                markers.push(marker(ordinal, None, SketchInputKind::LineOrCircle, None));
            }
            let refs = markers.iter().collect::<Vec<_>>();
            let mut entities_storage = ctx
                .reserve_scoped(0, "project SLDPRT legacy hex sketch")
                .unwrap();
            let (projected, entities) = legacy_config_hex_sketch(
                &ctx,
                &feature(),
                &sketch(),
                &refs,
                &|coordinates| Some(Point2::new(coordinates[0], coordinates[1])),
                &mut entities_storage,
            )
            .unwrap()
            .expect("exact hex grammar");

            assert_eq!(
                projected.profiles.iter().map(Vec::len).collect::<Vec<_>>(),
                [6, 1]
            );
            assert_eq!(entities.len(), 10);
            assert_eq!(
                entities.iter().filter(|entity| entity.construction).count(),
                3
            );
            assert_eq!(
                entities
                    .iter()
                    .filter(|entity| matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Line { .. }
                    ))
                    .count(),
                8
            );
            assert_eq!(
                entities
                    .iter()
                    .filter(|entity| matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Circle { .. }
                    ))
                    .count(),
                2
            );
        }
    }

    #[test]
    fn collinear_dimension_grammar_projects_four_lines_and_unique_points() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut payload = vec![0; 400];
        payload[56..58].copy_from_slice(&[0x1e, 0x00]);
        payload[58..66].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[66..74].copy_from_slice(&0.0f64.to_le_bytes());
        let mut markers = (0..4)
            .map(|ordinal| marker(ordinal, None, SketchInputKind::LineOrCircle, None))
            .collect::<Vec<_>>();
        markers.extend([
            marker(4, None, SketchInputKind::Point, Some([0.0, 0.0])),
            marker(5, Some(18), SketchInputKind::Point, Some([1.0, 0.0])),
            marker(6, Some(19), SketchInputKind::Point, Some([2.0, 0.0])),
            marker(7, Some(21), SketchInputKind::Point, Some([3.0, 0.0])),
        ]);
        for ordinal in 8..14 {
            markers.push(marker(
                ordinal,
                Some(ordinal + 20),
                SketchInputKind::Point,
                Some([f64::from(ordinal), 1.0]),
            ));
        }
        let lane = FeatureInputLane {
            id: "sldprt:feature-input:config-objects#1".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: markers,
        };
        let refs = lane.sketch_entities.iter().collect::<Vec<_>>();
        let mut entities_storage = ctx
            .reserve_scoped(0, "project SLDPRT legacy collinear sketch")
            .unwrap();
        let (_, entities) = legacy_config_collinear_sketch(
            &ctx,
            &lane,
            &feature(),
            &sketch(),
            &refs,
            &|coordinates| Some(Point2::new(coordinates[0], coordinates[1])),
            &mut entities_storage,
        )
        .unwrap()
        .expect("exact collinear grammar");

        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                ))
                .count(),
            4
        );
        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                ))
                .count(),
            11
        );
    }

    #[test]
    fn sketch_block_rotations_refuse_collection_limit() {
        let instances = [SketchBlockInstancePlacement {
            feature_id: "sldprt:model:feature#instance".into(),
            block_source: 23,
            transform: Transform::identity(),
        }];
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = sketch_block_assembly_frame(&ctx, &instances)
            .err()
            .expect("rotation collection must refuse the configured limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT sketch block rotations")
        );
    }

    #[test]
    fn sketch_block_projection_refuses_collection_limit() {
        let native_feature = feature();
        let mut block_definition = feature();
        block_definition.id = "block-definition".into();
        block_definition.input_class = Some("moSketchBlockDef_c".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native_feature, block_definition],
        };
        let lane_id = "sldprt:feature-input:resolved-features#1";
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 8,
                object_id: ObjectId::from_value(30),
                value: "profile".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project_sketch_block_profiles(
            &ctx,
            &mut [],
            &mut Vec::new(),
            &mut Vec::new(),
            &[history],
            &[lane],
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "project SLDPRT sketch block profiles")
        );
    }

    #[test]
    fn marker_profile_projection_refuses_work_limit() {
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (service, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        project_marker_backed_sketches(
            &service,
            &mut [],
            &mut Vec::new(),
            &mut Vec::new(),
            std::slice::from_ref(&history),
            &[],
        )
        .unwrap();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project_marker_backed_sketches(
            &ctx,
            &mut [],
            &mut Vec::new(),
            &mut Vec::new(),
            &[history],
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn marker_profile_projection_refuses_collection_limit() {
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        };
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project_marker_backed_sketches(
            &ctx,
            &mut [],
            &mut Vec::new(),
            &mut Vec::new(),
            &[history],
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "index SLDPRT marker profile native features")
        );
    }

    #[test]
    fn marker_profile_projection_refuses_retained_limit() {
        let mut native_feature = feature();
        native_feature.kind = "Sketch".into();
        native_feature.input_class = Some("moProfileFeature_c".into());
        native_feature.name = "empty".into();
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native_feature],
        };
        let lane_id = "sldprt:feature-input:resolved-features#1";
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: vec![0; 64],
            classes: Vec::new(),
            names: vec![crate::records::FeatureInputName {
                id: "name".into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: 8,
                object_id: ObjectId::from_value(30),
                value: "empty".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let neutral_feature = cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#neutral").unwrap(),
            ordinal: 30,
            name: Some("empty".into()),
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }),
            ),
            native_ref: Some("feature".into()),
        };
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project_marker_backed_sketches(
            &ctx,
            &mut [neutral_feature],
            &mut Vec::new(),
            &mut Vec::new(),
            &[history],
            &[lane],
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn sketch_block_profile_assembly_preserves_nurbs_weights_and_text() {
        let source_id = SketchId::mint("synthetic:test:id#source-sketch").unwrap();
        let curve_id = SketchEntityId::mint("synthetic:test:id#source-curve").unwrap();
        let text_id = SketchEntityId::mint("synthetic:test:id#source-text").unwrap();
        let mut source = sketch();
        source.id = source_id.clone();
        source.profiles = vec![vec![SketchEntityUse {
            entity: curve_id.clone(),
            reversed: false,
        }]]
        .try_into()
        .unwrap();
        let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap();
        let entities = [
            SketchEntity::new(curve_id, source_id.clone(), SketchGeometry::nurbs(curve)),
            SketchEntity::new(
                text_id,
                source_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Text {
                    text: cadmpeg_core::text::NonBlankString::try_from("label").unwrap(),
                    font_family: cadmpeg_core::text::NonBlankString::try_from("font").unwrap(),
                    font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
                    height: Length::new(2.0).unwrap(),
                    width_factor: Some(1.5),
                    placement: Some(cadmpeg_ir::sketches::TextPlacement {
                        anchor: Point2::new(1.0, 2.0),
                        rotation: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                    }),
                    horizontal_alignment: None,
                    vertical_alignment: None,
                })
                .unwrap(),
            ),
        ];
        let instances = [
            SketchBlockInstancePlacement {
                feature_id: "synthetic:test:id#first".into(),
                block_source: 23,
                transform: Transform::identity(),
            },
            SketchBlockInstancePlacement {
                feature_id: "synthetic:test:id#second".into(),
                block_source: 23,
                transform: Transform::affine([
                    [0.0, -1.0, 0.0, 5.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .unwrap(),
            },
        ];
        let sources = HashMap::from([(23, source_id)]);
        let assembled_id = SketchId::mint("synthetic:test:id#assembled-sketch").unwrap();
        let native = feature();
        let sketches = [source];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let (sketch_positions, _sketch_positions_storage) =
            index_sketch_ids(&ctx, &sketches, "test sketch block assembly").unwrap();
        let (entity_groups, _entity_groups_storage) =
            index_entity_groups(&ctx, &entities, "test sketch block assembly").unwrap();
        let assembled = assemble_sketch_block_profile(
            &ctx,
            &SketchBlockProfileInput {
                sketch_id: &assembled_id,
                native_profile: &native,
                native_ref: "lane",
                configuration: None,
                block_sketches: &sources,
                instances: &instances,
                sketches: &sketches,
                sketch_positions: &sketch_positions,
                sketch_entities: &entities,
                entity_groups: &entity_groups,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(assembled.entities.len(), 4);
        let SketchGeometryDefinition::Nurbs { curve } = assembled.entities[2].geometry.definition()
        else {
            panic!("transformed curve");
        };
        assert_eq!(curve.degree(), 1);
        assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            curve.pole_rows().raw_points(),
            [Point2::new(5.0, 0.0), Point2::new(3.0, 1.0)]
        );
        assert_eq!(curve.pole_rows().weights(), Some(vec![1.0, 2.0]));
        assert!(!curve.periodic());
        let SketchGeometryDefinition::Text {
            text,
            font_family,
            placement,
            height,
            width_factor,
            ..
        } = assembled.entities[3].geometry.definition()
        else {
            panic!("transformed text");
        };
        assert_eq!(text.as_str(), "label");
        assert_eq!(font_family.as_str(), "font");
        assert_eq!(height.get(), 2.0);
        assert_eq!(width_factor.unwrap().get(), 1.5);
        let placement = placement.unwrap();
        assert_eq!(placement.anchor.get(), Point2::new(3.0, 1.0));
        assert!((placement.rotation.get() - std::f64::consts::FRAC_PI_2).abs() <= f64::EPSILON);
    }

    #[test]
    fn sketch_block_profile_assembly_projects_each_instance_into_one_frame() {
        let block_sketch_id = SketchId::mint("synthetic:test:id#block-sketch").unwrap();
        let block_entity_id = SketchEntityId::mint("synthetic:test:id#block-circle").unwrap();
        let block_line_id = SketchEntityId::mint("synthetic:test:id#block-line").unwrap();
        let block_sketch = Sketch {
            id: block_sketch_id.clone(),
            name: Some("block".into()),
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
                entity: block_entity_id.clone(),
                reversed: false,
            }]])
            .unwrap(),
            native_ref: Some("lane".into()),
        };
        let block_entities = vec![
            SketchEntity::new(
                block_entity_id,
                block_sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(1.0, 2.0),
                    radius: Length::new(3.0).unwrap(),
                })
                .unwrap(),
            )
            .with_native_ref(Some("circle".into())),
            SketchEntity::new(
                block_line_id,
                block_sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            )
            .with_construction(true)
            .with_native_ref(Some("line".into())),
        ];
        let quarter_turn = Transform::affine([
            [0.0, -1.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform");
        let assembled_id = SketchId::mint("sldprt:model:sketch#block-profile:test").unwrap();
        let block_sketches = HashMap::from([(23, block_sketch_id)]);
        let instances = [
            SketchBlockInstancePlacement {
                feature_id: "sldprt:model:feature#instance-1".into(),
                block_source: 23,
                transform: Transform::identity(),
            },
            SketchBlockInstancePlacement {
                feature_id: "sldprt:model:feature#instance-2".into(),
                block_source: 23,
                transform: quarter_turn,
            },
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let block_sketches_arena = [block_sketch];
        let (sketch_positions, _sketch_positions_storage) =
            index_sketch_ids(&ctx, &block_sketches_arena, "test sketch block assembly").unwrap();
        let (entity_groups, _entity_groups_storage) =
            index_entity_groups(&ctx, &block_entities, "test sketch block assembly").unwrap();
        let assembled = assemble_sketch_block_profile(
            &ctx,
            &SketchBlockProfileInput {
                sketch_id: &assembled_id,
                native_profile: &feature(),
                native_ref: "lane",
                configuration: Some("configuration"),
                block_sketches: &block_sketches,
                instances: &instances,
                sketches: &block_sketches_arena,
                sketch_positions: &sketch_positions,
                sketch_entities: &block_entities,
                entity_groups: &entity_groups,
            },
        )
        .unwrap()
        .expect("rigid coplanar block placements assemble");

        assert_eq!(assembled.sketch.profiles.len(), 2);
        assert_eq!(assembled.entities.len(), 4);
        assert_eq!(
            assembled.sketch.placement,
            SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .unwrap()
        );
        let circles = assembled
            .entities
            .iter()
            .filter_map(|entity| match *entity.geometry.definition() {
                SketchGeometryDefinition::Circle { center, radius } => {
                    Some((center.get(), Length::from(radius)))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            circles,
            [
                (Point2::new(1.0, 2.0), Length::new(3.0).unwrap()),
                (Point2::new(3.0, 1.0), Length::new(3.0).unwrap()),
            ]
        );
        let lines = assembled
            .entities
            .iter()
            .filter_map(|entity| match *entity.geometry.definition() {
                SketchGeometryDefinition::Line { start, end } => Some((start.get(), end.get())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(lines[1], (Point2::new(5.0, 0.0), Point2::new(5.0, 1.0)));
    }
    #[test]
    fn rejected_block_frames_release_partial_rotations() {
        let instances = [
            SketchBlockInstancePlacement {
                feature_id: "first".into(),
                block_source: 1,
                transform: Transform::identity(),
            },
            SketchBlockInstancePlacement {
                feature_id: "second".into(),
                block_source: 1,
                transform: Transform::affine([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 1.0],
                ])
                .unwrap(),
            },
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 64;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..64 {
            assert!(sketch_block_assembly_frame(&ctx, &instances)
                .unwrap()
                .is_none());
        }
    }
}

#[cfg(test)]
mod compact_cycle_tests;
