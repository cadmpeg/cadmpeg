//! Marker arc, circle and rectangle profile resolution.

use super::compact_reference_planes::principal_sketch_frame;
use super::endpoints::arc_centers::{unique_arc_center_marker, ArcCenterIndex};
use super::endpoints::{
    compact_legacy_code_one_line_endpoint_indices, compact_legacy_curve_endpoint_indices,
    marker_profile_curve_role, minor_arc_angles, minor_arc_geometry, one_based_u16_endpoint_pair,
    wide_indexed_curve_endpoint_indices,
};
use super::grid::quantize;
use super::markers::{
    compact_legacy_marker_body, finite_coordinate_pair, marker_native_code, sketch_marker_prefix_at,
};
use super::reference_geometry::reference_plane_frame_key;
use super::relation_loci::same_dimension_length;
use super::scalars::ObjectNames;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::records::ObjectId;
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{bounded_len, DecodeContext, ScopedReservation, View};
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
use std::collections::{BTreeMap, HashMap, HashSet};

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

impl cadmpeg_core::decode::cost::DecodeCost for SketchPlaneFrame {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.origin,
                &self.normal,
                &self.u_axis,
                &self.u_axis_source,
            ),
            ctx,
            operation,
        )
    }
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
    entities_storage: &mut ScopedReservation<'_>,
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

    let mut scratch = ctx.reserve_scoped(0, "collect SLDPRT semicircle geometry scratch")?;
    let mut records = Vec::new();
    if ctx.any_by(
        markers,
        |marker| {
            if usize::try_from(marker.offset())
                .ok()
                .is_some_and(|offset| current_linked_semicircle_record(payload, offset))
            {
                if records.len() == 2 {
                    return Ok(true);
                }
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut records, 1, "collect SLDPRT semicircle records")
                })?;
                records.push(*marker);
            }
            Ok(false)
        },
        "collect SLDPRT semicircle records",
    )? {
        return Ok(());
    }
    let [first_record, second_record] = records.as_slice() else {
        return Ok(());
    };
    let record_refs = [first_record.id(), second_record.id()];
    let mut curve_entities = Vec::new();
    if ctx.any_by(
        &entities[..],
        |entity| {
            if matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
                    | SketchGeometryDefinition::Arc { .. }
                    | SketchGeometryDefinition::Circle { .. }
                    | SketchGeometryDefinition::Ellipse { .. }
                    | SketchGeometryDefinition::Nurbs { .. }
                    | SketchGeometryDefinition::Native { .. }
            ) {
                if curve_entities.len() == 2 {
                    return Ok(true);
                }
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut curve_entities, 1, "collect SLDPRT semicircle curves")
                })?;
                curve_entities.push(entity);
            }
            Ok(false)
        },
        "collect SLDPRT semicircle curves",
    )? {
        return Ok(());
    }
    if curve_entities.len() != 2 {
        return Ok(());
    }
    for entity in &curve_entities {
        let Some(id) = entity.native_ref.as_deref() else {
            return Ok(());
        };
        if !ctx.contains(&record_refs, &id, "collect SLDPRT semicircle curves")? {
            return Ok(());
        }
    }
    let mut points = Vec::new();
    if ctx.any_by(
        &entities[..],
        |entity| {
            let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
                return Ok(false);
            };
            let Some(native_ref) = entity.native_ref.as_deref() else {
                return Ok(false);
            };
            if points.len() == 6 {
                return Ok(true);
            }
            let native_ref = scratch.with_storage(|| {
                ctx.copy_retained_text(native_ref, "copy SLDPRT semicircle point identity")
            })?;
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut points, 1, "collect SLDPRT semicircle points")
            })?;
            points.push((native_ref, position.get()));
            Ok(false)
        },
        "collect SLDPRT semicircle points",
    )? {
        return Ok(());
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
        let mut linked_records = [None; 2];
        for (slot, record) in linked_records.iter_mut().zip(records.iter().copied()) {
            if ctx.any_by(
                record.links(),
                |link| {
                    ctx.equal(
                        link.entity_ref.as_str(),
                        center_ref.as_str(),
                        "collect SLDPRT semicircle centers",
                    )
                },
                "collect SLDPRT semicircle centers",
            )? {
                *slot = Some(record);
            }
        }
        let ((Some(record), None) | (None, Some(record))) = (linked_records[0], linked_records[1])
        else {
            continue;
        };
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut centers, 1, "collect SLDPRT semicircle centers")
        })?;
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
    if ctx.equal(first.0, second.0, "collect SLDPRT semicircle centers")?
        || !same_dimension_length(first.5, second.5)
    {
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
            let endpoint =
                ctx.copy_retained_text(endpoint, "copy SLDPRT semicircle endpoint identity")?;
            ctx.push_vec(
                &mut endpoint_refs,
                endpoint,
                "collect SLDPRT semicircle endpoints",
            )?;
        }
        Ok(Some((geometry, endpoint_refs)))
    };
    let (first_result, first_storage) = ctx
        .with_scoped_storage("collect SLDPRT semicircle endpoints", || {
            arc(first.2, first.5, &first_refs, first_endpoints, false)
        })?;
    let Some((first_geometry, first_endpoint_refs)) = first_result else {
        return Ok(());
    };
    let (second_result, second_storage) = ctx
        .with_scoped_storage("collect SLDPRT semicircle endpoints", || {
            arc(second.2, second.5, &second_refs, second_endpoints, true)
        })?;
    let Some((second_geometry, second_endpoint_refs)) = second_result else {
        return Ok(());
    };

    let native_position = |entities: &[SketchEntity], native: &str| {
        ctx.position_by(
            entities,
            |entity| {
                ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(native),
                    "find SLDPRT semicircle entity",
                )
            },
            "find SLDPRT semicircle entity",
        )
    };
    let Some(first_position) = native_position(entities, first.0)? else {
        return Ok(());
    };
    first_storage.commit()?;
    let first_entity = &mut entities[first_position];
    first_entity.construction = false;
    first_entity.endpoint_refs = first_endpoint_refs;
    first_entity.geometry = first_geometry;
    let sketch = scratch.with_storage(|| {
        first_entity
            .sketch
            .try_clone_for_decode(ctx, "copy SLDPRT semicircle sketch identity")
    })?;
    let Some(second_position) = native_position(entities, second.0)? else {
        return Ok(());
    };
    second_storage.commit()?;
    let second_entity = &mut entities[second_position];
    second_entity.construction = false;
    second_entity.endpoint_refs = second_endpoint_refs;
    second_entity.geometry = second_geometry;
    let sketch_key = ctx
        .rsplit_once(sketch.as_str(), "#", "split SLDPRT semicircle sketch key")?
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
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
        else {
            continue;
        };
        let Some(id) = super::profiles::mint_formatted::<SketchEntityId>(
            ctx,
            format_args!("sldprt:model:sketch-entity#linked-semicircle:{sketch_key}:{index}"),
            "format SLDPRT semicircle line identity",
        )?
        else {
            continue;
        };
        let mut endpoint_refs = Vec::new();
        for endpoint in [start_ref, end_ref] {
            let endpoint =
                ctx.copy_retained_text(endpoint, "copy SLDPRT semicircle line endpoint identity")?;
            ctx.push_vec(
                &mut endpoint_refs,
                endpoint,
                "collect SLDPRT semicircle line endpoints",
            )?;
        }
        let sketch_copy =
            sketch.try_clone_for_decode(ctx, "copy SLDPRT semicircle line sketch identity")?;
        entities_storage
            .with_storage(|| ctx.reserve_vec(entities, 1, "append SLDPRT semicircle line"))?;
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

