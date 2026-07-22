// SPDX-License-Identifier: Apache-2.0
//! Geometry record patchers and the `patch_*_definition` byte-patcher family.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::geometry::{NurbsCurve, PcurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Color, Sense};
use cadmpeg_ir::transform::Transform;

use super::edits::{
    NurbsCurveEdit, NurbsPcurveEdit, NurbsSurfaceEdit, ProceduralCurveEdit, ProceduralSurfaceEdit,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{finite_vector, native_bool, unique_knot_count};
use crate::{asm_header, sab};

pub(crate) fn valid_edited_curve_structure(before: &NurbsCurve, after: &NurbsCurve) -> bool {
    valid_edited_nurbs_direction(
        &before.knots,
        after.degree,
        &after.knots,
        after.control_points.len(),
    )
}

pub(crate) fn valid_edited_nurbs_direction(
    before_knots: &[f64],
    after_degree: u32,
    after_knots: &[f64],
    control_count: usize,
) -> bool {
    let Ok(degree) = usize::try_from(after_degree) else {
        return false;
    };
    (1..=20).contains(&after_degree)
        && after_knots.len() == control_count + degree + 1
        && unique_knot_count(after_knots) == unique_knot_count(before_knots)
        && after_knots.iter().all(|value| value.is_finite())
        && after_knots.windows(2).all(|pair| pair[0] <= pair[1])
}

pub(crate) fn orthonormal_pair(first: Vector3, second: Vector3) -> bool {
    finite_vector(first)
        && finite_vector(second)
        && (first.norm() - 1.0).abs() <= 1e-9
        && (second.norm() - 1.0).abs() <= 1e-9
        && (first.x * second.x + first.y * second.y + first.z * second.z).abs() <= 1e-9
}

/// The per-entity BREP edit maps that the geometry patchers apply as a unit.
///
/// Every field is keyed by BREP entity id (or record index) and carries the
/// edited value for one geometry aspect. The maps travel together from
/// validation through `patch_geometry` into `patch_framed_geometry`, so they
/// are bundled rather than threaded positionally.
#[derive(Clone, Copy)]
pub(crate) struct GeometryEdits<'a> {
    pub(crate) positions: &'a BTreeMap<String, Point3>,
    pub(crate) lines: &'a BTreeMap<String, (Point3, Vector3)>,
    pub(crate) conics: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    pub(crate) degenerate_curves: &'a BTreeMap<String, Point3>,
    pub(crate) planes: &'a BTreeMap<String, (Point3, Vector3, Vector3)>,
    pub(crate) spheres: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    pub(crate) tori: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    pub(crate) cones: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64, f64)>,
    pub(crate) body_transforms: &'a BTreeMap<String, Transform>,
    pub(crate) entity_colors: &'a BTreeMap<String, Color>,
    pub(crate) edge_ranges: &'a BTreeMap<String, [f64; 2]>,
    pub(crate) face_senses: &'a BTreeMap<String, Sense>,
    pub(crate) coedge_senses: &'a BTreeMap<String, Sense>,
    pub(crate) procedural_surface_edits: &'a BTreeMap<String, ProceduralSurfaceEdit>,
    pub(crate) nurbs_surfaces: &'a BTreeMap<String, NurbsSurfaceEdit>,
    pub(crate) nurbs_curves: &'a BTreeMap<String, NurbsCurveEdit>,
    pub(crate) pcurves: &'a BTreeMap<String, NurbsPcurveEdit>,
    pub(crate) procedural_curve_edits: &'a BTreeMap<String, ProceduralCurveEdit>,
    pub(crate) procedural_surface_fits: &'a BTreeMap<String, f64>,
    pub(crate) creation_timestamps: &'a BTreeMap<usize, f64>,
    pub(crate) edge_continuities: &'a BTreeMap<usize, (Sense, String)>,
    pub(crate) vertex_ownerships: &'a BTreeMap<usize, (i64, u8)>,
    pub(crate) face_sidedness: &'a BTreeMap<usize, crate::records::FaceContainment>,
    pub(crate) tolerant_edges: &'a BTreeMap<usize, f64>,
    pub(crate) tolerant_vertices: &'a BTreeMap<usize, (f64, [f64; 2])>,
}

pub(crate) fn patch_geometry(bytes: &mut [u8], edits: &GeometryEdits) -> Result<(), CodecError> {
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    let header_scale = asm_header::parse(bytes)
        .and_then(|header| header.scale)
        .unwrap_or(1.0);
    patch_framed_geometry(bytes, &records, edits, header_scale)
}

