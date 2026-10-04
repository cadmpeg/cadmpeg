//! Marker arc, circle and rectangle profile resolution.

use super::compact_reference_planes::principal_sketch_frame;
use super::endpoints::{
    compact_legacy_code_one_line_endpoint_indices, compact_legacy_curve_endpoint_indices,
    marker_profile_curve_role, minor_arc_angles, minor_arc_geometry, one_based_u16_endpoint_pair,
    unique_arc_center_marker, wide_indexed_curve_endpoint_indices,
};
use super::grid::quantize;
use super::markers::{
    compact_legacy_marker_body, finite_coordinate_pair, marker_native_code, sketch_marker_prefix_at,
};
use super::reference_geometry::reference_plane_frame_key;
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::records::ObjectId;
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_ir::{
    features::{FeatureDefinition, FeatureOperation},
    scalar::{Angle, Length},
};
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};

const EPS_CURVE_POSITION: f64 = 1.0e-8;
const EPS_CURVE_GEOMETRY: f64 = 1.0e-9;

pub(super) const REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY: &str = "UAxisSource";
pub(super) const CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE: &str = "constructed-mid-plane";

#[derive(Clone, Copy, Debug)]
struct CircularArcWitness<'a> {
    index: usize,
    sketch: &'a SketchId,
    endpoints: [&'a str; 2],
    center: Point2,
    radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SketchPlaneUAxisSource {
    Native,
    ConstructedMidPlane,
}

impl cadmpeg_core::decode::cost::DecodeCost for SketchPlaneUAxisSource {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SketchPlaneFrame {
    pub(super) origin: Point3,
    pub(super) normal: Vector3,
    pub(super) u_axis: Vector3,
    pub(super) u_axis_source: SketchPlaneUAxisSource,
}

impl SketchPlaneFrame {
    fn native((origin, normal, u_axis): (Point3, Vector3, Vector3)) -> Self {
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

fn current_linked_semicircle_record(payload: &[u8], offset: usize) -> bool {
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    markers: &[&SketchInputEntity],
    entities: &mut Vec<SketchEntity>,
    tolerance: f64,
) -> Result<(), CodecError> {
    fn order_endpoints(
        center: Point2,
        refs: [&str; 2],
        endpoints: [Point2; 2],
        perpendicular: Point2,
    ) -> ([&str; 2], [Point2; 2]) {
        let signed = endpoints.map(|point| {
            (
                (point.u - center.u) * perpendicular.u + (point.v - center.v) * perpendicular.v,
                point,
            )
        });
        if signed[0].0 > signed[1].0 {
            ([refs[0], refs[1]], [signed[0].1, signed[1].1])
        } else {
            ([refs[1], refs[0]], [signed[1].1, signed[0].1])
        }
    }

    let mut records = Vec::new();
    for marker in markers {
        if usize::try_from(marker.offset())
            .ok()
            .is_some_and(|offset| current_linked_semicircle_record(payload, offset))
        {
            ctx.reserve_vec(&mut records, 1, "collect SLDPRT semicircle records")?;
            records.push(*marker);
        }
    }
    let [first_record, second_record] = records.as_slice() else {
        return Ok(());
    };
    let record_refs = [first_record.id(), second_record.id()];
    let mut curve_entities = Vec::new();
    for entity in entities.iter() {
        if matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Ellipse { .. }
                | SketchGeometryDefinition::Nurbs { .. }
                | SketchGeometryDefinition::Native { .. }
        ) {
            ctx.reserve_vec(&mut curve_entities, 1, "collect SLDPRT semicircle curves")?;
            curve_entities.push(entity);
        }
    }
    if curve_entities.len() != 2
        || curve_entities.iter().any(|entity| {
            !entity
                .native_ref
                .as_deref()
                .is_some_and(|id| record_refs.contains(&id))
        })
    {
        return Ok(());
    }
    let mut points = Vec::new();
    for entity in entities.iter() {
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        let native_ref = ctx.format_retained(
            format_args!("{native_ref}"),
            "copy SLDPRT semicircle point identity",
        )?;
        ctx.reserve_vec(&mut points, 1, "collect SLDPRT semicircle points")?;
        points.push((native_ref, position.get()));
    }
    if points.len() != 6 {
        return Ok(());
    }
    let mut centers = Vec::new();
    for (center_index, (center_ref, center)) in points.iter().enumerate() {
        let mut pairs = points
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
                        let second_radius = (second.1.u - center.u).hypot(second.1.v - center.v);
                        (same_dimension_length(midpoint.u, center.u)
                            && same_dimension_length(midpoint.v, center.v)
                            && first_radius > tolerance
                            && same_dimension_length(first_radius, second_radius))
                        .then_some((
                            [first.0.as_str(), second.0.as_str()],
                            [first.1, second.1],
                            first_radius,
                        ))
                    })
            });
        let Some((endpoint_refs, endpoints, radius)) = pairs.next() else {
            continue;
        };
        if pairs.next().is_some() {
            continue;
        }
        let mut linked_records = records.iter().copied().filter(|record| {
            record
                .links()
                .iter()
                .any(|link| link.entity_ref.as_str() == center_ref.as_str())
        });
        let Some(record) = linked_records.next() else {
            continue;
        };
        if linked_records.next().is_some() {
            continue;
        }
        ctx.reserve_vec(&mut centers, 1, "collect SLDPRT semicircle centers")?;
        centers.push((
            record.id(),
            center_ref.as_str(),
            *center,
            endpoint_refs,
            endpoints,
            radius,
        ));
    }
    let [first, second] = centers.as_slice() else {
        return Ok(());
    };
    if first.0 == second.0 || !same_dimension_length(first.5, second.5) {
        return Ok(());
    }
    let center_delta = Point2::new(second.2.u - first.2.u, second.2.v - first.2.v);
    let center_distance = center_delta.u.hypot(center_delta.v);
    if center_distance <= tolerance {
        return Ok(());
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
        return Ok(());
    }
    let (first_refs, first_endpoints) = order_endpoints(first.2, first.3, first.4, perpendicular);
    let (second_refs, second_endpoints) =
        order_endpoints(second.2, second.3, second.4, perpendicular);
    let arc = |center: Point2,
               radius: f64,
               refs: &[&str; 2],
               endpoints: [Point2; 2],
               reverse: bool|
     -> Result<Option<_>, CodecError> {
        let (start_ref, end_ref, start, end) = if reverse {
            (&refs[1], &refs[0], endpoints[1], endpoints[0])
        } else {
            (&refs[0], &refs[1], endpoints[0], endpoints[1])
        };
        let Some(radius) = Length::new(radius) else {
            return Ok(None);
        };
        let Some(start_angle) = Angle::new((start.v - center.v).atan2(start.u - center.u)) else {
            return Ok(None);
        };
        let Some(end_angle) = Angle::new((end.v - center.v).atan2(end.u - center.u)) else {
            return Ok(None);
        };
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        }) else {
            return Ok(None);
        };
        let mut endpoint_refs = Vec::new();
        for endpoint in [start_ref, end_ref] {
            let endpoint = ctx.format_retained(
                format_args!("{endpoint}"),
                "copy SLDPRT semicircle endpoint identity",
            )?;
            ctx.reserve_vec(&mut endpoint_refs, 1, "collect SLDPRT semicircle endpoints")?;
            endpoint_refs.push(endpoint);
        }
        Ok(Some((geometry, endpoint_refs)))
    };
    let Some((first_geometry, first_endpoint_refs)) =
        arc(first.2, first.5, &first_refs, first_endpoints, false)?
    else {
        return Ok(());
    };
    let Some((second_geometry, second_endpoint_refs)) =
        arc(second.2, second.5, &second_refs, second_endpoints, true)?
    else {
        return Ok(());
    };

    let Some(first_entity) = entities
        .iter_mut()
        .find(|entity| entity.native_ref.as_deref() == Some(first.0))
    else {
        return Ok(());
    };
    first_entity.construction = false;
    first_entity.endpoint_refs = first_endpoint_refs;
    first_entity.geometry = first_geometry;
    let sketch_text = ctx.format_retained(
        format_args!("{}", first_entity.sketch.as_str()),
        "copy SLDPRT semicircle sketch identity",
    )?;
    let Ok(sketch) = SketchId::mint(sketch_text) else {
        return Ok(());
    };
    let Some(second_entity) = entities
        .iter_mut()
        .find(|entity| entity.native_ref.as_deref() == Some(second.0))
    else {
        return Ok(());
    };
    second_entity.construction = false;
    second_entity.endpoint_refs = second_endpoint_refs;
    second_entity.geometry = second_geometry;
    let sketch_key = ctx.rsplit_once(sketch.as_str(), "#", "split SLDPRT semicircle sketch key")?
        .map_or(sketch.as_str(), |(_, key)| key);
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
        let id_text = ctx.format_retained(
            format_args!("sldprt:model:sketch-entity#linked-semicircle:{sketch_key}:{index}"),
            "format SLDPRT semicircle line identity",
        )?;
        let Ok(id) = SketchEntityId::mint(id_text) else {
            continue;
        };
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
        else {
            continue;
        };
        let mut endpoint_refs = Vec::new();
        for endpoint in [start_ref, end_ref] {
            let endpoint = ctx.format_retained(
                format_args!("{endpoint}"),
                "copy SLDPRT semicircle line endpoint identity",
            )?;
            ctx.reserve_vec(
                &mut endpoint_refs,
                1,
                "collect SLDPRT semicircle line endpoints",
            )?;
            endpoint_refs.push(endpoint);
        }
        let sketch_copy = ctx.format_retained(
            format_args!("{}", sketch.as_str()),
            "copy SLDPRT semicircle line sketch identity",
        )?;
        let Ok(sketch_copy) = SketchId::mint(sketch_copy) else {
            continue;
        };
        ctx.reserve_vec(entities, 1, "append SLDPRT semicircle line")?;
        entities
            .push(SketchEntity::new(id, sketch_copy, geometry).with_endpoint_refs(endpoint_refs));
    }
    Ok(())
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
    (u.is_finite() && v.is_finite() && (u.hypot(v) - 1.0).abs() <= EPS_CURVE_GEOMETRY)
        .then_some([u, v])
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
        return SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).ok();
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
    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center,
        radius: Length::new(radius)?,
        start_angle: Angle::new(start_angle)?,
        end_angle: Angle::new(end_angle)?,
    })
    .ok()
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
            layout.indices[0],
            layout.indices[1],
            layout.indices[2],
            layout.indices[3],
        ],
        [layout.indices[4], layout.indices[5]],
    ))
}