/// Declared slot records and their fixed-stride continuations, resolved in offset order.
pub(super) struct SlotReferences<'ctx> {
    records: HashMap<usize, SlotReferenceLayout>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> SlotReferences<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, payload: &[u8]) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT slot predecessors";
        const SLOT_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
        let declared_at = |offset: usize| {
            offset
                .checked_sub(SLOT_DECLARATION.len())
                .and_then(|start| payload.get(start..offset))
                == Some(SLOT_DECLARATION)
        };
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut records: HashMap<usize, SlotReferenceLayout> = HashMap::new();
        for (offset, _) in ctx.admit_iter(payload, OPERATION)?.enumerate() {
            if !sketch_marker_prefix_at(payload, offset) {
                continue;
            }
            let Some(layout) = slot_curve_reference_cells(payload, offset) else {
                continue;
            };
            let mut declared = declared_at(offset);
            if !declared {
                if let Some(previous) = layout
                    .continuation_stride
                    .and_then(|stride| offset.checked_sub(stride))
                {
                    declared = declared_at(previous)
                        || ctx
                            .get_hash_map(&records, &previous, OPERATION)?
                            .is_some_and(|candidate| {
                                candidate.continuation_stride == layout.continuation_stride
                            });
                }
            }
            if declared {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut records, offset, layout, OPERATION)
                })?;
            }
        }
        Ok(Self {
            records,
            _storage: storage,
        })
    }
}

type SlotCurveAndCenterIndices = ([usize; 4], [usize; 2]);

pub(super) fn slot_curve_and_center_indices(
    ctx: &DecodeContext<'_>,
    slots: &SlotReferences<'_>,
    offset: usize,
) -> Result<Option<SlotCurveAndCenterIndices>, CodecError> {
    Ok(ctx
        .get_hash_map(&slots.records, &offset, "resolve SLDPRT indexed slot")?
        .map(|layout| {
            (
                [
                    layout.indices[0],
                    layout.indices[1],
                    layout.indices[2],
                    layout.indices[3],
                ],
                [layout.indices[4], layout.indices[5]],
            )
        }))
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
        Some(prefix) if prefix == SKETCH_MARKER => &[(72, 12, None)][..],
        Some(prefix) if prefix == LEGACY_SKETCH_MARKER => &[(64, 8, Some(126))][..],
        Some(prefix) if prefix == LEGACY_EXTENDED_SKETCH_MARKER => {
            &[(64, 8, Some(126)), (64, 12, None)][..]
        }
        _ => return None,
    };
    layouts
        .iter()
        .copied()
        .find_map(|(cells_offset, cell_size, continuation_stride)| {
            let mut cells = [(0, 0); 6];
            for (index, cell) in cells.iter_mut().enumerate() {
                let start = offset.checked_add(cells_offset + index * cell_size)?;
                let bytes = payload.get(start..start + cell_size)?;
                if bytes[4..8] != [0xff; 4] || (cell_size != 8 && bytes.get(8..12) != Some(&[0; 4]))
                {
                    return None;
                }
                *cell = (
                    View::u16_le_at(bytes, 0)?,
                    usize::from(View::u16_le_at(bytes, 2)?),
                );
            }
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
    slots: &SlotReferences<'_>,
    markers: &[&SketchInputEntity],
    entities: &mut [SketchEntity],
    tolerance: f64,
) -> Result<(), CodecError> {
    let Some((curve_indices, center_indices)) = ctx.find_map(
        markers,
        |marker| match usize::try_from(marker.offset()).ok() {
            Some(offset) => slot_curve_and_center_indices(ctx, slots, offset),
            None => Ok(None),
        },
        "find SLDPRT slot record",
    )?
    else {
        return Ok(());
    };
    let mut scratch = ctx.reserve_scoped(0, "collect SLDPRT slot geometry scratch")?;
    let mut curves = Vec::new();
    if ctx.any_by(
        markers,
        |marker| {
            if marker.coordinates_m.is_none()
                && matches!(
                    marker.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
            {
                if curves.len() == 4 {
                    return Ok(true);
                }
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut curves, 1, "collect SLDPRT slot curves")
                })?;
                curves.push(*marker);
            }
            Ok(false)
        },
        "collect SLDPRT slot curves",
    )? {
        return Ok(());
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
    for (index, marker) in cycle.iter().enumerate() {
        for other in &cycle[index + 1..] {
            if ctx.equal(marker.id(), other.id(), "collect SLDPRT slot curves")? {
                return Ok(());
            }
        }
    }
    let mut points = Vec::new();
    for marker in ctx.admit_iter(markers, "collect SLDPRT slot points")? {
        if marker.coordinates_m.is_some()
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        {
            scratch
                .with_storage(|| ctx.reserve_vec(&mut points, 1, "collect SLDPRT slot points"))?;
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
    if ctx.equal(center_refs[0], center_refs[1], "collect SLDPRT slot points")? {
        return Ok(());
    }
    let last_native = |native: &str| {
        ctx.rposition_by(
            &entities[..],
            |entity| {
                ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(native),
                    "find SLDPRT slot curve entities",
                )
            },
            "find SLDPRT slot curve entities",
        )
    };
    let [Some(first), Some(second), Some(third), Some(fourth)] = [
        last_native(first.id())?,
        last_native(second.id())?,
        last_native(third.id())?,
        last_native(fourth.id())?,
    ] else {
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
    let endpoint_not_shared = |entity: usize, other: usize| -> Result<Option<&str>, CodecError> {
        let Some(entity) = endpoint_refs(entity) else {
            return Ok(None);
        };
        let Some(other) = endpoint_refs(other) else {
            return Ok(None);
        };
        let first_unique = !ctx.contains(&other, &entity[0], "find SLDPRT slot endpoints")?;
        let second_unique = !ctx.contains(&other, &entity[1], "find SLDPRT slot endpoints")?;
        Ok(match (first_unique, second_unique) {
            (true, false) => Some(entity[0]),
            (false, true) => Some(entity[1]),
            _ => None,
        })
    };
    let previous = cycle_entities[(target_position + 3) % 4];
    let previous_other = cycle_entities[(target_position + 2) % 4];
    let next = cycle_entities[(target_position + 1) % 4];
    let next_other = cycle_entities[(target_position + 2) % 4];
    let Some(start_ref) = endpoint_not_shared(previous, previous_other)? else {
        return Ok(());
    };
    let Some(end_ref) = endpoint_not_shared(next, next_other)? else {
        return Ok(());
    };
    if ctx.equal(start_ref, end_ref, "find SLDPRT slot endpoints")? {
        return Ok(());
    }
    let point_position = |reference: &str| -> Result<Option<Point2>, CodecError> {
        let position = ctx.rposition_by(
            &entities[..],
            |entity| {
                Ok(matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                ) && ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(reference),
                    "find SLDPRT slot point entities",
                )?)
            },
            "find SLDPRT slot point entities",
        )?;
        Ok(
            position.and_then(|index| match *entities[index].geometry.definition() {
                SketchGeometryDefinition::Point { position } => Some(position.get()),
                _ => None,
            }),
        )
    };
    let (Some(start), Some(end)) = (point_position(start_ref)?, point_position(end_ref)?) else {
        return Ok(());
    };
    let (Some(first_center), Some(second_center)) = (
        point_position(center_refs[0])?,
        point_position(center_refs[1])?,
    ) else {
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
        let reference = ctx.copy_retained_text(reference, "copy SLDPRT slot endpoint identity")?;
        ctx.push_vec(
            &mut endpoint_refs,
            reference,
            "collect SLDPRT slot endpoints",
        )?;
    }
    entities[*target].endpoint_refs = endpoint_refs;
    entities[*target].geometry = geometry;
    Ok(())
}