pub(crate) fn patch_framed_geometry(
    bytes: &mut [u8],
    records: &[sab::Record],
    edits: &GeometryEdits,
    header_scale: f64,
) -> Result<(), CodecError> {
    let GeometryEdits {
        positions,
        lines,
        conics,
        degenerate_curves,
        planes,
        spheres,
        tori,
        cones,
        body_transforms,
        entity_colors,
        edge_ranges,
        face_senses,
        coedge_senses,
        procedural_surface_edits,
        nurbs_surfaces,
        nurbs_curves,
        pcurves,
        procedural_curve_edits,
        procedural_surface_fits,
        creation_timestamps,
        edge_continuities,
        vertex_ownerships,
        face_sidedness,
        tolerant_edges,
        tolerant_vertices,
    } = *edits;
    let records_by_index = records
        .iter()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    let transform_records = records
        .iter()
        .filter(|record| record.head == "body")
        .filter_map(|body| {
            body_transforms
                .get(&crate::ids::brep_entity_id(body.index))
                .and_then(|transform| {
                    body.ref_at(5)
                        .map(|reference| (reference as usize, *transform))
                })
        })
        .collect::<BTreeMap<_, _>>();
    let ref_pcurve_geometry = records
        .iter()
        .filter(|record| record.head == "pcurve")
        .filter_map(|record| {
            let edit = pcurves.get(&crate::ids::brep_entity_id(record.index))?;
            let target = usize::try_from(record.ref_at(4)?).ok()?;
            let mut geometry = edit.clone();
            geometry.wrapper_reversed = None;
            geometry.native_tail_flags = None;
            geometry.parameter_range = None;
            geometry.fit_tolerance = None;
            Some((target, geometry))
        })
        .collect::<BTreeMap<_, _>>();
    let mut color_records = BTreeMap::new();
    for entity in records
        .iter()
        .filter(|record| record.head == "body" || record.head == "face")
    {
        let id = crate::ids::brep_entity_id(entity.index);
        let Some(color) = entity_colors.get(&id) else {
            continue;
        };
        let mut next = entity.ref_at(0);
        let mut found = false;
        while let Some(index) = next.and_then(|index| usize::try_from(index).ok()) {
            let Some(attribute) = records_by_index.get(&index) else {
                break;
            };
            if attribute.head.contains("rgb_color") {
                color_records.insert(index, *color);
                found = true;
                break;
            }
            next = attribute.ref_at(0);
        }
        if !found {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity color {id} has no writable rgb_color attribute"
            )));
        }
    }
    for record in records {
        if let Some(timestamp) = creation_timestamps.get(&record.index) {
            if !record.head.contains("ATTRIB_CUSTOM")
                || !record.tokens.iter().any(
                    |token| matches!(token, sab::Token::Str(value) if value == "Timestamp_attrib_def"),
                )
            {
                return Err(CodecError::Malformed(format!(
                    "F3D timestamp record {} has the wrong attribute family",
                    record.index
                )));
            }
            let family = record
                .tokens
                .iter()
                .position(
                    |token| matches!(token, sab::Token::Str(value) if value == "Timestamp_attrib_def"),
                )
                .expect("timestamp family was checked");
            if !matches!(record.chunk(family + 1), Some(sab::Token::Long(1))) {
                return Err(CodecError::Malformed(format!(
                    "F3D timestamp record {} lacks marker 1 after its family",
                    record.index
                )));
            }
            let offset =
                required_payload_field(bytes, record, active_ref_width(bytes), family + 2, 0x06)?;
            bytes[offset + 1..offset + 9].copy_from_slice(&timestamp.to_le_bytes());
            continue;
        }
        if let Some((sense, continuity)) = edge_continuities.get(&record.index) {
            if !matches!(record.head.as_str(), "edge" | "tedge") {
                return Err(CodecError::Malformed(format!(
                    "F3D edge-continuity record {} is not an edge",
                    record.index
                )));
            }
            let ref_width = active_ref_width(bytes);
            patch_sense_field(bytes, record, ref_width, 9, *sense)?;
            patch_ascii_field(bytes, record, ref_width, 10, continuity)?;
        }
        if let Some((owning_edge, endpoint_index)) = vertex_ownerships.get(&record.index) {
            if !matches!(record.head.as_str(), "vertex" | "tvertex") {
                return Err(CodecError::Malformed(format!(
                    "F3D vertex-ownership record {} is not a vertex",
                    record.index
                )));
            }
            let ref_width = active_ref_width(bytes);
            for (index, tag, value) in [
                (3usize, 0x0c, *owning_edge),
                (4, 0x04, i64::from(*endpoint_index)),
            ] {
                patch_integer_field(bytes, record, ref_width, index, tag, value)?;
            }
        }
        if let Some(containment) = face_sidedness.get(&record.index) {
            if record.head != "face" || !matches!(record.chunk(9), Some(sab::Token::True)) {
                return Err(CodecError::Malformed(format!(
                    "F3D face-sidedness record {} is not double-sided",
                    record.index
                )));
            }
            let sense = match containment {
                crate::records::FaceContainment::In => Sense::Reversed,
                crate::records::FaceContainment::Out => Sense::Forward,
            };
            patch_sense_field(bytes, record, active_ref_width(bytes), 10, sense)?;
        }
        if let Some((tolerance, leading)) = tolerant_vertices.get(&record.index) {
            if record.head != "tvertex" {
                return Err(CodecError::Malformed(format!(
                    "F3D tolerant-vertex record {} is not a tvertex",
                    record.index
                )));
            }
            // The record's three f64 tolerance slots: the two leading slots
            // verbatim and the evaluated tolerance last.
            let ref_width = active_ref_width(bytes);
            for (index, value) in [(6usize, leading[0]), (7, leading[1]), (8, *tolerance)] {
                let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
                bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
            }
        }
        if let Some(tolerance) = tolerant_edges.get(&record.index) {
            if record.head != "tedge"
                || !matches!(record.chunk(12), Some(sab::Token::Long(_)))
                || !matches!(record.chunk(13), Some(sab::Token::Long(0)))
            {
                return Err(CodecError::Malformed(format!(
                    "F3D tolerant-edge record {} has the wrong layout",
                    record.index
                )));
            }
            let offset = required_payload_field(bytes, record, active_ref_width(bytes), 11, 0x06)?;
            bytes[offset + 1..offset + 9].copy_from_slice(&tolerance.to_le_bytes());
        }
        if let Some(color) = color_records.get(&record.index) {
            let ref_width = active_ref_width(bytes);
            for (index, value) in [
                (1usize, f64::from(color.r)),
                (2, f64::from(color.g)),
                (3, f64::from(color.b)),
            ] {
                let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
                bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
            }
            continue;
        }
        if let Some(transform) = transform_records.get(&record.index) {
            patch_transform_record(bytes, record, *transform, header_scale)?;
            continue;
        }
        let id = crate::ids::brep_entity_id(record.index);
        if let Some(edit) = ref_pcurve_geometry.get(&record.index) {
            patch_nurbs_pcurve_record(bytes, record, edit)?;
        }
        if let Some(edit) = pcurves.get(&id) {
            if record.ref_at(4).is_some() {
                patch_ref_pcurve_contract(bytes, record, edit)?;
            } else {
                patch_nurbs_pcurve_record(bytes, record, edit)?;
            }
        }
        if let Some(edit) = nurbs_curves.get(&id) {
            patch_nurbs_curve_record(bytes, record, edit, false)?;
        }
        let tolerant_curve_id = format!("f3d:brep:tolerant-coedge-curve#{}", record.index);
        if let Some(edit) = nurbs_curves.get(&tolerant_curve_id) {
            if record.head != "tcoedge" {
                return Err(CodecError::Malformed(format!(
                    "F3D tolerant use-curve carrier {tolerant_curve_id} is not a tcoedge record"
                )));
            }
            if matches!(record.chunk(15), Some(sab::Token::True)) {
                let mut native_curve = edit.curve.clone();
                crate::brep::geometry::reverse_nurbs_curve(&mut native_curve);
                patch_nurbs_curve_record(
                    bytes,
                    record,
                    &NurbsCurveEdit {
                        curve: native_curve,
                        periodic: edit.periodic,
                    },
                    false,
                )?;
            } else {
                patch_nurbs_curve_record(bytes, record, edit, false)?;
            }
        }
        let procedural_curve_id = format!("f3d:brep:procedural_curve#{}", record.index);
        if let Some(edit) = procedural_curve_edits.get(&procedural_curve_id) {
            if let Some(tolerance) = edit.fit_tolerance {
                patch_procedural_curve_fit(bytes, record, tolerance)?;
            }
            if let Some(definition) = &edit.definition {
                match definition {
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. } => {
                        patch_helix_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset { .. } => {
                        patch_vector_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { .. } => {
                        patch_subset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound { .. } => {
                        patch_compound_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset { .. } => {
                        patch_two_sided_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset { .. } => {
                        patch_surface_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring { .. } => {
                        patch_spring_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection { .. } => {
                        patch_projection_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. } => {
                        patch_intersection_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                        ..
                    } => patch_three_surface_intersection_definition(bytes, record, definition)?,
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { .. } => {
                        patch_surface_curve_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette { .. } => {
                        patch_silhouette_definition(bytes, record, definition)?;
                    }
                    _ => unreachable!("procedural edit validation limits writable definitions"),
                }
            }
        }
        let directrix_id = format!("f3d:brep:procedural_surface#{}:directrix", record.index);
        if let Some(edit) = nurbs_curves.get(&directrix_id) {
            patch_nurbs_curve_record(bytes, record, edit, false)?;
        }
        let spine_id = format!("f3d:brep:procedural_surface#{}:spine", record.index);
        if let Some(edit) = nurbs_curves.get(&spine_id) {
            patch_nurbs_curve_record(bytes, record, edit, true)?;
        }
        if let Some(edit) = nurbs_surfaces.get(&id) {
            patch_nurbs_surface_record(bytes, record, edit, None)?;
        }
        for side in 0..2 {
            let support_id = format!("f3d:brep:procedural_surface#{}:support{side}", record.index);
            if let Some(edit) = nurbs_surfaces.get(&support_id) {
                patch_nurbs_surface_record(bytes, record, edit, Some(side))?;
            }
        }
        let procedural_id = format!("f3d:brep:procedural_surface#{}", record.index);
        if let Some(tolerance) = procedural_surface_fits.get(&procedural_id) {
            patch_procedural_surface_fit(bytes, record, *tolerance)?;
        }
        if let Some(edit) = procedural_surface_edits.get(&procedural_id) {
            if record.head != "spline" {
                return Err(CodecError::Malformed(format!(
                    "F3D extrusion carrier {procedural_id} is not a spline record"
                )));
            }
            match edit {
                ProceduralSurfaceEdit::Extrusion {
                    parameter_interval,
                    direction,
                    native_position,
                } => {
                    patch_extrusion_definition(
                        bytes,
                        record,
                        *parameter_interval,
                        *direction,
                        *native_position,
                    )?;
                }
                ProceduralSurfaceEdit::BlendRadii(radii) => {
                    patch_blend_radius_tokens(bytes, record, *radii)?;
                }
            }
        }
        if record.head == "face" {
            if let Some(sense) = face_senses.get(&id) {
                patch_sense_field(bytes, record, active_ref_width(bytes), 8, *sense)?;
            }
        } else if matches!(record.head.as_str(), "coedge" | "tcoedge") {
            if let Some(sense) = coedge_senses.get(&id) {
                patch_sense_field(bytes, record, active_ref_width(bytes), 7, *sense)?;
            }
        } else if matches!(record.head.as_str(), "edge" | "tedge") {
            if let Some(range) = edge_ranges.get(&id) {
                let ref_width = active_ref_width(bytes);
                for (index, value) in [(4usize, range[0]), (6, range[1])] {
                    let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "point" {
            if let Some(position) = positions.get(&id) {
                let offset =
                    required_payload_field(bytes, record, active_ref_width(bytes), 3, 0x13)?;
                for (component, value) in [
                    position.x / LEN_TO_MM,
                    position.y / LEN_TO_MM,
                    position.z / LEN_TO_MM,
                ]
                .into_iter()
                .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "straight" {
            if let Some((origin, direction)) = lines.get(&id) {
                let field_indices = match record.name.as_str() {
                    "straight" => [0, 1],
                    "straight-curve" => [3, 4],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "straight record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                ];
                for (offset, values) in fields.into_iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [direction.x, direction.y, direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        } else if record.head == "degenerate_curve" {
            if let Some(point) = degenerate_curves.get(&id) {
                let field_index = match record.name.as_str() {
                    "degenerate_curve" => 0,
                    "degenerate_curve-curve" => 3,
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "degenerate-curve record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let offset = required_payload_field(
                    bytes,
                    record,
                    active_ref_width(bytes),
                    field_index,
                    0x13,
                )?;
                for (component, value) in [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ]
                .into_iter()
                .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "ellipse" {
            if let Some((center, axis, direction, major_radius, minor_radius)) = conics.get(&id) {
                let field_indices = match record.name.as_str() {
                    "ellipse" => [0, 1, 2, 3],
                    "ellipse-curve" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "ellipse record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                ];
                let major = major_radius / LEN_TO_MM;
                for (offset, values) in fields[..3].iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [axis.x, axis.y, axis.z],
                    [
                        direction.x * major,
                        direction.y * major,
                        direction.z * major,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                let ratio = minor_radius / major_radius;
                let old_ratio = f64::from_le_bytes(
                    bytes[fields[3] + 1..fields[3] + 9]
                        .try_into()
                        .expect("framed ellipse ratio has eight payload bytes"),
                );
                let signed_ratio = if old_ratio.is_sign_negative() {
                    -ratio
                } else {
                    ratio
                };
                bytes[fields[3] + 1..fields[3] + 9].copy_from_slice(&signed_ratio.to_le_bytes());
            }
        } else if record.head == "plane" {
            if let Some((origin, normal, u_axis)) = planes.get(&id) {
                let field_indices = match record.name.as_str() {
                    "plane" => [0, 1, 2],
                    "plane-surface" => [3, 4, 5],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "plane record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                ];
                for (offset, values) in fields.into_iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [normal.x, normal.y, normal.z],
                    [u_axis.x, u_axis.y, u_axis.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        } else if record.head == "sphere" {
            if let Some((center, axis, ref_direction, radius)) = spheres.get(&id) {
                let field_indices = match record.name.as_str() {
                    "sphere" => [0, 1, 2, 3],
                    "sphere-surface" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "sphere record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[2], fields[3]].into_iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                    [axis.x, axis.y, axis.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                bytes[fields[1] + 1..fields[1] + 9]
                    .copy_from_slice(&(radius / LEN_TO_MM).to_le_bytes());
            }
        } else if record.head == "torus" {
            if let Some((center, axis, ref_direction, major_radius, minor_radius)) = tori.get(&id) {
                let field_indices = match record.name.as_str() {
                    "torus" => [0, 1, 2, 3, 4],
                    "torus-surface" => [3, 4, 5, 6, 7],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "torus record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[4], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[1], fields[4]].into_iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                for (offset, value) in [fields[2], fields[3]]
                    .into_iter()
                    .zip([major_radius / LEN_TO_MM, minor_radius / LEN_TO_MM])
                {
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "cone" {
            if let Some((origin, axis, ref_direction, radius, ratio, half_angle)) = cones.get(&id) {
                let ref_width = active_ref_width(bytes);
                let field_indices = match record.name.as_str() {
                    "cone" => [0, 1, 2, 3, 4, 5, 6],
                    "cone-surface" => [3, 4, 5, 6, 9, 10, 11],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "cone record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[4], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[5], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[6], 0x06)?,
                ];
                let old_sine = f64::from_le_bytes(
                    bytes[fields[4] + 1..fields[4] + 9]
                        .try_into()
                        .expect("framed cone sine has eight payload bytes"),
                );
                let old_cosine = f64::from_le_bytes(
                    bytes[fields[5] + 1..fields[5] + 9]
                        .try_into()
                        .expect("framed cone cosine has eight payload bytes"),
                );
                let sine_sign = if old_sine < 0.0 { -1.0 } else { 1.0 };
                let cosine_sign = if old_cosine < 0.0 { -1.0 } else { 1.0 };
                let native_axis = if *half_angle > 0.0 && sine_sign * cosine_sign < 0.0 {
                    Vector3::new(-axis.x, -axis.y, -axis.z)
                } else {
                    *axis
                };
                let scaled_radius = radius / LEN_TO_MM;
                for (offset, values) in fields[..3].iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [native_axis.x, native_axis.y, native_axis.z],
                    [
                        ref_direction.x * scaled_radius,
                        ref_direction.y * scaled_radius,
                        ref_direction.z * scaled_radius,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                for (offset, value) in fields[3..].iter().zip([
                    *ratio,
                    sine_sign * half_angle.sin(),
                    cosine_sign * half_angle.cos(),
                    scaled_radius,
                ]) {
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn required_payload_field(
    bytes: &[u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    tag: u8,
) -> Result<usize, CodecError> {
    let offset = sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} record {} lacks payload field {index}",
            record.head, record.index
        ))
    })?;
    if bytes.get(offset) != Some(&tag) {
        return Err(CodecError::Malformed(format!(
            "{} record {} payload field {index} is not tag {tag:#04x}",
            record.head, record.index
        )));
    }
    Ok(offset)
}

/// Borrow a framed record's byte span, rejecting extent overflow and truncation
/// with `{label}`-tagged diagnostics. Shared by the geometry-cache patchers.
fn record_slice<'a>(
    bytes: &'a [u8],
    record: &sab::Record,
    label: &str,
) -> Result<&'a [u8], CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed(format!("{label} record extent overflows address space"))
    })?;
    bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed(format!("{label} record is truncated")))
}

/// Write each patch's little-endian `f64` payload at `record_offset + offset`.
fn apply_f64_patches(
    bytes: &mut [u8],
    record_offset: usize,
    patches: impl IntoIterator<Item = (usize, f64)>,
) {
    for (offset, value) in patches {
        let at = record_offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
}

/// Write a vector's three little-endian `f64` payloads at consecutive 8-byte
/// slots starting from `base_at`.
fn apply_vector_payload(bytes: &mut [u8], base_at: usize, components: [f64; 3]) {
    apply_f64_patches(
        bytes,
        base_at,
        components
            .into_iter()
            .enumerate()
            .map(|(component, value)| (component * 8, value)),
    );
}

fn patch_extrusion_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    parameter_interval: [f64; 2],
    direction: Vector3,
    native_position: cadmpeg_ir::math::Point3,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "extrusion")?;
    let layout =
        crate::nurbs::proc_curve::extrusion_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "spline record {} lacks writable extrusion fields",
                    record.index
                ))
            })?;
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_interval
            .into_iter()
            .zip(parameter_interval),
    );
    for (base, values) in [
        (
            layout.direction,
            [
                direction.x / LEN_TO_MM,
                direction.y / LEN_TO_MM,
                direction.z / LEN_TO_MM,
            ],
        ),
        (
            layout.native_position,
            [
                native_position.x / LEN_TO_MM,
                native_position.y / LEN_TO_MM,
                native_position.z / LEN_TO_MM,
            ],
        ),
    ] {
        apply_vector_payload(bytes, record.offset + base, values);
    }
    Ok(())
}

fn patch_ascii_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    value: &str,
) -> Result<(), CodecError> {
    let offset = required_payload_field(bytes, record, ref_width, index, 0x07)?;
    let encoded_length = bytes.get(offset + 1).copied().ok_or_else(|| {
        CodecError::Malformed(format!("{} record string is truncated", record.head))
    })? as usize;
    if value.len() != encoded_length || !value.is_ascii() {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} string edit must retain its encoded ASCII length",
            record.head, record.index
        )));
    }
    bytes[offset + 2..offset + 2 + encoded_length].copy_from_slice(value.as_bytes());
    Ok(())
}

pub(crate) fn patch_integer_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    tag: u8,
    value: i64,
) -> Result<(), CodecError> {
    let offset = required_payload_field(bytes, record, ref_width, index, tag)?;
    if ref_width == 4 && i64::from(value as i32) != value {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} integer edit exceeds BinaryFile4 range",
            record.head, record.index
        )));
    }
    bytes[offset + 1..offset + 1 + ref_width].copy_from_slice(&value.to_le_bytes()[..ref_width]);
    Ok(())
}

fn patch_transform_record(
    bytes: &mut [u8],
    record: &sab::Record,
    transform: Transform,
    header_scale: f64,
) -> Result<(), CodecError> {
    if header_scale == 0.0 {
        return Err(CodecError::Malformed(format!(
            "transform record {} has zero header scale",
            record.index
        )));
    }
    let ref_width = active_ref_width(bytes);
    let vectors = [
        [
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ],
        [
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ],
        [
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ],
        [
            transform.rows[0][3] / (header_scale * LEN_TO_MM),
            transform.rows[1][3] / (header_scale * LEN_TO_MM),
            transform.rows[2][3] / (header_scale * LEN_TO_MM),
        ],
    ];
    for (index, vector) in vectors.into_iter().enumerate() {
        let offset = required_payload_field(bytes, record, ref_width, index, 0x14)?;
        for (component, value) in vector.into_iter().enumerate() {
            let at = offset + 1 + component * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let scale = required_payload_field(bytes, record, ref_width, 4, 0x06)?;
    bytes[scale + 1..scale + 9].copy_from_slice(&transform.rows[3][3].to_le_bytes());
    Ok(())
}

fn patch_sense_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    sense: Sense,
) -> Result<(), CodecError> {
    let offset = sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} record {} lacks payload field {index}",
            record.head, record.index
        ))
    })?;
    if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
        return Err(CodecError::Malformed(format!(
            "{} record {} payload field {index} is not a sense token",
            record.head, record.index
        )));
    }
    bytes[offset] = match sense {
        Sense::Forward => 0x0b,
        Sense::Reversed => 0x0a,
    };
    Ok(())
}

pub(crate) fn active_ref_width(bytes: &[u8]) -> usize {
    asm_header::parse(bytes).map_or(8, |header| usize::from(header.width))
}

fn patch_blend_radius_tokens(
    bytes: &mut [u8],
    record: &sab::Record,
    radii: [f64; 2],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "rolling-ball")?;
    let layout =
        crate::nurbs::proc_curve::rolling_ball_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "spline record {} lacks a writable rolling-ball radius pair",
                    record.index
                ))
            })?;
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .radii
            .into_iter()
            .zip(radii)
            .map(|(offset, radius)| (offset, radius / LEN_TO_MM)),
    );
    Ok(())
}

