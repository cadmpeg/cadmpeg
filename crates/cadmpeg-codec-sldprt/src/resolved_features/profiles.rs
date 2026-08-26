//! Sketch profile projection from marker and compact records.

use super::assembly::is_supplemental_config_lane;
#[cfg(test)]
use super::bindings::bind_detached_legacy_sketch_objects;
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
use super::transforms::{quantize, sketch_frame_marker_transform};
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputLane, FeatureInputRelationFamily, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchId, SketchPlacement,
};
use cadmpeg_ir::transform::Transform;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use std::collections::BTreeMap;

/// Reconcile profile streams with uniquely enclosing sketch feature records.
// All sketch arenas and their annotations must be updated in one operation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bind_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    sketch_constraints: &mut Vec<SketchConstraint>,
    parameters: &[cadmpeg_ir::features::DesignParameter],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    annotations: &mut Annotations,
) {
    let declared_carriers = declared_entity_handle_circular_carriers(features, parameters, lanes);
    let mut superseded = HashSet::new();
    let metadata_ids = history_metadata_ids(histories);
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            if metadata_ids.contains(&feature.id) {
                continue;
            }
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            starts.push((name.offset, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let Some(feature) = features
                .iter_mut()
                .find(|feature| feature.native_ref.as_deref() == Some(native_feature.id.as_str()))
            else {
                continue;
            };
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let mut enclosed = sketches.iter_mut().filter(|sketch| {
                sketch.native_ref.as_deref() == Some(lane.id.as_str())
                    && annotations
                        .provenance
                        .get(&sketch.id.0)
                        .is_some_and(|source| source.offset > start && source.offset < end)
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
                superseded.insert(sketch.id.clone());
                continue;
            }
            match &mut feature.definition {
                cadmpeg_ir::features::FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Planar,
                    sketch: feature_sketch,
                    ..
                } => {
                    sketch.name = Some(native_feature.name.clone());
                    *feature_sketch = Some(sketch.id.clone());
                }
                cadmpeg_ir::features::FeatureDefinition::Sweep { section, .. }
                    if matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_)) =>
                {
                    *section = cadmpeg_ir::features::SweepSection::Profile(
                        cadmpeg_ir::features::ProfileRef::Sketch(sketch.id.clone()),
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Extrude { profile, .. } => {
                    if matches!(
                        &*profile,
                        cadmpeg_ir::features::ProfileRef::Unresolved(owner)
                            if owner == &native_feature.id
                    ) {
                        *profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.id.clone());
                    }
                }
                _ => {}
            }
        }
    }
    let mut removed = superseded
        .iter()
        .map(|sketch| sketch.0.clone())
        .collect::<HashSet<_>>();
    removed.extend(
        sketch_entities
            .iter()
            .filter(|entity| superseded.contains(&entity.sketch))
            .map(|entity| entity.id.0.clone()),
    );
    removed.extend(
        sketch_constraints
            .iter()
            .filter(|constraint| superseded.contains(&constraint.sketch))
            .map(|constraint| constraint.id.0.clone()),
    );
    sketches.retain(|sketch| !superseded.contains(&sketch.id));
    sketch_entities.retain(|entity| !superseded.contains(&entity.sketch));
    sketch_constraints.retain(|constraint| !superseded.contains(&constraint.sketch));
    annotations.provenance.retain(|id, _| !removed.contains(id));
    annotations.exactness.retain(|id, _| !removed.contains(id));
    bind_circular_profile_by_dimension(features, sketches, sketch_entities, parameters);
}

fn declared_entity_handle_circular_carriers(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<([f64; 2], f64)>> {
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let mut carriers = HashMap::<String, Vec<([f64; 2], f64)>>::new();
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
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
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
                .push((coordinates, encoded_radius));
        }
    }
    carriers
}

pub(super) fn nested_profile_contains_declared_circular_carriers(
    sketch: &Sketch,
    entities: &[SketchEntity],
    declared: &[([f64; 2], f64)],
) -> bool {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let Some(transform) = sketch_frame_marker_transform(sketch, QUANTUM) else {
        return true;
    };
    declared.iter().all(|([u, v], radius)| {
        let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
        let Some((center_u, center_v)) = transform.apply(native) else {
            return true;
        };
        let center = (center_u, center_v);
        entities.iter().any(|entity| {
            entity.sketch == sketch.id
                && match &entity.geometry {
                    SketchGeometry::Circle {
                        center: existing,
                        radius: existing_radius,
                    }
                    | SketchGeometry::Arc {
                        center: existing,
                        radius: existing_radius,
                        ..
                    } => {
                        quantize(*existing, QUANTUM) == center
                            && same_dimension_length(existing_radius.0, *radius)
                    }
                    _ => false,
                }
        })
    })
}

