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
    lane_sketch_plane_frames, legacy_extended_rectangle_diagonal_endpoint,
    legacy_extended_rectangle_line_endpoints, ordered_rectangle_corners,
    resolve_connected_marker_arcs, resolve_slot_marker_arcs, resolve_two_center_semicircle_profile,
    tangent_bounded_curve, unique_dimensioned_rectangle_markers,
};
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
    terminal_relation_class_offset, unique_arc_center_marker, wide_coordinate_roster_full_circle,
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
use super::scalars::feature_object_name;
use super::transforms::sketch_frame_marker_transform;
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputLane, FeatureInputRelationFamily, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{DecodeContext, View};
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
use std::collections::{HashMap, HashSet};

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

/// Reconcile profile streams with uniquely enclosing sketch feature records.
pub(crate) fn bind_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    arenas: SketchArenas<'_>,
    parameters: &[cadmpeg_ir::features::DesignParameter],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let SketchArenas {
        sketches,
        sketch_entities,
        sketch_constraints,
        annotations,
    } = arenas;

    let declared_carriers = declared_entity_handle_circular_carriers(features, parameters, lanes);
    let mut superseded = HashSet::new();
    let metadata_ids = history_metadata_ids(histories);
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    for lane in lanes {
        let mut starts = Vec::<(u64, usize, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            if metadata_ids.contains(feature.id.as_str()) {
                continue;
            }
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            let ordinal = starts.len();
            starts.push((name.offset, ordinal, feature));
        }
        starts.sort_unstable_by_key(|left| (left.0, left.1));
        for (index, &(start, _, native_feature)) in starts.iter().enumerate() {
            let Some(feature) = features
                .iter_mut()
                .find(|feature| feature.native_ref.as_deref() == Some(native_feature.id.as_str()))
            else {
                continue;
            };
            let end = starts.get(index + 1).map(|next| next.0);
            let mut enclosed = sketches.iter_mut().filter(|sketch| {
                sketch.native_ref.as_deref() == Some(lane.id.as_str())
                    && annotations
                        .provenance
                        .get(sketch.id.as_str())
                        .is_some_and(|source| {
                            source.offset > start && end.is_none_or(|end| source.offset < end)
                        })
            });
            let Some(sketch) = enclosed.next() else {
                continue;
            };
            if enclosed.next().is_some() {
                continue;
            }
            if declared_carriers
                .get(native_feature.id.as_str())
                .is_some_and(|carriers| {
                    !nested_profile_contains_declared_circular_carriers(
                        sketch,
                        sketch_entities,
                        carriers,
                    )
                })
            {
                superseded.insert(sketch.id.as_str().to_owned());
                continue;
            }
            let replace = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. }) => true,
                FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                    shape.section_is_unresolved()
                }
                FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                    matches!(profile,
                    cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Unresolved(owner))
                        if owner == &native_feature.id)
                }
                _ => false,
            };
            if !replace {
                continue;
            }
            let sketch_id = sketch.id.clone();
            if matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ) {
                sketch.name = Some(native_feature.name.clone());
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
    let mut removed = HashSet::new();
    for id in superseded
        .iter()
        .map(String::as_str)
        .chain(
            sketch_entities
                .iter()
                .filter(|entity| superseded.contains(entity.sketch.as_str()))
                .map(|entity| entity.id().as_str()),
        )
        .chain(
            sketch_constraints
                .iter()
                .filter(|constraint| superseded.contains(constraint.sketch.as_str()))
                .map(|constraint| constraint.id.as_str()),
        )
    {
        removed.insert(id.to_owned());
    }
    sketches.retain(|sketch| !superseded.contains(sketch.id.as_str()));
    sketch_entities.retain(|entity| !superseded.contains(entity.sketch.as_str()));
    sketch_constraints.retain(|constraint| !superseded.contains(constraint.sketch.as_str()));
    let mut keep = |id: &str| !removed.contains(id);
    annotations.retain_provenance(&mut keep);
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    builder.retain_exactness(keep);
    *annotations = builder.build();
    bind_circular_profile_by_dimension(features, sketches, sketch_entities, parameters);
}

#[derive(Debug)]
pub(super) struct CircleCarrier(pub [f64; 2], pub f64);

fn declared_entity_handle_circular_carriers(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<CircleCarrier>> {
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let mut carriers = HashMap::<String, Vec<CircleCarrier>>::new();
    for lane in lanes {
        for relation in lane
            .relation_instances
            .iter()
            .filter(|relation| relation.family == FeatureInputRelationFamily::CircleDiameter)
        {
            let [operand] = relation.operands.as_slice() else {
                continue;
            };
            let Some(parameter) = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|id| parameters_by_id.get(id))
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
                lanes,
                relation.feature_ref.as_str(),
                operand,
                radius,
            ) else {
                continue;
            };
            let Some(coordinates) = center.coordinates_m else {
                continue;
            };
            carriers
                .entry(relation.feature_ref.clone())
                .or_default()
                .push(CircleCarrier(coordinates.get(), encoded_radius));
        }
    }
    carriers
}