fn patch_nurbs_surface_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsSurfaceEdit,
    surface_ordinal: Option<usize>,
) -> Result<(), CodecError> {
    let surface = &edit.surface;
    let record_bytes = record_slice(bytes, record, "NURBS surface")?;
    let layout = surface_ordinal
        .map_or_else(
            || crate::nurbs::core::final_surface_patch_layout(record_bytes),
            |ordinal| crate::nurbs::core::surface_patch_layout_at(record_bytes, ordinal),
        )
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "spline record {} has no writable surface cache",
                record.index
            ))
        })?;
    let u_count = usize::try_from(surface.u_count)
        .map_err(|_| CodecError::Malformed("NURBS u pole count exceeds address space".into()))?;
    let v_count = usize::try_from(surface.v_count)
        .map_err(|_| CodecError::Malformed("NURBS v pole count exceeds address space".into()))?;
    if layout.u_count != u_count
        || layout.v_count != v_count
        || layout.rational != surface.weights.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS cache structure",
            record.index
        )));
    }
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.u_knots,
        &surface.u_knots,
        layout.int_width,
    )?;
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.v_knots,
        &surface.v_knots,
        layout.int_width,
    )?;
    for (offset, degree) in layout
        .degree_value_offsets
        .into_iter()
        .zip([surface.u_degree, surface.v_degree])
    {
        let at = record.offset + offset;
        patch_layout_integer(bytes, at, layout.int_width, i64::from(degree))?;
    }
    if let Some(periodic) = edit.periodic {
        for (offset, periodic) in layout.periodic_value_offsets.into_iter().zip(periodic) {
            let at = record.offset + offset;
            let value = if periodic { 2i64 } else { 0i64 };
            patch_layout_integer(bytes, at, layout.int_width, value)?;
        }
    }
    let components = if layout.rational { 4 } else { 3 };
    if layout.control_value_offsets.len() != u_count * v_count * components {
        return Err(CodecError::Malformed(format!(
            "spline record {} has an inconsistent NURBS control layout",
            record.index
        )));
    }
    let weights = surface.weights.as_deref();
    let mut ordinal = 0usize;
    for v in 0..v_count {
        for u in 0..u_count {
            let ir_index = u * v_count + v;
            let point = surface.control_points[ir_index];
            let values = [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
                weights.map_or(0.0, |weights| weights[ir_index]),
            ];
            for value in values.into_iter().take(components) {
                let at = record.offset + layout.control_value_offsets[ordinal];
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                ordinal += 1;
            }
        }
    }
    Ok(())
}

