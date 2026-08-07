//! Dimensioned sketch geometry and radial circle records.

use super::endpoints::{compact_indexed_curve_record_end, marker_profile_curve_role};
use super::markers::{marker_native_code, sketch_marker_prefix_at};
use super::relation_geometry::{implicit_circle_marker, owned_relation_parameters};
use super::relation_loci::{marker_transform_candidates_by_feature, same_dimension_length};
use super::transforms::{
    dimensioned_circle_surface_transforms, dimensioned_circle_transform, quantize,
    select_marker_transforms_by_frame,
};
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER};
use crate::records::{
    FeatureInputLane, FeatureInputRelationFamily, FeatureInputRelationInstance, SketchInputEntity,
    SketchInputKind,
};
use cadmpeg_ir::features::{FeatureDefinition, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry};
use std::collections::{HashMap, HashSet};

/// Materialize dimensioned circular sketch geometry omitted by a selected-profile stream.
pub(crate) fn project_dimensioned_sketch_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let relation_parameter = |relation: &FeatureInputRelationInstance| {
        ownership
            .get(&relation.id)?
            .as_ref()
            .and_then(|parameter| parameters_by_id.get(parameter))
            .copied()
    };
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let marker_transforms =
        marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let transforms = sketches_by_feature
        .iter()
        .filter_map(|(feature, sketch_id)| {
            let circles = lanes
                .iter()
                .flat_map(|lane| &lane.relation_instances)
                .filter(|relation| {
                    relation.feature_ref == *feature
                        && relation.family == FeatureInputRelationFamily::CircleDiameter
                })
                .filter_map(|relation| {
                    let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                        return None;
                    };
                    let parameter = relation_parameter(relation)?;
                    let cadmpeg_ir::features::ParameterValue::Length(value) =
                        parameter.value.as_ref()?
                    else {
                        return None;
                    };
                    let radius = match parameter.display {
                        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                        None => return None,
                    };
                    if !(radius.is_finite() && radius > 0.0) {
                        return None;
                    }
                    let explicit = operand
                        .entity_ref
                        .as_deref()
                        .and_then(|id| markers_by_id.get(id).copied());
                    let implicit = explicit.is_none().then(|| {
                        implicit_circle_marker(
                            lanes,
                            relation.feature_ref.as_str(),
                            operand.kind,
                            operand.entity_index,
                            radius,
                        )
                    });
                    let (marker, encoded_radius) = match (explicit, implicit.flatten()) {
                        (Some(marker), _) => (marker, None),
                        (None, Some((marker, radius))) => (marker, Some(radius)),
                        (None, None) => return None,
                    };
                    if !matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                    ) {
                        return None;
                    }
                    let [u, v] = marker.coordinates_m?;
                    if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius))
                    {
                        return None;
                    }
                    Some((
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM),
                        (radius / QUANTUM).round() as i64,
                    ))
                })
                .collect::<Vec<_>>();
            let candidates = marker_transforms.get(*feature).cloned().unwrap_or_else(|| {
                sketches
                    .iter()
                    .find(|sketch| sketch.id == *sketch_id)
                    .map_or_else(Vec::new, |sketch| {
                        dimensioned_circle_surface_transforms(sketch, surfaces, &circles, QUANTUM)
                    })
            });
            let candidates = sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)
                .map_or(candidates.clone(), |sketch| {
                    select_marker_transforms_by_frame(&candidates, sketch, QUANTUM)
                });
            dimensioned_circle_transform(&candidates, &circles)
                .map(|transform| ((*feature).to_string(), transform))
        })
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let (Some(sketch), Some(transform)) = (
                sketches_by_feature.get(relation.feature_ref.as_str()),
                transforms.get(relation.feature_ref.as_str()),
            ) else {
                continue;
            };
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let parameter = relation_parameter(relation);
            let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let radius = match parameter.and_then(|parameter| parameter.display) {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                None => continue,
            };
            if !(radius.is_finite() && radius > 0.0) {
                continue;
            }
            let explicit_marker = operand
                .entity_ref
                .as_deref()
                .and_then(|id| markers_by_id.get(id).copied());
            let implicit_marker = explicit_marker.is_none().then(|| {
                implicit_circle_marker(
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                    radius,
                )
            });
            let (marker, encoded_radius) = match (explicit_marker, implicit_marker.flatten()) {
                (Some(marker), _) => (marker, None),
                (None, Some((marker, radius))) => (marker, Some(radius)),
                (None, None) => continue,
            };
            if !matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
            ) {
                continue;
            }
            let Some([u, v]) = marker.coordinates_m else {
                continue;
            };
            if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius)) {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let Some(center) = transform.apply(native) else {
                continue;
            };
            let center = Point2::new(center.0 as f64 * QUANTUM, center.1 as f64 * QUANTUM);
            if entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && match &entity.geometry {
                        SketchGeometry::Circle {
                            center: existing,
                            radius: existing_radius,
                        } => {
                            quantize(*existing, QUANTUM) == quantize(center, QUANTUM)
                                && same_dimension_length(existing_radius.0, radius)
                        }
                        _ => false,
                    }
            }) {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#dimension:{lane_key}:{}",
                    relation.offset
                )),
                sketch: sketch.clone(),
                construction: false,
                native_ref: Some(marker.id.clone()),
                geometry_ref: Some(relation.id.clone()),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(radius),
                },
            });
        }
    }
}