fn closed_cycle_marker_arc_geometry(
    ctx: &DecodeContext<'_>,
    target_index: usize,
    target: &SketchEntity,
    lines_by_endpoint: &HashMap<&str, Vec<&SketchEntity>>,
    point_by_ref: &HashMap<&str, Point2>,
    circular_witnesses: &[CircularArcWitness<'_>],
    tolerance: f64,
) -> Result<Option<SketchGeometry>, CodecError> {
    const OPERATION: &str = "scan SLDPRT connected arc neighbors";
    if !matches!((
        target.geometry).definition(),
        SketchGeometryDefinition::Native { ref native_kind }
            if native_kind.as_str() == "sldprt:marker-geometry:2"
    ) || target.construction
        || target.endpoint_refs.len() != 2
    {
        return Ok(None);
    }
    let [target_start, target_end] = target.endpoint_refs.as_slice() else {
        return Ok(None);
    };
    let target_endpoints = [target_start.as_str(), target_end.as_str()];
    let (Some(target_start_point), Some(target_end_point)) = (
        ctx.get_hash_map(point_by_ref, target_start.as_str(), OPERATION)?,
        ctx.get_hash_map(point_by_ref, target_end.as_str(), OPERATION)?,
    ) else {
        return Ok(None);
    };
    if ctx.equal(target_endpoints[0], target_endpoints[1], OPERATION)? {
        return Ok(None);
    }
    let mut unique = None;
    let mut ambiguous = false;
    ctx.position_by(
        circular_witnesses,
        |witness| {
            if witness.index == target_index
                || !witness.radius.is_finite()
                || witness.radius <= 0.0
                || !ctx.equal(witness.sketch, &target.sketch, OPERATION)?
            {
                return Ok(false);
            }
            let witness_endpoints = witness.endpoints;
            if ctx.contains(&witness_endpoints, &target_endpoints[0], OPERATION)?
                || ctx.contains(&witness_endpoints, &target_endpoints[1], OPERATION)?
                || ctx.equal(witness_endpoints[0], witness_endpoints[1], OPERATION)?
            {
                return Ok(false);
            }
            let (Some(witness_start_point), Some(witness_end_point)) = (
                ctx.get_hash_map(point_by_ref, witness_endpoints[0], OPERATION)?,
                ctx.get_hash_map(point_by_ref, witness_endpoints[1], OPERATION)?,
            ) else {
                return Ok(false);
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
                return Ok(false);
            }
            // Lines joining a target endpoint to a witness endpoint, each counted once.
            let mut connecting_lines: [Option<&SketchEntity>; 2] = [None; 2];
            let mut connecting_count = 0;
            for endpoint in target_endpoints {
                let mut visited = ctx
                    .get_hash_map(lines_by_endpoint, endpoint, OPERATION)?
                    .map_or(&[][..], Vec::as_slice)
                    .iter();
                while let Some(&line) = ctx.next_charged(&mut visited, OPERATION)? {
                    let ([first_ref, second_ref], SketchGeometryDefinition::Line { start, end }) =
                        (line.endpoint_refs.as_slice(), line.geometry.definition())
                    else {
                        continue;
                    };
                    if connecting_lines
                        .iter()
                        .flatten()
                        .any(|existing| std::ptr::eq(*existing, line))
                        || !ctx.equal(&line.sketch, &target.sketch, OPERATION)?
                        || (!ctx.contains(&witness_endpoints, &first_ref.as_str(), OPERATION)?
                            && !ctx.contains(
                                &witness_endpoints,
                                &second_ref.as_str(),
                                OPERATION,
                            )?)
                    {
                        continue;
                    }
                    let (Some(first), Some(second)) = (
                        ctx.get_hash_map(point_by_ref, first_ref.as_str(), OPERATION)?,
                        ctx.get_hash_map(point_by_ref, second_ref.as_str(), OPERATION)?,
                    ) else {
                        continue;
                    };
                    let matches = (same_dimension_length(start.u, first.u)
                        && same_dimension_length(start.v, first.v)
                        && same_dimension_length(end.u, second.u)
                        && same_dimension_length(end.v, second.v))
                        || (same_dimension_length(start.u, second.u)
                            && same_dimension_length(start.v, second.v)
                            && same_dimension_length(end.u, first.u)
                            && same_dimension_length(end.v, first.v));
                    if !matches {
                        continue;
                    }
                    if connecting_count == 2 {
                        return Ok(false);
                    }
                    connecting_lines[connecting_count] = Some(line);
                    connecting_count += 1;
                }
            }
            let [Some(first_line), Some(second_line)] = connecting_lines else {
                return Ok(false);
            };
            let connecting_lines = [first_line, second_line];
            for line in connecting_lines {
                let [first, second] = line.endpoint_refs.as_slice() else {
                    return Ok(false);
                };
                if ctx.equal(first, second, OPERATION)?
                    || (ctx.contains(&target_endpoints, &first.as_str(), OPERATION)?
                        && ctx.contains(&target_endpoints, &second.as_str(), OPERATION)?)
                    || (ctx.contains(&witness_endpoints, &first.as_str(), OPERATION)?
                        && ctx.contains(&witness_endpoints, &second.as_str(), OPERATION)?)
                {
                    return Ok(false);
                }
            }
            for endpoint in [
                target_endpoints[0],
                target_endpoints[1],
                witness_endpoints[0],
                witness_endpoints[1],
            ] {
                if !ctx.any_by(
                    connecting_lines,
                    |line| {
                        ctx.any_by(
                            &line.endpoint_refs,
                            |reference| ctx.equal(reference.as_str(), endpoint, OPERATION),
                            OPERATION,
                        )
                    },
                    OPERATION,
                )? {
                    return Ok(false);
                }
            }
            let Some(geometry) = minor_arc_geometry(
                *target_start_point,
                *target_end_point,
                witness.center,
                tolerance,
            ) else {
                return Ok(false);
            };
            ambiguous = unique.is_some();
            unique = Some(geometry);
            Ok(ambiguous)
        },
        OPERATION,
    )?;
    Ok(unique.filter(|_| !ambiguous))
}

pub(super) fn resolve_connected_marker_arcs(
    ctx: &DecodeContext<'_>,
    entities: &mut [SketchEntity],
    tolerance: f64,
) -> Result<(), CodecError> {
    const OPERATION: &str = "scan SLDPRT connected arc neighbors";
    let is_native_arc = |entity: &SketchEntity| {
        matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Native { native_kind }
                if native_kind.as_str() == "sldprt:marker-geometry:2"
        )
    };
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    // Point positions by native reference, copied because the arena is edited below.
    let mut points = HashMap::<String, Point2>::new();
    let mut center_replacements = Vec::new();
    {
        let mut sketch_points = BTreeMap::<&SketchId, Vec<(Option<&str>, Point2)>>::new();
        for entity in ctx.admit_iter(&entities[..], OPERATION)? {
            let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
                continue;
            };
            let Some(native_ref) = entity.native_ref.as_deref() else {
                continue;
            };
            storage.with_storage(|| {
                let retained_ref =
                    ctx.copy_retained_text(native_ref, "copy SLDPRT connected arc point identity")?;
                ctx.insert_hash_map(
                    &mut points,
                    retained_ref,
                    position.get(),
                    "index SLDPRT connected arc points",
                )
            })?;
            storage.with_storage(|| {
                ctx.push_btree_group(
                    &mut sketch_points,
                    &entity.sketch,
                    (Some(native_ref), position.get()),
                    "index SLDPRT connected arc points",
                    "collect SLDPRT connected arc point records",
                )
            })?;
        }
        let mut center_indexes = HashMap::new();
        for (index, entity) in ctx.admit_iter(&entities[..], OPERATION)?.enumerate() {
            if !is_native_arc(entity) {
                continue;
            }
            let [start_ref, end_ref] = entity.endpoint_refs.as_slice() else {
                continue;
            };
            let (Some(start), Some(end)) = (
                ctx.get_hash_map(&points, start_ref.as_str(), OPERATION)?
                    .copied(),
                ctx.get_hash_map(&points, end_ref.as_str(), OPERATION)?
                    .copied(),
            ) else {
                continue;
            };
            if !ctx.contains_key_hash_map(&center_indexes, &entity.sketch, OPERATION)? {
                let Some(sketch_points) =
                    ctx.get_btree_map(&sketch_points, &entity.sketch, OPERATION)?
                else {
                    continue;
                };
                let index = ArcCenterIndex::from_points(ctx, sketch_points, tolerance)?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut center_indexes, &entity.sketch, index, OPERATION)
                })?;
            }
            let Some(candidates) = ctx.get_hash_map(&center_indexes, &entity.sketch, OPERATION)?
            else {
                continue;
            };
            let Some(center) = unique_arc_center_marker(
                ctx,
                start,
                end,
                candidates,
                tolerance,
                [Some(start_ref.as_str()), Some(end_ref.as_str())],
            )?
            else {
                continue;
            };
            let Some(geometry) = minor_arc_geometry(start, end, center, tolerance) else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut center_replacements,
                    (index, geometry),
                    "collect SLDPRT connected arc replacements",
                )
            })?;
        }
    }
    for (index, geometry) in ctx.admit_iter(center_replacements, OPERATION)? {
        entities[index].geometry = geometry;
    }
    let mut cycle_replacements = Vec::new();
    {
        let mut circular_witnesses = Vec::new();
        let mut point_by_ref = HashMap::new();
        let mut lines_by_endpoint = HashMap::<&str, Vec<&SketchEntity>>::new();
        for (index, entity) in ctx.admit_iter(&entities[..], OPERATION)?.enumerate() {
            match *entity.geometry.definition() {
                SketchGeometryDefinition::Arc { center, radius, .. } if !entity.construction => {
                    let [start, end] = entity.endpoint_refs.as_slice() else {
                        continue;
                    };
                    storage.with_storage(|| {
                        ctx.push_vec(
                            &mut circular_witnesses,
                            CircularArcWitness {
                                index,
                                sketch: &entity.sketch,
                                endpoints: [start.as_str(), end.as_str()],
                                center: center.get(),
                                radius: radius.get(),
                            },
                            "collect SLDPRT connected arc witnesses",
                        )
                    })?;
                }
                SketchGeometryDefinition::Point { position } => {
                    let Some(native_ref) = entity.native_ref.as_deref() else {
                        continue;
                    };
                    storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut point_by_ref,
                            native_ref,
                            position.get(),
                            "index SLDPRT connected arc point references",
                        )
                    })?;
                }
                SketchGeometryDefinition::Line { .. }
                    if !entity.construction && entity.endpoint_refs.len() == 2 =>
                {
                    for endpoint in ctx.admit_iter(&entity.endpoint_refs, OPERATION)? {
                        storage.with_storage(|| {
                            ctx.push_hash_group(
                                &mut lines_by_endpoint,
                                endpoint.as_str(),
                                entity,
                                OPERATION,
                                OPERATION,
                            )
                        })?;
                    }
                }
                _ => {}
            }
        }
        for (target_index, target) in ctx.admit_iter(&entities[..], OPERATION)?.enumerate() {
            if let Some(geometry) = closed_cycle_marker_arc_geometry(
                ctx,
                target_index,
                target,
                &lines_by_endpoint,
                &point_by_ref,
                &circular_witnesses,
                tolerance,
            )? {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut cycle_replacements,
                        (target_index, geometry),
                        "collect SLDPRT connected arc cycle replacements",
                    )
                })?;
            }
        }
    }
    for (index, geometry) in ctx.admit_iter(cycle_replacements, OPERATION)? {
        entities[index].geometry = geometry;
    }
    let mut replacements = Vec::new();
    {
        // Native arcs grouped by endpoint, so a component grows through shared endpoints.
        let mut arcs = Vec::new();
        let mut arcs_by_endpoint = HashMap::<&str, Vec<usize>>::new();
        for (index, entity) in ctx.admit_iter(&entities[..], OPERATION)?.enumerate() {
            if entity.endpoint_refs.len() != 2 || !is_native_arc(entity) {
                continue;
            }
            storage.with_storage(|| {
                ctx.push_vec(&mut arcs, index, "collect SLDPRT connected native arcs")
            })?;
            for endpoint in ctx.admit_iter(&entity.endpoint_refs, OPERATION)? {
                storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut arcs_by_endpoint,
                        endpoint.as_str(),
                        index,
                        "visit SLDPRT connected native arc",
                        "visit SLDPRT connected native arc",
                    )
                })?;
            }
        }
        let mut visited = storage.with_storage(|| {
            ctx.alloc_filled(entities.len(), false, "visit SLDPRT connected native arc")
        })?;
        for &first in ctx.admit_iter(&arcs, OPERATION)? {
            if visited[first] {
                continue;
            }
            visited[first] = true;
            let mut component_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut component = Vec::new();
            component_storage.with_storage(|| {
                ctx.push_vec(
                    &mut component,
                    first,
                    "collect SLDPRT connected arc component",
                )
            })?;
            let mut positions = 0..entities.len();
            let mut cursor = 0;
            while cursor < component.len() {
                let Some(position) = ctx.next_charged(&mut positions, OPERATION)? else {
                    break;
                };
                let current = component[position];
                cursor = position + 1;
                for endpoint in ctx.admit_iter(&entities[current].endpoint_refs, OPERATION)? {
                    for &candidate in ctx.admit_iter(
                        ctx.get_hash_map(&arcs_by_endpoint, endpoint.as_str(), OPERATION)?
                            .map_or(&[][..], Vec::as_slice),
                        OPERATION,
                    )? {
                        if visited[candidate] {
                            continue;
                        }
                        visited[candidate] = true;
                        component_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut component,
                                candidate,
                                "collect SLDPRT connected arc component",
                            )
                        })?;
                    }
                }
            }
            let mut endpoint_refs = Vec::new();
            for index in ctx.admit_iter(&component, OPERATION)? {
                for reference in ctx.admit_iter(&entities[*index].endpoint_refs, OPERATION)? {
                    component_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut endpoint_refs,
                            reference,
                            "collect SLDPRT connected arc endpoints",
                        )
                    })?;
                }
            }
            ctx.sort_unstable_by(
                &mut endpoint_refs,
                |value| value,
                Ord::cmp,
                "sort SLDPRT connected arc endpoints",
            )?;
            ctx.dedup_vec(
                &mut endpoint_refs,
                "deduplicate SLDPRT connected arc endpoints",
            )?;
            let mut component_points = Vec::new();
            let mut missing_point = false;
            let mut endpoint_iter = endpoint_refs.iter();
            while let Some(endpoint) = ctx.next_charged(&mut endpoint_iter, OPERATION)? {
                let Some(point) = ctx
                    .get_hash_map(&points, endpoint.as_str(), OPERATION)?
                    .copied()
                else {
                    missing_point = true;
                    break;
                };
                component_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut component_points,
                        point,
                        "collect SLDPRT connected arc component points",
                    )
                })?;
            }
            if missing_point {
                continue;
            }
            let Some((center, _)) = fitted_marker_circle(ctx, &component_points, tolerance)? else {
                continue;
            };
            let mut component_replacements = Vec::new();
            let mut visited = component.iter();
            while let Some(&index) = ctx.next_charged(&mut visited, OPERATION)? {
                let [start_ref, end_ref] = entities[index].endpoint_refs.as_slice() else {
                    continue;
                };
                let (Some(start), Some(end)) = (
                    ctx.get_hash_map(&points, start_ref.as_str(), OPERATION)?
                        .copied(),
                    ctx.get_hash_map(&points, end_ref.as_str(), OPERATION)?
                        .copied(),
                ) else {
                    component_replacements.clear();
                    break;
                };
                let Some(geometry) = minor_arc_geometry(start, end, center, tolerance) else {
                    component_replacements.clear();
                    break;
                };
                component_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut component_replacements,
                        (index, geometry),
                        "collect SLDPRT connected arc component replacements",
                    )
                })?;
            }
            if component_replacements.len() >= 2 {
                storage.with_storage(|| {
                    ctx.extend_vec(
                        &mut replacements,
                        component_replacements,
                        "collect SLDPRT connected arc replacements",
                    )
                })?;
            }
        }
    }
    for (index, geometry) in ctx.admit_iter(replacements, OPERATION)? {
        entities[index].geometry = geometry;
    }
    for entity in ctx.admit_iter(entities, OPERATION)? {
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
        let (Some(first), Some(second)) = (
            ctx.get_hash_map(&points, first_ref.as_str(), OPERATION)?,
            ctx.get_hash_map(&points, second_ref.as_str(), OPERATION)?,
        ) else {
            continue;
        };
        let geometry_start = Point2::new(
            center.u + radius.get() * start_angle.get().cos(),
            center.v + radius.get() * start_angle.get().sin(),
        );
        let first_distance = (geometry_start.u - first.u).hypot(geometry_start.v - first.v);
        let second_distance = (geometry_start.u - second.u).hypot(geometry_start.v - second.v);
        if second_distance < first_distance {
            ctx.reverse(&mut entity.endpoint_refs, OPERATION)?;
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
    const SCAN: &str = "scan SLDPRT closed curve incidence";
    const COPY: &str = "copy SLDPRT closed curve identity";
    let mut storage = ctx.reserve_scoped(0, "index SLDPRT closed curve component")?;
    let mut profiles = Vec::new();
    let mut curves = Vec::new();
    for (index, item) in ctx.admit_iter(entities, SCAN)?.enumerate() {
        let entity = item.borrow();
        if entity.construction {
            continue;
        }
        match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { .. } => {
                let mut profile = Vec::new();
                ctx.push_vec(
                    &mut profile,
                    SketchEntityUse {
                        entity: entity
                            .id()
                            .try_clone_for_decode(ctx, "copy SLDPRT closed circle identity")?,
                        reversed: false,
                    },
                    "collect SLDPRT closed circle profile",
                )?;
                ctx.push_vec(&mut profiles, profile, "collect SLDPRT closed profiles")?;
            }
            SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
                if entity.endpoint_refs.len() == 2 =>
            {
                storage.with_storage(|| {
                    ctx.push_vec(&mut curves, index, "collect SLDPRT closed curves")
                })?;
            }
            _ => {}
        }
    }
    let mut incidence = HashMap::<&str, Vec<usize>>::new();
    for &index in ctx.admit_iter(&curves, SCAN)? {
        for endpoint in ctx.admit_iter(&entities[index].borrow().endpoint_refs, SCAN)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut incidence,
                    endpoint.as_str(),
                    index,
                    "index SLDPRT closed curve endpoints",
                    "collect SLDPRT endpoint incidence",
                )
            })?;
        }
    }
    let incident = |endpoint: &str| -> Result<&[usize], CodecError> {
        Ok(ctx
            .get_hash_map(&incidence, endpoint, SCAN)?
            .map_or(&[][..], Vec::as_slice))
    };
    // Curves not yet placed in a profile, and the round that last reached each curve.
    let mut unused = storage.with_storage(|| {
        ctx.alloc_filled(entities.len(), false, "index SLDPRT unused closed curves")
    })?;
    for &index in ctx.admit_iter(&curves, SCAN)? {
        unused[index] = true;
    }
    let mut reached = storage.with_storage(|| {
        ctx.alloc_filled(
            entities.len(),
            0_usize,
            "index SLDPRT closed curve component",
        )
    })?;
    let mut component = Vec::new();
    let mut cursor = 0;
    let mut round = 0_usize;
    // Each round starts from the lowest unused curve; curves only leave the unused set.
    while let Some(position) =
        ctx.position_by(&curves[cursor..], |index| Ok(unused[*index]), SCAN)?
    {
        cursor += position;
        let first = curves[cursor];
        round += 1;
        ctx.clear_vec(&mut component, "collect SLDPRT closed curve frontier")?;
        reached[first] = round;
        storage.with_storage(|| {
            ctx.push_vec(
                &mut component,
                first,
                "collect SLDPRT closed curve frontier",
            )
        })?;
        let mut positions = 0..entities.len();
        let mut next = 0;
        while next < component.len() {
            let Some(position) = ctx.next_charged(&mut positions, SCAN)? else {
                break;
            };
            let curve = component[position];
            next = position + 1;
            for endpoint in ctx.admit_iter(&entities[curve].borrow().endpoint_refs, SCAN)? {
                for &adjacent in ctx.admit_iter(incident(endpoint.as_str())?, SCAN)? {
                    if reached[adjacent] == round {
                        continue;
                    }
                    reached[adjacent] = round;
                    storage.with_storage(|| {
                        ctx.push_vec(
                            &mut component,
                            adjacent,
                            "collect SLDPRT closed curve frontier",
                        )
                    })?;
                }
            }
        }
        if reject_branching_components
            && ctx.any_by(
                &component,
                |curve| {
                    ctx.any_by(
                        &entities[*curve].borrow().endpoint_refs,
                        |endpoint| Ok(incident(endpoint.as_str())?.len() != 2),
                        SCAN,
                    )
                },
                SCAN,
            )?
        {
            for &curve in ctx.admit_iter(&component, SCAN)? {
                unused[curve] = false;
            }
            continue;
        }
        let start = entities[first].borrow().endpoint_refs[0].as_str();
        let mut current = start;
        let mut curve = first;
        let mut profile_storage = ctx.reserve_scoped(0, "collect SLDPRT closed curve profile")?;
        let mut profile = Vec::new();
        let mut steps = 0..=entities.len();
        // Each step places one unused curve, so the walk ends within the curve count.
        while ctx.next_charged(&mut steps, SCAN)?.is_some() {
            if !unused[curve] {
                profile.clear();
                break;
            }
            unused[curve] = false;
            let [curve_start, curve_end] = entities[curve].borrow().endpoint_refs.as_slice() else {
                profile.clear();
                break;
            };
            let (reversed, next) = if ctx.equal(curve_start.as_str(), current, SCAN)? {
                (false, curve_end.as_str())
            } else if ctx.equal(curve_end.as_str(), current, SCAN)? {
                (true, curve_start.as_str())
            } else {
                profile.clear();
                break;
            };
            profile_storage.with_storage(|| {
                ctx.push_vec(
                    &mut profile,
                    SketchEntityUse {
                        entity: entities[curve]
                            .borrow()
                            .id()
                            .try_clone_for_decode(ctx, COPY)?,
                        reversed,
                    },
                    "collect SLDPRT closed curve profile",
                )
            })?;
            current = next;
            if ctx.equal(current, start, SCAN)? {
                break;
            }
            let candidates = incident(current)?;
            if candidates.is_empty() || (reject_branching_components && candidates.len() != 2) {
                profile.clear();
                break;
            }
            let Some(next_curve) =
                ctx.find_by(candidates.iter().copied(), |index| Ok(unused[*index]), SCAN)?
            else {
                profile.clear();
                break;
            };
            curve = next_curve;
        }
        if profile.len() >= 2 {
            ctx.push_vec(&mut profiles, profile, "collect SLDPRT closed profiles")?;
            profile_storage.commit()?;
        }
    }
    Ok(profiles)
}