fn patch_procedural_surface_fit(
    bytes: &mut [u8],
    record: &sab::Record,
    tolerance: f64,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "procedural-surface")?;
    let layout = crate::nurbs::core::final_surface_patch_layout(record_bytes).ok_or_else(|| {
        CodecError::Malformed(format!(
            "spline record {} has no solved surface cache",
            record.index
        ))
    })?;
    if record_bytes.get(layout.end) != Some(&0x06) {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} has no writable fit-tolerance carrier",
            record.index
        )));
    }
    let at = record.offset + layout.end + 1;
    bytes[at..at + 8].copy_from_slice(&(tolerance / LEN_TO_MM).to_le_bytes());
    Ok(())
}

fn patch_nurbs_curve_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsCurveEdit,
    final_cache: bool,
) -> Result<(), CodecError> {
    let curve = &edit.curve;
    let record_bytes = record_slice(bytes, record, "NURBS curve")?;
    let layout = if final_cache {
        crate::nurbs::core::final_curve_patch_layout(record_bytes)
    } else {
        crate::nurbs::core::first_curve_patch_layout(record_bytes)
    }
    .ok_or_else(|| {
        CodecError::Malformed(format!(
            "spline record {} has no writable curve cache",
            record.index
        ))
    })?;
    if layout.control_count != curve.control_points.len()
        || layout.rational != curve.weights.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS curve structure",
            record.index
        )));
    }
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.knots,
        &curve.knots,
        layout.int_width,
    )?;
    let degree_at = record.offset + layout.degree_value_offset;
    patch_layout_integer(bytes, degree_at, layout.int_width, i64::from(curve.degree))?;
    if let Some(periodic) = edit.periodic {
        let periodic = if periodic { 2i64 } else { 0i64 };
        let periodic_at = record.offset + layout.periodic_value_offset;
        patch_layout_integer(bytes, periodic_at, layout.int_width, periodic)?;
    }
    let components = if layout.rational { 4 } else { 3 };
    if layout.control_value_offsets.len() != curve.control_points.len() * components {
        return Err(CodecError::Malformed(format!(
            "spline record {} has an inconsistent NURBS curve layout",
            record.index
        )));
    }
    let weights = curve.weights.as_deref();
    let mut ordinal = 0usize;
    for (index, point) in curve.control_points.iter().enumerate() {
        let values = [
            point.x / LEN_TO_MM,
            point.y / LEN_TO_MM,
            point.z / LEN_TO_MM,
            weights.map_or(0.0, |weights| weights[index]),
        ];
        for value in values.into_iter().take(components) {
            let at = record.offset + layout.control_value_offsets[ordinal];
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            ordinal += 1;
        }
    }
    Ok(())
}