/// Materialize a circle dimension when its point operand already has one
/// neutral point witness in the owning sketch.
///
/// Some selected profile streams omit the circle carrier but retain the
/// dimension's point marker. The point marker is a center witness for this
/// relation family, not sufficient geometry by itself. Use it only after the
/// relation-point projector has established one same-sketch neutral point;
/// ambiguous or missing witnesses remain native.
pub(crate) fn project_relation_point_dimensioned_circles(
    entities: &mut Vec<SketchEntity>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let Some(parameter) = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
            else {
                continue;
            };
            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            let marker_id = operand.entity_ref.as_deref().or_else(|| {
                implicit_circle_marker(
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                    radius,
                )
                .map(|(marker, _)| marker.id.as_str())
            });
            let Some(marker_id) = marker_id else {
                continue;
            };
            let Some(marker) = markers_by_id.get(marker_id).copied() else {
                continue;
            };
            if !matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                continue;
            }
            let centers = entities
                .iter()
                .filter(|entity| {
                    entity.sketch == **sketch
                        && entity.native_ref.as_deref() == Some(marker_id)
                        && matches!(entity.geometry, SketchGeometry::Point { .. })
                })
                .collect::<Vec<_>>();
            let [center_entity] = centers.as_slice() else {
                continue;
            };
            let SketchGeometry::Point { position: center } = center_entity.geometry else {
                continue;
            };
            if entities.iter().any(|entity| {
                entity.sketch == **sketch
                    && matches!(&entity.geometry, SketchGeometry::Circle { center: existing, radius: existing_radius }
                        if quantize(*existing, 1.0e-8) == quantize(center, 1.0e-8)
                            && same_dimension_length(existing_radius.0, radius))
            }) {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}",
                    relation.offset
                )),
                sketch: (*sketch).clone(),
                construction: false,
                native_ref: Some(marker.id.clone()),
                geometry_ref: Some(relation.id.clone()),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center,
                    radius: Length(radius),
                },
            });
        }
    }
}

pub(super) fn compact_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    let marker = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len());
    if marker != Some(LEGACY_SKETCH_MARKER) && marker != Some(LEGACY_EXTENDED_SKETCH_MARKER) {
        return None;
    }
    let ordinary = matches!(marker_native_code(payload, offset), Some(1 | 2))
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes());
    let construction = marker == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(7)
        && payload.get(offset + 5..offset + 13)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff])
        && marker_profile_curve_role(payload, offset) == Some(2)
        && payload.get(offset + 29..offset + 31) == Some(&0u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        && payload.get(offset + 60..offset + 64) == Some(&0u32.to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&1i32.to_le_bytes())
        && payload.get(offset + 76..offset + 78) == Some(&8u16.to_le_bytes())
        && payload.get(offset + 78..offset + 94)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 94..offset + 96) == Some(&[0; 2])
        && sketch_marker_prefix_at(payload, offset.saturating_add(104));
    if !(ordinary || construction)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || (ordinary && compact_indexed_curve_record_end(payload, offset).is_none())
    {
        return None;
    }
    let first = u16::from_le_bytes(payload.get(offset + 56..offset + 58)?.try_into().ok()?);
    let second = u16::from_le_bytes(payload.get(offset + 58..offset + 60)?.try_into().ok()?);
    (first == second).then_some(usize::from(first))
}