pub(super) fn nested_profile_contains_declared_circular_carriers(
    sketch: &Sketch,
    entities: &[SketchEntity],
    declared: &[CircleCarrier],
) -> bool {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let Some(transform) = sketch_frame_marker_transform(sketch, QUANTUM) else {
        return true;
    };
    declared.iter().all(|CircleCarrier([u, v], radius)| {
        let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
        let Some((center_u, center_v)) = transform.apply(native) else {
            return true;
        };
        let center = (center_u, center_v);
        entities.iter().any(|entity| {
            entity.sketch == sketch.id
                && match entity.geometry.definition() {
                    SketchGeometryDefinition::Circle {
                        center: existing,
                        radius: existing_radius,
                    }
                    | SketchGeometryDefinition::Arc {
                        center: existing,
                        radius: existing_radius,
                        ..
                    } => {
                        quantize(existing.get(), QUANTUM) == center
                            && same_dimension_length(existing_radius.get(), *radius)
                    }
                    _ => false,
                }
        })
    })
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
    let metadata_ids = history_metadata_ids(histories);

    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    for lane in lanes {
        let plane_frames = lane_sketch_plane_frames(features, histories, lane);
        let plane_index = CompactReferencePlaneIndex::new(&lane.native_payload);
        let mut objects = native_features
            .values()
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| {
                let start = feature_object_name(feature, lane)
                    .map(|name| name.offset)
                    .or_else(|| {
                        lane.sketch_entities
                            .iter()
                            .filter(|marker| {
                                marker.feature_ref.as_deref() == Some(feature.id.as_str())
                            })
                            .map(crate::records::SketchInputEntity::offset)
                            .min()
                    })?;
                Some((start, *feature))
            })
            .enumerate()
            .map(|(ordinal, (offset, feature))| (offset, ordinal, feature))
            .collect::<Vec<_>>();
        objects.sort_unstable_by(
            |(left_offset, left_ordinal, _), (right_offset, right_ordinal, _)| {
                (left_offset, left_ordinal).cmp(&(right_offset, right_ordinal))
            },
        );
        for (object_index, &(start, _, native_feature)) in objects.iter().enumerate() {
            let Some(feature_index) = features.iter().position(|feature| {
                feature.native_ref.as_deref() == Some(native_feature.id.as_str())
                    && matches!(
                        feature.evaluation.definition(),
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Sketch {
                                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                                ..
                            }
                        )
                    )
            }) else {
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
            let region_addresses = compact_line_region_addresses(ctx, interval)?;
            let chain_addresses = compact_line_chain_addresses(ctx, interval)?;
            let addresses = region_addresses.as_ref().or(chain_addresses.as_ref());
            let owned_markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(native_feature.id.as_str()))
                .collect::<Vec<_>>();
            let dimensions = lane
                .relation_instances
                .iter()
                .filter(|relation| relation.feature_ref == native_feature.id)
                .filter(|relation| {
                    !matches!(
                        relation.family,
                        FeatureInputRelationFamily::Angle
                            | FeatureInputRelationFamily::CircleDiameter
                    )
                })
                .filter_map(|relation| relation.parameter_scalar_ref())
                .filter_map(|scalar| lane.scalars.iter().find(|record| record.id == scalar))
                .map(|scalar| scalar.value.get() * NATIVE_TO_IR)
                .collect::<Vec<_>>();
            let dimensioned_rectangle = if addresses.is_none() {
                unique_dimensioned_rectangle_markers(&owned_markers, &dimensions)
            } else {
                None
            };
            let markers = if let Some(rectangle) = dimensioned_rectangle {
                rectangle.to_vec()
            } else if region_addresses.is_some() {
                let line_classes = lane
                    .classes
                    .iter()
                    .filter(|class| {
                        class.name == "sgLineHandle"
                            && class.offset >= u64_from_index(start)
                            && class.offset < u64_from_index(end)
                    })
                    .collect::<Vec<_>>();
                let [line_class] = line_classes.as_slice() else {
                    continue;
                };
                if lane.classes.iter().any(|class| {
                    class.name == "sgArcHandle"
                        && class.offset >= u64_from_index(start)
                        && class.offset < u64_from_index(end)
                }) {
                    continue;
                }
                let Some(first_marker) = owned_markers
                    .iter()
                    .copied()
                    .filter(|marker| marker.offset() <= line_class.offset)
                    .max_by_key(|marker| marker.offset())
                else {
                    continue;
                };
                owned_markers
                    .iter()
                    .copied()
                    .skip_while(|marker| marker.offset() < first_marker.offset())
                    .take_while(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>()
            } else {
                let runs = owned_markers
                    .split(|marker| {
                        marker.coordinates_m.is_none()
                            || !matches!(
                                marker.kind(),
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    })
                    .filter(|run| addresses.is_some_and(|addresses| run.len() == addresses.len()))
                    .collect::<Vec<_>>();
                let [run] = runs.as_slice() else {
                    continue;
                };
                run.to_vec()
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
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            ) else {
                continue;
            };
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let Ok(sketch_id) = SketchId::mint(format!(
                "sldprt:model:sketch#compact:{lane_key}:{}",
                native_feature.ordinal
            )) else {
                continue;
            };
            if sketches.iter().any(|sketch| sketch.id == sketch_id) {
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
            let sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                    origin, normal, u_axis,
                ) {
                    Ok(placement) => placement,
                    Err(_) => continue,
                },
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let entity_start = sketch_entities.len();
            if dimensioned_rectangle.is_some() {
                let points = markers
                    .iter()
                    .filter_map(|marker| {
                        let [u, v] = marker.coordinates_m?.get();
                        let native =
                            quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                        let point = transform.apply(native)?;
                        Some(Point2::new(
                            f64_from_i64(point.0)? * QUANTUM,
                            f64_from_i64(point.1)? * QUANTUM,
                        ))
                    })
                    .collect::<Vec<_>>();
                let Some(corners) = ordered_rectangle_corners(&points) else {
                    continue;
                };
                let Some(corner_markers) = corners
                    .iter()
                    .map(|corner| {
                        points
                            .iter()
                            .position(|point| point == corner)
                            .and_then(|index| markers.get(index).copied())
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let mut profile = Vec::new();
                for (index, start) in corners.iter().enumerate() {
                    let end = corners[(index + 1) % corners.len()];
                    let start_marker = corner_markers[index];
                    let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                    let Ok(entity_id) = SketchEntityId::mint(format!(
                        "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                        native_feature.ordinal
                    )) else {
                        continue;
                    };
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: *start,
                        end,
                    }) else {
                        continue;
                    };
                    profile.push(SketchEntityUse {
                        entity: entity_id.clone(),
                        reversed: false,
                    });
                    let entity = profile_entity(
                        entity_id,
                        &sketch_id,
                        geometry,
                        Some(start_marker.id()),
                        Some([start_marker.id(), end_marker.id()]),
                    );
                    sketch_entities.push(entity);
                }
                let mut sketch = sketch;
                if profile.is_empty() {
                    let error = "sketch profile chain must be nonempty";
                    sketch_entities.truncate(entity_start);
                    losses.push(
                        crate::loss::SldprtLossCode::SketchProfileRejected.note(format!(
                            "Sketch {sketch_id} profile was not transferred: {error}"
                        )),
                    );
                    continue;
                }
                let profiles = vec![profile];
                sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
                sketches.push(sketch);
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
                let mut lines = Vec::new();
                for (index, (curve, vertex)) in curves.iter().zip(vertices).enumerate() {
                    let Some((curve, vertex, start, end)) = (|| {
                        let curve = markers.get(usize::from(*curve).checked_sub(1)?)?;
                        let vertex = markers.get(usize::from(*vertex).checked_sub(1)?)?;
                        let start = project(curve)?;
                        let end = project(vertex)?;
                        (start != end).then_some((*curve, *vertex, start, end))
                    })() else {
                        continue;
                    };
                    let Ok(entity_id) = SketchEntityId::mint(format!(
                        "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                        native_feature.ordinal,
                    )) else {
                        continue;
                    };
                    lines.push((entity_id, curve, vertex, start, end));
                }
                let profile = if let Some(profile) =
                    complete_ordered_compact_line_profile(ctx, &lines, markers.len())?
                {
                    let mut projected = Vec::new();
                    let mut complete = true;
                    for (entity_id, marker, vertex, start, end) in lines {
                        let Ok(geometry) =
                            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                        else {
                            complete = false;
                            break;
                        };
                        let entity = profile_entity(
                            entity_id,
                            &sketch_id,
                            geometry,
                            Some(marker.id()),
                            Some([marker.id(), vertex.id()]),
                        );
                        projected.push(entity);
                    }
                    if !complete {
                        continue;
                    }
                    sketch_entities.extend(projected);
                    profile
                } else {
                    let Some(points) = markers
                        .iter()
                        .map(|marker| project(marker))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let Some(corners) = ordered_rectangle_corners(&points) else {
                        continue;
                    };
                    let Some(corner_markers) = corners
                        .iter()
                        .map(|corner| {
                            points
                                .iter()
                                .position(|point| point == corner)
                                .and_then(|index| markers.get(index).copied())
                        })
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let mut profile = Vec::new();
                    for (index, start) in corners.iter().enumerate() {
                        let end = corners[(index + 1) % corners.len()];
                        let start_marker = corner_markers[index];
                        let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                        let Ok(entity_id) = SketchEntityId::mint(format!(
                            "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                            native_feature.ordinal
                        )) else {
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
                        profile.push(SketchEntityUse {
                            entity: entity_id.clone(),
                            reversed: false,
                        });
                        let entity = profile_entity(
                            entity_id,
                            &sketch_id,
                            geometry,
                            Some(start_marker.id()),
                            Some([start_marker.id(), end_marker.id()]),
                        );
                        sketch_entities.push(entity);
                    }
                    profile
                };
                let mut sketch = sketch;
                if profile.is_empty() {
                    let error = "sketch profile chain must be nonempty";
                    sketch_entities.truncate(entity_start);
                    losses.push(
                        crate::loss::SldprtLossCode::SketchProfileRejected.note(format!(
                            "Sketch {sketch_id} profile was not transferred: {error}"
                        )),
                    );
                    continue;
                }
                let profiles = vec![profile];
                sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
                sketches.push(sketch);
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
            let points = addresses
                .iter()
                .filter_map(|address| {
                    let marker = markers.get(usize::from(*address).checked_sub(1)?)?;
                    let [u, v] = marker.coordinates_m?.get();
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some((
                        *marker,
                        Point2::new(
                            f64_from_i64(point.0)? * QUANTUM,
                            f64_from_i64(point.1)? * QUANTUM,
                        ),
                    ))
                })
                .collect::<Vec<_>>();
            if points.len() != addresses.len()
                || points
                    .iter()
                    .enumerate()
                    .any(|(index, (_, point))| *point == points[(index + 1) % points.len()].1)
            {
                continue;
            }
            let mut profile = Vec::new();
            for (index, (marker, start)) in points.iter().enumerate() {
                let end = points[(index + 1) % points.len()].1;
                let Ok(entity_id) = SketchEntityId::mint(format!(
                    "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                    native_feature.ordinal
                )) else {
                    continue;
                };
                let Ok(geometry) =
                    SketchGeometry::try_from(SketchGeometryDefinition::Line { start: *start, end })
                else {
                    continue;
                };
                profile.push(SketchEntityUse {
                    entity: entity_id.clone(),
                    reversed: false,
                });
                let entity =
                    profile_entity(entity_id, &sketch_id, geometry, Some(marker.id()), None);
                sketch_entities.push(entity);
            }
            let mut sketch = sketch;
            if profile.is_empty() {
                let error = "sketch profile chain must be nonempty";
                sketch_entities.truncate(entity_start);
                losses.push(
                    crate::loss::SldprtLossCode::SketchProfileRejected.note(format!(
                        "Sketch {sketch_id} profile was not transferred: {error}"
                    )),
                );
                continue;
            }
            let profiles = vec![profile];
            sketch.profiles = profiles.try_into().map_err(CodecError::malformed)?;
            sketches.push(sketch);
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
    id: SketchEntityId,
    sketch: &SketchId,
    geometry: SketchGeometry,
    native: Option<&str>,
    endpoints: Option<[&str; 2]>,
) -> SketchEntity {
    let mut entity =
        SketchEntity::new(id, sketch.clone(), geometry).with_native_ref(native.map(str::to_owned));
    if let Some(endpoints) = endpoints {
        entity = entity.with_endpoint_refs(endpoints.map(str::to_owned).into());
    }
    entity
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

fn collect_marker_arc_centers(
    markers: &[&SketchInputEntity],
    first: &str,
    second: &str,
    scale: f64,
) -> Vec<Point2> {
    let mut candidates = Vec::new();
    for marker in markers {
        if marker.id() == first || marker.id() == second {
            continue;
        }
        let Some([u, v]) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        candidates.push(Point2::new(u * scale, v * scale));
    }
    candidates
}

fn terminal_relation_display_carrier(lane: &FeatureInputLane, marker: &SketchInputEntity) -> bool {
    if !matches!(
        marker.kind(),
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) || marker.coordinates_m.is_some()
    {
        return false;
    }
    let Some(feature_ref) = marker.feature_ref.as_deref() else {
        return false;
    };
    let Some(offset) = index_from_u64(marker.offset()) else {
        return false;
    };
    let Some(class_offset) = terminal_relation_class_offset(&lane.native_payload, offset) else {
        return false;
    };
    let Some(class) = lane
        .classes
        .iter()
        .find(|class| class.offset == u64_from_index(class_offset))
    else {
        return false;
    };
    lane.relation_instances
        .iter()
        .any(|relation| relation.feature_ref == feature_ref && relation.class_ref == class.id)
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
    let metadata_ids = history_metadata_ids(histories);

    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    let mut marker_owners = HashSet::new();
    for owner in lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .filter_map(|marker| marker.feature_ref.as_deref())
    {
        marker_owners.insert(owner);
    }
    let feature_frames = sketch_feature_frames(features, histories, lanes);
    project_detached_legacy_config_sketches(
        features,
        sketches,
        sketch_entities,
        &native_features,
        lanes,
        &feature_frames,
    );
    for lane in lanes {
        let plane_frames = lane_sketch_plane_frames(features, histories, lane);
        let plane_index = CompactReferencePlaneIndex::new(&lane.native_payload);
        let mut markers_by_id = HashMap::new();
        for marker in &lane.sketch_entities {
            markers_by_id.insert(marker.id(), marker);
        }
        let mut objects = Vec::new();
        for feature in native_features
            .values()
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
        {
            let Some((start, feature)) = (|| {
                let start = feature_object_name(feature, lane)
                    .map(|name| name.offset)
                    .or_else(|| {
                        lane.sketch_entities
                            .iter()
                            .filter(|marker| {
                                marker.feature_ref.as_deref() == Some(feature.id.as_str())
                            })
                            .map(crate::records::SketchInputEntity::offset)
                            .min()
                    })?;
                Some((start, *feature))
            })() else {
                continue;
            };
            objects.push((start, feature));
        }
        objects.sort_by_key(|left| left.0);
        for (object_index, &(start, native_feature)) in objects.iter().enumerate() {
            let Some((feature_index, bound_sketch, block_definition)) =
                features.iter().enumerate().find_map(|(index, feature)| {
                    if feature.native_ref.as_deref() != Some(native_feature.id.as_str()) {
                        return None;
                    }
                    match feature.evaluation.definition() {
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Sketch { sketch, .. },
                        ) => Some((index, sketch.id(), false)),
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::SketchBlockDefinition {
                                sketch,
                            },
                        ) => Some((index, sketch.as_ref(), true)),
                        _ => None,
                    }
                })
            else {
                continue;
            };
            let bound_sketch = bound_sketch.cloned();
            let end = objects
                .get(object_index + 1)
                .map_or(u64_from_index(lane.native_payload.len()), |(offset, _)| {
                    *offset
                });
            let mut object_markers = Vec::new();
            for marker in &lane.sketch_entities {
                if marker.feature_ref.as_deref() == Some(native_feature.id.as_str())
                    && marker.offset() < end
                {
                    object_markers.push(marker);
                }
            }
            let object_markers =
                super::endpoints::coordinate_rosters::CoordinateRosters::new(object_markers);
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
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            );
            let frame = frame
                .or_else(|| feature_frames.get(native_feature.id.as_str()).copied())
                .or_else(|| {
                    block_definition.then_some((
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    ))
                });
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let sketch_text = format!(
                "sldprt:model:sketch#markers:{lane_key}:{}",
                native_feature.ordinal
            );
            let Ok(sketch_id) = SketchId::mint(sketch_text) else {
                continue;
            };
            let mut markers = Vec::new();
            for marker in object_markers.iter().copied() {
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
                            &lane.native_payload,
                            marker,
                            &object_markers,
                        )
                        || terminal_relation_display_carrier(lane, marker)
                    {
                        continue;
                    }
                }
                markers.push(marker);
            }
            if markers.is_empty() {
                let has_unbound_marker = lane.sketch_entities.iter().any(|marker| {
                    marker.offset() > u64_from_index(start) && marker.offset() < u64_from_index(end)
                });
                if object_markers.is_empty()
                    && !has_unbound_marker
                    && !marker_owners.contains(native_feature.id.as_str())
                    && bound_sketch.is_none()
                    && !block_definition
                {
                    if !sketches.iter().any(|sketch| sketch.id == sketch_id) {
                        let sketch = Sketch {
                            id: sketch_id.clone(),
                            name: Some(native_feature.name.clone()),
                            configuration: lane.configuration.clone(),
                            visible: None,
                            placement: match frame {
                                Some((origin, normal, u_axis)) => {
                                    match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                                        origin, normal, u_axis,
                                    ) {
                                        Ok(placement) => placement,
                                        Err(_) => continue,
                                    }
                                }
                                None => cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
                            },
                            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                            native_ref: Some(lane.id.clone()),
                        };
                        sketches.push(sketch);
                    }
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
            if sketches.iter().any(|sketch| sketch.id == sketch_id) {
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
            if bound_sketch
                .as_ref()
                .is_some_and(|sketch| !sketch.as_str().contains("sketch#compact:"))
            {
                continue;
            }
            let mut sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: match frame {
                    Some((origin, normal, u_axis)) => {
                        match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                            origin, normal, u_axis,
                        ) {
                            Ok(placement) => placement,
                            Err(_) => continue,
                        }
                    }
                    None => cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
                },
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let encoded_rectangle =
                indexed_rectangle_from_line_cycle(&lane.native_payload, &object_markers);
            let inferred_points = std::cell::OnceCell::new();
            let mut projected = Vec::new();
            for marker in markers.iter().copied() {
                let native_kind = cadmpeg_core::nonblank_literal!(
                    "sldprt:marker-geometry:{}",
                    marker.kind().native_code()
                );
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
                            let mut circle_geometry = legacy_profile_radial_circle(
                                &lane.native_payload,
                                marker,
                                &object_markers,
                            );
                            if circle_geometry.is_none() {
                                circle_geometry = compact_profile_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = current_profile_circle_dimension(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = compact_legacy_terminal_diameter_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = compact_legacy_profile_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = extended_geometry_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = wide_coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
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
                                let endpoints = output_curve_endpoint_markers(
                                    &lane.native_payload,
                                    marker,
                                    &markers_by_id,
                                    &object_markers,
                                );
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
                                            SketchGeometry::native(native_kind)
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
                                    let mut endpoints = extended_declared_inline_line_endpoints(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                    .map(|endpoints| {
                                        endpoints.map(cadmpeg_ir::units::FiniteVector::get)
                                    })
                                    .or_else(|| {
                                        extended_linked_inline_line_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
                                        .map(|endpoints| {
                                            endpoints.map(cadmpeg_ir::units::FiniteVector::get)
                                        })
                                    })
                                    .or_else(|| {
                                        extended_identity_inline_line_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
                                        .map(|endpoints| {
                                            endpoints.map(cadmpeg_ir::units::FiniteVector::get)
                                        })
                                    });
                                    if endpoints.is_none() {
                                        let inferred = match inferred_points.get() {
                                            Some(inferred) => inferred,
                                            None => {
                                                let inferred = inferred_point_coordinates_by_index(
                                                    ctx,
                                                    lane,
                                                    native_feature.id.as_str(),
                                                )?;
                                                inferred_points.get_or_init(|| inferred)
                                            }
                                        };
                                        endpoints = implicit_coordinate_roster_curve_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                            inferred,
                                        );
                                    }
                                    if endpoints.is_none() {
                                        endpoints = implicit_profile_chain_closure_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        );
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
                                    SketchGeometry::native(native_kind)
                                }
                            }
                        }
                        SketchInputKind::Arc => {
                            let endpoints = marker_curve_endpoint_markers(
                                &lane.native_payload,
                                marker,
                                &markers_by_id,
                                &object_markers,
                            );
                            let mut circle_geometry = equal_index_coordinate_roster_full_circle(
                                &lane.native_payload,
                                marker,
                                &object_markers,
                            );
                            if circle_geometry.is_none() {
                                circle_geometry = compact_profile_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
                            }
                            if circle_geometry.is_none() {
                                circle_geometry = wide_coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                );
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
                                    let radius = coordinate_circle_radius(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    );
                                    if radius.is_some() {
                                        radius
                                    } else {
                                        legacy_coordinate_circle_radius(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
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
                                coordinate_ellipse_axes(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                ),
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
                                minor_arc_geometry(start, end, point, QUANTUM)
                                    .unwrap_or_else(|| SketchGeometry::native(native_kind))
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
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                        [endpoints[0], endpoints[1]],
                                    ) {
                                        Some(center) => Some(center),
                                        None => legacy_compact_diameter_arc_center(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                            [endpoints[0], endpoints[1]],
                                        ),
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
                                        let roster_center = coordinate_roster_arc_center(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                            [endpoints[0], endpoints[1]],
                                        )
                                        .map(|[u, v]| {
                                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR)
                                        });
                                        let roster_center_witness = roster_center.is_some();
                                        let candidates = collect_marker_arc_centers(
                                            &object_markers,
                                            endpoints[0].id(),
                                            endpoints[1].id(),
                                            NATIVE_TO_IR,
                                        );
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
                                            None => unique_arc_center_marker(
                                                center_start,
                                                center_end,
                                                &candidates,
                                                QUANTUM,
                                            ),
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
                                    let candidates = collect_marker_arc_centers(
                                        &object_markers,
                                        start_marker.id(),
                                        end_marker.id(),
                                        NATIVE_TO_IR,
                                    );
                                    if let Some(center) = unique_arc_center_marker(
                                        Point2::new(start_u * NATIVE_TO_IR, start_v * NATIVE_TO_IR),
                                        Point2::new(end_u * NATIVE_TO_IR, end_v * NATIVE_TO_IR),
                                        &candidates,
                                        QUANTUM,
                                    ) {
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
                                            Ok(SketchGeometry::native(native_kind))
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
                        let endpoints = output_curve_endpoint_markers(
                            &lane.native_payload,
                            marker,
                            &markers_by_id,
                            &object_markers,
                        );
                        for endpoint in endpoints {
                            endpoint_refs.push(endpoint.id().to_owned());
                        }
                    }
                    let id_text = format!(
                        "sldprt:model:sketch-entity#markers:{lane_key}:{}:{}",
                        native_feature.ordinal,
                        marker.ordinal()
                    );
                    let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                        continue;
                    };
                    projected.push(
                        SketchEntity::new(entity_id, sketch_id.clone(), geometry)
                            .with_construction(construction)
                            .with_native_ref(Some(marker.id().to_owned()))
                            .with_endpoint_refs(endpoint_refs),
                    );
                }
            }
            if let Some(rectangle) = encoded_rectangle {
                let mut rectangle_marker_refs = HashSet::new();
                for marker in &object_markers {
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
                    rectangle_marker_refs.insert(marker_id);
                }
                projected.retain(|entity| {
                    entity
                        .native_ref
                        .as_deref()
                        .is_none_or(|native| !rectangle_marker_refs.contains(native))
                });
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
                let mut misplaced_points =
                    projected.iter().enumerate().filter_map(|(index, entity)| {
                        let SketchGeometryDefinition::Point { position } =
                            *entity.geometry.definition()
                        else {
                            return None;
                        };
                        corners
                            .iter()
                            .all(|corner| !point_matches_corner(position.get(), *corner))
                            .then_some(index)
                    });
                let mut missing_corners = corners.iter().copied().filter(|corner| {
                    projected.iter().all(|entity| {
                        !matches!(entity.geometry.definition(),
                            SketchGeometryDefinition::Point { position }
                                if point_matches_corner(position.get(), *corner))
                    })
                });
                if let (Some(point), None, Some(corner), None) = (
                    misplaced_points.next(),
                    misplaced_points.next(),
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
                for entity in &mut projected {
                    let SketchGeometryDefinition::Native { .. } = *entity.geometry.definition()
                    else {
                        continue;
                    };
                    let Some(marker) = entity
                        .native_ref
                        .as_deref()
                        .and_then(|native| markers_by_id.get(native).copied())
                    else {
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
                    let id_text = format!(
                        "sldprt:model:sketch-entity#markers:{lane_key}:{}:rectangle:{index}",
                        native_feature.ordinal
                    );
                    let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                        continue;
                    };
                    let owner = sketch_id.clone();
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: *start,
                        end: corners[(index + 1) % corners.len()],
                    }) else {
                        continue;
                    };
                    projected.push(SketchEntity::new(entity_id, owner, geometry));
                }
            }
            resolve_two_center_semicircle_profile(
                &lane.native_payload,
                &object_markers,
                &mut projected,
                QUANTUM,
            );
            resolve_slot_marker_arcs(
                &lane.native_payload,
                &object_markers,
                &mut projected,
                QUANTUM,
            );
            resolve_connected_marker_arcs(&mut projected, QUANTUM);
            let Ok(profiles) =
                cadmpeg_ir::sketches::SketchProfiles::try_from(closed_marker_profiles(&projected))
            else {
                continue;
            };
            sketch.profiles = profiles;
            if projected.is_empty() || (bound_sketch.is_some() && sketch.profiles.is_empty()) {
                continue;
            }
            if let Some(bound_sketch) = &bound_sketch {
                sketch_entities.retain(|entity| entity.sketch != *bound_sketch);
                sketches.retain(|sketch| sketch.id != *bound_sketch);
            }
            sketch_entities.extend(projected);
            sketches.push(sketch);
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

struct AssembledSketchBlockProfile {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
}

struct SketchBlockProfileInput<'a> {
    sketch_id: &'a SketchId,
    native_profile: &'a crate::records::Feature,
    native_ref: &'a str,
    configuration: Option<&'a str>,
    block_sketches: &'a HashMap<u32, SketchId>,
    instances: &'a [SketchBlockInstancePlacement],
    sketches: &'a [Sketch],
    sketch_entities: &'a [SketchEntity],
}

/// Resolve a profile feature that owns a reusable sketch-block sequence.
///
/// A block definition stores geometry in its reusable local sketch coordinates.
/// An instance placement maps those coordinates into the owning profile plane;
/// the definition's own sketch frame is not applied a second time. This keeps
/// the assembled geometry planar when the reusable definition frame is a
/// source-local construction frame rather than the consuming profile plane.
pub(crate) fn project_sketch_block_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for lane in lanes {
        for history in histories {
            let mut objects = Vec::new();
            for (ordinal, feature) in history.features.iter().enumerate() {
                if let Some(name) = feature_object_name(feature, lane) {
                    if !crate::history::classify::is_history_metadata_record(
                        feature,
                        &history.features,
                    ) {
                        objects.push((name.offset, feature, ordinal));
                    }
                }
            }
            objects.sort_unstable_by(
                |(left_offset, _, left_ordinal), (right_offset, _, right_ordinal)| {
                    (left_offset, left_ordinal).cmp(&(right_offset, right_ordinal))
                },
            );

            for (profile_position, (_, native_profile, _)) in objects.iter().enumerate() {
                if !super::component_paths::is_profile_feature_object(native_profile) {
                    continue;
                }
                let explicit_children = native_profile
                    .properties
                    .get("DissectableChildren")
                    .map(|value| dissectable_child_sources(value));
                if explicit_children.as_ref().is_some_and(Option::is_none) {
                    continue;
                }
                let end = objects
                    .iter()
                    .enumerate()
                    .skip(profile_position + 1)
                    .find(|(_, (_, feature, _))| !is_sketch_block_object(feature))
                    .map_or(objects.len(), |(index, _)| index);
                let intervening = &objects[profile_position + 1..end];
                if !super::component_paths::profile_owns_intervening_sketch_blocks(
                    native_profile,
                    intervening.iter().map(|(_, feature, _)| *feature),
                ) {
                    continue;
                }
                let inferred_children = if explicit_children.is_none() {
                    let mut children = HashSet::new();
                    for (_, feature, _) in intervening.iter().filter(|(_, feature, _)| {
                        native_object_class(feature.input_class.as_deref().unwrap_or_default())
                            == NativeClassKind::SketchBlockDefinition
                    }) {
                        if let Some(source) = feature.source_value() {
                            children.insert(source);
                        }
                    }
                    (!children.is_empty()).then_some(children)
                } else {
                    None
                };
                let Some(children) = explicit_children.flatten().or(inferred_children) else {
                    continue;
                };
                let Some(profile_index) = features.iter().position(|feature| {
                    feature.native_ref.as_deref() == Some(native_profile.id.as_str())
                }) else {
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

                let mut block_sketches = HashMap::<u32, SketchId>::new();
                let mut block_feature_ids = HashMap::<u32, String>::new();
                let mut definitions_complete = true;
                for (_, native_definition, _) in intervening.iter().filter(|(_, feature, _)| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default())
                        == NativeClassKind::SketchBlockDefinition
                }) {
                    let Some(source) = native_definition
                        .source_value()
                        .filter(|source| children.contains(source))
                    else {
                        definitions_complete = false;
                        break;
                    };
                    let Some(definition_index) = features.iter().position(|feature| {
                        feature.native_ref.as_deref() == Some(native_definition.id.as_str())
                    }) else {
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
                    let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id)
                    else {
                        definitions_complete = false;
                        break;
                    };
                    if sketch.native_ref.as_deref() != Some(lane.id.as_str()) {
                        definitions_complete = false;
                        break;
                    }
                    block_sketches.insert(source, sketch_id.clone());
                    block_feature_ids
                        .insert(source, features[definition_index].id.as_str().to_owned());
                }
                if !definitions_complete || block_sketches.len() != children.len() {
                    continue;
                }

                let mut instances = Vec::new();
                let mut instances_complete = true;
                for (_, native_instance, _) in intervening.iter().filter(|(_, feature, _)| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default())
                        == NativeClassKind::SketchBlockInstance
                }) {
                    let Some(instance_index) = features.iter().position(|feature| {
                        feature.native_ref.as_deref() == Some(native_instance.id.as_str())
                    }) else {
                        instances_complete = false;
                        break;
                    };
                    let Some(block_source_number) = features[instance_index]
                        .source_properties
                        .get("BlockDefinition")
                        .or_else(|| native_instance.properties.get("BlockDefinition"))
                        .and_then(|source| source.parse::<u32>().ok())
                        .filter(|source| children.contains(source))
                    else {
                        instances_complete = false;
                        break;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance {
                        block: Some(block),
                        placement: Some(transform),
                    }) = features[instance_index].evaluation.definition()
                    else {
                        instances_complete = false;
                        break;
                    };
                    if !block_sketches.contains_key(&block_source_number)
                        || block_feature_ids
                            .get(&block_source_number)
                            .map(String::as_str)
                            != Some(block.as_str())
                    {
                        instances_complete = false;
                        break;
                    }
                    instances.push(SketchBlockInstancePlacement {
                        feature_id: features[instance_index].id.as_str().to_owned(),
                        block_source: block_source_number,
                        transform: *transform,
                    });
                }
                if !instances_complete || instances.is_empty() {
                    continue;
                }

                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let sketch_text = format!(
                    "sldprt:model:sketch#block-profile:{lane_key}:{}",
                    native_profile.ordinal
                );
                let Ok(sketch_id) = SketchId::mint(sketch_text) else {
                    continue;
                };
                let Some(assembled) = assemble_sketch_block_profile(&SketchBlockProfileInput {
                    sketch_id: &sketch_id,
                    native_profile,
                    native_ref: &lane.id,
                    configuration: lane.configuration.as_deref(),
                    block_sketches: &block_sketches,
                    instances: &instances,
                    sketches,
                    sketch_entities,
                }) else {
                    continue;
                };
                if !sketches.iter().any(|sketch| sketch.id == sketch_id) {
                    sketch_entities.extend(assembled.entities);
                    sketches.push(assembled.sketch);
                }
                features[profile_index]
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }));
            }
        }
    }
}