fn patch_procedural_curve_fit(
    bytes: &mut [u8],
    record: &sab::Record,
    tolerance: f64,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "procedural-curve")?;
    let layout = crate::nurbs::core::final_curve_patch_layout(record_bytes).ok_or_else(|| {
        CodecError::Malformed(format!(
            "intcurve record {} has no solved curve cache",
            record.index
        ))
    })?;
    if record_bytes.get(layout.end) != Some(&0x06) {
        return Err(CodecError::NotImplemented(format!(
            "intcurve record {} has no writable fit-tolerance carrier",
            record.index
        )));
    }
    let at = record.offset + layout.end + 1;
    bytes[at..at + 8].copy_from_slice(&(tolerance / LEN_TO_MM).to_le_bytes());
    Ok(())
}

fn patch_helix_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "helix patch received a non-helix definition".into(),
        ));
    };
    let record_bytes = record_slice(bytes, record, "helix")?;
    let layout =
        crate::nurbs::proc_curve::helix_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "procedural curve record {} lacks writable helix fields",
                    record.index
                ))
            })?;
    apply_f64_patches(
        bytes,
        record.offset,
        layout.angle_range.into_iter().zip(*angle_range),
    );
    for (offset, value) in layout.frame_vectors.into_iter().zip([
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
        [
            major.x / LEN_TO_MM,
            major.y / LEN_TO_MM,
            major.z / LEN_TO_MM,
        ],
        [
            minor.x / LEN_TO_MM,
            minor.y / LEN_TO_MM,
            minor.z / LEN_TO_MM,
        ],
        [
            pitch.x / LEN_TO_MM,
            pitch.y / LEN_TO_MM,
            pitch.z / LEN_TO_MM,
        ],
    ]) {
        apply_vector_payload(bytes, record.offset + offset, value);
    }
    let apex_at = record.offset + layout.apex_factor;
    bytes[apex_at..apex_at + 8].copy_from_slice(&apex_factor.to_le_bytes());
    apply_vector_payload(bytes, record.offset + layout.axis, [axis.x, axis.y, axis.z]);
    Ok(())
}