pub(super) fn compact_legacy_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    (payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER))
        .then(|| compact_radial_circle_index(payload, offset))
        .flatten()
}

fn radial_circle_records(payload: &[u8]) -> Vec<(usize, usize, bool)> {
    (0..payload.len().saturating_sub(LEGACY_SKETCH_MARKER.len() - 1))
        .filter_map(|offset| {
            let radial = compact_radial_circle_index(payload, offset)
                .or_else(|| extended_terminal_repeated_radial_circle_index(payload, offset))?;
            Some((
                offset,
                radial,
                marker_profile_curve_role(payload, offset) == Some(2),
            ))
        })
        .collect()
}

pub(super) fn extended_terminal_repeated_radial_circle_index(
    payload: &[u8],
    offset: usize,
) -> Option<usize> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != payload.get(offset + 58..offset + 60)
        || payload.get(offset + 56..offset + 58) == Some(&[0; 2])
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&(-1i32).to_le_bytes())
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 104) != Some(&[0; 10])
        || sketch_marker_prefix_at(payload, offset.checked_add(104)?)
    {
        return None;
    }
    Some(usize::from(u16::from_le_bytes(
        payload.get(offset + 56..offset + 58)?.try_into().ok()?,
    )))
}

pub(super) fn terminal_repeated_radial_circle_pairs<'a>(
    radial_index: usize,
    roster: &[&'a SketchInputEntity],
    radius: f64,
) -> Option<Vec<(&'a SketchInputEntity, &'a SketchInputEntity)>> {
    if radial_index != roster.len() || radius <= 0.0 || !radius.is_finite() {
        return None;
    }
    let terminal = *roster.last()?;
    let mut pairs = roster
        .windows(2)
        .filter_map(|window| {
            let [center, radial] = window else {
                unreachable!("two-wide roster window");
            };
            let center_index = center.object_index?;
            let radial_index = radial.object_index?;
            if center_index != radial_index.checked_add(1)? {
                return None;
            }
            let [cu, cv] = center.coordinates_m?;
            let [ru, rv] = radial.coordinates_m?;
            same_dimension_length((ru - cu).hypot(rv - cv), radius).then_some((*center, *radial))
        })
        .collect::<Vec<_>>();
    if pairs.len() < 2 || pairs.last().map(|(_, radial)| radial.id.as_str()) != Some(&terminal.id) {
        return None;
    }
    let mut used = HashSet::new();
    if pairs
        .iter()
        .any(|(center, radial)| !used.insert(&center.id) || !used.insert(&radial.id))
    {
        return None;
    }
    pairs.sort_unstable_by_key(|(center, _)| center.offset);
    Some(pairs)
}

pub(super) fn extended_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    let supported = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) == payload.get(offset + 66..offset + 68)
        && payload.get(offset + 64..offset + 66) != Some(&[0; 2])
        && payload.get(offset + 68..offset + 72) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 80..offset + 84) == Some(&1u32.to_le_bytes());
    supported.then(|| {
        usize::from(u16::from_le_bytes(
            payload[offset + 64..offset + 66]
                .try_into()
                .expect("guarded two-byte radial index"),
        ))
    })
}

pub(super) fn radial_dimension_radius(
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<f64> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    (radius.is_finite() && radius > 0.0).then_some(radius)
}