/// The circle through every point, found from the first non-degenerate point triple that every
/// point lies on.
pub(super) fn fitted_marker_circle(
    ctx: &DecodeContext<'_>,
    points: &[Point2],
    tolerance: f64,
) -> Result<Option<(Point2, f64)>, CodecError> {
    const OPERATION: &str = "fit SLDPRT connected arc circle";
    let [first, rest @ ..] = points else {
        return Ok(None);
    };
    ctx.find_map(
        0..rest.len(),
        |second_index| {
            let second = rest[second_index];
            ctx.find_map(
                &rest[second_index + 1..],
                |third| {
                    let determinant = 2.0
                        * (first.u * (second.v - third.v)
                            + second.u * (third.v - first.v)
                            + third.u * (first.v - second.v));
                    if !determinant.is_finite() || determinant.abs() <= tolerance * tolerance {
                        return Ok(None);
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
                    Ok((radius.is_finite()
                        && radius > tolerance
                        && ctx.all_by(
                            points,
                            |point| {
                                Ok(same_dimension_length(
                                    (point.u - center.u).hypot(point.v - center.v),
                                    radius,
                                ))
                            },
                            OPERATION,
                        )?)
                    .then_some((center, radius)))
                },
                OPERATION,
            )
        },
        OPERATION,
    )
}

/// The plane frame a principal or explicit datum plane states.
fn stated_plane_frame(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
) -> Result<Option<SketchPlaneFrame>, CodecError> {
    Ok(match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
            Some(SketchPlaneFrame::native(principal_sketch_frame(*plane)))
        }
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
            Some(SketchPlaneFrame::from_frame(
                (
                    frame.origin().get(),
                    frame.normal().get(),
                    frame.u_axis().get(),
                ),
                if ctx
                    .get_btree_map(
                        &feature.source_properties,
                        REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY,
                        "lookup SLDPRT plane axis source",
                    )?
                    .map(String::as_str)
                    == Some(CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE)
                {
                    SketchPlaneUAxisSource::ConstructedMidPlane
                } else {
                    SketchPlaneUAxisSource::Native
                },
            ))
        }
        _ => None,
    })
}