pub(crate) fn project_compact_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    let metadata_ids = history_metadata_ids(histories);

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let plane_frames = lane_sketch_plane_frames(features, histories, lane);
        let plane_index = CompactReferencePlaneIndex::new(&lane.native_payload);
        let mut objects = native_features
            .values()
            .filter(|feature| !metadata_ids.contains(&feature.id))
            .filter_map(|feature| {
                let start = feature_object_name(feature, lane)
                    .map(|name| name.offset)
                    .or_else(|| {
                        lane.sketch_entities
                            .iter()
                            .filter(|marker| {
                                marker.feature_ref.as_deref() == Some(feature.id.as_str())
                            })
                            .map(|marker| marker.offset)
                            .min()
                    })?;
                Some((start, *feature))
            })
            .collect::<Vec<_>>();
        objects.sort_by_key(|(offset, _)| *offset);
        for (object_index, &(start, native_feature)) in objects.iter().enumerate() {
            let Some(feature_index) = features.iter().position(|feature| {
                feature.native_ref.as_deref() == Some(native_feature.id.as_str())
                    && matches!(
                        feature.definition,
                        cadmpeg_ir::features::FeatureDefinition::Sketch { sketch: None, .. }
                    )
            }) else {
                continue;
            };
            let end = objects
                .get(object_index + 1)
                .map_or(lane.native_payload.len() as u64, |(offset, _)| *offset);
            let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
                continue;
            };
            let Some(interval) = lane.native_payload.get(start..end) else {
                continue;
            };
            let region_addresses = compact_line_region_addresses(interval);
            let chain_addresses = compact_line_chain_addresses(interval);
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
                .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
                .filter_map(|scalar| lane.scalars.iter().find(|record| record.id == scalar))
                .map(|scalar| scalar.value * NATIVE_TO_IR)
                .collect::<Vec<_>>();
            let dimensioned_rectangle = addresses
                .is_none()
                .then(|| unique_dimensioned_rectangle_markers(&owned_markers, &dimensions))
                .flatten();
            let markers = if let Some(rectangle) = dimensioned_rectangle {
                rectangle.to_vec()
            } else if region_addresses.is_some() {
                let line_classes = lane
                    .classes
                    .iter()
                    .filter(|class| {
                        class.name == "sgLineHandle"
                            && usize::try_from(class.offset)
                                .is_ok_and(|offset| offset >= start && offset < end)
                    })
                    .collect::<Vec<_>>();
                let [line_class] = line_classes.as_slice() else {
                    continue;
                };
                if lane.classes.iter().any(|class| {
                    class.name == "sgArcHandle"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| offset >= start && offset < end)
                }) {
                    continue;
                }
                let Some(first_marker) = owned_markers
                    .iter()
                    .copied()
                    .filter(|marker| marker.offset <= line_class.offset)
                    .max_by_key(|marker| marker.offset)
                else {
                    continue;
                };
                owned_markers
                    .iter()
                    .copied()
                    .skip_while(|marker| marker.offset < first_marker.offset)
                    .take_while(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>()
            } else {
                let runs = owned_markers
                    .split(|marker| {
                        marker.coordinates_m.is_none()
                            || !matches!(
                                marker.kind,
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
                .and_then(|(offset, _)| usize::try_from(*offset).ok())
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
            let sketch_id = SketchId(format!(
                "sldprt:model:sketch#compact:{lane_key}:{}",
                native_feature.ordinal
            ));
            if sketches.iter().any(|sketch| sketch.id == sketch_id) {
                features[feature_index].definition =
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    };
                continue;
            }
            let sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                    origin,
                    normal,
                    u_axis,
                },
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            if dimensioned_rectangle.is_some() {
                let points = markers
                    .iter()
                    .filter_map(|marker| {
                        let [u, v] = marker.coordinates_m?;
                        let native =
                            quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                        let point = transform.apply(native)?;
                        Some(Point2::new(
                            point.0 as f64 * QUANTUM,
                            point.1 as f64 * QUANTUM,
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
                let mut profile = Vec::with_capacity(corners.len());
                for (index, start) in corners.iter().enumerate() {
                    let end = corners[(index + 1) % corners.len()];
                    let start_marker = corner_markers[index];
                    let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                    let entity_id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                        native_feature.ordinal
                    ));
                    profile.push(SketchEntityUse {
                        entity: entity_id.clone(),
                        reversed: false,
                    });
                    sketch_entities.push(SketchEntity {
                        id: entity_id,
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: Some(start_marker.id.clone()),
                        geometry_ref: None,
                        endpoint_refs: vec![start_marker.id.clone(), end_marker.id.clone()],
                        geometry: SketchGeometry::Line { start: *start, end },
                    });
                }
                let mut sketch = sketch;
                sketch.profiles.push(profile);
                sketches.push(sketch);
                features[feature_index].definition =
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    };
                continue;
            }
            if let (Some(curves), Some(vertices)) =
                (region_addresses.as_deref(), chain_addresses.as_deref())
            {
                let project = |marker: &SketchInputEntity| {
                    let [u, v] = marker.coordinates_m?;
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some(Point2::new(
                        point.0 as f64 * QUANTUM,
                        point.1 as f64 * QUANTUM,
                    ))
                };
                let lines = curves
                    .iter()
                    .zip(vertices)
                    .enumerate()
                    .filter_map(|(index, (curve, vertex))| {
                        let curve = markers.get(usize::from(*curve).checked_sub(1)?)?;
                        let vertex = markers.get(usize::from(*vertex).checked_sub(1)?)?;
                        let start = project(curve)?;
                        let end = project(vertex)?;
                        (start != end).then(|| {
                            (
                                SketchEntityId(format!(
                                    "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                                    native_feature.ordinal
                                )),
                                *curve,
                                *vertex,
                                start,
                                end,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let profile = if let Some(profile) =
                    complete_ordered_compact_line_profile(&lines, markers.len())
                {
                    for (entity_id, marker, vertex, start, end) in lines {
                        sketch_entities.push(SketchEntity {
                            id: entity_id,
                            sketch: sketch_id.clone(),
                            construction: false,
                            native_ref: Some(marker.id.clone()),
                            geometry_ref: None,
                            endpoint_refs: vec![marker.id.clone(), vertex.id.clone()],
                            geometry: SketchGeometry::Line { start, end },
                        });
                    }
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
                    let mut profile = Vec::with_capacity(corners.len());
                    for (index, start) in corners.iter().enumerate() {
                        let end = corners[(index + 1) % corners.len()];
                        let start_marker = corner_markers[index];
                        let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                            native_feature.ordinal
                        ));
                        profile.push(SketchEntityUse {
                            entity: entity_id.clone(),
                            reversed: false,
                        });
                        sketch_entities.push(SketchEntity {
                            id: entity_id,
                            sketch: sketch_id.clone(),
                            construction: false,
                            native_ref: Some(start_marker.id.clone()),
                            geometry_ref: None,
                            endpoint_refs: vec![start_marker.id.clone(), end_marker.id.clone()],
                            geometry: SketchGeometry::Line { start: *start, end },
                        });
                    }
                    profile
                };
                let mut sketch = sketch;
                sketch.profiles.push(profile);
                sketches.push(sketch);
                features[feature_index].definition =
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    };
                continue;
            }
            let Some(addresses) = addresses else {
                continue;
            };
            let points = addresses
                .iter()
                .filter_map(|address| {
                    let marker = markers.get(usize::from(*address).checked_sub(1)?)?;
                    let [u, v] = marker.coordinates_m?;
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some((
                        *marker,
                        Point2::new(point.0 as f64 * QUANTUM, point.1 as f64 * QUANTUM),
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
            let mut profile = Vec::with_capacity(points.len());
            for (index, (marker, start)) in points.iter().enumerate() {
                let end = points[(index + 1) % points.len()].1;
                let entity_id = SketchEntityId(format!(
                    "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                    native_feature.ordinal
                ));
                profile.push(SketchEntityUse {
                    entity: entity_id.clone(),
                    reversed: false,
                });
                sketch_entities.push(SketchEntity {
                    id: entity_id,
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref: Some(marker.id.clone()),
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry: SketchGeometry::Line { start: *start, end },
                });
            }
            let mut sketch = sketch;
            sketch.profiles.push(profile);
            sketches.push(sketch);
            features[feature_index].definition = cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
            };
        }
    }
}

fn terminal_relation_display_carrier(lane: &FeatureInputLane, marker: &SketchInputEntity) -> bool {
    if !matches!(
        marker.kind,
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) || marker.coordinates_m.is_some()
    {
        return false;
    }
    let Some(feature_ref) = marker.feature_ref.as_deref() else {
        return false;
    };
    let Some(offset) = usize::try_from(marker.offset).ok() else {
        return false;
    };
    let Some(class_offset) = terminal_relation_class_offset(&lane.native_payload, offset) else {
        return false;
    };
    let Some(class) = lane
        .classes
        .iter()
        .find(|class| class.offset == class_offset as u64)
    else {
        return false;
    };
    lane.relation_instances
        .iter()
        .any(|relation| relation.feature_ref == feature_ref && relation.class_ref == class.id)
}

pub(crate) fn project_marker_backed_sketches(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    let metadata_ids = history_metadata_ids(histories);

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let marker_owners = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .filter_map(|marker| marker.feature_ref.as_deref())
        .collect::<HashSet<_>>();
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
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let mut objects = native_features
            .values()
            .filter(|feature| !metadata_ids.contains(&feature.id))
            .filter_map(|feature| {
                let start = feature_object_name(feature, lane)
                    .map(|name| name.offset)
                    .or_else(|| {
                        lane.sketch_entities
                            .iter()
                            .filter(|marker| {
                                marker.feature_ref.as_deref() == Some(feature.id.as_str())
                            })
                            .map(|marker| marker.offset)
                            .min()
                    })?;
                Some((start, *feature))
            })
            .collect::<Vec<_>>();
        objects.sort_by_key(|(offset, _)| *offset);
        for (object_index, &(start, native_feature)) in objects.iter().enumerate() {
            let Some((feature_index, bound_sketch, block_definition)) =
                features.iter().enumerate().find_map(|(index, feature)| {
                    if feature.native_ref.as_deref() != Some(native_feature.id.as_str()) {
                        return None;
                    }
                    match &feature.definition {
                        cadmpeg_ir::features::FeatureDefinition::Sketch { sketch, .. } => {
                            Some((index, sketch.clone(), false))
                        }
                        cadmpeg_ir::features::FeatureDefinition::SketchBlockDefinition {
                            sketch,
                        } => Some((index, sketch.clone(), true)),
                        _ => None,
                    }
                })
            else {
                continue;
            };
            let end = objects
                .get(object_index + 1)
                .map_or(lane.native_payload.len() as u64, |(offset, _)| *offset);
            let object_markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker.feature_ref.as_deref() == Some(native_feature.id.as_str())
                        && marker.offset < end
                })
                .collect::<Vec<_>>();
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
            let sketch_id = SketchId(format!(
                "sldprt:model:sketch#markers:{lane_key}:{}",
                native_feature.ordinal
            ));
            let markers = object_markers
                .iter()
                .copied()
                .filter(|marker| {
                    matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    ) && usize::try_from(marker.offset).ok().is_none_or(|offset| {
                        !legacy_unlocated_geometry_handle(&lane.native_payload, offset)
                            && !auxiliary_profile_record(&lane.native_payload, offset)
                            && !relation_reference_curve_record(
                                &lane.native_payload,
                                marker,
                                &object_markers,
                            )
                            && !terminal_relation_display_carrier(lane, marker)
                    })
                })
                .collect::<Vec<_>>();
            if markers.is_empty() {
                let has_unbound_marker = lane
                    .sketch_entities
                    .iter()
                    .any(|marker| marker.offset > start as u64 && marker.offset < end as u64);
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
                            placement: frame.map_or(
                                cadmpeg_ir::sketches::SketchPlacement::Unresolved,
                                |(origin, normal, u_axis)| {
                                    cadmpeg_ir::sketches::SketchPlacement::Resolved {
                                        origin,
                                        normal,
                                        u_axis,
                                    }
                                },
                            ),
                            profiles: Vec::new(),
                            native_ref: Some(lane.id.clone()),
                        };
                        sketches.push(sketch);
                    }
                    features[feature_index].definition =
                        cadmpeg_ir::features::FeatureDefinition::Sketch {
                            space: cadmpeg_ir::features::SketchSpace::Planar,
                            sketch: Some(sketch_id),
                        };
                }
                continue;
            }
            if sketches.iter().any(|sketch| sketch.id == sketch_id) {
                features[feature_index].definition = if block_definition {
                    cadmpeg_ir::features::FeatureDefinition::SketchBlockDefinition {
                        sketch: Some(sketch_id),
                    }
                } else {
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    }
                };
                continue;
            }
            if bound_sketch
                .as_ref()
                .is_some_and(|sketch| !sketch.0.contains("sketch#compact:"))
            {
                continue;
            }
            let mut sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: frame.map_or(
                    cadmpeg_ir::sketches::SketchPlacement::Unresolved,
                    |(origin, normal, u_axis)| cadmpeg_ir::sketches::SketchPlacement::Resolved {
                        origin,
                        normal,
                        u_axis,
                    },
                ),
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let encoded_rectangle =
                indexed_rectangle_from_line_cycle(&lane.native_payload, &object_markers);
            let inferred_points = std::cell::OnceCell::new();
            let mut projected = markers
                .iter()
                .copied()
                .filter_map(|marker| {
                    let project = |endpoint: &SketchInputEntity| {
                        let [u, v] = endpoint.coordinates_m?;
                        let point = transform.apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))?;
                        Some(Point2::new(
                            point.0 as f64 * QUANTUM,
                            point.1 as f64 * QUANTUM,
                        ))
                    };
                    let project_coordinates = |[u, v]: [f64; 2]| {
                        let point = transform.apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))?;
                        Some(Point2::new(
                            point.0 as f64 * QUANTUM,
                            point.1 as f64 * QUANTUM,
                        ))
                    };
                    let is_recovered_legacy_profile_point = |endpoint: &SketchInputEntity| {
                        usize::try_from(endpoint.offset).ok().is_some_and(|offset| {
                            legacy_140_profile_point_variant_coordinates(
                                &lane.native_payload,
                                offset,
                            )
                            .is_some()
                        })
                    };
                    let geometry = match marker.kind {
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                            let point = project(marker)?;
                            SketchGeometry::Point { position: point }
                        }
                        SketchInputKind::LineOrCircle => {
                            if let Some((center, radius)) = legacy_profile_radial_circle(
                                &lane.native_payload,
                                marker,
                                &object_markers,
                            )
                            .or_else(|| {
                                compact_profile_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                current_profile_circle_dimension(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                compact_legacy_terminal_diameter_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                compact_legacy_profile_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                extended_geometry_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            })
                            .or_else(|| {
                                wide_coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                            }) {
                                let point = transform.apply(quantize(
                                    Point2::new(center[0] * NATIVE_TO_IR, center[1] * NATIVE_TO_IR),
                                    QUANTUM,
                                ))?;
                                SketchGeometry::Circle {
                                    center: Point2::new(
                                        point.0 as f64 * QUANTUM,
                                        point.1 as f64 * QUANTUM,
                                    ),
                                    radius: Length(radius * NATIVE_TO_IR),
                                }
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
                                        return None;
                                    };
                                    if start == end {
                                        if is_recovered_legacy_profile_point(start_marker)
                                            || is_recovered_legacy_profile_point(end_marker)
                                        {
                                            // A zero-length line is not valid IR geometry. Preserve
                                            // the marker whose endpoint collapsed because a newly
                                            // recognized profile point supplied its coordinates.
                                            SketchGeometry::Native {
                                                native_kind: format!(
                                                    "sldprt:marker-geometry:{}",
                                                    marker.kind.native_code()
                                                ),
                                            }
                                        } else {
                                            return None;
                                        }
                                    } else {
                                        SketchGeometry::Line { start, end }
                                    }
                                } else if let Some([start, end]) =
                                    extended_declared_inline_line_endpoints(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                    .or_else(|| {
                                        extended_linked_inline_line_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
                                    })
                                    .or_else(|| {
                                        extended_identity_inline_line_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
                                    })
                                    .or_else(|| {
                                        implicit_coordinate_roster_curve_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                            inferred_points.get_or_init(|| {
                                                inferred_point_coordinates_by_index(
                                                    lane,
                                                    native_feature.id.as_str(),
                                                )
                                            }),
                                        )
                                    })
                                    .or_else(|| {
                                        implicit_profile_chain_closure_endpoints(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                        )
                                    })
                                    .or_else(|| {
                                        compact_legacy_142_profile_curve_endpoints(
                                            &lane.native_payload,
                                            usize::try_from(marker.offset).ok()?,
                                        )
                                    })
                                {
                                    let (Some(start), Some(end)) =
                                        (project_coordinates(start), project_coordinates(end))
                                    else {
                                        return None;
                                    };
                                    if start == end {
                                        return None;
                                    }
                                    SketchGeometry::Line { start, end }
                                } else {
                                    SketchGeometry::Native {
                                        native_kind: format!(
                                            "sldprt:marker-geometry:{}",
                                            marker.kind.native_code()
                                        ),
                                    }
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
                            if let Some((center, radius)) =
                                equal_index_coordinate_roster_full_circle(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                                .or_else(|| {
                                    compact_profile_full_circle(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                })
                                .or_else(|| {
                                    coordinate_roster_full_circle(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                })
                                .or_else(|| {
                                    wide_coordinate_roster_full_circle(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                })
                            {
                                let point = transform.apply(quantize(
                                    Point2::new(center[0] * NATIVE_TO_IR, center[1] * NATIVE_TO_IR),
                                    QUANTUM,
                                ))?;
                                SketchGeometry::Circle {
                                    center: Point2::new(
                                        point.0 as f64 * QUANTUM,
                                        point.1 as f64 * QUANTUM,
                                    ),
                                    radius: Length(radius * NATIVE_TO_IR),
                                }
                            } else if let (Some(point), Some(radius)) = (
                                marker.coordinates_m.and_then(|_| project(marker)),
                                coordinate_circle_radius(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                )
                                .or_else(|| {
                                    legacy_coordinate_circle_radius(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                    )
                                }),
                            ) {
                                SketchGeometry::Circle {
                                    center: point,
                                    radius: Length(radius * NATIVE_TO_IR),
                                }
                            } else if let (Some(center), Some((major_axis, major, minor))) = (
                                marker.coordinates_m.and_then(|_| project(marker)),
                                coordinate_ellipse_axes(
                                    &lane.native_payload,
                                    marker,
                                    &object_markers,
                                ),
                            ) {
                                let axis = transform.apply_axes(quantize(
                                    Point2::new(major_axis[0], major_axis[1]),
                                    QUANTUM,
                                ))?;
                                SketchGeometry::Ellipse {
                                    center,
                                    major_angle: Angle((axis.1 as f64).atan2(axis.0 as f64)),
                                    major_radius: Length(major * NATIVE_TO_IR),
                                    minor_radius: Length(minor * NATIVE_TO_IR),
                                    start_angle: None,
                                    end_angle: None,
                                }
                            } else if let Some([center, start, end]) =
                                usize::try_from(marker.offset).ok().and_then(|offset| {
                                    inline_arc_coordinates(&lane.native_payload, offset)
                                })
                            {
                                let (Some(center), Some(start), Some(end)) = (
                                    project_coordinates(center),
                                    project_coordinates(start),
                                    project_coordinates(end),
                                ) else {
                                    return None;
                                };
                                minor_arc_geometry(start, end, center, QUANTUM)?
                            } else if let ([start, end], Some(point)) = (
                                endpoints.as_slice(),
                                marker.coordinates_m.and_then(|_| project(marker)),
                            ) {
                                let (Some(start), Some(end)) = (project(start), project(end))
                                else {
                                    return None;
                                };
                                minor_arc_geometry(start, end, point, QUANTUM).unwrap_or_else(
                                    || SketchGeometry::Native {
                                        native_kind: format!(
                                            "sldprt:marker-geometry:{}",
                                            marker.kind.native_code()
                                        ),
                                    },
                                )
                            } else {
                                (|| {
                                    let [start, end] = endpoints.as_slice() else {
                                        return None;
                                    };
                                    let (start, end) = (project(start)?, project(end)?);
                                    let offset = usize::try_from(marker.offset).ok()?;
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
                                        return Some(SketchGeometry::Line { start, end });
                                    }
                                    if let Some([u, v]) = legacy_marker104_arc_center(
                                        &lane.native_payload,
                                        marker,
                                        &object_markers,
                                        [endpoints[0], endpoints[1]],
                                    )
                                    .or_else(|| {
                                        legacy_compact_diameter_arc_center(
                                            &lane.native_payload,
                                            marker,
                                            &object_markers,
                                            [endpoints[0], endpoints[1]],
                                        )
                                    }) {
                                        let center = transform.apply(quantize(
                                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                                            QUANTUM,
                                        ))?;
                                        let center = Point2::new(
                                            center.0 as f64 * QUANTUM,
                                            center.1 as f64 * QUANTUM,
                                        );
                                        return minor_arc_geometry(start, end, center, QUANTUM);
                                    }
                                    if indexed_arc_uses_coordinate_center(
                                        &lane.native_payload,
                                        offset,
                                    ) {
                                        let [start_u, start_v] = endpoints[0].coordinates_m?;
                                        let [end_u, end_v] = endpoints[1].coordinates_m?;
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
                                        let candidates = object_markers
                                            .iter()
                                            .copied()
                                            .filter(|candidate| {
                                                candidate.id != endpoints[0].id
                                                    && candidate.id != endpoints[1].id
                                            })
                                            .filter_map(|candidate| {
                                                let [u, v] = candidate.coordinates_m?;
                                                Some(Point2::new(
                                                    u * NATIVE_TO_IR,
                                                    v * NATIVE_TO_IR,
                                                ))
                                            })
                                            .collect::<Vec<_>>();
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
                                        if let Some(center) = roster_center.or_else(|| {
                                            unique_arc_center_marker(
                                                center_start,
                                                center_end,
                                                &candidates,
                                                QUANTUM,
                                            )
                                        }) {
                                            let center =
                                                transform.apply(quantize(center, QUANTUM))?;
                                            let center = Point2::new(
                                                center.0 as f64 * QUANTUM,
                                                center.1 as f64 * QUANTUM,
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
                                            return geometry;
                                        }
                                    }
                                    let [start_marker, end_marker] = endpoints.as_slice() else {
                                        return None;
                                    };
                                    let [start_u, start_v] = start_marker.coordinates_m?;
                                    let [end_u, end_v] = end_marker.coordinates_m?;
                                    let candidates = object_markers
                                        .iter()
                                        .copied()
                                        .filter(|candidate| {
                                            candidate.id != start_marker.id
                                                && candidate.id != end_marker.id
                                        })
                                        .filter_map(|candidate| {
                                            let [u, v] = candidate.coordinates_m?;
                                            Some(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR))
                                        })
                                        .collect::<Vec<_>>();
                                    if let Some(center) = unique_arc_center_marker(
                                        Point2::new(start_u * NATIVE_TO_IR, start_v * NATIVE_TO_IR),
                                        Point2::new(end_u * NATIVE_TO_IR, end_v * NATIVE_TO_IR),
                                        &candidates,
                                        QUANTUM,
                                    ) {
                                        let center = transform.apply(quantize(center, QUANTUM))?;
                                        let center = Point2::new(
                                            center.0 as f64 * QUANTUM,
                                            center.1 as f64 * QUANTUM,
                                        );
                                        return minor_arc_geometry(start, end, center, QUANTUM);
                                    }
                                    if let Some([tu, tv]) =
                                        compact_bounded_curve_tangent(&lane.native_payload, offset)
                                    {
                                        let (tu, tv) = transform
                                            .apply_axes(quantize(Point2::new(tu, tv), QUANTUM))?;
                                        return tangent_bounded_curve(
                                            start,
                                            end,
                                            [tu as f64 * QUANTUM, tv as f64 * QUANTUM],
                                            QUANTUM,
                                        );
                                    }
                                    (current_undetailed_bounded_curve_is_line(
                                        &lane.native_payload,
                                        offset,
                                    ) || legacy_undetailed_profile_line(
                                        &lane.native_payload,
                                        offset,
                                    ))
                                    .then_some(SketchGeometry::Line { start, end })
                                })()
                                .unwrap_or_else(|| {
                                    SketchGeometry::Native {
                                        native_kind: format!(
                                            "sldprt:marker-geometry:{}",
                                            marker.kind.native_code()
                                        ),
                                    }
                                })
                            }
                        }
                        SketchInputKind::Relation(_) | SketchInputKind::Native(_) => return None,
                    };
                    let endpoint_refs = if matches!(
                        marker.kind,
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    ) {
                        let endpoints = output_curve_endpoint_markers(
                            &lane.native_payload,
                            marker,
                            &markers_by_id,
                            &object_markers,
                        );
                        endpoints
                            .iter()
                            .map(|endpoint| endpoint.id.clone())
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    if matches!(geometry, SketchGeometry::Native { .. })
                        && marker.coordinates_m.is_some()
                        && usize::try_from(marker.offset).ok().is_none_or(|offset| {
                            !marker_is_geometry_locus(&lane.native_payload, offset)
                        })
                    {
                        return None;
                    }
                    Some(SketchEntity {
                        id: SketchEntityId(format!(
                            "sldprt:model:sketch-entity#markers:{lane_key}:{}:{}",
                            native_feature.ordinal, marker.ordinal
                        )),
                        sketch: sketch_id.clone(),
                        construction: usize::try_from(marker.offset).ok().is_some_and(|offset| {
                            (marker_is_selected_construction_line(&lane.native_payload, offset)
                                || current_compact_roster_selected_axis(
                                    &lane.native_payload,
                                    offset,
                                ))
                                && !(matches!(&geometry, SketchGeometry::Circle { .. })
                                    && marker_profile_curve_role(&lane.native_payload, offset)
                                        == Some(1))
                        }),
                        native_ref: Some(marker.id.clone()),
                        geometry_ref: None,
                        endpoint_refs,
                        geometry,
                    })
                })
                .collect::<Vec<_>>();
            if let Some(rectangle) = encoded_rectangle {
                let rectangle_marker_refs = object_markers
                    .iter()
                    .filter_map(|marker| {
                        let offset = usize::try_from(marker.offset).ok()?;
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
                        Some(marker.id.as_str())
                    })
                    .collect::<HashSet<_>>();
                projected.retain(|entity| {
                    entity
                        .native_ref
                        .as_deref()
                        .is_none_or(|native| !rectangle_marker_refs.contains(native))
                });
                let Some(corners) = rectangle
                    .map(|point| {
                        let point = transform.apply(quantize(
                            Point2::new(point.u * NATIVE_TO_IR, point.v * NATIVE_TO_IR),
                            QUANTUM,
                        ))?;
                        Some(Point2::new(
                            point.0 as f64 * QUANTUM,
                            point.1 as f64 * QUANTUM,
                        ))
                    })
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let point_matches_corner = |point: Point2, corner: Point2| {
                    same_dimension_length(point.u, corner.u)
                        && same_dimension_length(point.v, corner.v)
                };
                let misplaced_points = projected
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entity)| {
                        let SketchGeometry::Point { position } = entity.geometry else {
                            return None;
                        };
                        corners
                            .iter()
                            .all(|corner| !point_matches_corner(position, *corner))
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let missing_corners = corners
                    .iter()
                    .copied()
                    .filter(|corner| {
                        projected.iter().all(|entity| {
                            !matches!(
                                entity.geometry,
                                SketchGeometry::Point { position }
                                    if point_matches_corner(position, *corner)
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if let ([point], [corner]) =
                    (misplaced_points.as_slice(), missing_corners.as_slice())
                {
                    projected[*point].geometry = SketchGeometry::Point { position: *corner };
                }
                for entity in &mut projected {
                    let SketchGeometry::Native { .. } = entity.geometry else {
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
                    else {
                        continue;
                    };
                    let Some(endpoint) = transform
                        .apply(quantize(
                            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                            QUANTUM,
                        ))
                        .map(|point| {
                            Point2::new(point.0 as f64 * QUANTUM, point.1 as f64 * QUANTUM)
                        })
                    else {
                        continue;
                    };
                    let matching = corners
                        .iter()
                        .enumerate()
                        .filter(|(_, corner)| {
                            same_dimension_length(corner.u, endpoint.u)
                                && same_dimension_length(corner.v, endpoint.v)
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    let [index] = matching.as_slice() else {
                        continue;
                    };
                    entity.geometry = SketchGeometry::Line {
                        start: endpoint,
                        end: corners[(index + 2) % corners.len()],
                    };
                }
                for (index, start) in corners.iter().enumerate() {
                    projected.push(SketchEntity {
                        id: SketchEntityId(format!(
                            "sldprt:model:sketch-entity#markers:{lane_key}:{}:rectangle:{index}",
                            native_feature.ordinal
                        )),
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: None,
                        geometry_ref: None,
                        endpoint_refs: Vec::new(),
                        geometry: SketchGeometry::Line {
                            start: *start,
                            end: corners[(index + 1) % corners.len()],
                        },
                    });
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
            sketch.profiles = closed_marker_profiles(&projected);
            if projected.is_empty() || (bound_sketch.is_some() && sketch.profiles.is_empty()) {
                continue;
            }
            if let Some(bound_sketch) = &bound_sketch {
                sketch_entities.retain(|entity| entity.sketch != *bound_sketch);
                sketches.retain(|sketch| sketch.id != *bound_sketch);
            }
            sketch_entities.extend(projected);
            sketches.push(sketch);
            features[feature_index].definition = if block_definition {
                cadmpeg_ir::features::FeatureDefinition::SketchBlockDefinition {
                    sketch: Some(sketch_id),
                }
            } else {
                cadmpeg_ir::features::FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Planar,
                    sketch: Some(sketch_id),
                }
            };
        }
    }
}

#[derive(Clone, Copy)]
struct SketchBlockAssemblyFrame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

struct SketchBlockInstancePlacement {
    feature_id: String,
    block_source: String,
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
    block_sketches: &'a HashMap<String, SketchId>,
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
            let mut objects = history
                .features
                .iter()
                .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
                .filter(|(_, feature)| {
                    !crate::history::is_history_metadata_record(feature, &history.features)
                })
                .collect::<Vec<_>>();
            objects.sort_by_key(|(offset, _)| *offset);

            for (profile_position, (_, native_profile)) in objects.iter().enumerate() {
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
                    .find(|(_, (_, feature))| !is_sketch_block_object(feature))
                    .map_or(objects.len(), |(index, _)| index);
                let intervening = &objects[profile_position + 1..end];
                if !super::component_paths::profile_owns_intervening_sketch_blocks(
                    native_profile,
                    intervening.iter().map(|(_, feature)| *feature),
                ) {
                    continue;
                }
                let Some(children) = explicit_children.flatten().or_else(|| {
                    let children = intervening
                        .iter()
                        .filter(|(_, feature)| {
                            native_object_class(feature.input_class.as_deref().unwrap_or_default())
                                .kind
                                == NativeClassKind::SketchBlockDefinition
                        })
                        .filter_map(|(_, feature)| {
                            feature.source_id.as_deref()?.parse::<u32>().ok()
                        })
                        .collect::<HashSet<_>>();
                    (!children.is_empty()).then_some(children)
                }) else {
                    continue;
                };
                let Some(profile_index) = features.iter().position(|feature| {
                    feature.native_ref.as_deref() == Some(native_profile.id.as_str())
                }) else {
                    continue;
                };
                if !matches!(
                    features[profile_index].definition,
                    FeatureDefinition::Sketch { sketch: None, .. }
                ) {
                    continue;
                }

                let mut block_sketches = HashMap::<String, SketchId>::new();
                let mut block_feature_ids = HashMap::<String, String>::new();
                let mut definitions_complete = true;
                for (_, native_definition) in intervening.iter().filter(|(_, feature)| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                        == NativeClassKind::SketchBlockDefinition
                }) {
                    let Some(source) = native_definition.source_id.as_deref().filter(|source| {
                        source
                            .parse::<u32>()
                            .ok()
                            .is_some_and(|source| children.contains(&source))
                    }) else {
                        definitions_complete = false;
                        break;
                    };
                    let Some(definition_index) = features.iter().position(|feature| {
                        feature.native_ref.as_deref() == Some(native_definition.id.as_str())
                    }) else {
                        definitions_complete = false;
                        break;
                    };
                    let FeatureDefinition::SketchBlockDefinition {
                        sketch: Some(sketch_id),
                    } = &features[definition_index].definition
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
                    block_sketches.insert(source.to_string(), sketch_id.clone());
                    block_feature_ids
                        .insert(source.to_string(), features[definition_index].id.0.clone());
                }
                if !definitions_complete || block_sketches.len() != children.len() {
                    continue;
                }

                let mut instances = Vec::new();
                let mut instances_complete = true;
                for (_, native_instance) in intervening.iter().filter(|(_, feature)| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                        == NativeClassKind::SketchBlockInstance
                }) {
                    let Some(instance_index) = features.iter().position(|feature| {
                        feature.native_ref.as_deref() == Some(native_instance.id.as_str())
                    }) else {
                        instances_complete = false;
                        break;
                    };
                    let Some(block_source) = features[instance_index]
                        .source_properties
                        .get("BlockDefinition")
                        .or_else(|| native_instance.properties.get("BlockDefinition"))
                        .and_then(|source| source.parse::<u32>().ok())
                        .filter(|source| children.contains(source))
                        .map(|source| source.to_string())
                    else {
                        instances_complete = false;
                        break;
                    };
                    let FeatureDefinition::SketchBlockInstance {
                        block: Some(block),
                        placement: Some(transform),
                    } = &features[instance_index].definition
                    else {
                        instances_complete = false;
                        break;
                    };
                    if !block_sketches.contains_key(&block_source)
                        || block_feature_ids.get(&block_source) != Some(&block.0)
                    {
                        instances_complete = false;
                        break;
                    }
                    instances.push(SketchBlockInstancePlacement {
                        feature_id: features[instance_index].id.0.clone(),
                        block_source,
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
                let sketch_id = SketchId(format!(
                    "sldprt:model:sketch#block-profile:{lane_key}:{}",
                    native_profile.ordinal
                ));
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
                if let FeatureDefinition::Sketch { sketch, .. } =
                    &mut features[profile_index].definition
                {
                    *sketch = Some(sketch_id);
                }
            }
        }
    }
}

fn dissectable_child_sources(value: &str) -> Option<HashSet<u32>> {
    let values = value
        .split(',')
        .map(str::trim)
        .map(str::parse::<u32>)
        .collect::<Result<HashSet<_>, _>>()
        .ok()?;
    (!values.is_empty() && !values.contains(&0) && values.len() == value.split(',').count())
        .then_some(values)
}

fn is_sketch_block_object(feature: &crate::records::Feature) -> bool {
    matches!(
        native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind,
        NativeClassKind::SketchBlockDefinition | NativeClassKind::SketchBlockInstance
    )
}

fn assemble_sketch_block_profile(
    input: &SketchBlockProfileInput<'_>,
) -> Option<AssembledSketchBlockProfile> {
    let (placement, rotations) = sketch_block_assembly_frame(
        &input
            .instances
            .iter()
            .map(|instance| instance.transform)
            .collect::<Vec<_>>(),
    )?;
    let mut assembled_profiles = Vec::new();
    let mut assembled_entities = Vec::new();
    for (instance, rotation) in input.instances.iter().zip(rotations) {
        let source_sketch_id = input.block_sketches.get(&instance.block_source)?;
        let source_sketch = input
            .sketches
            .iter()
            .find(|sketch| sketch.id == *source_sketch_id)?;
        let source_entities = input
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == source_sketch.id)
            .cloned()
            .collect::<Vec<_>>();
        let entity_ids = source_entities
            .iter()
            .map(|entity| {
                (
                    entity.id.clone(),
                    SketchEntityId(format!(
                        "sldprt:model:sketch-entity#{}:instance:{}:entity:{}",
                        id_key(&input.sketch_id.0),
                        id_key(&instance.feature_id),
                        id_key(&entity.id.0)
                    )),
                )
            })
            .collect::<HashMap<_, _>>();
        for source_entity in &source_entities {
            let id = entity_ids.get(&source_entity.id)?.clone();
            assembled_entities.push(SketchEntity {
                id,
                sketch: input.sketch_id.clone(),
                construction: source_entity.construction,
                native_ref: Some(format!(
                    "{}:{}",
                    instance.feature_id,
                    source_entity
                        .native_ref
                        .as_deref()
                        .unwrap_or(&source_entity.id.0)
                )),
                geometry_ref: source_entity.geometry_ref.clone(),
                endpoint_refs: source_entity.endpoint_refs.clone(),
                geometry: transform_sketch_block_geometry(
                    &source_entity.geometry,
                    instance.transform,
                    placement,
                    rotation,
                )?,
            });
        }
        let source_profiles = if source_sketch.profiles.is_empty() {
            closed_marker_profiles_allowing_shared_endpoints(&source_entities)
        } else {
            source_sketch.profiles.clone()
        };
        for profile in &source_profiles {
            let mut assembled_profile = Vec::with_capacity(profile.len());
            for use_ in profile {
                assembled_profile.push(SketchEntityUse {
                    entity: entity_ids.get(&use_.entity)?.clone(),
                    reversed: use_.reversed,
                });
            }
            assembled_profiles.push(assembled_profile);
        }
    }
    (!assembled_profiles.is_empty()).then_some(())?;
    Some(AssembledSketchBlockProfile {
        sketch: Sketch {
            id: input.sketch_id.clone(),
            name: Some(input.native_profile.name.clone()),
            configuration: input.configuration.map(str::to_string),
            visible: None,
            placement: SketchPlacement::Resolved {
                origin: placement.origin,
                normal: placement.normal,
                u_axis: placement.u_axis,
            },
            profiles: assembled_profiles,
            native_ref: Some(input.native_ref.to_string()),
        },
        entities: assembled_entities,
    })
}

fn id_key(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, key)| key)
}

fn sketch_block_assembly_frame(
    placements: &[Transform],
) -> Option<(SketchBlockAssemblyFrame, Vec<f64>)> {
    const TOLERANCE: f64 = 1.0e-8;
    let first = *placements.first()?;
    if !first.is_proper_rigid() {
        return None;
    }
    let origin = first.apply_point(Point3::new(0.0, 0.0, 0.0));
    let u_axis = first.apply_vector(Vector3::new(1.0, 0.0, 0.0)).unit()?;
    let first_v = first.apply_vector(Vector3::new(0.0, 1.0, 0.0)).unit()?;
    let normal = u_axis.cross(first_v).unit()?;
    let v_axis = normal.cross(u_axis).unit()?;
    let frame = SketchBlockAssemblyFrame {
        origin,
        normal,
        u_axis,
    };
    let mut rotations = Vec::with_capacity(placements.len());
    for placement in placements {
        if !placement.is_proper_rigid() {
            return None;
        }
        let instance_origin = placement.apply_point(Point3::new(0.0, 0.0, 0.0));
        let origin_delta = instance_origin.vector_from(origin);
        if origin_delta.dot(normal).abs()
            > TOLERANCE * (1.0 + origin.distance(Point3::new(0.0, 0.0, 0.0)))
        {
            return None;
        }
        let instance_u = placement.apply_vector(Vector3::new(1.0, 0.0, 0.0)).unit()?;
        let instance_v = placement.apply_vector(Vector3::new(0.0, 1.0, 0.0)).unit()?;
        if instance_u.cross(instance_v).dot(normal) < 1.0 - TOLERANCE {
            return None;
        }
        let projected_u = Point2::new(instance_u.dot(u_axis), instance_u.dot(v_axis));
        let projected_v = Point2::new(instance_v.dot(u_axis), instance_v.dot(v_axis));
        if (projected_u.u * projected_u.u + projected_u.v * projected_u.v - 1.0).abs() > TOLERANCE
            || (projected_v.u * projected_v.u + projected_v.v * projected_v.v - 1.0).abs()
                > TOLERANCE
            || (projected_u.u * projected_v.v - projected_u.v * projected_v.u - 1.0).abs()
                > TOLERANCE
        {
            return None;
        }
        rotations.push(projected_u.v.atan2(projected_u.u));
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
    let direction = |direction| transform_sketch_block_direction(direction, transform, frame);
    let angle = |value: Angle| Angle(value.0 + rotation);
    Some(match geometry {
        SketchGeometry::Point { position } => SketchGeometry::Point {
            position: point(*position)?,
        },
        SketchGeometry::Line { start, end } => SketchGeometry::Line {
            start: point(*start)?,
            end: point(*end)?,
        },
        SketchGeometry::ReferenceLine {
            origin,
            direction: axis,
        } => SketchGeometry::ReferenceLine {
            origin: point(*origin)?,
            direction: direction(*axis)?,
        },
        SketchGeometry::Circle { center, radius } => SketchGeometry::Circle {
            center: point(*center)?,
            radius: *radius,
        },
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => SketchGeometry::Arc {
            center: point(*center)?,
            radius: *radius,
            start_angle: angle(*start_angle),
            end_angle: angle(*end_angle),
        },
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => SketchGeometry::Ellipse {
            center: point(*center)?,
            major_angle: angle(*major_angle),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
            start_angle: start_angle.map(angle),
            end_angle: end_angle.map(angle),
        },
        SketchGeometry::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_parameter,
            end_parameter,
        } => SketchGeometry::Hyperbola {
            center: point(*center)?,
            major_angle: angle(*major_angle),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
            start_parameter: *start_parameter,
            end_parameter: *end_parameter,
        },
        SketchGeometry::Parabola {
            vertex,
            axis_angle,
            focal_length,
            start_parameter,
            end_parameter,
        } => SketchGeometry::Parabola {
            vertex: point(*vertex)?,
            axis_angle: angle(*axis_angle),
            focal_length: *focal_length,
            start_parameter: *start_parameter,
            end_parameter: *end_parameter,
        },
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => SketchGeometry::Nurbs {
            degree: *degree,
            knots: knots.clone(),
            control_points: control_points
                .iter()
                .copied()
                .map(point)
                .collect::<Option<Vec<_>>>()?,
            weights: weights.clone(),
            periodic: *periodic,
        },
        SketchGeometry::Text {
            text,
            font_family,
            font_weight,
            height,
            width_factor,
            anchor,
            rotation: text_rotation,
            horizontal_alignment,
            vertical_alignment,
        } => SketchGeometry::Text {
            text: text.clone(),
            font_family: font_family.clone(),
            font_weight: *font_weight,
            height: *height,
            width_factor: *width_factor,
            anchor: match anchor {
                Some(anchor) => Some(point(*anchor)?),
                None => None,
            },
            rotation: text_rotation.map(angle),
            horizontal_alignment: *horizontal_alignment,
            vertical_alignment: *vertical_alignment,
        },
        SketchGeometry::ExternalReference { .. } | SketchGeometry::Native { .. } => return None,
    })
}

fn transform_sketch_block_point(
    point: Point2,
    transform: Transform,
    frame: SketchBlockAssemblyFrame,
) -> Option<Point2> {
    const TOLERANCE: f64 = 1.0e-8;
    let transformed = transform.apply_point(Point3::new(point.u, point.v, 0.0));
    let delta = transformed.vector_from(frame.origin);
    (delta.dot(frame.normal).abs()
        <= TOLERANCE * (1.0 + frame.origin.distance(Point3::new(0.0, 0.0, 0.0)))
        && transformed.x.is_finite()
        && transformed.y.is_finite()
        && transformed.z.is_finite())
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
    let transformed = transform.apply_vector(Vector3::new(direction.u, direction.v, 0.0));
    let v_axis = frame.normal.cross(frame.u_axis);
    let result = Point2::new(transformed.dot(frame.u_axis), transformed.dot(v_axis));
    (result.u.is_finite() && result.v.is_finite()).then_some(result)
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
            frames.sort_by_key(reference_plane_frame_key);
            frames.dedup();
            let [frame] = frames.as_slice() else {
                continue;
            };
            *frame
        };
        for feature in features.iter_mut() {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            if !matches!(
                feature.definition,
                FeatureDefinition::Sketch { sketch: None, .. }
            ) {
                continue;
            }
            let Some(native_feature) = native_features.get(native_ref).copied() else {
                continue;
            };
            let markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                .collect::<Vec<_>>();
            if markers.is_empty() {
                continue;
            }
            let (origin, normal, u_axis) = feature_frames
                .get(native_ref)
                .copied()
                .unwrap_or(detached_frame);
            let sketch_id = SketchId(format!(
                "sldprt:model:sketch#legacy-config:{lane_key}:{}",
                native_feature.ordinal
            ));
            let sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                    origin,
                    normal,
                    u_axis,
                },
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            let project = |coordinates: [f64; 2]| {
                let native = quantize(
                    Point2::new(coordinates[0] * NATIVE_TO_IR, coordinates[1] * NATIVE_TO_IR),
                    QUANTUM,
                );
                let point = transform.apply(native)?;
                Some(Point2::new(
                    point.0 as f64 * QUANTUM,
                    point.1 as f64 * QUANTUM,
                ))
            };

            let projected = legacy_config_hex_sketch(native_feature, &sketch, &markers, &project)
                .or_else(|| {
                    legacy_config_collinear_sketch(
                        lane,
                        native_feature,
                        &sketch,
                        &markers,
                        &project,
                    )
                });
            let Some((sketch, mut entities)) = projected else {
                continue;
            };
            if entities
                .iter()
                .any(|entity| matches!(entity.geometry, SketchGeometry::Native { .. }))
            {
                continue;
            }
            sketch_entities.append(&mut entities);
            sketches.push(sketch.clone());
            feature.definition = FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch.id),
            };
        }
    }
}