struct SlotReferenceLayout {
    indices: [usize; 6],
    continuation_stride: Option<usize>,
}

fn slot_curve_reference_cells(payload: &[u8], offset: usize) -> Option<SlotReferenceLayout> {
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
                    indices: cells.map(|(_, index)| index),
                    continuation_stride,
                })
        })
}

pub(super) fn resolve_slot_marker_arcs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    markers: &[&SketchInputEntity],
    entities: &mut [SketchEntity],
    tolerance: f64,
) -> Result<(), CodecError> {
    let Some((curve_indices, center_indices)) = markers.iter().find_map(|marker| {
        let offset = usize::try_from(marker.offset()).ok()?;
        slot_curve_and_center_indices(payload, offset)
    }) else {
        return Ok(());
    };
    let mut curves = Vec::new();
    for marker in markers {
        if marker.coordinates_m.is_none()
            && matches!(
                marker.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        {
            ctx.reserve_vec(&mut curves, 1, "collect SLDPRT slot curves")?;
            curves.push(*marker);
        }
    }
    ctx.sort_unstable_by_key(
        &mut curves,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT slot curves",
    )?;
    if curves.len() != 4 {
        return Ok(());
    }
    let [Some(first), Some(second), Some(third), Some(fourth)] =
        curve_indices.map(|index| curves.get(index).copied())
    else {
        return Ok(());
    };
    let cycle = [first, second, third, fourth];
    if cycle.iter().enumerate().any(|(index, marker)| {
        cycle[index + 1..]
            .iter()
            .any(|other| marker.id() == other.id())
    }) {
        return Ok(());
    }
    let mut points = Vec::new();
    for marker in markers {
        if marker.coordinates_m.is_some()
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        {
            ctx.reserve_vec(&mut points, 1, "collect SLDPRT slot points")?;
            points.push(*marker);
        }
    }
    ctx.sort_unstable_by_key(
        &mut points,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT slot points",
    )?;
    let [Some(first_center), Some(second_center)] =
        center_indices.map(|index| points.get(index).map(|point| point.id()))
    else {
        return Ok(());
    };
    let center_refs = [first_center, second_center];
    if center_refs[0] == center_refs[1] {
        return Ok(());
    }
    let lookup_work = cadmpeg_core::decode::u64_from_index(entities.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("find SLDPRT slot curve entities", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(lookup_work, "find SLDPRT slot curve entities")?;
    let [Some(first), Some(second), Some(third), Some(fourth)] = cycle.map(|marker| {
        entities
            .iter()
            .rposition(|entity| entity.native_ref.as_deref() == Some(marker.id()))
    }) else {
        return Ok(());
    };
    let cycle_entities = [first, second, third, fourth];
    let mut native_arcs = cycle_entities.iter().filter(|index| {
        matches!((
            entities[**index].geometry).definition(),
            SketchGeometryDefinition::Native { ref native_kind }
                if native_kind.as_str() == "sldprt:marker-geometry:2"
        )
    });
    let mut resolved_arcs = cycle_entities.iter().filter(|index| {
        matches!(
            (entities[**index].geometry).definition(),
            SketchGeometryDefinition::Arc { .. }
        )
    });
    let lines = cycle_entities
        .iter()
        .copied()
        .filter(|index| {
            matches!(
                (entities[*index].geometry).definition(),
                SketchGeometryDefinition::Line { .. }
            )
        })
        .count();
    let (Some(target), Some(resolved_arc)) = (native_arcs.next(), resolved_arcs.next()) else {
        return Ok(());
    };
    if native_arcs.next().is_some() || resolved_arcs.next().is_some() {
        return Ok(());
    }
    if lines != 2 {
        return Ok(());
    }
    let Some(target_position) = cycle_entities
        .iter()
        .position(|candidate| candidate == target)
    else {
        return Ok(());
    };
    let Some(resolved_position) = cycle_entities
        .iter()
        .position(|candidate| candidate == resolved_arc)
    else {
        return Ok(());
    };
    if (target_position + 2) % 4 != resolved_position {
        return Ok(());
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
        let mut unique = entity
            .into_iter()
            .filter(|endpoint| !other.contains(endpoint));
        let endpoint = unique.next()?;
        unique.next().is_none().then_some(endpoint)
    };
    let previous = cycle_entities[(target_position + 3) % 4];
    let previous_other = cycle_entities[(target_position + 2) % 4];
    let next = cycle_entities[(target_position + 1) % 4];
    let next_other = cycle_entities[(target_position + 2) % 4];
    let Some(start_ref) = endpoint_not_shared(previous, previous_other) else {
        return Ok(());
    };
    let Some(end_ref) = endpoint_not_shared(next, next_other) else {
        return Ok(());
    };
    if start_ref == end_ref {
        return Ok(());
    }
    let point_lookup_work = cadmpeg_core::decode::u64_from_index(entities.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("find SLDPRT slot point entities", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(point_lookup_work, "find SLDPRT slot point entities")?;
    let point_position = |reference: &str| {
        entities.iter().rev().find_map(|entity| {
            if entity.native_ref.as_deref() != Some(reference) {
                return None;
            }
            match *entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => Some(position.get()),
                _ => None,
            }
        })
    };
    let (Some(start), Some(end)) = (point_position(start_ref), point_position(end_ref)) else {
        return Ok(());
    };
    let [Some(first_center), Some(second_center)] = center_refs.map(point_position) else {
        return Ok(());
    };
    let centers = [first_center, second_center];
    let SketchGeometryDefinition::Arc { center: used, .. } =
        *(entities[*resolved_arc].geometry).definition()
    else {
        return Ok(());
    };
    let mut remaining = centers.into_iter().filter(|center| {
        !same_dimension_length(center.u, used.u) || !same_dimension_length(center.v, used.v)
    });
    let Some(center) = remaining.next() else {
        return Ok(());
    };
    if remaining.next().is_some() {
        return Ok(());
    }
    let Some(geometry) = minor_arc_geometry(start, end, center, tolerance) else {
        return Ok(());
    };
    let mut endpoint_refs = Vec::new();
    for reference in [start_ref, end_ref] {
        let reference = ctx.format_retained(
            format_args!("{reference}"),
            "copy SLDPRT slot endpoint identity",
        )?;
        ctx.reserve_vec(&mut endpoint_refs, 1, "collect SLDPRT slot endpoints")?;
        endpoint_refs.push(reference);
    }
    entities[*target].endpoint_refs = endpoint_refs;
    entities[*target].geometry = geometry;
    Ok(())
}

fn closed_cycle_marker_arc_geometry(
    target_index: usize,
    target: &SketchEntity,
    entities: &[SketchEntity],
    point_by_ref: &HashMap<&str, Point2>,
    circular_witnesses: &[CircularArcWitness<'_>],
    tolerance: f64,
) -> Option<SketchGeometry> {
    if !matches!((
        target.geometry).definition(),
        SketchGeometryDefinition::Native { ref native_kind }
            if native_kind.as_str() == "sldprt:marker-geometry:2"
    ) || target.construction
        || target.endpoint_refs.len() != 2
    {
        return None;
    }
    let [target_start, target_end] = target.endpoint_refs.as_slice() else {
        return None;
    };
    let target_endpoints = [target_start.as_str(), target_end.as_str()];
    let (Some(target_start_point), Some(target_end_point)) = (
        point_by_ref.get(target_start.as_str()),
        point_by_ref.get(target_end.as_str()),
    ) else {
        return None;
    };
    let mut candidates = circular_witnesses.iter().filter_map(|witness| {
        if witness.index == target_index
            || witness.sketch != &target.sketch
            || !witness.radius.is_finite()
            || witness.radius <= 0.0
        {
            return None;
        }
        let [witness_start, witness_end] = &witness.endpoints;
        let witness_endpoints = [*witness_start, *witness_end];
        if target_endpoints
            .iter()
            .any(|endpoint| witness_endpoints.contains(endpoint))
        {
            return None;
        }
        let (Some(witness_start_point), Some(witness_end_point)) = (
            point_by_ref.get(*witness_start),
            point_by_ref.get(*witness_end),
        ) else {
            return None;
        };
        let on_witness_circle = |point: Point2| {
            same_dimension_length(
                (point.u - witness.center.u).hypot(point.v - witness.center.v),
                witness.radius,
            )
        };
        if !on_witness_circle(*witness_start_point)
            || !on_witness_circle(*witness_end_point)
            || !on_witness_circle(*target_start_point)
            || !on_witness_circle(*target_end_point)
        {
            return None;
        }
        let mut connecting_lines = entities.iter().filter(|entity| {
            !entity.construction
                && entity.sketch == target.sketch
                && entity.endpoint_refs.len() == 2
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
                && entity
                    .endpoint_refs
                    .iter()
                    .any(|endpoint| target_endpoints.contains(&endpoint.as_str()))
                && entity
                    .endpoint_refs
                    .iter()
                    .any(|endpoint| witness_endpoints.contains(&endpoint.as_str()))
                && match (
                    entity.endpoint_refs.as_slice(),
                    entity.geometry.definition(),
                ) {
                    ([first_ref, second_ref], SketchGeometryDefinition::Line { start, end }) => {
                        let (Some(first), Some(second)) = (
                            point_by_ref.get(first_ref.as_str()),
                            point_by_ref.get(second_ref.as_str()),
                        ) else {
                            return false;
                        };
                        (same_dimension_length(start.u, first.u)
                            && same_dimension_length(start.v, first.v)
                            && same_dimension_length(end.u, second.u)
                            && same_dimension_length(end.v, second.v))
                            || (same_dimension_length(start.u, second.u)
                                && same_dimension_length(start.v, second.v)
                                && same_dimension_length(end.u, first.u)
                                && same_dimension_length(end.v, first.v))
                    }
                    _ => false,
                }
        });
        let (Some(first_line), Some(second_line), None) = (
            connecting_lines.next(),
            connecting_lines.next(),
            connecting_lines.next(),
        ) else {
            return None;
        };
        let connecting_lines = [first_line, second_line];
        if connecting_lines.iter().any(|line| {
            let [first, second] = line.endpoint_refs.as_slice() else {
                return true;
            };
            first == second
                || (target_endpoints.contains(&first.as_str())
                    && target_endpoints.contains(&second.as_str()))
                || (witness_endpoints.contains(&first.as_str())
                    && witness_endpoints.contains(&second.as_str()))
        }) {
            return None;
        }
        let both_connected = |endpoints: [&str; 2]| {
            endpoints.iter().all(|endpoint| {
                connecting_lines.iter().any(|line| {
                    line.endpoint_refs
                        .iter()
                        .any(|reference| reference.as_str() == *endpoint)
                })
            })
        };
        if target_endpoints[0] == target_endpoints[1]
            || witness_endpoints[0] == witness_endpoints[1]
            || !both_connected(target_endpoints)
            || !both_connected(witness_endpoints)
        {
            return None;
        }
        minor_arc_geometry(
            *target_start_point,
            *target_end_point,
            witness.center,
            tolerance,
        )
    });
    let geometry = candidates.next()?;
    candidates.next().is_none().then_some(geometry)
}

pub(super) fn resolve_connected_marker_arcs(
    ctx: &DecodeContext<'_>,
    entities: &mut [SketchEntity],
    tolerance: f64,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(entities.len()),
        "scan SLDPRT connected arc neighbors",
    )?;
    let mut points = HashMap::new();
    let mut point_records = Vec::new();
    for entity in entities.iter() {
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        let retained_ref = ctx.format_retained(
            format_args!("{native_ref}"),
            "copy SLDPRT connected arc point identity",
        )?;
        ctx.admit_hash_map_entry(
            &mut points,
            &retained_ref,
            "index SLDPRT connected arc points",
        )?;
        points.insert(retained_ref, position.get());
        ctx.reserve_vec(
            &mut point_records,
            1,
            "collect SLDPRT connected arc point records",
        )?;
        point_records.push((&entity.sketch, native_ref, position.get()));
    }
    let mut center_replacements = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        if !matches!((
            entity.geometry).definition(),
            SketchGeometryDefinition::Native { ref native_kind }
                if native_kind.as_str() == "sldprt:marker-geometry:2"
        ) {
            continue;
        }
        let [start_ref, end_ref] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        let (Some(start), Some(end)) =
            (points.get(start_ref).copied(), points.get(end_ref).copied())
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for (sketch, reference, center) in &point_records {
            if *sketch == &entity.sketch
                && *reference != start_ref.as_str()
                && *reference != end_ref.as_str()
            {
                ctx.reserve_vec(&mut candidates, 1, "collect SLDPRT connected arc centers")?;
                candidates.push(*center);
            }
        }
        let Some(center) = unique_arc_center_marker(ctx, start, end, &candidates, tolerance)?
        else {
            continue;
        };
        let Some(geometry) = minor_arc_geometry(start, end, center, tolerance) else {
            continue;
        };
        ctx.reserve_vec(
            &mut center_replacements,
            1,
            "collect SLDPRT connected arc replacements",
        )?;
        center_replacements.push((index, geometry));
    }
    for (index, geometry) in center_replacements {
        entities[index].geometry = geometry;
    }
    let mut circular_witnesses = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        let SketchGeometryDefinition::Arc { center, radius, .. } = *entity.geometry.definition()
        else {
            continue;
        };
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        if !entity.construction {
            ctx.reserve_vec(
                &mut circular_witnesses,
                1,
                "collect SLDPRT connected arc witnesses",
            )?;
            circular_witnesses.push(CircularArcWitness {
                index,
                sketch: &entity.sketch,
                endpoints: [start.as_str(), end.as_str()],
                center: center.get(),
                radius: radius.get(),
            });
        }
    }
    let mut point_by_ref = HashMap::new();
    for entity in entities.iter() {
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut point_by_ref,
            &native_ref,
            "index SLDPRT connected arc point references",
        )?;
        point_by_ref.insert(native_ref, position.get());
    }
    let mut cycle_replacements = Vec::new();
    for (target_index, target) in entities.iter().enumerate() {
        if let Some(geometry) = closed_cycle_marker_arc_geometry(
            target_index,
            target,
            entities,
            &point_by_ref,
            &circular_witnesses,
            tolerance,
        ) {
            ctx.reserve_vec(
                &mut cycle_replacements,
                1,
                "collect SLDPRT connected arc cycle replacements",
            )?;
            cycle_replacements.push((target_index, geometry));
        }
    }
    for (index, geometry) in cycle_replacements {
        entities[index].geometry = geometry;
    }
    let mut arcs = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        if entity.endpoint_refs.len() == 2
            && matches!((entity.geometry).definition(),
                SketchGeometryDefinition::Native { ref native_kind }
                    if native_kind.as_str() == "sldprt:marker-geometry:2")
        {
            ctx.reserve_vec(&mut arcs, 1, "collect SLDPRT connected native arcs")?;
            arcs.push(index);
        }
    }
    let mut visited = HashSet::new();
    let mut replacements = Vec::new();
    for first in arcs.iter().copied() {
        if visited.contains(&first) {
            continue;
        }
        ctx.reserve_set(&mut visited, 1, "visit SLDPRT connected native arc")?;
        visited.insert(first);
        let mut component = Vec::new();
        ctx.reserve_vec(&mut component, 1, "collect SLDPRT connected arc component")?;
        component.push(first);
        let mut cursor = 0;
        while let Some(&current) = component.get(cursor) {
            cursor += 1;
            for candidate in arcs.iter().copied() {
                ctx.charge_work(1, "scan SLDPRT connected arc neighbors")?;
                if visited.contains(&candidate)
                    || !entities[current]
                        .endpoint_refs
                        .iter()
                        .any(|endpoint| entities[candidate].endpoint_refs.contains(endpoint))
                {
                    continue;
                }
                ctx.reserve_set(&mut visited, 1, "visit SLDPRT connected native arc")?;
                visited.insert(candidate);
                ctx.reserve_vec(&mut component, 1, "collect SLDPRT connected arc component")?;
                component.push(candidate);
            }
        }
        let mut endpoint_refs = Vec::new();
        for index in &component {
            for reference in &entities[*index].endpoint_refs {
                ctx.reserve_vec(
                    &mut endpoint_refs,
                    1,
                    "collect SLDPRT connected arc endpoints",
                )?;
                endpoint_refs.push(reference);
            }
        }
        ctx.sort_unstable_by(
            &mut endpoint_refs,
            |value| value,
            Ord::cmp,
            "sort SLDPRT connected arc endpoints",
        )?;
        endpoint_refs.dedup();
        let mut component_points = Vec::new();
        let mut missing_point = false;
        for endpoint in &endpoint_refs {
            let Some(point) = points.get(endpoint.as_str()).copied() else {
                missing_point = true;
                break;
            };
            ctx.reserve_vec(
                &mut component_points,
                1,
                "collect SLDPRT connected arc component points",
            )?;
            component_points.push(point);
        }
        if missing_point {
            continue;
        }
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
            ctx.reserve_vec(
                &mut component_replacements,
                1,
                "collect SLDPRT connected arc component replacements",
            )?;
            component_replacements.push((index, geometry));
        }
        if component_replacements.len() >= 2 {
            ctx.extend_vec(&mut replacements, component_replacements, "collect SLDPRT connected arc replacements")?;
        }
    }
    for (index, geometry) in replacements {
        entities[index].geometry = geometry;
    }
    for entity in entities {
        let SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            ..
        } = *entity.geometry.definition()
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
            center.u + radius.get() * start_angle.get().cos(),
            center.v + radius.get() * start_angle.get().sin(),
        );
        let first_distance = (geometry_start.u - first.u).hypot(geometry_start.v - first.v);
        let second_distance = (geometry_start.u - second.u).hypot(geometry_start.v - second.v);
        if second_distance < first_distance {
            entity.endpoint_refs.reverse();
        }
    }
    Ok(())
}