/// The first model feature naming each native record.
fn first_model_feature_by_native<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [cadmpeg_ir::features::Feature],
    operation: &'static str,
) -> Result<HashMap<&'a str, &'a cadmpeg_ir::features::Feature>, CodecError> {
    let mut by_native = HashMap::new();
    for feature in ctx.admit_iter(features, operation)? {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !ctx.contains_key_hash_map(&by_native, native_ref, operation)? {
            ctx.insert_hash_map(&mut by_native, native_ref, feature, operation)?;
        }
    }
    Ok(by_native)
}

pub(super) fn sketch_plane_frames(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
    let (entries, _entry_storage) = ctx.with_scoped_storage("hold SLDPRT plane entries", || {
        sketch_plane_frame_entries(ctx, features, histories)
    })?;
    frames_from_entries(ctx, &entries)
}

/// A frame map holding `entries`, a later entry replacing an earlier one with the same source.
fn frames_from_entries(
    ctx: &DecodeContext<'_>,
    entries: &[(u32, SketchPlaneFrame)],
) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
    let mut frames = HashMap::new();
    for (source, frame) in ctx.admit_iter(entries, "index SLDPRT sketch plane source frames")? {
        ctx.insert_hash_map(
            &mut frames,
            *source,
            *frame,
            "index SLDPRT sketch plane source frames",
        )?;
    }
    Ok(frames)
}