fn dissectable_child_sources(value: &str) -> Option<HashSet<u32>> {
    let mut values = HashSet::new();
    for part in value.split(',') {
        let Ok(source) = part.trim().parse::<u32>() else {
            return None;
        };
        values.insert(source);
    }
    (!values.is_empty() && !values.contains(&0) && values.len() == value.split(',').count())
        .then_some(values)
}

fn is_sketch_block_object(feature: &crate::records::Feature) -> bool {
    matches!(
        native_object_class(feature.input_class.as_deref().unwrap_or_default()),
        NativeClassKind::SketchBlockDefinition | NativeClassKind::SketchBlockInstance
    )
}

fn assemble_sketch_block_profile(
    input: &SketchBlockProfileInput<'_>,
) -> Option<AssembledSketchBlockProfile> {
    let (placement, rotations) = sketch_block_assembly_frame(input.instances)?;
    let mut assembled_profiles = Vec::new();
    let mut assembled_entities = Vec::new();
    for (instance, rotation) in input.instances.iter().zip(rotations) {
        let source_sketch_id = input.block_sketches.get(&instance.block_source)?;
        let source_sketch = input
            .sketches
            .iter()
            .find(|sketch| sketch.id == *source_sketch_id)?;
        let mut source_entities = Vec::new();
        for entity in input.sketch_entities {
            if entity.sketch == source_sketch.id {
                source_entities.push(entity);
            }
        }
        let mut entity_ids = HashMap::new();
        for entity in &source_entities {
            let id_text = format!(
                "sldprt:model:sketch-entity#{}:instance:{}:entity:{}",
                id_key(input.sketch_id.as_str()),
                id_key(&instance.feature_id),
                id_key(entity.id().as_str())
            );
            let Ok(id) = SketchEntityId::mint(id_text) else {
                return None;
            };
            entity_ids.insert(entity.id(), id);
        }
        for source_entity in &source_entities {
            let id = entity_ids.get(source_entity.id())?;
            let geometry = transform_sketch_block_geometry(
                &source_entity.geometry,
                instance.transform,
                placement,
                rotation,
            )?;
            let native_ref = format!(
                "{}:{}",
                instance.feature_id,
                source_entity
                    .native_ref
                    .as_deref()
                    .unwrap_or(source_entity.id().as_str())
            );
            assembled_entities.push(
                SketchEntity::new(id.clone(), input.sketch_id.clone(), geometry)
                    .with_construction(source_entity.construction)
                    .with_native_ref(Some(native_ref))
                    .with_geometry_ref(source_entity.geometry_ref.clone())
                    .with_endpoint_refs(source_entity.endpoint_refs.clone()),
            );
        }
        let recovered_profiles = if source_sketch.profiles.is_empty() {
            Some(closed_marker_profiles_allowing_shared_endpoints(
                &source_entities,
            ))
        } else {
            None
        };
        let source_profiles = recovered_profiles
            .as_deref()
            .unwrap_or(source_sketch.profiles.as_slice());
        for profile in source_profiles {
            let mut assembled_profile = Vec::new();
            for use_ in profile {
                let id = entity_ids.get(&use_.entity)?;
                assembled_profile.push(SketchEntityUse {
                    entity: id.clone(),
                    reversed: use_.reversed,
                });
            }
            assembled_profiles.push(assembled_profile);
        }
    }
    if assembled_profiles.is_empty() {
        return None;
    }
    let placement =
        SketchPlacement::try_resolved(placement.origin, placement.normal, placement.u_axis).ok()?;
    let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(assembled_profiles) else {
        return None;
    };
    Some(AssembledSketchBlockProfile {
        sketch: Sketch {
            id: input.sketch_id.clone(),
            name: Some(input.native_profile.name.clone()),
            configuration: input.configuration.map(str::to_owned),
            visible: None,
            placement,
            profiles,
            native_ref: Some(input.native_ref.to_owned()),
        },
        entities: assembled_entities,
    })
}