pub(super) fn closed_marker_profiles<E: Borrow<SketchEntity>>(
    ctx: &DecodeContext<'_>,
    entities: &[E],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    closed_marker_profiles_with_policy(ctx, entities, true)
}

/// Recover closed curve cycles when endpoint markers are shared by construction geometry.
pub(super) fn closed_marker_profiles_allowing_shared_endpoints<E: Borrow<SketchEntity>>(
    ctx: &DecodeContext<'_>,
    entities: &[E],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    closed_marker_profiles_with_policy(ctx, entities, false)
}

fn closed_marker_profiles_with_policy<E: Borrow<SketchEntity>>(
    ctx: &DecodeContext<'_>,
    entities: &[E],
    reject_branching_components: bool,
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    let mut profiles = Vec::new();
    let mut curves = Vec::new();
    for (index, item) in entities.iter().enumerate() {
        let entity = item.borrow();
        if !entity.construction
            && matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Circle { .. }
            )
        {
            let id_text = ctx.format_retained(
                format_args!("{}", entity.id().as_str()),
                "copy SLDPRT closed circle identity",
            )?;
            if let Ok(id) = SketchEntityId::mint(id_text) {
                let mut profile = Vec::new();
                ctx.reserve_vec(&mut profile, 1, "collect SLDPRT closed circle profile")?;
                profile.push(SketchEntityUse {
                    entity: id,
                    reversed: false,
                });
                ctx.reserve_vec(&mut profiles, 1, "collect SLDPRT closed profiles")?;
                profiles.push(profile);
            }
        }
        if !entity.construction
            && entity.endpoint_refs.len() == 2
            && matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
            )
        {
            ctx.reserve_vec(&mut curves, 1, "collect SLDPRT closed curves")?;
            curves.push(index);
        }
    }
    let mut incidence = HashMap::<&str, Vec<usize>>::new();
    for index in &curves {
        let entity = entities[*index].borrow();
        for endpoint in &entity.endpoint_refs {
            let adjacent = ctx.entry_hash_map(&mut incidence, endpoint.as_str(), "index SLDPRT closed curve endpoints")?.or_default();
            ctx.reserve_vec(adjacent, 1, "collect SLDPRT endpoint incidence")?;
            adjacent.push(*index);
        }
    }
    let mut unused = HashSet::new();
    for index in &curves {
        ctx.reserve_set(&mut unused, 1, "index SLDPRT unused closed curves")?;
        unused.insert(*index);
    }
    while let Some(&first) = unused.iter().min() {
        let mut component = HashSet::new();
        ctx.reserve_set(&mut component, 1, "index SLDPRT closed curve component")?;
        component.insert(first);
        let mut frontier = Vec::new();
        ctx.reserve_vec(&mut frontier, 1, "collect SLDPRT closed curve frontier")?;
        frontier.push(first);
        while let Some(curve) = frontier.pop() {
            for endpoint in &entities[curve].borrow().endpoint_refs {
                for adjacent in incidence.get(endpoint.as_str()).into_iter().flatten() {
                    ctx.charge_work(1, "scan SLDPRT closed curve incidence")?;
                    if !component.contains(adjacent) {
                        ctx.reserve_set(&mut component, 1, "index SLDPRT closed curve component")?;
                        component.insert(*adjacent);
                        ctx.reserve_vec(&mut frontier, 1, "collect SLDPRT closed curve frontier")?;
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        if reject_branching_components
            && component.iter().any(|curve| {
                entities[*curve]
                    .borrow()
                    .endpoint_refs
                    .iter()
                    .any(|endpoint| {
                        incidence
                            .get(endpoint.as_str())
                            .is_none_or(|curves| curves.len() != 2)
                    })
            })
        {
            unused.retain(|curve| !component.contains(curve));
            continue;
        }
        let start = entities[first].borrow().endpoint_refs[0].as_str();
        let mut current = start;
        let mut curve = first;
        let mut profile = Vec::new();
        loop {
            if !unused.remove(&curve) {
                profile.clear();
                break;
            }
            let [curve_start, curve_end] = entities[curve].borrow().endpoint_refs.as_slice() else {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            };
            let (reversed, next) = if curve_start == current {
                (false, curve_end.as_str())
            } else if curve_end == current {
                (true, curve_start.as_str())
            } else {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            };
            let id_text = ctx.format_retained(
                format_args!("{}", entities[curve].borrow().id().as_str()),
                "copy SLDPRT closed curve identity",
            )?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            };
            ctx.reserve_vec(&mut profile, 1, "collect SLDPRT closed curve profile")?;
            profile.push(SketchEntityUse {
                entity: id,
                reversed,
            });
            current = next;
            if current == start {
                break;
            }
            let Some(candidates) = incidence.get(current) else {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            };
            if reject_branching_components && candidates.len() != 2 {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            }
            let Some(next_curve) = candidates
                .iter()
                .copied()
                .find(|index| unused.contains(index))
            else {
                ctx.clear_vec(&mut profile, "discard SLDPRT incomplete closed profile")?;
                break;
            };
            curve = next_curve;
        }
        if profile.len() >= 2 {
            ctx.reserve_vec(&mut profiles, 1, "collect SLDPRT closed profiles")?;
            profiles.push(profile);
        }
    }
    Ok(profiles)
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
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
    let mut source_by_feature = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let Some((neutral_id, source)) = features
            .iter()
            .find(|neutral| neutral.native_ref.as_deref() == Some(feature.id.as_str()))
            .and_then(|neutral| Some((neutral.id.as_str(), feature.source_value()?)))
        else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut source_by_feature,
            &neutral_id,
            "index SLDPRT sketch plane sources",
        )?;
        source_by_feature.insert(neutral_id, source);
    }
    let mut frames_by_feature = HashMap::new();
    for feature in features {
        let frame = (|| {
            let frame = match feature.evaluation.definition() {
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::DatumPrincipalPlane { plane },
                ) => SketchPlaneFrame::native(principal_sketch_frame(*plane)),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::DatumPlane { frame },
                ) => SketchPlaneFrame::from_frame(
                    (
                        frame.origin().get(),
                        frame.normal().get(),
                        frame.u_axis().get(),
                    ),
                    feature_u_axis_source(feature),
                ),
                _ => return None,
            };
            Some(frame)
        })();
        let Some(frame) = frame else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut frames_by_feature,
            &feature.id.as_str(),
            "index SLDPRT resolved sketch planes",
        )?;
        frames_by_feature.insert(feature.id.as_str(), frame);
    }
    loop {
        let mut derived = Vec::new();
        for feature in features
            .iter()
            .filter(|feature| !frames_by_feature.contains_key(feature.id.as_str()))
        {
            let Some(frame) = (|| {
                let cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane {
                        reference:
                            Some(cadmpeg_ir::features::DatumPlaneReference::Feature {
                                feature: reference,
                            }),
                        distance,
                    },
                ) = feature.evaluation.definition()
                else {
                    return None;
                };
                let frame = *frames_by_feature.get(reference.as_str())?;
                Some(SketchPlaneFrame {
                    origin: Point3::new(
                        frame.origin.x + frame.normal.x * distance.get(),
                        frame.origin.y + frame.normal.y * distance.get(),
                        frame.origin.z + frame.normal.z * distance.get(),
                    ),
                    ..frame
                })
            })() else {
                continue;
            };
            ctx.reserve_vec(&mut derived, 1, "collect SLDPRT derived sketch planes")?;
            derived.push((feature.id.as_str(), frame));
        }
        if derived.is_empty() {
            break;
        }
        for (id, frame) in derived {
            ctx.admit_hash_map_entry(
                &mut frames_by_feature,
                &id,
                "index SLDPRT derived sketch planes",
            )?;
            frames_by_feature.insert(id, frame);
        }
    }
    let mut frames = HashMap::new();
    for (feature, source) in source_by_feature {
        let Some(frame) = frames_by_feature.get(feature).copied() else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut frames,
            &source,
            "index SLDPRT sketch plane source frames",
        )?;
        frames.insert(source, frame);
    }
    Ok(frames)
}