/// The frame of each native source, ordered by the identity of the model feature it names.
fn sketch_plane_frame_entries(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
) -> Result<Vec<(u32, SketchPlaneFrame)>, CodecError> {
    const OPERATION: &str = "index SLDPRT sketch plane sources";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let by_native =
        storage.with_storage(|| first_model_feature_by_native(ctx, features, OPERATION))?;
    // Each model feature's native source identifier; a later record replaces an earlier one.
    let mut source_by_feature = BTreeMap::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            let Some(neutral) = ctx
                .get_hash_map(&by_native, feature.id.as_str(), OPERATION)?
                .copied()
            else {
                continue;
            };
            let Some(source) = feature.source_value() else {
                continue;
            };
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut source_by_feature,
                    neutral.id.as_str(),
                    source,
                    "index SLDPRT sketch plane sources",
                )
            })?;
        }
    }
    let mut frames_by_feature = HashMap::new();
    // Offset planes by identity, resolved from their reference chain.
    let mut offsets = HashMap::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        if let Some(frame) = stated_plane_frame(ctx, feature)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut frames_by_feature,
                    feature.id.as_str(),
                    frame,
                    "index SLDPRT resolved sketch planes",
                )
            })?;
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference:
                Some(cadmpeg_ir::features::DatumPlaneReference::Feature { feature: reference }),
            distance,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if !ctx.contains_key_hash_map(&offsets, feature.id.as_str(), OPERATION)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut offsets,
                    feature.id.as_str(),
                    (reference.as_str(), distance.get()),
                    "index SLDPRT derived sketch planes",
                )
            })?;
        }
    }
    // Resolve each offset plane once by walking its reference chain to a resolved frame; a chain
    // that ends unresolved or returns to itself leaves every plane on it unresolved.
    let mut settled = HashSet::new();
    let mut chain = Vec::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        let id = feature.id.as_str();
        if ctx.contains_key_hash_map(&frames_by_feature, id, OPERATION)?
            || ctx.contains_hash_set(&settled, id, OPERATION)?
            || !ctx.contains_key_hash_map(&offsets, id, OPERATION)?
        {
            continue;
        }
        ctx.clear_vec(&mut chain, "collect SLDPRT derived sketch planes")?;
        let mut current = id;
        let base = loop {
            if let Some(frame) = ctx.get_hash_map(&frames_by_feature, current, OPERATION)? {
                break Some(*frame);
            }
            let Some(&(reference, distance)) = ctx.get_hash_map(&offsets, current, OPERATION)?
            else {
                break None;
            };
            if !storage.with_storage(|| ctx.insert_hash_set(&mut settled, current, OPERATION))? {
                break None;
            }
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut chain,
                    (current, distance),
                    "collect SLDPRT derived sketch planes",
                )
            })?;
            current = reference;
        };
        let Some(mut frame) = base else {
            continue;
        };
        for &(id, distance) in ctx.admit_iter(&chain, OPERATION)?.rev() {
            frame = SketchPlaneFrame {
                origin: Point3::new(
                    frame.origin.x + frame.normal.x * distance,
                    frame.origin.y + frame.normal.y * distance,
                    frame.origin.z + frame.normal.z * distance,
                ),
                ..frame
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut frames_by_feature,
                    id,
                    frame,
                    "index SLDPRT derived sketch planes",
                )
            })?;
        }
    }
    let mut entries = Vec::new();
    for (feature, source) in ctx.admit_iter(&source_by_feature, OPERATION)? {
        let Some(frame) = ctx
            .get_hash_map(&frames_by_feature, *feature, OPERATION)?
            .copied()
        else {
            continue;
        };
        ctx.push_vec(
            &mut entries,
            (*source, frame),
            "index SLDPRT sketch plane source frames",
        )?;
    }
    Ok(entries)
}

/// The sketch plane frames every lane shares, computed once for many lanes.
pub(super) struct LaneFrameIndex<'h> {
    entries: Vec<(u32, SketchPlaneFrame)>,
    /// The frame each native record's plane feature states.
    stated_by_native: HashMap<&'h str, SketchPlaneFrame>,
}

impl<'h> LaneFrameIndex<'h> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        features: &[cadmpeg_ir::features::Feature],
        histories: &'h [crate::records::FeatureHistory],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT lane sketch plane candidates";
        let (by_native, _native_storage) = ctx.with_scoped_storage(OPERATION, || {
            first_model_feature_by_native(ctx, features, OPERATION)
        })?;
        let mut stated_by_native = HashMap::new();
        for history in ctx.admit_iter(histories, OPERATION)? {
            for native in ctx.admit_iter(&history.features, OPERATION)? {
                let Some(frame) = ctx
                    .get_hash_map(&by_native, native.id.as_str(), OPERATION)?
                    .map(|feature| stated_plane_frame(ctx, feature))
                    .transpose()?
                    .flatten()
                else {
                    continue;
                };
                ctx.insert_hash_map(&mut stated_by_native, native.id.as_str(), frame, OPERATION)?;
            }
        }
        Ok(Self {
            entries: sketch_plane_frame_entries(ctx, features, histories)?,
            stated_by_native,
        })
    }

    /// The shared frames, with the frames that `lane` alone names under unclaimed sources.
    pub(super) fn lane_frames(
        &self,
        ctx: &DecodeContext<'_>,
        histories: &[crate::records::FeatureHistory],
        names: &ObjectNames<'_, '_>,
    ) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
        const OPERATION: &str = "index SLDPRT lane sketch plane candidates";
        let mut frames = frames_from_entries(ctx, &self.entries)?;
        let mut lane_candidates = BTreeMap::<u32, Vec<SketchPlaneFrame>>::new();
        for history in ctx.admit_iter(histories, OPERATION)? {
            for native in ctx.admit_iter(&history.features, OPERATION)? {
                let Some(source) = names
                    .of(ctx, native)?
                    .and_then(|name| name.object_id.and_then(ObjectId::value))
                else {
                    continue;
                };
                let Some(frame) = ctx
                    .get_hash_map(&self.stated_by_native, native.id.as_str(), OPERATION)?
                    .copied()
                else {
                    continue;
                };
                ctx.push_btree_group(
                    &mut lane_candidates,
                    source,
                    frame,
                    OPERATION,
                    "collect SLDPRT lane sketch plane candidates",
                )?;
            }
        }
        for (source, mut candidates) in ctx.admit_iter(lane_candidates, OPERATION)? {
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
            ctx.dedup_by_key(
                &mut candidates,
                |frame| Ok(reference_plane_frame_key(&frame.as_tuple())),
                "deduplicate SLDPRT sketch plane frames",
            )?;
            if let [frame] = candidates.as_slice() {
                if !ctx.contains_key_hash_map(&frames, &source, OPERATION)? {
                    ctx.insert_hash_map(&mut frames, source, *frame, OPERATION)?;
                }
            }
        }
        Ok(frames)
    }
}