fn patch_vector_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
        parameter_range,
        offset,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "vector-offset patch received another definition".into(),
        ));
    };
    let record_bytes = record_slice(bytes, record, "vector-offset")?;
    let layout =
        crate::nurbs::proc_curve::vector_offset_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "vector-offset record {} lacks writable construction fields",
                    record.index
                ))
            })?;
    apply_f64_patches(
        bytes,
        record.offset,
        layout.parameter_range.into_iter().zip(*parameter_range),
    );
    apply_vector_payload(
        bytes,
        record.offset + layout.offset,
        [
            offset.x / LEN_TO_MM,
            offset.y / LEN_TO_MM,
            offset.z / LEN_TO_MM,
        ],
    );
    Ok(())
}

fn patch_subset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
        parameter_range, ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "subset patch received another definition".into(),
        ));
    };
    let record_bytes = record_slice(bytes, record, "subset")?;
    let layout =
        crate::nurbs::proc_curve::subset_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "subset record {} lacks writable construction fields",
                    record.index
                ))
            })?;
    apply_f64_patches(
        bytes,
        record.offset,
        layout.parameter_range.into_iter().zip(*parameter_range),
    );
    Ok(())
}

fn patch_compound_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "compound patch received another definition".into(),
        ));
    };
    let record_bytes = record_slice(bytes, record, "compound")?;
    let layout =
        crate::nurbs::proc_curve::compound_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "compound record {} lacks writable parameter arrays",
                    record.index
                ))
            })?;
    if layout.parameters.len() != parameters.len()
        || layout.component_parameters.len() != component_parameters.len()
    {
        return Err(CodecError::NotImplemented(
            "compound edit changes native parameter cardinality".into(),
        ));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameters
            .into_iter()
            .chain(layout.component_parameters)
            .zip(parameters.iter().chain(component_parameters).copied()),
    );
    Ok(())
}

fn patch_two_sided_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "two-sided offset patch received another definition".into(),
        ));
    };
    let record_bytes = record_slice(bytes, record, "two-sided offset")?;
    let layout = [8usize, 4]
        .into_iter()
        .filter_map(|width| {
            crate::nurbs::proc_curve::two_sided_offset_patch_layout(record_bytes, width)
        })
        .find(|layout| {
            layout
                .discontinuities
                .iter()
                .map(Vec::len)
                .eq(context.discontinuities.iter().map(Vec::len))
        })
        .ok_or_else(|| CodecError::Malformed("two-sided offset layout is malformed".into()))?;
    for (at, value) in layout
        .parameter_range
        .into_iter()
        .zip(context.parameter_range)
    {
        patch_f64_payload(bytes, record.offset + at, value)?;
    }
    for (locations, values) in layout.discontinuities.iter().zip(&context.discontinuities) {
        for (at, value) in locations.iter().zip(values) {
            patch_f64_payload(bytes, record.offset + *at, *value)?;
        }
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    for (at, value) in layout.offsets.into_iter().zip(offsets) {
        patch_f64_payload(bytes, record.offset + at, *value / LEN_TO_MM)?;
    }
    Ok(())
}

fn patch_f64_payload(bytes: &mut [u8], at: usize, value: f64) -> Result<(), CodecError> {
    let payload = bytes
        .get_mut(at..at + 8)
        .ok_or_else(|| CodecError::Malformed("native double payload is truncated".into()))?;
    payload.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn patch_surface_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "surface-offset patch received another definition".into(),
        ));
    };
    if !distance.is_finite() || !shift.is_finite() || !scale.is_finite() {
        return Err(CodecError::Malformed(
            "surface-offset scalars must be finite".into(),
        ));
    }
    if [base_u_range, base_v_range, base_range]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-offset ranges must be finite".into(),
        ));
    }
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-offset context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "surface-offset")?;
    let layout = crate::nurbs::proc_curve::surface_offset_patch_layout(
        record_bytes,
        active_ref_width(bytes),
    )
    .ok_or_else(|| CodecError::Malformed("surface-offset construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "surface-offset context is incomplete".into(),
        ));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .chain(layout.base_u_range)
            .chain(layout.base_v_range)
            .chain(layout.base_range)
            .chain([layout.distance, layout.shift, layout.scale])
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied())
                    .chain(base_u_range.iter().copied())
                    .chain(base_v_range.iter().copied())
                    .chain(base_range.iter().copied().chain([
                        distance / LEN_TO_MM,
                        *shift,
                        *scale,
                    ])),
            ),
    );
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_spring_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
        context,
        discontinuity_flag,
        direction,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "spring patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "spring context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "spring")?;
    let int_width = active_ref_width(bytes);
    let layout = crate::nurbs::proc_curve::spring_patch_layout(record_bytes, int_width)
        .ok_or_else(|| CodecError::Malformed("spring construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed("spring context is incomplete".into()));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied()),
            ),
    );
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    patch_tagged_integer_at(
        bytes,
        record.offset + layout.direction,
        int_width,
        *direction,
    )?;
    Ok(())
}