pub(super) fn lane_sketch_plane_frames(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
    let mut frames = sketch_plane_frames(ctx, features, histories)?;
    let mut lane_candidates = HashMap::<u32, Vec<SketchPlaneFrame>>::new();
    for native in histories.iter().flat_map(|history| &history.features) {
        let Some(source) = feature_object_name(native, lane)
            .and_then(|name| name.object_id.and_then(ObjectId::value))
        else {
            continue;
        };
        let Some(feature) = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(native.id.as_str()))
        else {
            continue;
        };
        let frame = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                SketchPlaneFrame::native(principal_sketch_frame(*plane))
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
                SketchPlaneFrame::from_frame(
                    (
                        frame.origin().get(),
                        frame.normal().get(),
                        frame.u_axis().get(),
                    ),
                    feature_u_axis_source(feature),
                )
            }
            _ => continue,
        };
        ctx.admit_hash_map_entry(
            &mut lane_candidates,
            &source,
            "index SLDPRT lane sketch plane candidates",
        )?;
        let candidates = lane_candidates.entry(source).or_default();
        ctx.reserve_vec(candidates, 1, "collect SLDPRT lane sketch plane candidates")?;
        candidates.push(frame);
    }
    for (source, mut candidates) in lane_candidates {
        ctx.stable_sort_by_key(
            &mut candidates,
            |value| {
                (
                    reference_plane_frame_key(&value.as_tuple()),
                    value.u_axis_source,
                )
            },
            Ord::cmp,
            "sort SLDPRT sketch plane frames",
        )?;
        candidates.dedup_by_key(|frame| reference_plane_frame_key(&frame.as_tuple()));
        if let [frame] = candidates.as_slice() {
            ctx.admit_hash_map_entry(
                &mut frames,
                &source,
                "index SLDPRT lane sketch plane frames",
            )?;
            frames.entry(source).or_insert(*frame);
        }
    }
    Ok(frames)
}