pub(super) fn lane_sketch_plane_frames(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<HashMap<u32, SketchPlaneFrame>, CodecError> {
    let (index, _index_storage) = ctx
        .with_scoped_storage("hold SLDPRT lane plane index", || {
            LaneFrameIndex::new(ctx, features, histories)
        })?;
    index.lane_frames(ctx, histories, &ObjectNames::new(ctx, lane)?)
}

pub(super) fn ordered_rectangle_corners(
    ctx: &DecodeContext<'_>,
    points: &[Point2],
) -> Result<Option<[Point2; 4]>, CodecError> {
    let [_, _, _, _] = points else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "hold SLDPRT rectangle coordinates")?;
    let mut u = storage.with_storage(|| {
        ctx.collect_vec(
            points.iter().map(|point| point.u),
            "collect SLDPRT rectangle u coordinates",
        )
    })?;
    ctx.stable_sort_by(
        &mut u,
        |value| value,
        f64::total_cmp,
        "sldprt rectangle u sort",
    )?;
    ctx.dedup_vec(&mut u, "deduplicate SLDPRT rectangle u coordinates")?;
    let mut v = storage.with_storage(|| {
        ctx.collect_vec(
            points.iter().map(|point| point.v),
            "collect SLDPRT rectangle v coordinates",
        )
    })?;
    ctx.stable_sort_by(
        &mut v,
        |value| value,
        f64::total_cmp,
        "sldprt rectangle v sort",
    )?;
    ctx.dedup_vec(&mut v, "deduplicate SLDPRT rectangle v coordinates")?;
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
    let mut storage = ctx.reserve_scoped(0, "hold SLDPRT tolerant rectangle coordinates")?;
    let mut u = storage.with_storage(|| {
        ctx.collect_vec(
            points.iter().map(|point| point.u),
            "collect SLDPRT tolerant rectangle u coordinates",
        )
    })?;
    ctx.stable_sort_by(
        &mut u,
        |value| value,
        f64::total_cmp,
        "sldprt tolerant rectangle u sort",
    )?;
    ctx.dedup_by(
        &mut u,
        |left, right| Ok(same_dimension_length(*left, *right)),
        "deduplicate SLDPRT tolerant rectangle u coordinates",
    )?;
    let mut v = storage.with_storage(|| {
        ctx.collect_vec(
            points.iter().map(|point| point.v),
            "collect SLDPRT tolerant rectangle v coordinates",
        )
    })?;
    ctx.stable_sort_by(
        &mut v,
        |value| value,
        f64::total_cmp,
        "sldprt tolerant rectangle v sort",
    )?;
    ctx.dedup_by(
        &mut v,
        |left, right| Ok(same_dimension_length(*left, *right)),
        "deduplicate SLDPRT tolerant rectangle v coordinates",
    )?;
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

    let (result, _storage) = ctx.with_scoped_storage("resolve SLDPRT rectangle scratch", || {
        let mut roster = Vec::new();
        ctx.extend_from_slice(
            &mut roster,
            markers,
            "collect SLDPRT rectangle marker roster",
        )?;
        ctx.sort_unstable_by_key(
            &mut roster,
            |value| value.offset(),
            Ord::cmp,
            "sldprt rectangle marker roster sort",
        )?;
        let mut records = Vec::new();
        let too_many = ctx.position_by(
            markers,
            |marker| {
                let record = (|| {
                    let offset = usize::try_from(marker.offset()).ok()?;
                    if let Some(endpoints) =
                        legacy_extended_rectangle_line_endpoints(payload, offset)
                    {
                        return (marker.kind() == SketchInputKind::LineOrCircle).then_some(
                            RectangleLineRecord::Indexed {
                                endpoints,
                                space: EndpointSpace::Roster,
                            },
                        );
                    }
                    if let Some(endpoints) =
                        current_compact_rectangle_line_endpoints(payload, offset)
                    {
                        return matches!(
                            marker.kind(),
                            SketchInputKind::LineOrCircle | SketchInputKind::Arc
                        )
                        .then_some(RectangleLineRecord::Indexed {
                            endpoints,
                            space: EndpointSpace::Object,
                        });
                    }
                    if let Some(endpoints) =
                        compact_legacy_rectangle_line_endpoints(payload, offset)
                    {
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
                    ctx.push_vec(
                        &mut records,
                        record,
                        "collect SLDPRT rectangle line records",
                    )?;
                }
                Ok(records.len() > 4)
            },
            "collect SLDPRT rectangle line records",
        )?;
        if too_many.is_some() {
            return Ok(None);
        }
        // The one located point marker with each object index, for object-space endpoints.
        let object_point = |vertex: u32| -> Result<Option<&SketchInputEntity>, CodecError> {
            let mut found = None;
            let mut ambiguous = false;
            ctx.position_by(
                markers,
                |marker| {
                    if marker.object_index() != Some(vertex)
                        || marker.coordinates_m.is_none()
                        || !matches!(
                            marker.kind(),
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                    {
                        return Ok(false);
                    }
                    ambiguous = found.is_some();
                    found = Some(*marker);
                    Ok(ambiguous)
                },
                "find SLDPRT rectangle object vertex",
            )?;
            Ok(found.filter(|_| !ambiguous))
        };
        let mut object_points = [None; 4];
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
            if endpoint_space == EndpointSpace::Object {
                for (slot, vertex) in object_points.iter_mut().zip(vertices) {
                    *slot = match object_point(*vertex) {
                        Ok(point) => point,
                        Err(error) => return Some(Err(error)),
                    };
                }
            }
            let mut known = [(0u32, [0.0f64; 2]); 4];
            let mut known_count = 0;
            for (vertex_index, vertex) in vertices.iter().enumerate() {
                let candidate = (|| {
                    let marker = match endpoint_space {
                        EndpointSpace::Roster => *roster.get(usize::try_from(*vertex).ok()?)?,
                        EndpointSpace::Object => object_points[vertex_index]?,
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
    })?;
    Ok(result)
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
    const OPERATION: &str = "scan SLDPRT rectangle corners";
    if dimensions_mm.len() < 2 {
        return Ok(None);
    }
    // Each located marker's grid cell; a cell two markers share holds `None`.
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut cells = HashMap::<(i64, i64), Option<&'a SketchInputEntity>>::new();
    let mut points = Vec::new();
    for marker in ctx.admit_iter(markers, OPERATION)? {
        let Some([u, v]) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let Some(cell) = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM).cells()
        else {
            continue;
        };
        if let Some(existing) = ctx.get_mut_hash_map(&mut cells, &cell, OPERATION)? {
            *existing = None;
            continue;
        }
        storage.with_storage(|| ctx.insert_hash_map(&mut cells, cell, Some(*marker), OPERATION))?;
        storage
            .with_storage(|| ctx.push_vec(&mut points, cell, "collect SLDPRT rectangle points"))?;
    }
    let unique = |cell: (i64, i64)| -> Result<Option<&'a SketchInputEntity>, CodecError> {
        Ok(ctx
            .get_hash_map(&cells, &cell, OPERATION)?
            .copied()
            .flatten())
    };
    let dimensions_match = |u_span: f64, v_span: f64| {
        ctx.any_by(
            dimensions_mm.iter().enumerate(),
            |(first_index, first)| {
                ctx.any_by(
                    dimensions_mm.iter().enumerate(),
                    |(second_index, second)| {
                        Ok(first_index != second_index
                            && ((same_dimension_length(*first, u_span)
                                && same_dimension_length(*second, v_span))
                                || (same_dimension_length(*first, v_span)
                                    && same_dimension_length(*second, u_span))))
                    },
                    "scan SLDPRT rectangle dimensions",
                )
            },
            "scan SLDPRT rectangle dimensions",
        )
    };
    // A rectangle is found once, from its lower-left and upper-right corner cells.
    let mut selected = None;
    let mut visited = points.iter();
    while let Some(&(u0, v0)) = ctx.next_charged(&mut visited, OPERATION)? {
        let Some(lower) = unique((u0, v0))? else {
            continue;
        };
        let mut visited = points.iter();
        while let Some(&(u1, v1)) = ctx.next_charged(&mut visited, OPERATION)? {
            if u1 <= u0 || v1 <= v0 {
                continue;
            }
            let (Some(u_cells), Some(v_cells)) = (
                u1.checked_sub(u0)
                    .and_then(cadmpeg_core::convert::f64_from_i64),
                v1.checked_sub(v0)
                    .and_then(cadmpeg_core::convert::f64_from_i64),
            ) else {
                continue;
            };
            if !dimensions_match(u_cells * QUANTUM, v_cells * QUANTUM)? {
                continue;
            }
            let (Some(second), Some(third), Some(fourth)) =
                (unique((u1, v0))?, unique((u1, v1))?, unique((u0, v1))?)
            else {
                continue;
            };
            if selected.replace([lower, second, third, fourth]).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(selected)
}

/// A line's endpoint position as an exact key; both zeros map to one key.
fn line_point_key(point: Point2) -> (u64, u64) {
    ((point.u + 0.0).to_bits(), (point.v + 0.0).to_bits())
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
    let (result, profile_storage) = ctx.with_scoped_storage(
        "SLDPRT compact line profile",
        || -> Result<_, CodecError> {
            const OPERATION: &str = "scan SLDPRT compact line profile adjacency";
            if lines.len() < 3 {
                return Ok(None);
            }
            // The lines that start or end at each exact position, in line order.
            let mut storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut by_point = HashMap::<(u64, u64), Vec<(usize, bool)>>::new();
            for (index, line) in ctx.admit_iter(lines, OPERATION)?.enumerate() {
                if line.3.u.is_nan() || line.3.v.is_nan() || line.4.u.is_nan() || line.4.v.is_nan()
                {
                    continue;
                }
                storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut by_point,
                        line_point_key(line.3),
                        (index, false),
                        OPERATION,
                        OPERATION,
                    )
                })?;
                if line_point_key(line.4) != line_point_key(line.3) {
                    storage.with_storage(|| {
                        ctx.push_hash_group(
                            &mut by_point,
                            line_point_key(line.4),
                            (index, true),
                            OPERATION,
                            OPERATION,
                        )
                    })?;
                }
            }
            let mut used = storage.with_storage(|| {
                ctx.alloc_filled(lines.len(), false, "SLDPRT compact line profile usage")
            })?;
            let mut profile = ctx.vector_storage(lines.len(), "SLDPRT compact line profile")?;
            let Some(first) = lines.first() else {
                return Ok(None);
            };
            used[0] = true;
            ctx.push_vec(
                &mut profile,
                SketchEntityUse {
                    entity: first
                        .0
                        .try_clone_for_decode(ctx, "SLDPRT compact line profile")?,
                    reversed: false,
                },
                "SLDPRT compact line profile",
            )?;
            let origin = first.3;
            let mut current = first.4;
            while profile.len() < lines.len() {
                // The one unused line touching the current position.
                let mut candidate = None;
                let mut ambiguous = false;
                let touching = if current.u.is_nan() || current.v.is_nan() {
                    &[][..]
                } else {
                    ctx.get_hash_map(&by_point, &line_point_key(current), OPERATION)?
                        .map_or(&[][..], Vec::as_slice)
                };
                ctx.position_by(
                    touching,
                    |&(index, at_end)| {
                        if used[index] {
                            return Ok(false);
                        }
                        ambiguous = candidate.is_some();
                        let next = if at_end {
                            lines[index].3
                        } else {
                            lines[index].4
                        };
                        candidate = Some((index, at_end, next));
                        Ok(ambiguous)
                    },
                    OPERATION,
                )?;
                let (Some(candidate), false) = (candidate, ambiguous) else {
                    return Ok(None);
                };
                used[candidate.0] = true;
                ctx.push_vec(
                    &mut profile,
                    SketchEntityUse {
                        entity: lines[candidate.0]
                            .0
                            .try_clone_for_decode(ctx, "SLDPRT compact line profile")?,
                        reversed: candidate.1,
                    },
                    "SLDPRT compact line profile",
                )?;
                current = candidate.2;
            }
            Ok((current == origin).then_some(profile))
        },
    )?;
    if result.is_some() {
        profile_storage.commit()?;
    }
    Ok(result)
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
    const COLLECT: &str = "collect SLDPRT compact region addresses";
    const VALIDATE: &str = "validate SLDPRT compact region addresses";
    let Some(offset) = ctx.find_bytes(payload, NAME, "scan SLDPRT compact region")? else {
        return Ok(None);
    };
    if ctx
        .find_bytes_from(payload, NAME, offset + 1, "scan SLDPRT compact region")?
        .is_some()
    {
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
    let Some(entries_start) = header.checked_add(4) else {
        return Ok(None);
    };
    let Some(remaining) = payload.len().checked_sub(entries_start) else {
        return Ok(None);
    };
    if bounded_len(cadmpeg_core::decode::u64_from_index(count), 12, remaining).is_none() {
        return Ok(None);
    }
    let (mut addresses, addresses_storage) =
        ctx.with_scoped_storage(COLLECT, || ctx.alloc_filled(count, 0u16, COLLECT))?;
    let mut entry_token = None;
    if !ctx.all_by(
        addresses.iter_mut().enumerate(),
        |(index, slot)| {
            let Some(entry) = header.checked_add(4 + index * 12) else {
                return Ok(false);
            };
            let Some(token) = View::u16_le_at(payload, entry) else {
                return Ok(false);
            };
            if !matches!(token, 0x80e1 | 0x8386 | 0xbc87)
                || entry_token.is_some_and(|existing| existing != token)
                || payload.get(entry + 4..entry + 8) != Some(&[0xff; 4])
                || payload.get(entry + 8..entry + 12) != Some(&[0; 4])
            {
                return Ok(false);
            }
            entry_token = Some(token);
            let Some(address) = View::u16_le_at(payload, entry + 2) else {
                return Ok(false);
            };
            *slot = address;
            Ok(true)
        },
        COLLECT,
    )? {
        return Ok(None);
    }
    let (mut seen, _seen_storage) =
        ctx.with_scoped_storage(VALIDATE, || ctx.alloc_filled(count, false, VALIDATE))?;
    if !ctx.all_by(
        &addresses,
        |address| {
            let Some(index) = usize::from(*address)
                .checked_sub(1)
                .filter(|index| *index < count)
            else {
                return Ok(false);
            };
            if seen[index] {
                return Ok(false);
            }
            seen[index] = true;
            Ok(true)
        },
        VALIDATE,
    )? {
        return Ok(None);
    }
    addresses_storage.commit()?;
    Ok(Some(addresses))
}

pub(super) fn compact_line_chain_addresses(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<u16>>, CodecError> {
    const SCAN: &str = "scan SLDPRT compact line chain";
    const COLLECT: &str = "collect SLDPRT compact chain addresses";
    const VALIDATE: &str = "validate SLDPRT compact line chain";
    let mut selected = None;
    let ambiguous = ctx.any_by(
        0..payload.len(),
        |offset| {
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
                return Ok(false);
            };
            let (mut addresses, storage) =
                ctx.with_scoped_storage(COLLECT, || ctx.alloc_filled(count, 0u16, COLLECT))?;
            let mut seen = [false; 64];
            let valid = ctx.all_by(
                addresses.iter_mut().enumerate(),
                |(index, address)| {
                    let Some(parsed) = View::u32_le_at(bytes, 2 + index * 4)
                        .and_then(|value| u16::try_from(value).ok())
                    else {
                        return Ok(false);
                    };
                    let Some(position) = usize::from(parsed)
                        .checked_sub(1)
                        .filter(|value| *value < count)
                    else {
                        return Ok(false);
                    };
                    if seen[position] {
                        return Ok(false);
                    }
                    seen[position] = true;
                    *address = parsed;
                    Ok(true)
                },
                VALIDATE,
            )?;
            if !valid {
                return Ok(false);
            }
            match selected.as_ref() {
                Some((previous, _)) => Ok(!ctx.equal(previous, &addresses, VALIDATE)?),
                None => {
                    selected = Some((addresses, storage));
                    Ok(false)
                }
            }
        },
        SCAN,
    )?;
    if ambiguous {
        return Ok(None);
    }
    match selected {
        Some((addresses, storage)) => {
            storage.commit()?;
            Ok(Some(addresses))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod curves_tests;