fn patch_projection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        tail,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "projection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "projection context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "projection")?;
    let layout =
        crate::nurbs::proc_curve::projection_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| CodecError::Malformed("projection construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "projection context is incomplete".into(),
        ));
    }
    match (&layout.tail, tail) {
        (
            crate::nurbs::proc_curve::ProjectionTailPatchLayout::EarlyClose { flag: offset },
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag },
        ) => bytes[record.offset + offset] = native_bool(*flag),
        (
            crate::nurbs::proc_curve::ProjectionTailPatchLayout::Ranged {
                flag: flag_offset,
                parameter_range: range_offsets,
                role: role_range,
            },
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            },
        ) => {
            if !parameter_range.iter().copied().all(f64::is_finite) {
                return Err(CodecError::Malformed(
                    "projection tail range must be finite".into(),
                ));
            }
            if !role.is_ascii() || role.len() != role_range.len() {
                return Err(CodecError::NotImplemented(
                    "projection role edit must retain its encoded ASCII length".into(),
                ));
            }
            bytes[record.offset + flag_offset] = native_bool(*flag);
            apply_f64_patches(
                bytes,
                record.offset,
                range_offsets
                    .iter()
                    .zip(parameter_range)
                    .map(|(offset, value)| (*offset, *value)),
            );
            let role_target = record.offset + role_range.start..record.offset + role_range.end;
            bytes[role_target].copy_from_slice(role.as_bytes());
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "projection edit cannot change native tail form".into(),
            ))
        }
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied()),
            ),
    );
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_intersection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "intersection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "intersection context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "intersection")?;
    let layout =
        crate::nurbs::proc_curve::intersection_patch_layout(record_bytes, active_ref_width(bytes))
            .ok_or_else(|| {
                CodecError::Malformed("intersection construction is malformed".into())
            })?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "intersection context is incomplete".into(),
        ));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied()),
            ),
    );
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_three_surface_intersection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "three-surface intersection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "three-surface intersection context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "three-surface intersection")?;
    let int_width = active_ref_width(bytes);
    let layout = crate::nurbs::proc_curve::three_surface_patch_layout(record_bytes, int_width)
        .ok_or_else(|| CodecError::Malformed("three-surface construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "three-surface intersection context is incomplete".into(),
        ));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied()),
            ),
    );
    patch_tagged_integer_at(bytes, record.offset + layout.selector, int_width, *selector)?;
    Ok(())
}

fn patch_surface_curve_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
        family, context, ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "surface-curve patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-curve context values must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "surface-curve")?;
    let layout = crate::nurbs::proc_curve::surface_curve_patch_layout(
        record_bytes,
        active_ref_width(bytes),
        family,
    )
    .ok_or_else(|| CodecError::Malformed("surface-curve construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "surface-curve context is incomplete".into(),
        ));
    }
    apply_f64_patches(
        bytes,
        record.offset,
        layout
            .parameter_range
            .into_iter()
            .chain(layout.discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range
                    .into_iter()
                    .chain(context.discontinuities.iter().flatten().copied()),
            ),
    );
    Ok(())
}

fn patch_silhouette_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
        silhouette,
        light_direction,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "silhouette patch received another definition".into(),
        ));
    };
    if !finite_vector(*light_direction) {
        return Err(CodecError::Malformed(
            "silhouette light direction must be finite".into(),
        ));
    }
    let draft_factor = match silhouette {
        cadmpeg_ir::geometry::SilhouetteKind::Standard
        | cadmpeg_ir::geometry::SilhouetteKind::Parametric => None,
        cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
            if !draft_factor.is_finite() {
                return Err(CodecError::Malformed(
                    "silhouette draft factor must be finite".into(),
                ));
            }
            Some(*draft_factor)
        }
    };
    let record_bytes = record_slice(bytes, record, "silhouette")?;
    let layout = crate::nurbs::proc_curve::silhouette_patch_layout(
        record_bytes,
        active_ref_width(bytes),
        silhouette,
    )
    .ok_or_else(|| CodecError::Malformed("silhouette construction is malformed".into()))?;
    apply_vector_payload(
        bytes,
        record.offset + layout.light_direction,
        [light_direction.x, light_direction.y, light_direction.z],
    );
    if let Some(draft_factor) = draft_factor {
        let draft_offset = layout
            .draft_factor
            .ok_or_else(|| CodecError::Malformed("silhouette draft factor is missing".into()))?;
        let draft_offset = record.offset + draft_offset;
        bytes[draft_offset..draft_offset + 8].copy_from_slice(&draft_factor.to_le_bytes());
    }
    Ok(())
}