fn id_key(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, key)| key)
}

fn sketch_block_assembly_frame(
    instances: &[SketchBlockInstancePlacement],
) -> Option<(SketchBlockAssemblyFrame, Vec<f64>)> {
    const TOLERANCE: f64 = 1.0e-8;
    let first = instances.first().map(|instance| instance.transform)?;
    if !first.is_proper_rigid() {
        return None;
    }
    let (frame, v_axis) = (|| {
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
    })()?;
    let SketchBlockAssemblyFrame {
        origin,
        normal,
        u_axis,
    } = frame;
    let mut rotations = Vec::new();
    for instance in instances {
        let placement = instance.transform;
        if !placement.is_proper_rigid() {
            return None;
        }
        let rotation = (|| {
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
                || (projected_v.u * projected_v.u + projected_v.v * projected_v.v - 1.0).abs()
                    > TOLERANCE
                || (projected_u.u * projected_v.v - projected_u.v * projected_v.u - 1.0).abs()
                    > TOLERANCE
            {
                return None;
            }
            Some(projected_u.v.atan2(projected_u.u))
        })()?;
        rotations.push(rotation);
    }
    Some((frame, rotations))
}

fn transform_sketch_block_geometry(
    geometry: &SketchGeometry,
    transform: Transform,
    frame: SketchBlockAssemblyFrame,
    rotation: f64,
) -> Option<SketchGeometry> {
    let point = |point| transform_sketch_block_point(point, transform, frame);
    let finite_point =
        |value| transform_sketch_block_point(value, transform, frame).and_then(FinitePoint2::new);
    let direction = |direction| {
        transform_sketch_block_direction(direction, transform, frame).and_then(FinitePoint2::new)
    };
    let angle = |value: Angle| Angle::new(value.get() + rotation);
    match geometry.definition() {
        SketchGeometryDefinition::Nurbs { curve } => {
            let mut copied = curve.clone();
            if copied
                .try_map_control_points_in_place(|pole| {
                    point(pole.get()).and_then(FinitePoint2::new).ok_or(())
                })
                .is_err()
            {
                return None;
            }
            return Some(SketchGeometry::nurbs(copied));
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
            let (placement,) = prepared?;
            return SketchGeometry::from_parts(SketchGeometryDefinition::Text {
                text: text.clone(),
                font_family: font_family.clone(),
                font_weight: *font_weight,
                height: *height,
                width_factor: *width_factor,
                placement,
                horizontal_alignment: *horizontal_alignment,
                vertical_alignment: *vertical_alignment,
            })
            .ok();
        }
        _ => {}
    }
    (|| {
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
    })()
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
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    native_features: &HashMap<&str, &crate::records::Feature>,
    lanes: &[FeatureInputLane],
    feature_frames: &HashMap<String, (Point3, Vector3, Vector3)>,
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    for lane in lanes
        .iter()
        .filter(|lane| is_supplemental_config_lane(lane))
    {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let detached_frame = {
            let mut frames = lane
                .sketch_entities
                .iter()
                .filter_map(|marker| marker.feature_ref.as_deref())
                .filter_map(|feature| feature_frames.get(feature).copied())
                .collect::<Vec<_>>();
            frames.sort_unstable_by(|left, right| {
                reference_plane_frame_key(left).cmp(&reference_plane_frame_key(right))
            });
            frames.dedup();
            let [frame] = frames.as_slice() else {
                continue;
            };
            *frame
        };
        for feature in features.iter_mut() {
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
                let Some(native_feature) = native_features.get(native_ref).copied() else {
                    break 'feature_edit;
                };
                let markers = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .collect::<Vec<_>>();
                if markers.is_empty() {
                    break 'feature_edit;
                }
                let (origin, normal, u_axis) = feature_frames
                    .get(native_ref)
                    .copied()
                    .unwrap_or(detached_frame);
                let Ok(sketch_id) = SketchId::mint(format!(
                    "sldprt:model:sketch#legacy-config:{lane_key}:{}",
                    native_feature.ordinal
                )) else {
                    break 'feature_edit;
                };
                let sketch = Sketch {
                    id: sketch_id,
                    name: Some(native_feature.name.clone()),
                    configuration: lane.configuration.clone(),
                    visible: None,
                    placement: match cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                        origin, normal, u_axis,
                    ) {
                        Ok(placement) => placement,
                        Err(_) => break 'feature_edit,
                    },
                    profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                    native_ref: Some(lane.id.clone()),
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

                let projected =
                    match legacy_config_hex_sketch(native_feature, &sketch, &markers, &project) {
                        Some(projected) => Some(projected),
                        None => legacy_config_collinear_sketch(
                            lane,
                            native_feature,
                            &sketch,
                            &markers,
                            &project,
                        ),
                    };
                let Some((sketch, mut entities)) = projected else {
                    break 'feature_edit;
                };
                if entities.iter().any(|entity| {
                    matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Native { .. }
                    )
                }) {
                    break 'feature_edit;
                }
                let sketch_id = sketch.id.clone();
                sketch_entities.append(&mut entities);
                sketches.push(sketch);
                feature
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }));
            }
        }
    }
}

