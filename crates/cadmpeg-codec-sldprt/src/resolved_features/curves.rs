//! Marker arc, circle and rectangle profile resolution.

use super::compact_reference_planes::principal_sketch_frame;
use super::endpoints::{
    compact_legacy_code_one_line_endpoint_indices, compact_legacy_curve_endpoint_indices,
    marker_profile_curve_role, minor_arc_angles, minor_arc_geometry, one_based_u16_endpoint_pair,
    unique_arc_center_marker, wide_indexed_curve_endpoint_indices,
};
use super::markers::{
    compact_legacy_marker_body, finite_coordinate_pair, marker_native_code, sketch_marker_prefix_at,
};
use super::reference_geometry::reference_plane_frame_key;
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::transforms::quantize;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{alloc_filled, bounded_len, View};
use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry};
use std::collections::{HashMap, HashSet};

pub(super) const REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY: &str = "UAxisSource";
pub(super) const CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE: &str = "constructed-mid-plane";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SketchPlaneUAxisSource {
    Native,
    ConstructedMidPlane,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SketchPlaneFrame {
    pub(super) origin: Point3,
    pub(super) normal: Vector3,
    pub(super) u_axis: Vector3,
    pub(super) u_axis_source: SketchPlaneUAxisSource,
}

impl SketchPlaneFrame {
    pub(super) fn native((origin, normal, u_axis): (Point3, Vector3, Vector3)) -> Self {
        Self {
            origin,
            normal,
            u_axis,
            u_axis_source: SketchPlaneUAxisSource::Native,
        }
    }

    pub(super) fn from_frame(
        (origin, normal, u_axis): (Point3, Vector3, Vector3),
        u_axis_source: SketchPlaneUAxisSource,
    ) -> Self {
        Self {
            origin,
            normal,
            u_axis,
            u_axis_source,
        }
    }

    pub(super) fn as_tuple(self) -> (Point3, Vector3, Vector3) {
        (self.origin, self.normal, self.u_axis)
    }
}

fn feature_u_axis_source(feature: &cadmpeg_ir::features::Feature) -> SketchPlaneUAxisSource {
    if feature
        .source_properties
        .get(REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY)
        .map(String::as_str)
        == Some(CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE)
    {
        SketchPlaneUAxisSource::ConstructedMidPlane
    } else {
        SketchPlaneUAxisSource::Native
    }
}

pub(super) fn current_linked_semicircle_record(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) != payload.get(offset + 66..offset + 68)
        && payload.get(offset + 68..offset + 72) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 80..offset + 84) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 86..offset + 102)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 102..offset + 104) == Some(&[0; 2])
}