fn legacy_config_hex_sketch(
    native_feature: &crate::records::Feature,
    sketch: &Sketch,
    markers: &[&SketchInputEntity],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<(Sketch, Vec<SketchEntity>)> {
    let unique_object = |object_index: u32| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.object_index == Some(object_index) && marker.coordinates_m.is_some()
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let vertices = (9..=14).map(&unique_object).collect::<Option<Vec<_>>>()?;
    let origin = unique_object(3).or_else(|| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.object_index.is_none()
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
    let mut curves = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_none() && marker.kind == SketchInputKind::LineOrCircle
        })
        .collect::<Vec<_>>();
    curves.sort_unstable_by_key(|marker| marker.offset);
    let [horizontal_curve, vertical_curve, construction_circle, line0, line1, line2, line3, line4, line5] =
        curves.as_slice()
    else {
        return None;
    };
    if [horizontal_curve, vertical_curve, construction_circle]
        .iter()
        .any(|marker| marker.offset >= line0.offset)
        || vertices
            .windows(2)
            .any(|pair| pair[0].offset >= pair[1].offset)
    {
        return None;
    }
    let point = |marker: &SketchInputEntity| project(marker.coordinates_m?);
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
    let sketch_key = sketch
        .id
        .0
        .rsplit_once('#')
        .map_or(sketch.id.0.as_str(), |(_, key)| key);
    let entity_id = |kind: &str, index: usize| {
        SketchEntityId(format!(
            "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
            native_feature.ordinal
        ))
    };
    let mut entities = Vec::new();
    for (index, (curve, endpoints)) in
        [(*horizontal_curve, horizontal), (*vertical_curve, vertical)]
            .into_iter()
            .enumerate()
    {
        entities.push(SketchEntity {
            id: entity_id("axis", index),
            sketch: sketch.id.clone(),
            construction: true,
            native_ref: Some(curve.id.clone()),
            geometry_ref: None,
            endpoint_refs: endpoints.iter().map(|marker| marker.id.clone()).collect(),
            geometry: SketchGeometry::Line {
                start: point(endpoints[0])?,
                end: point(endpoints[1])?,
            },
        });
    }
    entities.push(SketchEntity {
        id: entity_id("circle", 0),
        sketch: sketch.id.clone(),
        construction: false,
        native_ref: Some(circle.id.clone()),
        geometry_ref: None,
        endpoint_refs: vec![circle.id.clone(), circle_radial.id.clone()],
        geometry: SketchGeometry::Circle {
            center,
            radius: Length(circle_radius),
        },
    });
    entities.push(SketchEntity {
        id: entity_id("circle", 1),
        sketch: sketch.id.clone(),
        construction: true,
        native_ref: Some(construction_circle.id.clone()),
        geometry_ref: None,
        endpoint_refs: vec![origin.id.clone(), construction_radial.id.clone()],
        geometry: SketchGeometry::Circle {
            center: construction_center,
            radius: Length(construction_radius),
        },
    });
    let line_curves = [line0, line1, line2, line3, line4, line5];
    let mut outer_profile = Vec::new();
    for (index, curve) in line_curves.into_iter().enumerate() {
        let start = vertices[index];
        let end = vertices[(index + 1) % vertices.len()];
        let id = entity_id("profile", index);
        outer_profile.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity {
            id,
            sketch: sketch.id.clone(),
            construction: false,
            native_ref: Some(curve.id.clone()),
            geometry_ref: None,
            endpoint_refs: vec![start.id.clone(), end.id.clone()],
            geometry: SketchGeometry::Line {
                start: point(start)?,
                end: point(end)?,
            },
        });
    }
    let mut sketch = sketch.clone();
    sketch.profiles = vec![
        outer_profile,
        vec![SketchEntityUse {
            entity: entity_id("circle", 0),
            reversed: false,
        }],
    ];
    Some((sketch, entities))
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
            marker.coordinates_m.is_none() && marker.kind == SketchInputKind::LineOrCircle
        })
        .collect::<Vec<_>>();
    curves.sort_unstable_by_key(|marker| marker.offset);
    let [negative_curve, first_curve, second_curve, third_curve] = curves.as_slice() else {
        return None;
    };
    let negative_offset = usize::try_from(negative_curve.offset).ok()?;
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
    let mut chain = markers
        .iter()
        .copied()
        .filter(|marker| matches!(marker.object_index, Some(18 | 19 | 21)))
        .filter_map(|marker| Some((marker, marker.coordinates_m?)))
        .collect::<Vec<_>>();
    let origin = markers
        .iter()
        .copied()
        .filter(|marker| marker.object_index.is_none())
        .filter_map(|marker| Some((marker, marker.coordinates_m?)))
        .min_by_key(|(marker, _)| marker.offset)?;
    chain.push(origin);
    chain.sort_by(|left, right| left.1[0].total_cmp(&right.1[0]));
    chain.dedup_by(|left, right| {
        same_dimension_length(left.1[0], right.1[0]) && same_dimension_length(left.1[1], right.1[1])
    });
    if chain.len() != 4
        || chain.iter().any(|(_, coordinates)| {
            !same_dimension_length(coordinates[1], origin.1[1]) || coordinates[0] < 0.0
        })
    {
        return None;
    }
    let negative = [negative_u, origin.1[1]];
    let sketch_key = sketch
        .id
        .0
        .rsplit_once('#')
        .map_or(sketch.id.0.as_str(), |(_, key)| key);
    let entity_id = |kind: &str, index: usize| {
        SketchEntityId(format!(
            "sldprt:model:sketch-entity#legacy-config:{sketch_key}:{}:{kind}:{index}",
            native_feature.ordinal
        ))
    };
    let line_curves = [negative_curve, first_curve, second_curve, third_curve];
    let segments = [
        (negative, origin.1),
        (chain[0].1, chain[1].1),
        (chain[1].1, chain[2].1),
        (chain[2].1, chain[3].1),
    ];
    let mut entities = line_curves
        .into_iter()
        .zip(segments)
        .enumerate()
        .map(|(index, (curve, (start, end)))| {
            Some(SketchEntity {
                id: entity_id("line", index),
                sketch: sketch.id.clone(),
                construction: false,
                native_ref: Some(curve.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: project(start)?,
                    end: project(end)?,
                },
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let mut points = markers
        .iter()
        .copied()
        .filter_map(|marker| Some((Some(marker), marker.coordinates_m?)))
        .chain(std::iter::once((None, negative)))
        .collect::<Vec<_>>();
    points.sort_by(|left, right| {
        left.1[0]
            .total_cmp(&right.1[0])
            .then_with(|| left.1[1].total_cmp(&right.1[1]))
    });
    points.dedup_by(|left, right| {
        same_dimension_length(left.1[0], right.1[0]) && same_dimension_length(left.1[1], right.1[1])
    });
    for (index, (marker, coordinates)) in points.into_iter().enumerate() {
        entities.push(SketchEntity {
            id: entity_id("point", index),
            sketch: sketch.id.clone(),
            construction: false,
            native_ref: marker.map(|marker| marker.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: project(coordinates)?,
            },
        });
    }
    Some((sketch.clone(), entities))
}

#[cfg(test)]
mod detached_legacy_sketch_tests {
    use super::*;
    use crate::layout::current_terminal_relation_carrier as terminal;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
        FeatureInputRelationFamily, FeatureInputRelationInstance,
    };

    fn feature() -> Feature {
        Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: Some("30".into()),
            parent_source_id: None,
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
            id: SketchId("sketch".into()),
            name: Some("profile".into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: Some("lane".into()),
        }
    }

    fn marker(
        ordinal: u32,
        object_index: Option<u32>,
        kind: SketchInputKind,
        coordinates_m: Option<[f64; 2]>,
    ) -> SketchInputEntity {
        SketchInputEntity {
            id: format!("marker-{ordinal}"),
            parent: "sldprt:feature-input:config-objects#1".into(),
            feature_ref: Some("feature".into()),
            ordinal,
            offset: u64::from(ordinal) * 100,
            object_index,
            local_id: None,
            kind,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
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
            .copy_from_slice(&(CLASS.len() as u16).to_le_bytes());
        payload[class_offset + 6..class_offset + 6 + CLASS.len()].copy_from_slice(CLASS);
        payload
    }

    #[test]
    fn terminal_relation_display_carrier_requires_same_feature_and_class() {
        let lane_id = "lane";
        let feature_id = "feature";
        let class_id = "class";
        let mut carrier = marker(0, None, SketchInputKind::LineOrCircle, None);
        carrier.offset = 0;
        let lane = FeatureInputLane {
            id: lane_id.into(),
            configuration: None,
            native_payload: current_terminal_relation_payload(),
            classes: vec![FeatureInputClass {
                id: class_id.into(),
                parent: lane_id.into(),
                ordinal: 0,
                offset: terminal::LEN as u64,
                name: "sgCircleDim".into(),
                role: FeatureInputClassRole::SketchConstraint,
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
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
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
        detached.offset = 100;
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
            std::slice::from_ref(&history),
            &HashSet::new(),
            &mut lane,
        );

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
                object_id: Some(30),
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
        let expected_sketch = SketchId("sldprt:model:sketch#markers:1:30".into());
        let mut neutral_feature = cadmpeg_ir::features::Feature::new(
            cadmpeg_ir::features::FeatureId("neutral".into()),
            30,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        );
        neutral_feature.name = Some("empty".into());
        neutral_feature.native_ref = Some("feature".into());
        let mut features = vec![neutral_feature];
        let mut sketches = Vec::new();
        let mut sketch_entities = Vec::new();

        project_marker_backed_sketches(
            &mut features,
            &mut sketches,
            &mut sketch_entities,
            &[history],
            &[lane],
        );

        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].id, expected_sketch);
        assert_eq!(sketches[0].profiles, Vec::<Vec<SketchEntityUse>>::new());
        assert_eq!(sketches[0].placement, SketchPlacement::Unresolved);
        assert!(sketch_entities.is_empty());
        assert!(matches!(
            &features[0].definition,
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } if sketch == &expected_sketch
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
        unbound.offset = 20;
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
                object_id: Some(30),
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
        let mut neutral_feature = cadmpeg_ir::features::Feature::new(
            cadmpeg_ir::features::FeatureId("neutral".into()),
            30,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        );
        neutral_feature.name = Some("empty".into());
        neutral_feature.native_ref = Some("feature".into());
        let mut features = vec![neutral_feature];
        let mut sketches = Vec::new();
        let mut sketch_entities = Vec::new();

        project_marker_backed_sketches(
            &mut features,
            &mut sketches,
            &mut sketch_entities,
            &[history],
            &[lane],
        );

        assert!(sketches.is_empty());
        assert!(matches!(
            features[0].definition,
            FeatureDefinition::Sketch { sketch: None, .. }
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
                    5 + index as u32,
                    Some(9 + index as u32),
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
                    .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
                    .count(),
                8
            );
            assert_eq!(
                entities
                    .iter()
                    .filter(|entity| matches!(entity.geometry, SketchGeometry::Circle { .. }))
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
        let (_, entities) =
            legacy_config_collinear_sketch(&lane, &feature(), &sketch(), &refs, &|coordinates| {
                Some(Point2::new(coordinates[0], coordinates[1]))
            })
            .expect("exact collinear grammar");

        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
                .count(),
            4
        );
        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
                .count(),
            11
        );
    }

    #[test]
    fn sketch_block_profile_assembly_projects_each_instance_into_one_frame() {
        let block_sketch_id = SketchId("block-sketch".into());
        let block_entity_id = SketchEntityId("block-circle".into());
        let block_line_id = SketchEntityId("block-line".into());
        let block_sketch = Sketch {
            id: block_sketch_id.clone(),
            name: Some("block".into()),
            configuration: None,
            visible: None,
            placement: SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: vec![vec![SketchEntityUse {
                entity: block_entity_id.clone(),
                reversed: false,
            }]],
            native_ref: Some("lane".into()),
        };
        let block_entities = vec![
            SketchEntity {
                id: block_entity_id,
                sketch: block_sketch_id.clone(),
                construction: false,
                native_ref: Some("circle".into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center: Point2::new(1.0, 2.0),
                    radius: Length(3.0),
                },
            },
            SketchEntity {
                id: block_line_id,
                sketch: block_sketch_id.clone(),
                construction: true,
                native_ref: Some("line".into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            },
        ];
        let quarter_turn = Transform {
            rows: [
                [0.0, -1.0, 0.0, 5.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let assembled_id = SketchId("sldprt:model:sketch#block-profile:test".into());
        let block_sketches = HashMap::from([("23".into(), block_sketch_id)]);
        let instances = [
            SketchBlockInstancePlacement {
                feature_id: "sldprt:model:feature#instance-1".into(),
                block_source: "23".into(),
                transform: Transform::identity(),
            },
            SketchBlockInstancePlacement {
                feature_id: "sldprt:model:feature#instance-2".into(),
                block_source: "23".into(),
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
            SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }
        );
        let circles = assembled
            .entities
            .iter()
            .filter_map(|entity| match entity.geometry {
                SketchGeometry::Circle { center, radius } => Some((center, radius)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            circles,
            [
                (Point2::new(1.0, 2.0), Length(3.0)),
                (Point2::new(3.0, 1.0), Length(3.0)),
            ]
        );
        let lines = assembled
            .entities
            .iter()
            .filter_map(|entity| match entity.geometry {
                SketchGeometry::Line { start, end } => Some((start, end)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(lines[1], (Point2::new(5.0, 0.0), Point2::new(5.0, 1.0)));
    }
}