fn legacy_config_hex_sketch(
    native_feature: &crate::records::Feature,
    sketch: &Sketch,
    markers: &[&SketchInputEntity],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<(Sketch, Vec<SketchEntity>)> {
    let mut curves = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_none() && marker.kind() == SketchInputKind::LineOrCircle
        })
        .collect::<Vec<_>>();
    curves.sort_unstable_by_key(|left| left.offset());
    let prepared = (|| {
        let unique_object = |object_index: u32| {
            let mut candidates = markers.iter().copied().filter(|marker| {
                marker.object_index() == Some(object_index) && marker.coordinates_m.is_some()
            });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        };
        let vertices = [
            unique_object(9)?,
            unique_object(10)?,
            unique_object(11)?,
            unique_object(12)?,
            unique_object(13)?,
            unique_object(14)?,
        ];
        let origin = unique_object(3).or_else(|| {
            let mut candidates = markers.iter().copied().filter(|marker| {
                marker.object_index().is_none()
                    && marker.coordinates_m.is_some_and(|coordinates| {
                        same_dimension_length(coordinates[0], 0.0)
                            && same_dimension_length(coordinates[1], 0.0)
                    })
            });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        })?;
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
    let (
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
    ) = prepared?;
    let point = |marker: &SketchInputEntity| project(marker.coordinates_m?.get());
    let sketch_key = sketch
        .id
        .as_str()
        .rsplit_once('#')
        .map_or(sketch.id.as_str(), |(_, key)| key);
    let entity_id = |kind: &str, index: usize| {
        SketchEntityId::mint(format!(
            "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
            native_feature.ordinal,
        ))
        .ok()
    };
    let mut entities = Vec::new();
    for (index, (curve, endpoints)) in [(horizontal_curve, horizontal), (vertical_curve, vertical)]
        .into_iter()
        .enumerate()
    {
        let geometry = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: point(endpoints[0])?,
                end: point(endpoints[1])?,
            })
            .ok()
        })()?;
        let id = entity_id("axis", index)?;
        let entity = profile_entity(
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([endpoints[0].id(), endpoints[1].id()]),
        )
        .with_construction(true);
        entities.push(entity);
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
        let geometry = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center,
                radius: Length::new(radius)?,
            })
            .ok()
        })()?;
        let id = entity_id("circle", index)?;
        let mut entity = profile_entity(
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([endpoints[0].id(), endpoints[1].id()]),
        );
        if construction {
            entity = entity.with_construction(true);
        }
        entities.push(entity);
    }
    let mut outer_profile = Vec::new();
    for (index, curve) in [line0, line1, line2, line3, line4, line5]
        .into_iter()
        .enumerate()
    {
        let start = vertices[index];
        let end = vertices[(index + 1) % vertices.len()];
        let id = entity_id("profile", index)?;
        outer_profile.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        let geometry = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: point(start)?,
                end: point(end)?,
            })
            .ok()
        })()?;
        let entity = profile_entity(
            id,
            &sketch.id,
            geometry,
            Some(curve.id()),
            Some([start.id(), end.id()]),
        );
        entities.push(entity);
    }
    let mut copied = sketch.clone();
    let circle_id = entity_id("circle", 0)?;
    let circle_profile = vec![SketchEntityUse {
        entity: circle_id,
        reversed: false,
    }];
    let profiles = vec![outer_profile, circle_profile];
    let Ok(profiles) = profiles.try_into() else {
        return None;
    };
    copied.profiles = profiles;
    Some((copied, entities))
}