pub(super) fn resolve_two_center_semicircle_profile(
    payload: &[u8],
    markers: &[&SketchInputEntity],
    entities: &mut Vec<SketchEntity>,
    tolerance: f64,
) {
    let records = markers
        .iter()
        .copied()
        .filter(|marker| {
            usize::try_from(marker.offset)
                .ok()
                .is_some_and(|offset| current_linked_semicircle_record(payload, offset))
        })
        .collect::<Vec<_>>();
    let [first_record, second_record] = records.as_slice() else {
        return;
    };
    let record_refs = [first_record.id.as_str(), second_record.id.as_str()];
    let curve_entities = entities
        .iter()
        .filter(|entity| {
            matches!(
                entity.geometry,
                SketchGeometry::Line { .. }
                    | SketchGeometry::Arc { .. }
                    | SketchGeometry::Circle { .. }
                    | SketchGeometry::Ellipse { .. }
                    | SketchGeometry::Nurbs { .. }
                    | SketchGeometry::Native { .. }
            )
        })
        .collect::<Vec<_>>();
    if curve_entities.len() != 2
        || curve_entities.iter().any(|entity| {
            !entity
                .native_ref
                .as_deref()
                .is_some_and(|id| record_refs.contains(&id))
        })
    {
        return;
    }
    let points = entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Point { position } => Some((entity.native_ref.clone()?, position)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if points.len() != 6 {
        return;
    }
    let centers = points
        .iter()
        .enumerate()
        .filter_map(|(center_index, (center_ref, center))| {
            let pairs = points
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != center_index)
                .flat_map(|(first_index, first)| {
                    points
                        .iter()
                        .enumerate()
                        .skip(first_index + 1)
                        .filter(move |(second_index, _)| *second_index != center_index)
                        .filter_map(move |(_, second)| {
                            let midpoint = Point2::new(
                                (first.1.u + second.1.u) * 0.5,
                                (first.1.v + second.1.v) * 0.5,
                            );
                            let first_radius = (first.1.u - center.u).hypot(first.1.v - center.v);
                            let second_radius =
                                (second.1.u - center.u).hypot(second.1.v - center.v);
                            (same_dimension_length(midpoint.u, center.u)
                                && same_dimension_length(midpoint.v, center.v)
                                && first_radius > tolerance
                                && same_dimension_length(first_radius, second_radius))
                            .then_some((
                                [first.0.clone(), second.0.clone()],
                                [first.1, second.1],
                                first_radius,
                            ))
                        })
                })
                .collect::<Vec<_>>();
            let [(endpoint_refs, endpoints, radius)] = pairs.as_slice() else {
                return None;
            };
            let linked_records = records
                .iter()
                .copied()
                .filter(|record| {
                    record
                        .links
                        .iter()
                        .any(|link| link.entity_ref == **center_ref)
                })
                .collect::<Vec<_>>();
            let [record] = linked_records.as_slice() else {
                return None;
            };
            Some((
                record.id.clone(),
                (*center_ref).clone(),
                *center,
                endpoint_refs.clone(),
                *endpoints,
                *radius,
            ))
        })
        .collect::<Vec<_>>();
    let [first, second] = centers.as_slice() else {
        return;
    };
    if first.0 == second.0 || !same_dimension_length(first.5, second.5) {
        return;
    }
    let center_delta = Point2::new(second.2.u - first.2.u, second.2.v - first.2.v);
    let center_distance = center_delta.u.hypot(center_delta.v);
    if center_distance <= tolerance {
        return;
    }
    let direction = Point2::new(
        center_delta.u / center_distance,
        center_delta.v / center_distance,
    );
    let perpendicular = Point2::new(-direction.v, direction.u);
    let first_radial = Point2::new(first.4[0].u - first.2.u, first.4[0].v - first.2.v);
    let second_radial = Point2::new(second.4[0].u - second.2.u, second.4[0].v - second.2.v);
    if (first_radial.u * direction.u + first_radial.v * direction.v).abs() > tolerance
        || (second_radial.u * direction.u + second_radial.v * direction.v).abs() > tolerance
    {
        return;
    }
    let order_endpoints = |center: Point2, refs: &[String; 2], endpoints: [Point2; 2]| {
        let signed = endpoints.map(|point| {
            (
                (point.u - center.u) * perpendicular.u + (point.v - center.v) * perpendicular.v,
                point,
            )
        });
        if signed[0].0 > signed[1].0 {
            (
                [refs[0].clone(), refs[1].clone()],
                [signed[0].1, signed[1].1],
            )
        } else {
            (
                [refs[1].clone(), refs[0].clone()],
                [signed[1].1, signed[0].1],
            )
        }
    };
    let (first_refs, first_endpoints) = order_endpoints(first.2, &first.3, first.4);
    let (second_refs, second_endpoints) = order_endpoints(second.2, &second.3, second.4);
    let set_arc = |entity: &mut SketchEntity,
                   center: Point2,
                   radius: f64,
                   refs: &[String; 2],
                   endpoints: [Point2; 2],
                   reverse: bool| {
        let (start_ref, end_ref, start, end) = if reverse {
            (&refs[1], &refs[0], endpoints[1], endpoints[0])
        } else {
            (&refs[0], &refs[1], endpoints[0], endpoints[1])
        };
        entity.construction = false;
        entity.endpoint_refs = vec![start_ref.clone(), end_ref.clone()];
        entity.geometry = SketchGeometry::Arc {
            center,
            radius: Length(radius),
            start_angle: Angle((start.v - center.v).atan2(start.u - center.u)),
            end_angle: Angle((end.v - center.v).atan2(end.u - center.u)),
        };
    };
    let Some(first_entity) = entities
        .iter_mut()
        .find(|entity| entity.native_ref.as_deref() == Some(first.0.as_str()))
    else {
        return;
    };
    set_arc(
        first_entity,
        first.2,
        first.5,
        &first_refs,
        first_endpoints,
        false,
    );
    let sketch = first_entity.sketch.clone();
    let Some(second_entity) = entities
        .iter_mut()
        .find(|entity| entity.native_ref.as_deref() == Some(second.0.as_str()))
    else {
        return;
    };
    set_arc(
        second_entity,
        second.2,
        second.5,
        &second_refs,
        second_endpoints,
        true,
    );
    let sketch_key = sketch
        .0
        .rsplit_once('#')
        .map_or(sketch.0.as_str(), |(_, key)| key);
    for (index, (start_ref, end_ref, start, end)) in [
        (
            &first_refs[0],
            &second_refs[0],
            first_endpoints[0],
            second_endpoints[0],
        ),
        (
            &first_refs[1],
            &second_refs[1],
            first_endpoints[1],
            second_endpoints[1],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        entities.push(SketchEntity {
            id: SketchEntityId(format!(
                "sldprt:model:sketch-entity#linked-semicircle:{sketch_key}:{index}"
            )),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: vec![start_ref.clone(), end_ref.clone()],
            geometry: SketchGeometry::Line { start, end },
        });
    }
}

pub(super) fn compact_bounded_curve_tangent(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    let record_size = if wide_indexed_curve_endpoint_indices(payload, offset).is_some() {
        if sketch_marker_prefix_at(payload, offset.checked_add(92)?) {
            92
        } else if sketch_marker_prefix_at(payload, offset.checked_add(112)?) {
            112
        } else {
            return None;
        }
    } else {
        84
    };
    let detail = offset.checked_add(record_size)?;
    if !sketch_marker_prefix_at(payload, detail)
        || payload.get(detail + 5..detail + 13)
            != Some(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff])
        || payload.get(detail + 13..detail + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || payload.get(detail + 23..detail + 27) != payload.get(offset + 23..offset + 27)
        || marker_profile_curve_role(payload, detail) != Some(2)
        || payload.get(detail + 31..detail + 35) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || payload.get(detail + 35..detail + 39) != Some(&[0x00, 0x00, 0x0c, 0x00])
        || View::f64_le_at(payload, detail + 48)? != 1.0
    {
        return None;
    }
    let u = View::f64_le_at(payload, detail + 64)?;
    let v = View::f64_le_at(payload, detail + 72)?;
    (u.is_finite() && v.is_finite() && (u.hypot(v) - 1.0).abs() <= 1.0e-9).then_some([u, v])
}

pub(super) fn tangent_bounded_curve(
    start: Point2,
    end: Point2,
    tangent: [f64; 2],
    tolerance: f64,
) -> Option<SketchGeometry> {
    let tangent_length = tangent[0].hypot(tangent[1]);
    if !tangent_length.is_finite() || tangent_length <= tolerance {
        return None;
    }
    let tangent = [tangent[0] / tangent_length, tangent[1] / tangent_length];
    let chord = [end.u - start.u, end.v - start.v];
    let chord_length = chord[0].hypot(chord[1]);
    if !chord_length.is_finite() || chord_length <= tolerance {
        return None;
    }
    let cross = tangent[0] * chord[1] - tangent[1] * chord[0];
    if cross.abs() <= tolerance * chord_length {
        return Some(SketchGeometry::Line { start, end });
    }
    let normal = [-tangent[1], tangent[0]];
    let denominator = 2.0 * (chord[0] * normal[0] + chord[1] * normal[1]);
    if !denominator.is_finite() || denominator.abs() <= tolerance {
        return None;
    }
    let scale = (chord[0] * chord[0] + chord[1] * chord[1]) / denominator;
    let center = Point2::new(start.u + normal[0] * scale, start.v + normal[1] * scale);
    let radius = (start.u - center.u).hypot(start.v - center.v);
    let end_radius = (end.u - center.u).hypot(end.v - center.v);
    if !radius.is_finite() || radius <= tolerance || !same_dimension_length(radius, end_radius) {
        return None;
    }
    let first = (start.v - center.v).atan2(start.u - center.u);
    let second = (end.v - center.v).atan2(end.u - center.u);
    let (start_angle, end_angle, _) = minor_arc_angles(first, second);
    Some(SketchGeometry::Arc {
        center,
        radius: Length(radius),
        start_angle: Angle(start_angle),
        end_angle: Angle(end_angle),
    })
}

pub(super) fn slot_curve_and_center_indices(
    payload: &[u8],
    offset: usize,
) -> Option<([usize; 4], [usize; 2])> {
    const SLOT_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    let layout = slot_curve_reference_cells(payload, offset)?;
    let declared = if payload.get(offset.checked_sub(SLOT_DECLARATION.len())?..offset)
        == Some(SLOT_DECLARATION)
    {
        true
    } else if let Some(stride) = layout.continuation_stride {
        let mut cursor = offset;
        loop {
            cursor = cursor.checked_sub(stride)?;
            if payload.get(cursor.checked_sub(SLOT_DECLARATION.len())?..cursor)
                == Some(SLOT_DECLARATION)
            {
                break true;
            }
            if slot_curve_reference_cells(payload, cursor)
                .is_none_or(|candidate| candidate.continuation_stride != Some(stride))
            {
                break false;
            }
        }
    } else {
        false
    };
    if !declared {
        return None;
    }
    Some((
        [
            layout.cells[0].1,
            layout.cells[1].1,
            layout.cells[2].1,
            layout.cells[3].1,
        ],
        [layout.cells[4].1, layout.cells[5].1],
    ))
}

pub(super) struct SlotReferenceLayout {
    cells: [(u16, usize); 6],
    continuation_stride: Option<usize>,
}

pub(super) fn slot_curve_reference_cells(
    payload: &[u8],
    offset: usize,
) -> Option<SlotReferenceLayout> {
    if marker_native_code(payload, offset).is_none()
        || payload.get(offset + 23..offset + 29) != Some(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    let layouts = match payload.get(offset..offset + SKETCH_MARKER.len()) {
        Some(prefix) if prefix == SKETCH_MARKER => vec![(72, 12, None)],
        Some(prefix) if prefix == LEGACY_SKETCH_MARKER => {
            vec![(64, 8, Some(126))]
        }
        Some(prefix) if prefix == LEGACY_EXTENDED_SKETCH_MARKER => {
            vec![(64, 8, Some(126)), (64, 12, None)]
        }
        _ => return None,
    };
    layouts
        .into_iter()
        .find_map(|(cells_offset, cell_size, continuation_stride)| {
            let cells: [(u16, usize); 6] = (0..6)
                .map(|index| {
                    let start = offset.checked_add(cells_offset + index * cell_size)?;
                    let cell = payload.get(start..start + cell_size)?;
                    (cell[4..8] == [0xff; 4]
                        && (cell_size == 8 || cell.get(8..12) == Some(&[0; 4])))
                    .then_some((
                        View::u16_le_at(cell, 0)?,
                        usize::from(View::u16_le_at(cell, 2)?),
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            let component_tag = cells[0].0;
            (component_tag != 0
                && cells[1].0 != 0
                && cells[1].0 != component_tag
                && cells[2].0 == component_tag
                && cells[3].0 == component_tag
                && cells[4].0 != 0
                && cells[4].0 != component_tag
                && cells[4].0 != cells[1].0
                && cells[5].0 == cells[4].0)
                .then_some(SlotReferenceLayout {
                    cells,
                    continuation_stride,
                })
        })
}

pub(super) fn resolve_slot_marker_arcs(
    payload: &[u8],
    markers: &[&SketchInputEntity],
    entities: &mut [SketchEntity],
    tolerance: f64,
) {
    let Some((curve_indices, center_indices)) = markers.iter().find_map(|marker| {
        let offset = usize::try_from(marker.offset).ok()?;
        slot_curve_and_center_indices(payload, offset)
    }) else {
        return;
    };
    let mut curves = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_none()
                && matches!(
                    marker.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
        })
        .collect::<Vec<_>>();
    curves.sort_unstable_by_key(|marker| marker.offset);
    if curves.len() != 4 {
        return;
    }
    let Some(cycle) = curve_indices
        .map(|index| curves.get(index).copied())
        .into_iter()
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if cycle
        .iter()
        .map(|marker| marker.id.as_str())
        .collect::<HashSet<_>>()
        .len()
        != 4
    {
        return;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let Some(center_refs) = center_indices
        .map(|index| points.get(index).map(|point| point.id.as_str()))
        .into_iter()
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if center_refs[0] == center_refs[1] {
        return;
    }
    let by_native_ref = entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| Some((entity.native_ref.as_deref()?, index)))
        .collect::<HashMap<_, _>>();
    let Some(cycle_entities) = cycle
        .iter()
        .map(|marker| by_native_ref.get(marker.id.as_str()).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let native_arcs = cycle_entities
        .iter()
        .copied()
        .filter(|index| {
            matches!(
                entities[*index].geometry,
                SketchGeometry::Native { ref native_kind }
                    if native_kind == "sldprt:marker-geometry:2"
            )
        })
        .collect::<Vec<_>>();
    let resolved_arcs = cycle_entities
        .iter()
        .copied()
        .filter(|index| matches!(entities[*index].geometry, SketchGeometry::Arc { .. }))
        .collect::<Vec<_>>();
    let lines = cycle_entities
        .iter()
        .copied()
        .filter(|index| matches!(entities[*index].geometry, SketchGeometry::Line { .. }))
        .count();
    let ([target], [resolved_arc]) = (native_arcs.as_slice(), resolved_arcs.as_slice()) else {
        return;
    };
    if lines != 2 {
        return;
    }
    let Some(target_position) = cycle_entities
        .iter()
        .position(|candidate| candidate == target)
    else {
        return;
    };
    let Some(resolved_position) = cycle_entities
        .iter()
        .position(|candidate| candidate == resolved_arc)
    else {
        return;
    };
    if (target_position + 2) % 4 != resolved_position {
        return;
    }
    let endpoint_refs = |index: usize| {
        let refs = entities[index].endpoint_refs.as_slice();
        let [first, second] = refs else {
            return None;
        };
        Some([first.as_str(), second.as_str()])
    };
    let endpoint_not_shared = |entity: usize, other: usize| {
        let entity = endpoint_refs(entity)?;
        let other = endpoint_refs(other)?;
        let unique = entity
            .into_iter()
            .filter(|endpoint| !other.contains(endpoint))
            .collect::<Vec<_>>();
        let [endpoint] = unique.as_slice() else {
            return None;
        };
        Some(*endpoint)
    };
    let previous = cycle_entities[(target_position + 3) % 4];
    let previous_other = cycle_entities[(target_position + 2) % 4];
    let next = cycle_entities[(target_position + 1) % 4];
    let next_other = cycle_entities[(target_position + 2) % 4];
    let Some(start_ref) = endpoint_not_shared(previous, previous_other) else {
        return;
    };
    let Some(end_ref) = endpoint_not_shared(next, next_other) else {
        return;
    };
    if start_ref == end_ref {
        return;
    }
    let point_positions = entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Point { position } => Some((entity.native_ref.as_deref()?, position)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let (Some(start), Some(end)) = (
        point_positions.get(start_ref).copied(),
        point_positions.get(end_ref).copied(),
    ) else {
        return;
    };
    let Some(centers) = center_refs
        .iter()
        .map(|reference| point_positions.get(reference).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let SketchGeometry::Arc { center: used, .. } = entities[*resolved_arc].geometry else {
        return;
    };
    let remaining = centers
        .into_iter()
        .filter(|center| {
            !same_dimension_length(center.u, used.u) || !same_dimension_length(center.v, used.v)
        })
        .collect::<Vec<_>>();
    let [center] = remaining.as_slice() else {
        return;
    };
    let Some(geometry) = minor_arc_geometry(start, end, *center, tolerance) else {
        return;
    };
    entities[*target].endpoint_refs = vec![start_ref.to_string(), end_ref.to_string()];
    entities[*target].geometry = geometry;
}

pub(super) fn resolve_connected_marker_arcs(entities: &mut [SketchEntity], tolerance: f64) {
    let points = entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Point { position } => Some((entity.native_ref.clone()?, position)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let point_records = entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Point { position } => {
                Some((entity.sketch.clone(), entity.native_ref.clone()?, position))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let center_replacements = entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| {
            if !matches!(
                entity.geometry,
                SketchGeometry::Native { ref native_kind }
                    if native_kind == "sldprt:marker-geometry:2"
            ) {
                return None;
            }
            let [start_ref, end_ref] = entity.endpoint_refs.as_slice() else {
                return None;
            };
            let start = points.get(start_ref).copied()?;
            let end = points.get(end_ref).copied()?;
            let candidates = point_records
                .iter()
                .filter(|(sketch, reference, _)| {
                    sketch == &entity.sketch && reference != start_ref && reference != end_ref
                })
                .map(|(_, _, center)| *center)
                .collect::<Vec<_>>();
            let center = unique_arc_center_marker(start, end, &candidates, tolerance)?;
            minor_arc_geometry(start, end, center, tolerance).map(|geometry| (index, geometry))
        })
        .collect::<Vec<_>>();
    for (index, geometry) in center_replacements {
        entities[index].geometry = geometry;
    }
    let arcs = entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| {
            (entity.endpoint_refs.len() == 2
                && matches!(
                    entity.geometry,
                    SketchGeometry::Native { ref native_kind }
                        if native_kind == "sldprt:marker-geometry:2"
                ))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut replacements = Vec::new();
    for first in arcs.iter().copied() {
        if !visited.insert(first) {
            continue;
        }
        let mut component = vec![first];
        let mut cursor = 0;
        while let Some(&current) = component.get(cursor) {
            cursor += 1;
            for candidate in arcs.iter().copied() {
                if visited.contains(&candidate)
                    || !entities[current]
                        .endpoint_refs
                        .iter()
                        .any(|endpoint| entities[candidate].endpoint_refs.contains(endpoint))
                {
                    continue;
                }
                visited.insert(candidate);
                component.push(candidate);
            }
        }
        let mut endpoint_refs = component
            .iter()
            .flat_map(|index| &entities[*index].endpoint_refs)
            .collect::<Vec<_>>();
        endpoint_refs.sort_unstable();
        endpoint_refs.dedup();
        let Some(component_points) = endpoint_refs
            .iter()
            .map(|endpoint| points.get(endpoint.as_str()).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some((center, _)) = fitted_marker_circle(&component_points, tolerance) else {
            continue;
        };
        let mut component_replacements = Vec::new();
        for index in component {
            let [start_ref, end_ref] = entities[index].endpoint_refs.as_slice() else {
                continue;
            };
            let (Some(start), Some(end)) = (
                points.get(start_ref.as_str()).copied(),
                points.get(end_ref.as_str()).copied(),
            ) else {
                component_replacements.clear();
                break;
            };
            let Some(geometry) = minor_arc_geometry(start, end, center, tolerance) else {
                component_replacements.clear();
                break;
            };
            component_replacements.push((index, geometry));
        }
        if component_replacements.len() >= 2 {
            replacements.extend(component_replacements);
        }
    }
    for (index, geometry) in replacements {
        entities[index].geometry = geometry;
    }
    for entity in entities {
        let SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            ..
        } = entity.geometry
        else {
            continue;
        };
        let [first_ref, second_ref] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        let (Some(first), Some(second)) = (points.get(first_ref), points.get(second_ref)) else {
            continue;
        };
        let geometry_start = Point2::new(
            center.u + radius.0 * start_angle.0.cos(),
            center.v + radius.0 * start_angle.0.sin(),
        );
        let first_distance = (geometry_start.u - first.u).hypot(geometry_start.v - first.v);
        let second_distance = (geometry_start.u - second.u).hypot(geometry_start.v - second.v);
        if second_distance < first_distance {
            entity.endpoint_refs.reverse();
        }
    }
}

pub(super) fn closed_marker_profiles(entities: &[SketchEntity]) -> Vec<Vec<SketchEntityUse>> {
    closed_marker_profiles_with_policy(entities, true)
}

/// Recover closed curve cycles when endpoint markers are shared by construction geometry.
pub(super) fn closed_marker_profiles_allowing_shared_endpoints(
    entities: &[SketchEntity],
) -> Vec<Vec<SketchEntityUse>> {
    closed_marker_profiles_with_policy(entities, false)
}

fn closed_marker_profiles_with_policy(
    entities: &[SketchEntity],
    reject_branching_components: bool,
) -> Vec<Vec<SketchEntityUse>> {
    let mut profiles = entities
        .iter()
        .filter(|entity| {
            !entity.construction && matches!(entity.geometry, SketchGeometry::Circle { .. })
        })
        .map(|entity| {
            vec![SketchEntityUse {
                entity: entity.id.clone(),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let curves = entities
        .iter()
        .enumerate()
        .filter(|(_, entity)| {
            !entity.construction
                && entity.endpoint_refs.len() == 2
                && matches!(
                    entity.geometry,
                    SketchGeometry::Line { .. } | SketchGeometry::Arc { .. }
                )
        })
        .collect::<Vec<_>>();
    let mut incidence = HashMap::<&str, Vec<usize>>::new();
    for (index, entity) in &curves {
        for endpoint in &entity.endpoint_refs {
            incidence.entry(endpoint).or_default().push(*index);
        }
    }
    let mut unused = curves
        .iter()
        .map(|(index, _)| *index)
        .collect::<HashSet<_>>();
    while let Some(&first) = unused.iter().min() {
        let mut component = HashSet::from([first]);
        let mut frontier = vec![first];
        while let Some(curve) = frontier.pop() {
            for endpoint in &entities[curve].endpoint_refs {
                for adjacent in incidence.get(endpoint.as_str()).into_iter().flatten() {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        if reject_branching_components
            && component.iter().any(|curve| {
                entities[*curve].endpoint_refs.iter().any(|endpoint| {
                    incidence
                        .get(endpoint.as_str())
                        .is_none_or(|curves| curves.len() != 2)
                })
            })
        {
            unused.retain(|curve| !component.contains(curve));
            continue;
        }
        let start = entities[first].endpoint_refs[0].as_str();
        let mut current = start;
        let mut curve = first;
        let mut profile = Vec::new();
        loop {
            if !unused.remove(&curve) {
                profile.clear();
                break;
            }
            let [curve_start, curve_end] = entities[curve].endpoint_refs.as_slice() else {
                profile.clear();
                break;
            };
            let (reversed, next) = if curve_start == current {
                (false, curve_end.as_str())
            } else if curve_end == current {
                (true, curve_start.as_str())
            } else {
                profile.clear();
                break;
            };
            profile.push(SketchEntityUse {
                entity: entities[curve].id.clone(),
                reversed,
            });
            current = next;
            if current == start {
                break;
            }
            let Some(candidates) = incidence.get(current) else {
                profile.clear();
                break;
            };
            if reject_branching_components && candidates.len() != 2 {
                profile.clear();
                break;
            }
            let Some(next_curve) = candidates
                .iter()
                .copied()
                .find(|index| unused.contains(index))
            else {
                profile.clear();
                break;
            };
            curve = next_curve;
        }
        if profile.len() >= 2 {
            profiles.push(profile);
        }
    }
    profiles
}

pub(super) fn fitted_marker_circle(points: &[Point2], tolerance: f64) -> Option<(Point2, f64)> {
    let [first, rest @ ..] = points else {
        return None;
    };
    for (second_index, second) in rest.iter().enumerate() {
        for third in &rest[second_index + 1..] {
            let determinant = 2.0
                * (first.u * (second.v - third.v)
                    + second.u * (third.v - first.v)
                    + third.u * (first.v - second.v));
            if !determinant.is_finite() || determinant.abs() <= tolerance * tolerance {
                continue;
            }
            let first_norm = first.u * first.u + first.v * first.v;
            let second_norm = second.u * second.u + second.v * second.v;
            let third_norm = third.u * third.u + third.v * third.v;
            let center = Point2::new(
                (first_norm * (second.v - third.v)
                    + second_norm * (third.v - first.v)
                    + third_norm * (first.v - second.v))
                    / determinant,
                (first_norm * (third.u - second.u)
                    + second_norm * (first.u - third.u)
                    + third_norm * (second.u - first.u))
                    / determinant,
            );
            let radius = (first.u - center.u).hypot(first.v - center.v);
            if radius.is_finite()
                && radius > tolerance
                && points.iter().all(|point| {
                    same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius)
                })
            {
                return Some((center, radius));
            }
        }
    }
    None
}

pub(super) fn sketch_plane_frames(
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
) -> HashMap<u32, SketchPlaneFrame> {
    let source_by_feature = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            Some((
                features
                    .iter()
                    .find(|neutral| neutral.native_ref.as_deref() == Some(feature.id.as_str()))?
                    .id
                    .clone(),
                feature.source_id.as_deref()?.parse::<u32>().ok()?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut frames_by_feature = features
        .iter()
        .filter_map(|feature| {
            let frame = match feature.definition {
                cadmpeg_ir::features::FeatureDefinition::DatumPrincipalPlane { plane } => {
                    SketchPlaneFrame::native(principal_sketch_frame(plane))
                }
                cadmpeg_ir::features::FeatureDefinition::DatumPlane {
                    origin,
                    normal,
                    u_axis,
                } => SketchPlaneFrame::from_frame(
                    (origin, normal, u_axis),
                    feature_u_axis_source(feature),
                ),
                _ => return None,
            };
            Some((feature.id.clone(), frame))
        })
        .collect::<HashMap<_, _>>();
    loop {
        let derived = features
            .iter()
            .filter(|feature| !frames_by_feature.contains_key(&feature.id))
            .filter_map(|feature| {
                let cadmpeg_ir::features::FeatureDefinition::DatumOffsetPlane {
                    reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(reference)),
                    distance,
                } = &feature.definition
                else {
                    return None;
                };
                let frame = *frames_by_feature.get(reference)?;
                Some((
                    feature.id.clone(),
                    SketchPlaneFrame {
                        origin: Point3::new(
                            frame.origin.x + frame.normal.x * distance.0,
                            frame.origin.y + frame.normal.y * distance.0,
                            frame.origin.z + frame.normal.z * distance.0,
                        ),
                        ..frame
                    },
                ))
            })
            .collect::<Vec<_>>();
        if derived.is_empty() {
            break;
        }
        frames_by_feature.extend(derived);
    }
    source_by_feature
        .into_iter()
        .filter_map(|(feature, source)| Some((source, *frames_by_feature.get(&feature)?)))
        .collect()
}

pub(super) fn lane_sketch_plane_frames(
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> HashMap<u32, SketchPlaneFrame> {
    let mut frames = sketch_plane_frames(features, histories);
    let mut lane_candidates = HashMap::<u32, Vec<SketchPlaneFrame>>::new();
    for native in histories.iter().flat_map(|history| &history.features) {
        let Some(source) = feature_object_name(native, lane).and_then(|name| name.object_id) else {
            continue;
        };
        let Some(feature) = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(native.id.as_str()))
        else {
            continue;
        };
        let frame = match feature.definition {
            FeatureDefinition::DatumPrincipalPlane { plane } => {
                SketchPlaneFrame::native(principal_sketch_frame(plane))
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } => SketchPlaneFrame::from_frame(
                (origin, normal, u_axis),
                feature_u_axis_source(feature),
            ),
            _ => continue,
        };
        lane_candidates.entry(source).or_default().push(frame);
    }
    for (source, mut candidates) in lane_candidates {
        candidates.sort_by_key(|frame| {
            (
                reference_plane_frame_key(&frame.as_tuple()),
                frame.u_axis_source,
            )
        });
        candidates.dedup_by_key(|frame| reference_plane_frame_key(&frame.as_tuple()));
        if let [frame] = candidates.as_slice() {
            frames.entry(source).or_insert(*frame);
        }
    }
    frames
}

pub(super) fn ordered_rectangle_corners(points: &[Point2]) -> Option<[Point2; 4]> {
    let [_, _, _, _] = points else {
        return None;
    };
    let mut u = points.iter().map(|point| point.u).collect::<Vec<_>>();
    u.sort_by(f64::total_cmp);
    u.dedup();
    let mut v = points.iter().map(|point| point.v).collect::<Vec<_>>();
    v.sort_by(f64::total_cmp);
    v.dedup();
    let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
        return None;
    };
    let corners = [
        Point2::new(*u0, *v0),
        Point2::new(*u1, *v0),
        Point2::new(*u1, *v1),
        Point2::new(*u0, *v1),
    ];
    corners
        .iter()
        .all(|corner| points.iter().filter(|point| *point == corner).count() == 1)
        .then_some(corners)
}

fn ordered_tolerant_rectangle_corners(points: &[Point2]) -> Option<[Point2; 4]> {
    let [_, _, _, _] = points else {
        return None;
    };
    let mut u = points.iter().map(|point| point.u).collect::<Vec<_>>();
    u.sort_by(f64::total_cmp);
    u.dedup_by(|left, right| same_dimension_length(*left, *right));
    let mut v = points.iter().map(|point| point.v).collect::<Vec<_>>();
    v.sort_by(f64::total_cmp);
    v.dedup_by(|left, right| same_dimension_length(*left, *right));
    let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
        return None;
    };
    let corners = [
        Point2::new(*u0, *v0),
        Point2::new(*u1, *v0),
        Point2::new(*u1, *v1),
        Point2::new(*u0, *v1),
    ];
    corners
        .iter()
        .all(|corner| {
            points
                .iter()
                .filter(|point| {
                    same_dimension_length(point.u, corner.u)
                        && same_dimension_length(point.v, corner.v)
                })
                .count()
                == 1
        })
        .then_some(corners)
}

pub(super) fn indexed_rectangle_from_line_cycle(
    payload: &[u8],
    markers: &[&SketchInputEntity],
) -> Option<[Point2; 4]> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum EndpointSpace {
        Roster,
        Object,
    }

    let mut roster = markers.to_vec();
    roster.sort_unstable_by_key(|marker| marker.offset);
    let records = markers
        .iter()
        .filter_map(|marker| {
            let offset = usize::try_from(marker.offset).ok()?;
            if let Some(endpoints) = legacy_extended_rectangle_line_endpoints(payload, offset) {
                return (marker.kind == SketchInputKind::LineOrCircle).then_some((
                    endpoints,
                    None,
                    false,
                    EndpointSpace::Roster,
                ));
            }
            if let Some(endpoints) = current_compact_rectangle_line_endpoints(payload, offset) {
                return matches!(
                    marker.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
                .then_some((endpoints, None, false, EndpointSpace::Object));
            }
            if let Some(endpoints) = compact_legacy_rectangle_line_endpoints(payload, offset) {
                return (marker.kind == SketchInputKind::LineOrCircle).then_some((
                    endpoints,
                    None,
                    false,
                    EndpointSpace::Object,
                ));
            }
            if let Some(endpoints) = compact_legacy_curve_endpoint_indices(payload, offset)
                .or_else(|| compact_legacy_code_one_line_endpoint_indices(payload, offset))
            {
                return (marker.kind == SketchInputKind::LineOrCircle).then_some((
                    endpoints,
                    None,
                    false,
                    EndpointSpace::Object,
                ));
            }
            let endpoints = current_wide_rectangle_line_endpoints(payload, offset)?;
            if endpoints.iter().any(|endpoint| {
                usize::try_from(*endpoint)
                    .ok()
                    .and_then(|endpoint| roster.get(endpoint))
                    .is_none_or(|endpoint| {
                        endpoint.coordinates_m.is_none()
                            || !matches!(
                                endpoint.kind,
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    })
            }) {
                return None;
            }
            matches!(
                marker.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
            .then_some((
                endpoints,
                marker_native_code(payload, offset),
                payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00]),
                EndpointSpace::Roster,
            ))
        })
        .collect::<Vec<_>>();
    let mut endpoint_spaces = records
        .iter()
        .map(|(_, _, _, space)| *space)
        .collect::<Vec<_>>();
    endpoint_spaces.sort_unstable_by_key(|space| match space {
        EndpointSpace::Roster => 0,
        EndpointSpace::Object => 1,
    });
    endpoint_spaces.dedup();
    let [endpoint_space] = endpoint_spaces.as_slice() else {
        return None;
    };
    let current_codes = records
        .iter()
        .filter_map(|(_, current_code, _, _)| *current_code)
        .collect::<Vec<_>>();
    if !(current_codes.is_empty()
        || current_codes.len() == 4
            && current_codes.iter().filter(|code| **code == 1).count() == 3
            && current_codes.iter().filter(|code| **code == 2).count() == 1
        || current_codes.len() == 3
            && current_codes.iter().filter(|code| **code == 1).count() == 2
            && current_codes.iter().filter(|code| **code == 2).count() == 1)
    {
        return None;
    }
    let edges = records
        .into_iter()
        .map(|(endpoints, _, alternate_locus, _)| (endpoints, alternate_locus))
        .collect::<Vec<_>>();
    if !matches!(edges.len(), 3 | 4) || edges.len() == 3 && current_codes.len() != 3 {
        return None;
    }
    if edges.len() == 4 && edges.iter().any(|(_, alternate_locus)| *alternate_locus) {
        return None;
    }
    let mut edges = edges
        .into_iter()
        .map(|(endpoints, _)| endpoints)
        .collect::<Vec<_>>();
    let edge_count = edges.len();
    edges.sort_unstable();
    edges.dedup();
    if edges.len() != edge_count || edges.iter().any(|edge| edge[0] == edge[1]) {
        return None;
    }
    let mut vertices = edges.iter().flatten().copied().collect::<Vec<_>>();
    vertices.sort_unstable();
    vertices.dedup();
    let [_, _, _, _] = vertices.as_slice() else {
        return None;
    };
    let mut degrees = vertices
        .iter()
        .map(|vertex| edges.iter().filter(|edge| edge.contains(vertex)).count())
        .collect::<Vec<_>>();
    degrees.sort_unstable();
    if !matches!(degrees.as_slice(), [2, 2, 2, 2] | [1, 1, 2, 2]) {
        return None;
    }
    let mut known = vertices
        .iter()
        .filter_map(|vertex| {
            let marker = match endpoint_space {
                EndpointSpace::Roster => *roster.get(usize::try_from(*vertex).ok()?)?,
                EndpointSpace::Object => {
                    let mut candidates = markers.iter().copied().filter(|marker| {
                        marker.object_index == Some(*vertex)
                            && marker.coordinates_m.is_some()
                            && matches!(
                                marker.kind,
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    });
                    let marker = candidates.next()?;
                    candidates.next().is_none().then_some(marker)?
                }
            };
            (matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) && marker.coordinates_m.is_some())
            .then_some((*vertex, marker.coordinates_m?))
        })
        .collect::<Vec<_>>();
    known.sort_unstable_by_key(|(vertex, _)| *vertex);
    if edges.len() == 3 && known.len() != 4 {
        return None;
    }
    let corners = match known.as_slice() {
        [(first_vertex, [first_u, first_v]), (second_vertex, [second_u, second_v])] => {
            if edges
                .iter()
                .any(|edge| edge.contains(first_vertex) && edge.contains(second_vertex))
                || first_u == second_u
                || first_v == second_v
            {
                return None;
            }
            vec![
                Point2::new(*first_u, *first_v),
                Point2::new(*first_u, *second_v),
                Point2::new(*second_u, *first_v),
                Point2::new(*second_u, *second_v),
            ]
        }
        [_, _, _] => {
            let axis_aligned = (|| {
                let mut u = Vec::<f64>::new();
                let mut v = Vec::<f64>::new();
                for (_, [point_u, point_v]) in &known {
                    if u.iter()
                        .all(|candidate| !same_dimension_length(*candidate, *point_u))
                    {
                        u.push(*point_u);
                    }
                    if v.iter()
                        .all(|candidate| !same_dimension_length(*candidate, *point_v))
                    {
                        v.push(*point_v);
                    }
                }
                u.sort_by(f64::total_cmp);
                v.sort_by(f64::total_cmp);
                let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
                    return None;
                };
                let products = [
                    Point2::new(*u0, *v0),
                    Point2::new(*u1, *v0),
                    Point2::new(*u1, *v1),
                    Point2::new(*u0, *v1),
                ];
                let mut occupied = [false; 4];
                for (_, [point_u, point_v]) in &known {
                    let mut matches = products.iter().enumerate().filter(|(_, product)| {
                        same_dimension_length(product.u, *point_u)
                            && same_dimension_length(product.v, *point_v)
                    });
                    let (index, _) = matches.next()?;
                    if matches.next().is_some() || occupied[index] {
                        return None;
                    }
                    occupied[index] = true;
                }
                (occupied.iter().filter(|occupied| **occupied).count() == 3)
                    .then_some(products.to_vec())
            })();
            if let Some(corners) = axis_aligned {
                corners
            } else {
                let missing = *vertices
                    .iter()
                    .find(|vertex| known.iter().all(|(known, _)| known != *vertex))?;
                let neighbors = edges
                    .iter()
                    .filter(|edge| edge.contains(&missing))
                    .map(|edge| edge[usize::from(edge[0] == missing)])
                    .collect::<Vec<_>>();
                let [first_neighbor, second_neighbor] = neighbors.as_slice() else {
                    return None;
                };
                let opposite = *vertices.iter().find(|vertex| {
                    **vertex != missing
                        && **vertex != *first_neighbor
                        && **vertex != *second_neighbor
                })?;
                let coordinates = |vertex| {
                    known
                        .iter()
                        .find_map(|(known, coordinates)| (*known == vertex).then_some(*coordinates))
                };
                let [first_u, first_v] = coordinates(*first_neighbor)?;
                let [second_u, second_v] = coordinates(*second_neighbor)?;
                let [opposite_u, opposite_v] = coordinates(opposite)?;
                let inferred = [
                    first_u + second_u - opposite_u,
                    first_v + second_v - opposite_v,
                ];
                known
                    .iter()
                    .map(|(_, [u, v])| Point2::new(*u, *v))
                    .chain(std::iter::once(Point2::new(inferred[0], inferred[1])))
                    .collect()
            }
        }
        [_, _, _, _] => known
            .iter()
            .map(|(_, [u, v])| Point2::new(*u, *v))
            .collect(),
        _ => return None,
    };
    let corners = corners
        .iter()
        .all(|corner| corner.u.is_finite() && corner.v.is_finite())
        .then(|| {
            if edges.len() == 3 {
                ordered_tolerant_rectangle_corners(&corners)
            } else {
                ordered_rectangle_corners(&corners)
            }
        })
        .flatten()?;
    let coordinates = known.iter().copied().collect::<HashMap<_, _>>();
    (edges.len() == 4
        || edges.iter().all(|[first, second]| {
            let (Some(first), Some(second)) = (coordinates.get(first), coordinates.get(second))
            else {
                return false;
            };
            same_dimension_length(first[0], second[0]) ^ same_dimension_length(first[1], second[1])
        }))
    .then_some(corners)
}

pub(super) fn compact_legacy_rectangle_line_endpoints(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !compact_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(1)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 25..offset + 27) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 42) != Some(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 46..offset + 50) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 42)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn legacy_extended_rectangle_line_endpoints(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !matches!(marker_native_code(payload, offset), Some(1 | 2))
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || payload.get(offset + 27..offset + 29) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 74) != Some(&[0; 2])
    {
        return None;
    }
    let endpoint = |relative: usize| View::u16_le_at(payload, offset + relative).map(u32::from);
    let endpoints = [endpoint(56)?, endpoint(58)?];
    let terminal_state = View::u16_le_at(payload, offset + 74)?;
    let continued = sketch_marker_prefix_at(payload, offset.saturating_add(84));
    let terminal = payload.get(offset + 72..offset + 84) == Some(&[0; 12]);
    (matches!(terminal_state, 0 | 2) && endpoints[0] != endpoints[1] && (continued || terminal))
        .then_some(endpoints)
}

pub(super) fn current_compact_rectangle_line_endpoints(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !matches!(marker_native_code(payload, offset), Some(1 | 2))
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 74) != Some(&[0; 2])
    {
        return None;
    }
    let endpoints = one_based_u16_endpoint_pair(payload, offset, 56)?;
    let terminal_state = View::u16_le_at(payload, offset + 74)?;
    let continued = sketch_marker_prefix_at(payload, offset.saturating_add(84));
    let terminal = payload.get(offset + 72..offset + 84) == Some(&[0; 12]);
    (matches!(terminal_state, 0 | 2) && endpoints[0] != endpoints[1] && (continued || terminal))
        .then_some(endpoints)
}

pub(super) fn current_wide_rectangle_line_endpoints(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || !matches!(marker_native_code(payload, offset), Some(1 | 2))
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || wide_indexed_curve_endpoint_indices(payload, offset).is_none()
        || !sketch_marker_prefix_at(payload, offset.saturating_add(92))
    {
        return None;
    }
    let endpoint = |relative: usize| View::u16_le_at(payload, offset + relative).map(u32::from);
    let endpoints = [endpoint(64)?, endpoint(66)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn legacy_extended_rectangle_diagonal_endpoint(
    payload: &[u8],
    marker: &SketchInputEntity,
) -> Option<[f64; 2]> {
    let offset = usize::try_from(marker.offset).ok()?;
    if marker.kind != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
            != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
    {
        return None;
    }
    let (first, second, tail_valid) = if payload.get(offset + 74..offset + 78)
        == Some(&[0x00, 0x00, 0x03, 0x00])
    {
        let identity_end = payload.get(offset + 100..offset + 136) == Some(&[0; 36])
            && payload.get(offset + 136..offset + 140) == Some(&1u32.to_le_bytes())
            && payload.get(offset + 140..offset + 142) == Some(&[0; 2])
            && payload
                .get(offset + 142..offset + 146)
                .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
            && sketch_marker_prefix_at(payload, offset.saturating_add(146));
        let terminal_end = payload.get(offset + 100..offset + 142) == Some(&[0; 42])
            && payload.get(offset + 142..offset + 146) == Some(&[0xff; 4])
            && sketch_marker_prefix_at(payload, offset.saturating_add(146));
        (
            payload.get(offset + 78..offset + 86)?,
            payload.get(offset + 86..offset + 94)?,
            payload.get(offset + 94..offset + 100) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
                && (identity_end || terminal_end),
        )
    } else {
        return None;
    };
    if first[..2] != second[..2]
        || first[..2] == [0; 2]
        || first[2..4] == [0; 2]
        || second[2..4] == [0; 2]
        || first[2..4] == second[2..4]
        || first[4..8] != [0xff; 4]
        || second[4..8] != [0xff; 4]
        || !tail_valid
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

pub(super) fn unique_dimensioned_rectangle_markers<'a>(
    markers: &[&'a SketchInputEntity],
    dimensions_mm: &[f64],
) -> Option<[&'a SketchInputEntity; 4]> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    if dimensions_mm.len() < 2 {
        return None;
    }
    let points = markers
        .iter()
        .filter_map(|marker| {
            let [u, v] = marker.coordinates_m?;
            Some((
                *marker,
                quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM),
            ))
        })
        .collect::<Vec<_>>();
    let mut u = points.iter().map(|(_, point)| point.0).collect::<Vec<_>>();
    u.sort_unstable();
    u.dedup();
    let mut v = points.iter().map(|(_, point)| point.1).collect::<Vec<_>>();
    v.sort_unstable();
    v.dedup();
    let dimensions_match = |u0: i64, u1: i64, v0: i64, v1: i64| {
        let u_span = (u1 - u0) as f64 * QUANTUM;
        let v_span = (v1 - v0) as f64 * QUANTUM;
        dimensions_mm
            .iter()
            .enumerate()
            .any(|(first_index, first)| {
                dimensions_mm
                    .iter()
                    .enumerate()
                    .any(|(second_index, second)| {
                        first_index != second_index
                            && ((same_dimension_length(*first, u_span)
                                && same_dimension_length(*second, v_span))
                                || (same_dimension_length(*first, v_span)
                                    && same_dimension_length(*second, u_span)))
                    })
            })
    };
    let mut candidates = Vec::new();
    for (first_u_index, &u0) in u.iter().enumerate() {
        for &u1 in &u[first_u_index + 1..] {
            for (first_v_index, &v0) in v.iter().enumerate() {
                for &v1 in &v[first_v_index + 1..] {
                    if !dimensions_match(u0, u1, v0, v1) {
                        continue;
                    }
                    let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
                    let matched = corners.map(|corner| {
                        let mut matches = points
                            .iter()
                            .filter(|(_, point)| *point == corner)
                            .map(|(marker, _)| *marker);
                        let marker = matches.next()?;
                        matches.next().is_none().then_some(marker)
                    });
                    let [Some(first), Some(second), Some(third), Some(fourth)] = matched else {
                        continue;
                    };
                    candidates.push([first, second, third, fourth]);
                }
            }
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn ordered_compact_line_profile(
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
) -> Option<Vec<SketchEntityUse>> {
    if lines.len() < 3 {
        return None;
    }
    let mut used = alloc_filled(lines.len(), false, "SLDPRT compact line profile usage").ok()?;
    let mut profile = Vec::with_capacity(lines.len());
    let first = lines.first()?;
    used[0] = true;
    profile.push(SketchEntityUse {
        entity: first.0.clone(),
        reversed: false,
    });
    let origin = first.3;
    let mut current = first.4;
    while profile.len() < lines.len() {
        let mut candidates = lines.iter().enumerate().filter_map(|(index, line)| {
            if used[index] {
                None
            } else if line.3 == current {
                Some((index, false, line.4))
            } else if line.4 == current {
                Some((index, true, line.3))
            } else {
                None
            }
        });
        let candidate = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        used[candidate.0] = true;
        profile.push(SketchEntityUse {
            entity: lines[candidate.0].0.clone(),
            reversed: candidate.1,
        });
        current = candidate.2;
    }
    (current == origin).then_some(profile)
}

pub(super) fn complete_ordered_compact_line_profile(
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
    marker_count: usize,
) -> Option<Vec<SketchEntityUse>> {
    (lines.len() == marker_count)
        .then(|| ordered_compact_line_profile(lines))
        .flatten()
}

pub(super) fn compact_line_region_addresses(payload: &[u8]) -> Option<Vec<u16>> {
    const NAME: &[u8] = b"moSketchRegion_c";
    let matches = payload
        .windows(NAME.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == NAME).then_some(offset))
        .collect::<Vec<_>>();
    let [offset] = matches.as_slice() else {
        return None;
    };
    let header = offset.checked_add(NAME.len())?;
    let region_token = View::u16_le_at(payload, header)?;
    if region_token == 0 {
        return None;
    }
    let count = usize::from(View::u16_le_at(payload, header + 2)?);
    if count < 3 {
        return None;
    }
    // Each region entry consumes a 12-byte record from `header + 4` onward.
    bounded_len(count as u64, 12, payload.len().saturating_sub(header + 4))?;
    let mut addresses = Vec::with_capacity(count);
    let mut entry_token = None;
    for index in 0..count {
        let entry = header.checked_add(4 + index * 12)?;
        let token = View::u16_le_at(payload, entry)?;
        if !matches!(token, 0x80e1 | 0x8386 | 0xbc87)
            || entry_token.is_some_and(|existing| existing != token)
            || payload.get(entry + 4..entry + 8)? != [0xff; 4]
            || payload.get(entry + 8..entry + 12)? != [0; 4]
        {
            return None;
        }
        entry_token = Some(token);
        addresses.push(View::u16_le_at(payload, entry + 2)?);
    }
    let expected = (1..=u16::try_from(count).ok()?).collect::<HashSet<_>>();
    (addresses.iter().copied().collect::<HashSet<_>>() == expected).then_some(addresses)
}

pub(super) fn compact_line_chain_addresses(payload: &[u8]) -> Option<Vec<u16>> {
    let matches = (0..payload.len()).filter_map(|offset| {
        let bytes = payload.get(offset..)?;
        let count = usize::from(View::u16_le_at(bytes, 0)?);
        if !(3..=64).contains(&count) {
            return None;
        }
        let addresses_end = 2usize.checked_add(count.checked_mul(4)?)?;
        let trailer = bytes.get(addresses_end..addresses_end.checked_add(40)?)?;
        if View::u32_le_at(trailer, 0)? != 1
            || trailer.get(4..6)? != [0, 0]
            || View::u32_le_at(trailer, 6)? != u32::try_from(count + 2).ok()?
            || trailer.get(10..14)? != [0xff; 4]
            || trailer.get(14..22)?.iter().any(|byte| *byte != 0)
            || View::u32_le_at(trailer, 22)? != u32::try_from(count + 1).ok()?
            || View::u32_le_at(trailer, 26)? != u32::try_from(count + 1).ok()?
            || trailer.get(30..36)? != [0xff, 0xfe, 0xff, 0, 0, 0]
            || trailer.get(36..40)? != [0xff; 4]
        {
            return None;
        }
        let addresses = (0..count)
            .filter_map(|index| {
                let offset = 2 + index * 4;
                u16::try_from(View::u32_le_at(bytes, offset)?).ok()
            })
            .collect::<Vec<_>>();
        let expected = (1..=u16::try_from(count).ok()?).collect::<HashSet<_>>();
        (addresses.len() == count && addresses.iter().copied().collect::<HashSet<_>>() == expected)
            .then_some(addresses)
    });
    let mut matches = matches.collect::<Vec<_>>();
    matches.dedup();
    let [addresses] = matches.as_slice() else {
        return None;
    };
    Some(addresses.clone())
}

#[cfg(test)]
mod curves_tests;