pub(super) fn ordered_rectangle_corners(
    ctx: &DecodeContext<'_>,
    points: &[Point2],
) -> Result<Option<[Point2; 4]>, CodecError> {
    let [_, _, _, _] = points else {
        return Ok(None);
    };
    let mut u = points.iter().map(|point| point.u).collect::<Vec<_>>();
    ctx.stable_sort_by(
        &mut u,
        |value| value,
        f64::total_cmp,
        "sldprt rectangle u sort",
    )?;
    u.dedup();
    let mut v = points.iter().map(|point| point.v).collect::<Vec<_>>();
    ctx.stable_sort_by(
        &mut v,
        |value| value,
        f64::total_cmp,
        "sldprt rectangle v sort",
    )?;
    v.dedup();
    let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
        return Ok(None);
    };
    let corners = [
        Point2::new(*u0, *v0),
        Point2::new(*u1, *v0),
        Point2::new(*u1, *v1),
        Point2::new(*u0, *v1),
    ];
    Ok(corners
        .iter()
        .all(|corner| points.iter().filter(|point| *point == corner).count() == 1)
        .then_some(corners))
}

fn ordered_tolerant_rectangle_corners(
    ctx: &DecodeContext<'_>,
    points: &[Point2],
) -> Result<Option<[Point2; 4]>, CodecError> {
    let [_, _, _, _] = points else {
        return Ok(None);
    };
    let mut u = points.iter().map(|point| point.u).collect::<Vec<_>>();
    ctx.stable_sort_by(
        &mut u,
        |value| value,
        f64::total_cmp,
        "sldprt tolerant rectangle u sort",
    )?;
    u.dedup_by(|left, right| same_dimension_length(*left, *right));
    let mut v = points.iter().map(|point| point.v).collect::<Vec<_>>();
    ctx.stable_sort_by(
        &mut v,
        |value| value,
        f64::total_cmp,
        "sldprt tolerant rectangle v sort",
    )?;
    v.dedup_by(|left, right| same_dimension_length(*left, *right));
    let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
        return Ok(None);
    };
    let corners = [
        Point2::new(*u0, *v0),
        Point2::new(*u1, *v0),
        Point2::new(*u1, *v1),
        Point2::new(*u0, *v1),
    ];
    Ok(corners
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
        .then_some(corners))
}