/// Materialize marker-only circles whose radial witnesses have exact radial
/// dimensions, including repeated circles constrained to the same radius.
pub(crate) fn project_marker_dimensioned_circles(
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let radial_records_by_lane = lanes
        .iter()
        .map(|lane| {
            (
                lane.id.as_str(),
                radial_circle_records(&lane.native_payload),
            )
        })
        .collect::<HashMap<_, _>>();
    'feature: for feature in features {
        let (
            Some(native_ref),
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
                ..
            },
        ) = (feature.native_ref.as_deref(), &feature.definition)
        else {
            continue;
        };
        let radial_dimensions = parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
            .filter_map(|parameter| {
                radial_dimension_radius(parameter).map(|radius| (parameter, radius))
            })
            .collect::<Vec<_>>();
        if radial_dimensions.is_empty() {
            continue;
        }
        let owned_lanes = lanes
            .iter()
            .filter(|lane| {
                lane.sketch_entities
                    .iter()
                    .any(|marker| marker.feature_ref.as_deref() == Some(native_ref))
            })
            .collect::<Vec<_>>();
        let markers = owned_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
            .filter(|marker| marker.coordinates_m.is_some())
            .collect::<Vec<_>>();
        let native_carriers = entities
            .iter()
            .filter(|entity| entity.sketch == *sketch_id)
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Native { .. }))
            .collect::<Vec<_>>();
        let has_resolved_curves = entities.iter().any(|entity| {
            entity.sketch == *sketch_id
                && matches!(
                    entity.geometry,
                    SketchGeometry::Line { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Circle { .. }
                        | SketchGeometry::Ellipse { .. }
                        | SketchGeometry::Nurbs { .. }
                )
        });
        let circle_only_carrier = match native_carriers.as_slice() {
            [carrier] if !has_resolved_curves => {
                carrier.native_ref.as_ref().and_then(|reference| {
                    owned_lanes
                        .iter()
                        .find_map(|lane| {
                            lane.sketch_entities
                                .iter()
                                .find(|marker| marker.id == *reference)
                                .and_then(|marker| {
                                    let offset = usize::try_from(marker.offset).ok()?;
                                    extended_radial_circle_index(&lane.native_payload, offset)
                                })
                        })
                        .map(|radial_index| (carrier.id.clone(), reference.clone(), radial_index))
                })
            }
            _ => None,
        };
        if let Some((carrier_id, carrier_ref, radial_index)) = circle_only_carrier {
            let mut roster = markers
                .iter()
                .copied()
                .filter(|marker| {
                    matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                })
                .collect::<Vec<_>>();
            roster.sort_unstable_by_key(|marker| marker.offset);
            let centers = roster
                .iter()
                .enumerate()
                .filter_map(|(center_index, center)| {
                    let [cu, cv] = center.coordinates_m?;
                    let later = &roster[center_index + 1..];
                    radial_dimensions
                        .iter()
                        .all(|(_, radius)| {
                            later
                                .iter()
                                .filter(|radial| {
                                    let [ru, rv] = radial
                                        .coordinates_m
                                        .expect("coordinate markers carry coordinates");
                                    same_dimension_length(
                                        (ru - cu).hypot(rv - cv) * NATIVE_TO_IR,
                                        *radius,
                                    )
                                })
                                .count()
                                == 1
                        })
                        .then_some((center_index, *center))
                })
                .collect::<Vec<_>>();
            if let [(center_index, center_marker)] = centers.as_slice() {
                let [cu, cv] = center_marker
                    .coordinates_m
                    .expect("coordinate markers carry coordinates");
                let mut radii = roster[center_index + 1..]
                    .iter()
                    .filter_map(|radial| {
                        let [ru, rv] = radial.coordinates_m?;
                        let radius = (ru - cu).hypot(rv - cv) * NATIVE_TO_IR;
                        (radius.is_finite() && radius > QUANTUM).then_some(radius)
                    })
                    .collect::<Vec<_>>();
                radii.sort_by(f64::total_cmp);
                radii.dedup_by(|left, right| same_dimension_length(*left, *right));
                let carrier_radius = roster
                    .get(radial_index)
                    .and_then(|radial| radial.coordinates_m)
                    .map(|[ru, rv]| (ru - cu).hypot(rv - cv) * NATIVE_TO_IR);
                let native_center =
                    quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                let centers = transforms
                    .get(native_ref)
                    .into_iter()
                    .flatten()
                    .filter_map(|transform| transform.apply(native_center))
                    .collect::<HashSet<_>>();
                if let [center] = centers.into_iter().collect::<Vec<_>>().as_slice() {
                    let center = Point2::new(center.0 as f64 * QUANTUM, center.1 as f64 * QUANTUM);
                    let removed = carrier_id;
                    entities.retain(|entity| entity.id != removed);
                    let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                    else {
                        continue;
                    };
                    for profile in &mut sketch.profiles {
                        profile.retain(|usage| usage.entity != removed);
                    }
                    sketch.profiles.retain(|profile| !profile.is_empty());
                    let feature_key = feature
                        .id
                        .0
                        .rsplit_once('#')
                        .map_or(feature.id.0.as_str(), |(_, key)| key);
                    for (index, radius) in radii.into_iter().enumerate() {
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#radial-roster:{feature_key}:{index}"
                        ));
                        entities.push(SketchEntity {
                            id: entity_id.clone(),
                            sketch: sketch_id.clone(),
                            construction: false,
                            native_ref: carrier_radius
                                .is_some_and(|carrier| same_dimension_length(carrier, radius))
                                .then(|| carrier_ref.clone()),
                            geometry_ref: radial_dimensions
                                .iter()
                                .find(|(_, candidate)| same_dimension_length(*candidate, radius))
                                .and_then(|(parameter, _)| parameter.native_ref.clone()),
                            endpoint_refs: Vec::new(),
                            geometry: SketchGeometry::Circle {
                                center,
                                radius: Length(radius),
                            },
                        });
                        sketch.profiles.push(vec![SketchEntityUse {
                            entity: entity_id,
                            reversed: false,
                        }]);
                    }
                    continue;
                }
            }
        }
        let radial_records = owned_lanes
            .iter()
            .flat_map(|lane| {
                let range = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .map(|marker| marker.offset as usize)
                    .collect::<Vec<_>>();
                let start = range.iter().min().copied().unwrap_or(0);
                let end = range.iter().max().copied().unwrap_or(0);
                radial_records_by_lane
                    .get(lane.id.as_str())
                    .into_iter()
                    .flatten()
                    .filter(move |(offset, ..)| *offset >= start && *offset <= end)
                    .map(move |record| (*lane, *record))
            })
            .filter(|(lane, (offset, ..))| {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}");
                entities.iter().any(|entity| {
                    entity.sketch == *sketch_id
                        && entity.native_ref.as_deref() == Some(carrier_ref.as_str())
                        && matches!(entity.geometry, SketchGeometry::Native { .. })
                })
            })
            .collect::<Vec<_>>();
        let repeated_radial_sets = radial_records
            .iter()
            .flat_map(|(lane, (offset, radial_index, construction))| {
                if *construction {
                    return Vec::new();
                }
                let mut roster = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .filter(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>();
                roster.sort_unstable_by_key(|marker| marker.offset);
                radial_dimensions
                    .iter()
                    .filter_map(|(parameter, radius)| {
                        let pairs = terminal_repeated_radial_circle_pairs(
                            *radial_index,
                            &roster,
                            *radius / NATIVE_TO_IR,
                        )?;
                        Some((*lane, *offset, *parameter, *radius, pairs))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if let [(lane, offset, parameter, radius, pairs)] = repeated_radial_sets.as_slice() {
            let transformed = pairs
                .iter()
                .filter_map(|(center, _)| {
                    let [cu, cv] = center.coordinates_m?;
                    let native =
                        quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                    let centers = transforms
                        .get(native_ref)
                        .into_iter()
                        .flatten()
                        .filter_map(|transform| transform.apply(native))
                        .collect::<HashSet<_>>();
                    let centers = centers.into_iter().collect::<Vec<_>>();
                    let [(u, v)] = centers.as_slice() else {
                        return None;
                    };
                    Some(Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM))
                })
                .collect::<Vec<_>>();
            if transformed.len() == pairs.len() {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}");
                let pair_radial_object_indices = pairs
                    .iter()
                    .filter_map(|(_, radial)| radial.object_index)
                    .collect::<HashSet<_>>();
                let consumed_carrier_refs = radial_records_by_lane
                    .get(lane.id.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|(candidate_offset, candidate_radial_index, construction)| {
                        !*construction
                            && lane.sketch_entities.iter().any(|marker| {
                                marker.feature_ref.as_deref() == Some(native_ref)
                                    && marker.offset == *candidate_offset as u64
                            })
                            && (*candidate_offset == *offset
                                || pair_radial_object_indices.contains(
                                    &u32::try_from(*candidate_radial_index).unwrap_or(u32::MAX),
                                ))
                    })
                    .map(|(candidate_offset, ..)| {
                        format!("sldprt:feature-input:sketch-entity#{lane_key}:{candidate_offset}")
                    })
                    .collect::<HashSet<_>>();
                let removed = entities
                    .iter()
                    .filter(|entity| {
                        entity.sketch == *sketch_id
                            && entity.native_ref.as_deref().is_some_and(|native_ref| {
                                consumed_carrier_refs.contains(native_ref)
                            })
                    })
                    .map(|entity| entity.id.clone())
                    .collect::<HashSet<_>>();
                entities.retain(|entity| !removed.contains(&entity.id));
                let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                else {
                    continue;
                };
                for profile in &mut sketch.profiles {
                    profile.retain(|usage| !removed.contains(&usage.entity));
                }
                sketch.profiles.retain(|profile| !profile.is_empty());
                for (index, center) in transformed.into_iter().enumerate() {
                    let entity_id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#repeated-radial-circle:{lane_key}:{offset}:{index}"
                    ));
                    entities.push(SketchEntity {
                        id: entity_id.clone(),
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: (index == pairs.len() - 1).then(|| carrier_ref.clone()),
                        geometry_ref: parameter.native_ref.clone(),
                        endpoint_refs: Vec::new(),
                        geometry: SketchGeometry::Circle {
                            center,
                            radius: Length(*radius),
                        },
                    });
                    sketch.profiles.push(vec![SketchEntityUse {
                        entity: entity_id,
                        reversed: false,
                    }]);
                }
                continue 'feature;
            }
        }
        if !radial_records.is_empty() {
            let radial_record_count = radial_records.len();
            let mut resolved = Vec::with_capacity(radial_records.len());
            for (lane, (offset, radial_index, construction)) in radial_records {
                let mut roster = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .filter(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>();
                roster.sort_unstable_by_key(|marker| marker.offset);
                let Some(radial) = roster.get(radial_index).copied() else {
                    continue;
                };
                let [ru, rv] = radial
                    .coordinates_m
                    .expect("coordinate markers carry coordinates");
                let mut candidates = markers
                    .iter()
                    .copied()
                    .filter(|marker| marker.id != radial.id)
                    .filter_map(|marker| {
                        let [cu, cv] = marker.coordinates_m?;
                        let radius = (ru - cu).hypot(rv - cv) * NATIVE_TO_IR;
                        let parameters = radial_dimensions
                            .iter()
                            .filter(|(_, candidate)| same_dimension_length(*candidate, radius))
                            .collect::<Vec<_>>();
                        let [(parameter, radius)] = parameters.as_slice() else {
                            return None;
                        };
                        Some((
                            quantize(Point2::new(cu, cv), QUANTUM),
                            marker,
                            *parameter,
                            *radius,
                        ))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable_by_key(|(center, marker, _, _)| (*center, marker.offset));
                candidates.dedup_by_key(|(center, _, _, _)| *center);
                let [(center, marker, parameter, radius)] = candidates.as_slice() else {
                    continue;
                };
                resolved.push((
                    lane,
                    offset,
                    construction,
                    *center,
                    *marker,
                    *parameter,
                    *radius,
                ));
            }
            if resolved.len() == radial_record_count {
                let transformed = resolved
                    .iter()
                    .filter_map(|record| {
                        let native = quantize(
                            Point2::new(
                                record.3 .0 as f64 * QUANTUM * NATIVE_TO_IR,
                                record.3 .1 as f64 * QUANTUM * NATIVE_TO_IR,
                            ),
                            QUANTUM,
                        );
                        let centers = transforms
                            .get(native_ref)
                            .into_iter()
                            .flatten()
                            .filter_map(|transform| transform.apply(native))
                            .collect::<HashSet<_>>();
                        let centers = centers.into_iter().collect::<Vec<_>>();
                        let [(u, v)] = centers.as_slice() else {
                            return None;
                        };
                        Some((
                            record,
                            Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM),
                        ))
                    })
                    .collect::<Vec<_>>();
                if transformed.len() == resolved.len() {
                    let carrier_refs = resolved
                        .iter()
                        .map(|(lane, offset, ..)| {
                            format!(
                                "sldprt:feature-input:sketch-entity#{}:{offset}",
                                lane.id
                                    .rsplit_once('#')
                                    .map_or(lane.id.as_str(), |(_, key)| key)
                            )
                        })
                        .collect::<HashSet<_>>();
                    let center_refs = resolved
                        .iter()
                        .map(|record| record.4.id.as_str())
                        .collect::<HashSet<_>>();
                    let removed = entities
                        .iter()
                        .filter(|entity| {
                            entity.sketch == *sketch_id
                                && entity.native_ref.as_deref().is_some_and(|reference| {
                                    carrier_refs.contains(reference)
                                        || (center_refs.contains(reference)
                                            && !matches!(
                                                entity.geometry,
                                                SketchGeometry::Point { .. }
                                            ))
                                })
                        })
                        .map(|entity| entity.id.clone())
                        .collect::<HashSet<_>>();
                    entities.retain(|entity| !removed.contains(&entity.id));
                    let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                    else {
                        continue;
                    };
                    for profile in &mut sketch.profiles {
                        profile.retain(|usage| !removed.contains(&usage.entity));
                    }
                    sketch.profiles.retain(|profile| !profile.is_empty());
                    for (record, center) in transformed {
                        let lane_key = record
                            .0
                            .id
                            .rsplit_once('#')
                            .map_or(record.0.id.as_str(), |(_, key)| key);
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#radial-circle:{lane_key}:{}",
                            record.1
                        ));
                        entities.push(SketchEntity {
                            id: entity_id.clone(),
                            sketch: sketch_id.clone(),
                            construction: record.2,
                            native_ref: Some(format!(
                                "sldprt:feature-input:sketch-entity#{lane_key}:{}",
                                record.1
                            )),
                            geometry_ref: record.5.native_ref.clone(),
                            endpoint_refs: Vec::new(),
                            geometry: SketchGeometry::Circle {
                                center,
                                radius: Length(record.6),
                            },
                        });
                        if !record.2 {
                            sketch.profiles.push(vec![SketchEntityUse {
                                entity: entity_id,
                                reversed: false,
                            }]);
                        }
                    }
                    continue;
                }
            }
        }
        let centers = markers
            .iter()
            .copied()
            .filter(|marker| marker.kind == SketchInputKind::LineOrCircle)
            .collect::<Vec<_>>();
        let radial = markers
            .iter()
            .copied()
            .filter(|marker| {
                matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .collect::<Vec<_>>();
        let [center] = centers.as_slice() else {
            continue;
        };
        if radial.len() != radial_dimensions.len() {
            continue;
        }
        let [cu, cv] = center
            .coordinates_m
            .expect("coordinate markers carry coordinates");
        let matches = radial_dimensions
            .iter()
            .map(|(_, radius)| {
                radial
                    .iter()
                    .enumerate()
                    .filter_map(|(index, marker)| {
                        let [u, v] = marker
                            .coordinates_m
                            .expect("coordinate markers carry coordinates");
                        same_dimension_length((u - cu).hypot(v - cv) * NATIVE_TO_IR, *radius)
                            .then_some(index)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if matches.iter().any(|matches| matches.len() != 1)
            || matches
                .iter()
                .map(|matches| matches[0])
                .collect::<HashSet<_>>()
                .len()
                != radial.len()
        {
            continue;
        }
        let native_center = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
        let centers = transforms
            .get(native_ref)
            .into_iter()
            .flatten()
            .filter_map(|transform| transform.apply(native_center))
            .collect::<HashSet<_>>();
        let centers = centers.into_iter().collect::<Vec<_>>();
        let [(u, v)] = centers.as_slice() else {
            continue;
        };
        let center = Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM);
        let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        for (parameter, radius) in radial_dimensions {
            if entities.iter().any(|entity| {
                entity.sketch == *sketch_id
                    && matches!(&entity.geometry, SketchGeometry::Circle { center: existing, radius: existing_radius }
                        if quantize(*existing, QUANTUM) == quantize(center, QUANTUM)
                            && same_dimension_length(existing_radius.0, radius))
            }) {
                continue;
            }
            let feature_key = feature
                .id
                .0
                .rsplit_once('#')
                .map_or(feature.id.0.as_str(), |(_, key)| key);
            let entity_id = SketchEntityId(format!(
                "sldprt:model:sketch-entity#marker-circle:{}:{}",
                feature_key, parameter.ordinal
            ));
            entities.push(SketchEntity {
                id: entity_id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: parameter.native_ref.clone(),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center,
                    radius: Length(radius),
                },
            });
            sketch.profiles.push(vec![SketchEntityUse {
                entity: entity_id,
                reversed: false,
            }]);
        }
    }
}

#[cfg(test)]
mod dimensions_tests;