fn patch_nurbs_pcurve_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsPcurveEdit,
) -> Result<(), CodecError> {
    let geometry = &edit.geometry;
    let PcurveGeometry::Nurbs {
        degree,
        control_points,
        weights,
        ..
    } = geometry
    else {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} is not a writable NURBS cache",
            record.index
        )));
    };
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let scope = if record.head == "pcurve" {
        sab::payload_subtype_range(bytes, record, 5, ref_width, "exp_par_cur").ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} has no exp_par_cur payload",
                record.index
            ))
        })?
    } else if record.head == "intcurve" {
        record.offset..record.offset.checked_add(record.len).ok_or_else(|| {
            CodecError::Malformed("NURBS pcurve record extent overflows address space".into())
        })?
    } else {
        return Err(CodecError::Malformed(format!(
            "record {} is not a pcurve carrier",
            record.index
        )));
    };
    let layout =
        crate::nurbs::pcurve::final_pcurve_patch_layout(bytes.get(scope.clone()).ok_or_else(
            || CodecError::Malformed("NURBS pcurve subtype extent is truncated".into()),
        )?)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} has no writable UV cache",
                record.index
            ))
        })?;
    if layout.control_count != control_points.len()
        || layout.control_value_offsets.len() != control_points.len() * 2
        || layout.weight_value_offsets.len() != weights.as_ref().map_or(0, Vec::len)
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} changed UV cache structure",
            record.index
        )));
    }
    let PcurveGeometry::Nurbs { knots, .. } = geometry else {
        unreachable!()
    };
    patch_knot_structure(bytes, scope.start, &layout.knots, knots, layout.int_width)?;
    let at = scope.start + layout.degree_value_offset;
    patch_layout_integer(bytes, at, layout.int_width, i64::from(*degree))?;
    if let Some(periodic) = edit.periodic {
        let value = if periodic { 2i64 } else { 0i64 };
        let at = scope.start + layout.periodic_value_offset;
        patch_layout_integer(bytes, at, layout.int_width, value)?;
    }
    if record.head == "pcurve" {
        if let Some(reversed) = edit.wrapper_reversed {
            let offset =
                sab::payload_token_offset(bytes, record, ref_width, 4).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve record {} lacks wrapper-reversal carrier",
                        record.index
                    ))
                })?;
            if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
                return Err(CodecError::Malformed(format!(
                    "pcurve record {} has a non-boolean wrapper-reversal carrier",
                    record.index
                )));
            }
            bytes[offset] = if reversed { 0x0a } else { 0x0b };
        }
        if bytes.get(scope.end) != Some(&0x10) {
            return Err(CodecError::Malformed(format!(
                "pcurve record {} lacks the exp_par_cur close",
                record.index
            )));
        }
        let suffix_start = record.tokens.len().checked_sub(6).ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} lacks its native metadata suffix",
                record.index
            ))
        })?;
        let suffix_offsets = (suffix_start..record.tokens.len())
            .map(|index| {
                sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native metadata suffix",
                        record.index
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(flags) = edit.native_tail_flags {
            for (offset, flag) in suffix_offsets[..4].iter().zip(flags) {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
                bytes[*offset] = native_bool(flag);
            }
        } else {
            for offset in &suffix_offsets[..4] {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
            }
        }
        if let Some(range) = edit.parameter_range {
            for (offset, value) in suffix_offsets[4..].iter().zip(range) {
                if bytes.get(*offset) != Some(&0x06) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete parameter range",
                        record.index
                    )));
                }
                bytes[*offset + 1..*offset + 9].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    if let Some(tolerance) = edit.fit_tolerance {
        if bytes.get(scope.start + layout.control_end) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "pcurve record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        let at = scope.start + layout.control_end + 1;
        bytes[at..at + 8].copy_from_slice(&tolerance.to_le_bytes());
    }
    for (point, offsets) in control_points
        .iter()
        .zip(layout.control_value_offsets.chunks_exact(2))
    {
        for (value, offset) in [point.u, point.v].into_iter().zip(offsets) {
            let at = scope.start + offset;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    if let Some(weights) = weights {
        for (weight, offset) in weights.iter().zip(&layout.weight_value_offsets) {
            let at = scope.start + offset;
            bytes[at..at + 8].copy_from_slice(&weight.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_ref_pcurve_contract(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsPcurveEdit,
) -> Result<(), CodecError> {
    if edit.wrapper_reversed.is_some()
        || edit.native_tail_flags.is_some()
        || edit.fit_tolerance.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "ref-form pcurve record {} cannot carry wrapper or inline fit edits",
            record.index
        )));
    }
    let Some(range) = edit.parameter_range else {
        return Ok(());
    };
    let ref_width = active_ref_width(bytes);
    for (index, value) in [5usize, 6].into_iter().zip(range) {
        let offset =
            sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "ref-form pcurve record {} lacks parameter-range field {index}",
                    record.index
                ))
            })?;
        if bytes.get(offset) != Some(&0x06) {
            return Err(CodecError::Malformed(format!(
                "ref-form pcurve record {} parameter-range field {index} is not a double",
                record.index
            )));
        }
        bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_knot_structure(
    bytes: &mut [u8],
    record_offset: usize,
    layout: &crate::nurbs::core::KnotPatchLayout,
    knots: &[f64],
    int_width: usize,
) -> Result<(), CodecError> {
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for knot in knots {
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
        }
        runs.push((*knot, 1));
    }
    if runs.len() != layout.value_offsets.len() || runs.len() != layout.multiplicity_offsets.len() {
        return Err(CodecError::NotImplemented(
            "F3D NURBS curve edit changes the unique-knot count".into(),
        ));
    }
    for (ordinal, ((value, expanded_count), (value_offset, multiplicity_offset))) in runs
        .into_iter()
        .zip(
            layout
                .value_offsets
                .iter()
                .zip(&layout.multiplicity_offsets),
        )
        .enumerate()
    {
        let endpoint_extra = usize::from(ordinal == 0 || ordinal + 1 == layout.value_offsets.len());
        let stored = expanded_count
            .checked_sub(endpoint_extra)
            .filter(|count| *count > 0)
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "F3D NURBS curve knot multiplicity is not writable".into(),
                )
            })?;
        let stored = i64::try_from(stored).map_err(|_| {
            CodecError::Malformed("F3D NURBS curve knot multiplicity exceeds i64".into())
        })?;
        let value_at = record_offset + *value_offset;
        bytes[value_at..value_at + 8].copy_from_slice(&value.to_le_bytes());
        let multiplicity_at = record_offset + *multiplicity_offset;
        patch_layout_integer(bytes, multiplicity_at, int_width, stored)?;
    }
    Ok(())
}

fn patch_layout_integer(
    bytes: &mut [u8],
    offset: usize,
    width: usize,
    value: i64,
) -> Result<(), CodecError> {
    if width == 4 && i64::from(value as i32) != value {
        return Err(CodecError::NotImplemented(
            "F3D NURBS integer edit exceeds BinaryFile4 range".into(),
        ));
    }
    let target = bytes
        .get_mut(offset..offset + width)
        .ok_or_else(|| CodecError::Malformed("F3D NURBS integer payload is truncated".into()))?;
    target.copy_from_slice(&value.to_le_bytes()[..width]);
    Ok(())
}

pub(crate) fn patch_tagged_integer_at(
    bytes: &mut [u8],
    tag_offset: usize,
    width: usize,
    value: i64,
) -> Result<(), CodecError> {
    if !matches!(bytes.get(tag_offset), Some(0x04 | 0x0c | 0x15)) {
        return Err(CodecError::Malformed(
            "F3D tagged integer carrier is missing".into(),
        ));
    }
    patch_layout_integer(bytes, tag_offset + 1, width, value)
}