pub(super) fn indexed_rectangle_from_line_cycle(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    markers: &[&SketchInputEntity],
) -> Result<Option<[Point2; 4]>, CodecError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum EndpointSpace {
        Roster,
        Object,
    }

    enum RectangleLineRecord {
        Indexed {
            endpoints: [u32; 2],
            space: EndpointSpace,
        },
        CurrentWide {
            endpoints: [u32; 2],
            code: Option<u32>,
            alternate_locus: bool,
        },
    }

    impl RectangleLineRecord {
        fn endpoint_space(&self) -> EndpointSpace {
            match self {
                Self::Indexed { space, .. } => *space,
                Self::CurrentWide { .. } => EndpointSpace::Roster,
            }
        }
    }

    let mut roster = Vec::new();
    ctx.extend_from_slice(&mut roster, markers, "collect SLDPRT rectangle marker roster")?;
    ctx.sort_unstable_by_key(
        &mut roster,
        |value| value.offset(),
        Ord::cmp,
        "sldprt rectangle marker roster sort",
    )?;
    let mut records = Vec::new();
    for marker in markers {
        let record = (|| {
            let offset = usize::try_from(marker.offset()).ok()?;
            if let Some(endpoints) = legacy_extended_rectangle_line_endpoints(payload, offset) {
                return (marker.kind() == SketchInputKind::LineOrCircle).then_some(
                    RectangleLineRecord::Indexed {
                        endpoints,
                        space: EndpointSpace::Roster,
                    },
                );
            }
            if let Some(endpoints) = current_compact_rectangle_line_endpoints(payload, offset) {
                return matches!(
                    marker.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
                .then_some(RectangleLineRecord::Indexed {
                    endpoints,
                    space: EndpointSpace::Object,
                });
            }
            if let Some(endpoints) = compact_legacy_rectangle_line_endpoints(payload, offset) {
                return (marker.kind() == SketchInputKind::LineOrCircle).then_some(
                    RectangleLineRecord::Indexed {
                        endpoints,
                        space: EndpointSpace::Object,
                    },
                );
            }
            if let Some(endpoints) = compact_legacy_curve_endpoint_indices(payload, offset)
                .or_else(|| compact_legacy_code_one_line_endpoint_indices(payload, offset))
            {
                return (marker.kind() == SketchInputKind::LineOrCircle).then_some(
                    RectangleLineRecord::Indexed {
                        endpoints,
                        space: EndpointSpace::Object,
                    },
                );
            }
            let endpoints = current_wide_rectangle_line_endpoints(payload, offset)?;
            if endpoints.iter().any(|endpoint| {
                usize::try_from(*endpoint)
                    .ok()
                    .and_then(|endpoint| roster.get(endpoint))
                    .is_none_or(|endpoint| {
                        endpoint.coordinates_m.is_none()
                            || !matches!(
                                endpoint.kind(),
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    })
            }) {
                return None;
            }
            matches!(
                marker.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
            .then_some(RectangleLineRecord::CurrentWide {
                endpoints,
                code: marker_native_code(payload, offset),
                alternate_locus: payload.get(offset + 23..offset + 27)
                    == Some(&[0x05, 0x00, 0x01, 0x00]),
            })
        })();
        if let Some(record) = record {
            ctx.reserve_vec(&mut records, 1, "collect SLDPRT rectangle line records")?;
            records.push(record);
            if records.len() > 4 {
                return Ok(None);
            }
        }
    }
    (|| {
        let endpoint_space = records.first().map(RectangleLineRecord::endpoint_space)?;
        if records
            .iter()
            .any(|record| record.endpoint_space() != endpoint_space)
        {
            return None;
        }
        let mut current_code_count = 0;
        let mut code_one_count = 0;
        let mut code_two_count = 0;
        for code in records.iter().filter_map(|record| match record {
            RectangleLineRecord::CurrentWide { code, .. } => *code,
            RectangleLineRecord::Indexed { .. } => None,
        }) {
            current_code_count += 1;
            code_one_count += usize::from(code == 1);
            code_two_count += usize::from(code == 2);
        }
        if !(current_code_count == 0
            || current_code_count == 4 && code_one_count == 3 && code_two_count == 1
            || current_code_count == 3 && code_one_count == 2 && code_two_count == 1)
        {
            return None;
        }
        let edge_count = records.len();
        if !matches!(edge_count, 3 | 4) || edge_count == 3 && current_code_count != 3 {
            return None;
        }
        let mut edges = [[0u32; 2]; 4];
        for (index, record) in records.iter().enumerate() {
            let (endpoints, alternate_locus) = match record {
                RectangleLineRecord::Indexed { endpoints, .. } => (*endpoints, false),
                RectangleLineRecord::CurrentWide {
                    endpoints,
                    alternate_locus,
                    ..
                } => (*endpoints, *alternate_locus),
            };
            if edge_count == 4 && alternate_locus {
                return None;
            }
            edges[index] = endpoints;
        }
        if let Err(error) = ctx.sort_unstable_by(
            &mut edges[..edge_count],
            |value| value,
            Ord::cmp,
            "sldprt rectangle line edges sort",
        ) {
            return Some(Err(error));
        }
        let edges = &edges[..edge_count];
        if edges.windows(2).any(|pair| pair[0] == pair[1])
            || edges.iter().any(|edge| edge[0] == edge[1])
        {
            return None;
        }
        let mut vertices = [0u32; 8];
        for (index, vertex) in edges.iter().flatten().enumerate() {
            vertices[index] = *vertex;
        }
        if let Err(error) = ctx.sort_unstable_by(
            &mut vertices[..edge_count * 2],
            |value| value,
            Ord::cmp,
            "sldprt rectangle line vertices sort",
        ) {
            return Some(Err(error));
        }
        let mut unique_vertices = [0u32; 4];
        let mut vertex_count = 0;
        for vertex in &vertices[..edge_count * 2] {
            if vertex_count == 0 || unique_vertices[vertex_count - 1] != *vertex {
                if vertex_count == unique_vertices.len() {
                    return None;
                }
                unique_vertices[vertex_count] = *vertex;
                vertex_count += 1;
            }
        }
        if vertex_count != 4 {
            return None;
        }
        let vertices = &unique_vertices;
        let mut degrees = [0usize; 4];
        for (index, vertex) in vertices.iter().enumerate() {
            degrees[index] = edges.iter().filter(|edge| edge.contains(vertex)).count();
        }
        if let Err(error) = ctx.sort_unstable_by(
            &mut degrees,
            |value| value,
            Ord::cmp,
            "sldprt rectangle vertex degrees sort",
        ) {
            return Some(Err(error));
        }
        if !matches!(degrees, [2, 2, 2, 2] | [1, 1, 2, 2]) {
            return None;
        }
        let mut known = [(0u32, [0.0f64; 2]); 4];
        let mut known_count = 0;
        for vertex in vertices {
            let candidate = (|| {
                let marker = match endpoint_space {
                    EndpointSpace::Roster => *roster.get(usize::try_from(*vertex).ok()?)?,
                    EndpointSpace::Object => {
                        let mut candidates = markers.iter().copied().filter(|marker| {
                            marker.object_index() == Some(*vertex)
                                && marker.coordinates_m.is_some()
                                && matches!(
                                    marker.kind(),
                                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                                )
                        });
                        let marker = candidates.next()?;
                        candidates.next().is_none().then_some(marker)?
                    }
                };
                (matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ) && marker.coordinates_m.is_some())
                .then_some((*vertex, marker.coordinates_m?.get()))
            })();
            if let Some(candidate) = candidate {
                known[known_count] = candidate;
                known_count += 1;
            }
        }
        let known = &known[..known_count];
        if edge_count == 3 && known.len() != 4 {
            return None;
        }
        let corners = match known {
            [(first_vertex, [first_u, first_v]), (second_vertex, [second_u, second_v])] => {
                if edges
                    .iter()
                    .any(|edge| edge.contains(first_vertex) && edge.contains(second_vertex))
                    || first_u == second_u
                    || first_v == second_v
                {
                    return None;
                }
                [
                    Point2::new(*first_u, *first_v),
                    Point2::new(*first_u, *second_v),
                    Point2::new(*second_u, *first_v),
                    Point2::new(*second_u, *second_v),
                ]
            }
            [_, _, _] => {
                let axis_aligned = (|| -> Result<Option<[Point2; 4]>, CodecError> {
                    let mut u = [0.0; 3];
                    let mut v = [0.0; 3];
                    let mut u_len = 0;
                    let mut v_len = 0;
                    for (_, [point_u, point_v]) in known {
                        if u[..u_len]
                            .iter()
                            .all(|candidate| !same_dimension_length(*candidate, *point_u))
                        {
                            u[u_len] = *point_u;
                            u_len += 1;
                        }
                        if v[..v_len]
                            .iter()
                            .all(|candidate| !same_dimension_length(*candidate, *point_v))
                        {
                            v[v_len] = *point_v;
                            v_len += 1;
                        }
                    }
                    ctx.stable_sort_by(
                        &mut u[..u_len],
                        |value| value,
                        f64::total_cmp,
                        "sldprt rectangle axis u sort",
                    )?;
                    ctx.stable_sort_by(
                        &mut v[..v_len],
                        |value| value,
                        f64::total_cmp,
                        "sldprt rectangle axis v sort",
                    )?;
                    let ([u0, u1], [v0, v1]) = (&u[..u_len], &v[..v_len]) else {
                        return Ok(None);
                    };
                    let products = [
                        Point2::new(*u0, *v0),
                        Point2::new(*u1, *v0),
                        Point2::new(*u1, *v1),
                        Point2::new(*u0, *v1),
                    ];
                    let mut occupied = [false; 4];
                    for (_, [point_u, point_v]) in known {
                        let mut matches = products.iter().enumerate().filter(|(_, product)| {
                            same_dimension_length(product.u, *point_u)
                                && same_dimension_length(product.v, *point_v)
                        });
                        let Some((index, _)) = matches.next() else {
                            return Ok(None);
                        };
                        if matches.next().is_some() || occupied[index] {
                            return Ok(None);
                        }
                        occupied[index] = true;
                    }
                    Ok((occupied.iter().filter(|occupied| **occupied).count() == 3)
                        .then_some(products))
                })();
                let axis_aligned = match axis_aligned {
                    Ok(corners) => corners,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(corners) = axis_aligned {
                    corners
                } else {
                    let missing = *vertices
                        .iter()
                        .find(|vertex| known.iter().all(|(known, _)| known != *vertex))?;
                    let mut neighbors = edges
                        .iter()
                        .filter(|edge| edge.contains(&missing))
                        .map(|edge| edge[usize::from(edge[0] == missing)]);
                    let (Some(first_neighbor), Some(second_neighbor), None) =
                        (neighbors.next(), neighbors.next(), neighbors.next())
                    else {
                        return None;
                    };
                    let opposite = *vertices.iter().find(|vertex| {
                        **vertex != missing
                            && **vertex != first_neighbor
                            && **vertex != second_neighbor
                    })?;
                    let coordinates = |vertex| {
                        known.iter().find_map(|(known, coordinates)| {
                            (*known == vertex).then_some(*coordinates)
                        })
                    };
                    let [first_u, first_v] = coordinates(first_neighbor)?;
                    let [second_u, second_v] = coordinates(second_neighbor)?;
                    let [opposite_u, opposite_v] = coordinates(opposite)?;
                    let inferred = [
                        first_u + second_u - opposite_u,
                        first_v + second_v - opposite_v,
                    ];
                    let [(_, first), (_, second), (_, third)] = known else {
                        return None;
                    };
                    [
                        Point2::new(first[0], first[1]),
                        Point2::new(second[0], second[1]),
                        Point2::new(third[0], third[1]),
                        Point2::new(inferred[0], inferred[1]),
                    ]
                }
            }
            [(_, first), (_, second), (_, third), (_, fourth)] => [
                Point2::new(first[0], first[1]),
                Point2::new(second[0], second[1]),
                Point2::new(third[0], third[1]),
                Point2::new(fourth[0], fourth[1]),
            ],
            _ => return None,
        };
        if !corners.iter().all(Point2::is_finite) {
            return None;
        }
        let ordered = if edges.len() == 3 {
            ordered_tolerant_rectangle_corners(ctx, &corners)
        } else {
            ordered_rectangle_corners(ctx, &corners)
        };
        let corners = match ordered {
            Ok(Some(corners)) => corners,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        (edges.len() == 4
            || edges.iter().all(|[first, second]| {
                let (Some(first), Some(second)) = (
                    known
                        .iter()
                        .find(|(vertex, _)| vertex == first)
                        .map(|(_, point)| point),
                    known
                        .iter()
                        .find(|(vertex, _)| vertex == second)
                        .map(|(_, point)| point),
                ) else {
                    return false;
                };
                same_dimension_length(first[0], second[0])
                    ^ same_dimension_length(first[1], second[1])
            }))
        .then_some(Ok(corners))
    })()
    .transpose()
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
    let continued = offset
        .checked_add(84)
        .is_some_and(|at| sketch_marker_prefix_at(payload, at));
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
    let continued = offset
        .checked_add(84)
        .is_some_and(|at| sketch_marker_prefix_at(payload, at));
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
        || !offset
            .checked_add(92)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
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
) -> Option<cadmpeg_ir::units::FiniteVector<2>> {
    let offset = usize::try_from(marker.offset()).ok()?;
    if marker.kind() != SketchInputKind::LineOrCircle
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
            && offset
                .checked_add(146)
                .is_some_and(|at| sketch_marker_prefix_at(payload, at));
        let terminal_end = payload.get(offset + 100..offset + 142) == Some(&[0; 42])
            && payload.get(offset + 142..offset + 146) == Some(&[0xff; 4])
            && offset
                .checked_add(146)
                .is_some_and(|at| sketch_marker_prefix_at(payload, at));
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
    ctx: &DecodeContext<'_>,
    markers: &[&'a SketchInputEntity],
    dimensions_mm: &[f64],
) -> Result<Option<[&'a SketchInputEntity; 4]>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = EPS_CURVE_POSITION;
    if dimensions_mm.len() < 2 {
        return Ok(None);
    }
    let mut points = Vec::new();
    for marker in markers {
        let Some([u, v]) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let Some(cells) =
            quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM).cells()
        else {
            continue;
        };
        ctx.reserve_vec(&mut points, 1, "collect SLDPRT rectangle points")?;
        points.push((*marker, cells));
    }
    let mut u = Vec::new();
    let mut v = Vec::new();
    for (_, point) in &points {
        ctx.reserve_vec(&mut u, 1, "collect SLDPRT rectangle u coordinates")?;
        ctx.reserve_vec(&mut v, 1, "collect SLDPRT rectangle v coordinates")?;
        u.push(point.0);
        v.push(point.1);
    }
    let point_count = cadmpeg_core::decode::u64_from_index(points.len());
    ctx.sort_unstable_by(
        &mut u,
        |value| value,
        Ord::cmp,
        "sldprt rectangle cells u sort",
    )?;
    u.dedup();
    ctx.sort_unstable_by(
        &mut v,
        |value| value,
        Ord::cmp,
        "sldprt rectangle cells v sort",
    )?;
    v.dedup();
    let dimension_count = cadmpeg_core::decode::u64_from_index(dimensions_mm.len());
    let dimension_work = dimension_count
        .checked_mul(dimension_count)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("scan SLDPRT rectangle dimensions", u64::MAX - 1, u64::MAX)
        })?;
    let dimensions_match = |u0: i64, u1: i64, v0: i64, v1: i64| {
        let (Some(u_cells), Some(v_cells)) = (
            u1.checked_sub(u0)
                .and_then(cadmpeg_core::convert::f64_from_i64),
            v1.checked_sub(v0)
                .and_then(cadmpeg_core::convert::f64_from_i64),
        ) else {
            return false;
        };
        let u_span = u_cells * QUANTUM;
        let v_span = v_cells * QUANTUM;
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
    let mut selected = None;
    for (first_u_index, &u0) in u.iter().enumerate() {
        for &u1 in &u[first_u_index + 1..] {
            for (first_v_index, &v0) in v.iter().enumerate() {
                for &v1 in &v[first_v_index + 1..] {
                    ctx.charge_work(dimension_work, "scan SLDPRT rectangle dimensions")?;
                    if !dimensions_match(u0, u1, v0, v1) {
                        continue;
                    }
                    ctx.charge_work(
                        point_count.checked_mul(4).ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "scan SLDPRT rectangle corners",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?,
                        "scan SLDPRT rectangle corners",
                    )?;
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
                    if selected.replace([first, second, third, fourth]).is_some() {
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(selected)
}

fn ordered_compact_line_profile(
    ctx: &DecodeContext<'_>,
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
) -> Result<Option<Vec<SketchEntityUse>>, CodecError> {
    if lines.len() < 3 {
        return Ok(None);
    }
    let mut used = ctx.alloc_filled(lines.len(), false, "SLDPRT compact line profile usage")?;
    let mut profile = ctx.vector_storage(lines.len(), "SLDPRT compact line profile")?;
    let Some(first) = lines.first() else {
        return Ok(None);
    };
    used[0] = true;
    ctx.push_vec(&mut profile, SketchEntityUse {
        entity: super::transforms::copy_sketch_entity_identity(
            ctx,
            &first.0,
            "SLDPRT compact line profile",
        )?,
        reversed: false,
    }, "SLDPRT compact line profile")?;
    let origin = first.3;
    let mut current = first.4;
    while profile.len() < lines.len() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(lines.len()),
            "scan SLDPRT compact line profile adjacency",
        )?;
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
        let Some(candidate) = candidates.next() else {
            return Ok(None);
        };
        if candidates.next().is_some() {
            return Ok(None);
        }
        used[candidate.0] = true;
        ctx.push_vec(&mut profile, SketchEntityUse {
            entity: super::transforms::copy_sketch_entity_identity(
                ctx,
                &lines[candidate.0].0,
                "SLDPRT compact line profile",
            )?,
            reversed: candidate.1,
        }, "SLDPRT compact line profile")?;
        current = candidate.2;
    }
    Ok((current == origin).then_some(profile))
}

pub(super) fn complete_ordered_compact_line_profile(
    ctx: &DecodeContext<'_>,
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
    marker_count: usize,
) -> Result<Option<Vec<SketchEntityUse>>, CodecError> {
    if lines.len() != marker_count {
        return Ok(None);
    }
    ordered_compact_line_profile(ctx, lines)
}

pub(super) fn compact_line_region_addresses(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<u16>>, CodecError> {
    const NAME: &[u8] = b"moSketchRegion_c";
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(payload.len()),
        "scan SLDPRT compact region",
    )?;
    let mut matches = payload
        .windows(NAME.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == NAME).then_some(offset));
    let Some(offset) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Ok(None);
    }
    let Some(header) = offset.checked_add(NAME.len()) else {
        return Ok(None);
    };
    let Some(region_token) = View::u16_le_at(payload, header) else {
        return Ok(None);
    };
    if region_token == 0 {
        return Ok(None);
    }
    let Some(count) = View::u16_le_at(payload, header + 2).map(usize::from) else {
        return Ok(None);
    };
    if count < 3 {
        return Ok(None);
    }
    // Each region entry consumes a 12-byte record from `header + 4` onward.
    let Some(entries_start) = header.checked_add(4) else {
        return Ok(None);
    };
    let Some(remaining) = payload.len().checked_sub(entries_start) else {
        return Ok(None);
    };
    if bounded_len(cadmpeg_core::decode::u64_from_index(count), 12, remaining).is_none() {
        return Ok(None);
    }
    let mut addresses = ctx.alloc_filled(count, 0u16, "collect SLDPRT compact region addresses")?;
    let mut entry_token = None;
    for (index, slot) in addresses.iter_mut().enumerate() {
        let Some(entry) = header.checked_add(4 + index * 12) else {
            return Ok(None);
        };
        let Some(token) = View::u16_le_at(payload, entry) else {
            return Ok(None);
        };
        if !matches!(token, 0x80e1 | 0x8386 | 0xbc87)
            || entry_token.is_some_and(|existing| existing != token)
            || payload.get(entry + 4..entry + 8) != Some(&[0xff; 4])
            || payload.get(entry + 8..entry + 12) != Some(&[0; 4])
        {
            return Ok(None);
        }
        entry_token = Some(token);
        let Some(address) = View::u16_le_at(payload, entry + 2) else {
            return Ok(None);
        };
        *slot = address;
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(count),
        "validate SLDPRT compact region addresses",
    )?;
    let mut seen = ctx.alloc_filled(count, false, "validate SLDPRT compact region addresses")?;
    for address in &addresses {
        let Some(index) = usize::from(*address)
            .checked_sub(1)
            .filter(|index| *index < count)
        else {
            return Ok(None);
        };
        if seen[index] {
            return Ok(None);
        }
        seen[index] = true;
    }
    Ok(Some(addresses))
}

pub(super) fn compact_line_chain_addresses(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<u16>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(payload.len()),
        "scan SLDPRT compact line chain",
    )?;
    let mut selected = None;
    for offset in 0..payload.len() {
        let Some((bytes, count)) = (|| {
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
            Some((bytes, count))
        })() else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(count),
            "validate SLDPRT compact line chain",
        )?;
        let mut addresses =
            ctx.alloc_filled(count, 0u16, "collect SLDPRT compact chain addresses")?;
        let mut seen = [false; 64];
        let mut valid = true;
        for (index, address) in addresses.iter_mut().enumerate() {
            let parsed =
                View::u32_le_at(bytes, 2 + index * 4).and_then(|value| u16::try_from(value).ok());
            let Some(parsed) = parsed else {
                valid = false;
                break;
            };
            let Some(position) = usize::from(parsed)
                .checked_sub(1)
                .filter(|value| *value < count)
            else {
                valid = false;
                break;
            };
            if seen[position] {
                valid = false;
                break;
            }
            seen[position] = true;
            *address = parsed;
        }
        if !valid {
            continue;
        }
        match selected.as_ref() {
            Some(previous) if previous != &addresses => return Ok(None),
            Some(_) => {}
            None => selected = Some(addresses),
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod curves_tests;