fn legacy_config_collinear_sketch(
    lane: &FeatureInputLane,
    native_feature: &crate::records::Feature,
    sketch: &Sketch,
    markers: &[&SketchInputEntity],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<(Sketch, Vec<SketchEntity>)> {
    let mut curves = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_none() && marker.kind() == SketchInputKind::LineOrCircle
        })
        .collect::<Vec<_>>();
    curves.sort_unstable_by_key(|left| left.offset());
    let prepared = (|| {
        let [negative_curve, first_curve, second_curve, third_curve] = curves.as_slice() else {
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
    let (negative_u, line_curves) = prepared?;
    let mut chain = markers
        .iter()
        .copied()
        .filter(|marker| matches!(marker.object_index(), Some(18 | 19 | 21)))
        .filter_map(|marker| Some((marker, marker.coordinates_m?.get())))
        .enumerate()
        .map(|(ordinal, (marker, point))| (marker, point, ordinal))
        .collect::<Vec<_>>();
    let origin = markers
        .iter()
        .copied()
        .filter(|marker| marker.object_index().is_none())
        .filter_map(|marker| Some((marker, marker.coordinates_m?.get())))
        .min_by_key(|(marker, _)| marker.offset())?;
    let ordinal = chain.len();
    chain.push((origin.0, origin.1, ordinal));
    chain.sort_unstable_by(|left, right| {
        left.1[0]
            .total_cmp(&right.1[0])
            .then_with(|| left.2.cmp(&right.2))
    });
    chain.dedup_by(|left, right| {
        same_dimension_length(left.1[0], right.1[0]) && same_dimension_length(left.1[1], right.1[1])
    });
    if chain.len() != 4
        || chain.iter().any(|(_, coordinates, _)| {
            !same_dimension_length(coordinates[1], origin.1[1]) || coordinates[0] < 0.0
        })
    {
        return None;
    }
    let negative = [negative_u, origin.1[1]];
    let sketch_key = sketch
        .id
        .as_str()
        .rsplit_once('#')
        .map_or(sketch.id.as_str(), |(_, key)| key);
    let entity_id = |kind: &str, index: usize| {
        SketchEntityId::mint(format!(
            "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
            native_feature.ordinal,
        ))
        .ok()
    };
    let segments = [
        (negative, origin.1),
        (chain[0].1, chain[1].1),
        (chain[1].1, chain[2].1),
        (chain[2].1, chain[3].1),
    ];
    let mut entities = Vec::new();
    for (index, (curve, (start, end))) in line_curves.into_iter().zip(segments).enumerate() {
        let id = entity_id("line", index)?;
        let geometry = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: project(start)?,
                end: project(end)?,
            })
            .ok()
        })()?;
        let entity = profile_entity(id, &sketch.id, geometry, Some(curve.id()), None);
        entities.push(entity);
    }
    let mut points = markers
        .iter()
        .copied()
        .filter_map(|marker| Some((Some(marker), marker.coordinates_m?.get())))
        .chain(std::iter::once((None, negative)))
        .enumerate()
        .map(|(ordinal, (marker, point))| (marker, point, ordinal))
        .collect::<Vec<_>>();
    points.sort_unstable_by(|left, right| {
        left.1[0]
            .total_cmp(&right.1[0])
            .then_with(|| left.1[1].total_cmp(&right.1[1]))
            .then_with(|| left.2.cmp(&right.2))
    });
    points.dedup_by(|left, right| {
        same_dimension_length(left.1[0], right.1[0]) && same_dimension_length(left.1[1], right.1[1])
    });
    for (index, (marker, coordinates, _)) in points.into_iter().enumerate() {
        let id = entity_id("point", index)?;
        let geometry = (|| {
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: project(coordinates)?,
            })
            .ok()
        })()?;
        let entity = profile_entity(
            id,
            &sketch.id,
            geometry,
            marker.map(SketchInputEntity::id),
            None,
        );
        entities.push(entity);
    }
    Some((sketch.clone(), entities))
}

#[cfg(test)]
mod detached_legacy_sketch_tests {
    use super::super::bindings::bind_detached_legacy_sketch_objects;
    use super::{
        assemble_sketch_block_profile, legacy_config_collinear_sketch, legacy_config_hex_sketch,
        project_marker_backed_sketches, terminal_relation_display_carrier,
        SketchBlockInstancePlacement, SketchBlockProfileInput,
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

    #[test]
    fn sketch_profile_binding_binds_named_sketch() {
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
            native_payload: vec![0; 32].into(),
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
        let stream =
            cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test:profile"));
        builder.note(sketch.id.as_str(), &stream, 1, Some("profile"));
        let annotations = builder.build();
        let mut admitted = [feature.clone()];
        super::bind_sketch_profiles(
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
        );
        assert!(matches!(admitted[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)),
            }) if id == &sketch.id));
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
            native_payload: payload.into(),
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

    #[test]
    fn compact_profile_projection_binds_rectangle() {
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
            native_payload: current_terminal_relation_payload().into(),
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

        assert!(terminal_relation_display_carrier(&lane, &carrier));

        let mut wrong_feature = lane.relation_instances[0].clone();
        wrong_feature.feature_ref = "other-feature".into();
        let mut wrong_feature_lane = lane.clone();
        wrong_feature_lane.relation_instances = vec![wrong_feature];
        assert!(!terminal_relation_display_carrier(
            &wrong_feature_lane,
            &carrier
        ));

        let mut wrong_class = lane.relation_instances[0].clone();
        wrong_class.class_ref = "other-class".into();
        let mut wrong_class_lane = lane;
        wrong_class_lane.relation_instances = vec![wrong_class];
        assert!(!terminal_relation_display_carrier(
            &wrong_class_lane,
            &carrier
        ));
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
            native_payload: vec![0; 512].into(),
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
            native_payload: vec![0; 64].into(),
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
            native_payload: vec![0; 64].into(),
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
            let (projected, entities) =
                legacy_config_hex_sketch(&feature(), &sketch(), &refs, &|coordinates| {
                    Some(Point2::new(coordinates[0], coordinates[1]))
                })
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
            native_payload: payload.into(),
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
        let (_, entities) =
            legacy_config_collinear_sketch(&lane, &feature(), &sketch(), &refs, &|coordinates| {
                Some(Point2::new(coordinates[0], coordinates[1]))
            })
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
    fn marker_profile_projection_accepts_history_without_lanes() {
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
            &[history],
            &[],
        )
        .unwrap();
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
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap();
        let entities = [
            SketchEntity::new(curve_id, source_id.clone(), SketchGeometry::nurbs(curve)),
            SketchEntity::new(
                text_id,
                source_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Text {
                    text: cadmpeg_core::text::NonBlankString::new("label").unwrap(),
                    font_family: cadmpeg_core::text::NonBlankString::new("font").unwrap(),
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
        let assembled = assemble_sketch_block_profile(&SketchBlockProfileInput {
            sketch_id: &assembled_id,
            native_profile: &native,
            native_ref: "lane",
            configuration: None,
            block_sketches: &sources,
            instances: &instances,
            sketches: &sketches,
            sketch_entities: &entities,
        })
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
        let assembled = assemble_sketch_block_profile(&SketchBlockProfileInput {
            sketch_id: &assembled_id,
            native_profile: &feature(),
            native_ref: "lane",
            configuration: Some("configuration"),
            block_sketches: &block_sketches,
            instances: &instances,
            sketches: &[block_sketch],
            sketch_entities: &block_entities,
        })
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
}

#[cfg(test)]
mod compact_cycle_tests;
