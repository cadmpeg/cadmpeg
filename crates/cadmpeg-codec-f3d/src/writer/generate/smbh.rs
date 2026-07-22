// SPDX-License-Identifier: Apache-2.0
//! SMBH body encoders and edge/vertex/point normalization for source-less
//! generation.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::ids::{ShellId, VertexId};
use cadmpeg_ir::topology::Sense;

use super::attributes::{
    edge_persistent_attribute_ref, encode_source_less_attributes, owner_color_or_body_tag_ref,
    owner_color_or_face_tag_ref, sketch_link_attribute_ref, source_less_body_key,
    timestamp_attribute_ref,
};
use super::native_bytes::{
    native_curve_base, native_f64, native_history_tail, native_i64, native_ident, native_point,
    native_record_index, native_ref, native_string, native_surface_base, native_transform,
    native_vector,
};
use super::native_geometry::{
    native_cacheless_procedural_curve, native_cacheless_procedural_surface, native_nurbs_curve,
    native_nurbs_surface, native_pcurve, native_procedural_curve, native_procedural_surface,
    native_ref_pcurve_companion, native_smbh_header, pcurve_uses_ref_form,
};
use super::preconditions::{
    validate_source_less_body_kinds, validate_source_less_wire_vertices, WireVerticesValidated,
};
use super::records::{native_tolerant_coedge_extension, tolerant_coedge_range};
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{f3d_native, native_bool, normalized_face_sense_to_native};

pub(crate) fn encode_planar_triangle_smbh(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let model = &target.model;
    if model.faces.is_empty()
        && model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty())
    {
        return encode_wire_body_smbh(target);
    }
    validate_source_less_body_kinds(model)?;
    let wire_vertices = validate_source_less_wire_vertices(target)?;
    if model.faces.len() > 1
        || model.loops.len() > 1
        || model.surfaces.len() > 1
        || model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty())
        || model
            .bodies
            .iter()
            .any(|body| body.color.is_some() || body.transform.is_some())
        || model.faces.iter().any(|face| face.color.is_some())
    {
        return encode_multi_face_shell_smbh(target, wire_vertices);
    }
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || model.faces.len() != 1
        || model.loops.len() != 1
        || model.coedges.len() < 3
        || model.edges.len() != model.coedges.len()
        || model.vertices.len() != model.coedges.len()
        || model.points.len() != model.coedges.len()
        || model.surfaces.len() != 1
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation currently requires one polygonal planar face".into(),
        ));
    }
    let body = &model.bodies[0];
    let region = &model.regions[0];
    let shell = &model.shells[0];
    let face = &model.faces[0];
    let loop_ = &model.loops[0];
    let surface_geometry = &model.surfaces[0].geometry;
    if body.regions.as_slice() != [region.id.clone()]
        || region.body != body.id
        || region.shells.as_slice() != [shell.id.clone()]
        || shell.region != region.id
        || shell.faces.as_slice() != [face.id.clone()]
        || !shell.wire_edges.is_empty()
        || !shell.free_vertices.is_empty()
        || face.shell != shell.id
        || face.surface != model.surfaces[0].id
        || face.loops.as_slice() != [loop_.id.clone()]
        || loop_.face != face.id
        || loop_.coedges.len() != model.coedges.len()
        || body.transform.is_some()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation requires one directly owned polygonal face".into(),
        ));
    }

    let coedges = loop_
        .coedges
        .iter()
        .map(|id| {
            model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("loop references missing coedge {id}"))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, coedge) in coedges.iter().enumerate() {
        if coedge.pcurves.len() > 1 {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} has an ordered pcurve collection",
                coedge.id
            )));
        }
        let next = coedges[(index + 1) % coedges.len()];
        let previous = coedges[(index + coedges.len() - 1) % coedges.len()];
        if coedge.owner_loop != loop_.id
            || coedge.next != next.id
            || coedge.previous != previous.id
            || coedge.radial_next != coedge.id
        {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation requires a laminar polygon coedge ring".into(),
            ));
        }
    }

    let curve_start = 7i64;
    let pcurve_start = native_record_index(curve_start, model.curves.len())?;
    let ref_pcurve_count = model
        .pcurves
        .iter()
        .map(pcurve_uses_ref_form)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|uses_ref_form| *uses_ref_form)
        .count();
    let pcurve_record_count = model
        .pcurves
        .len()
        .checked_add(ref_pcurve_count)
        .ok_or_else(|| CodecError::Malformed("pcurve record count overflows usize".into()))?;
    let coedge_start = native_record_index(pcurve_start, pcurve_record_count)?;
    let edge_start = native_record_index(coedge_start, coedges.len())?;
    let vertex_start = native_record_index(edge_start, model.edges.len())?;
    let point_start = native_record_index(vertex_start, model.vertices.len())?;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);

    native_ident(&mut records, "body")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, source_less_body_key(target, body, 0)?);
    native_ref(&mut records, -1);
    native_ref(&mut records, 2);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    records.push(0x11);

    native_ident(&mut records, "region")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 3);
    native_ref(&mut records, 1);
    records.push(0x11);

    native_ident(&mut records, "shell")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 4);
    native_ref(&mut records, -1);
    native_ref(&mut records, 2);
    records.push(0x11);

    native_ident(&mut records, "face")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 5);
    native_ref(&mut records, 3);
    native_ref(&mut records, -1);
    native_ref(&mut records, 6);
    records.push(native_bool(
        native_face_sense(target, face)? == Sense::Reversed,
    ));
    native_face_sidedness(&mut records, target, face)?;
    records.push(0x11);

    native_ident(&mut records, "loop")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, coedge_start);
    native_ref(&mut records, 4);
    records.push(0x11);

    match *surface_geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            native_surface_base(&mut records, "plane")?;
            native_point(
                &mut records,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            native_vector(&mut records, [normal.x, normal.y, normal.z]);
            native_vector(&mut records, [u_axis.x, u_axis.y, u_axis.z]);
            records.push(0x0b);
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            native_surface_base(&mut records, "cone")?;
            native_point(
                &mut records,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_vector(
                &mut records,
                [
                    ref_direction.x * radius / LEN_TO_MM,
                    ref_direction.y * radius / LEN_TO_MM,
                    ref_direction.z * radius / LEN_TO_MM,
                ],
            );
            native_f64(&mut records, 1.0);
            records.extend_from_slice(&[0x0b, 0x0b]);
            native_f64(&mut records, 0.0);
            native_f64(&mut records, 1.0);
            native_f64(&mut records, radius / LEN_TO_MM);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            native_surface_base(&mut records, "cone")?;
            native_point(
                &mut records,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_vector(
                &mut records,
                [
                    ref_direction.x * radius / LEN_TO_MM,
                    ref_direction.y * radius / LEN_TO_MM,
                    ref_direction.z * radius / LEN_TO_MM,
                ],
            );
            native_f64(&mut records, ratio);
            records.extend_from_slice(&[0x0b, 0x0b]);
            native_f64(&mut records, half_angle.sin());
            native_f64(&mut records, half_angle.cos());
            native_f64(&mut records, radius / LEN_TO_MM);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            native_surface_base(&mut records, "sphere")?;
            native_point(
                &mut records,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_f64(&mut records, radius / LEN_TO_MM);
            native_vector(
                &mut records,
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            native_surface_base(&mut records, "torus")?;
            native_point(
                &mut records,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_f64(&mut records, major_radius / LEN_TO_MM);
            native_f64(&mut records, minor_radius / LEN_TO_MM);
            native_vector(
                &mut records,
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Nurbs(ref surface) => {
            if !native_procedural_surface(&mut records, target, &model.surfaces[0], surface)? {
                native_surface_base(&mut records, "spline")?;
                native_nurbs_surface(&mut records, surface)?;
            }
        }
        SurfaceGeometry::Polygonal { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation does not support polygonal surface carriers".into(),
            ));
        }
        SurfaceGeometry::Transformed { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation does not support transformed surface carriers".into(),
            ));
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
            if !native_cacheless_procedural_surface(&mut records, target, &model.surfaces[0])? {
                return Err(CodecError::NotImplemented(
                    "source-less F3D generation does not support this surface carrier".into(),
                ));
            }
        }
    }
    records.push(0x11);

    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(&mut records, "straight")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if major_radius == 0.0 {
                    return Err(CodecError::Malformed(
                        "source-less F3D ellipse has zero major radius".into(),
                    ));
                }
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        major_direction.x * major_radius / LEN_TO_MM,
                        major_direction.y * major_radius / LEN_TO_MM,
                        major_direction.z * major_radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, minor_radius / major_radius);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(&mut records, target, &carrier.id, curve)? {
                    native_curve_base(&mut records, "intcurve")?;
                    native_nurbs_curve(&mut records, curve)?;
                }
            }
            CurveGeometry::Procedural { .. } => {
                if !native_cacheless_procedural_curve(&mut records, target, &carrier.id)? {
                    return Err(CodecError::Malformed(format!(
                        "procedural curve carrier {} has no construction",
                        carrier.id
                    )));
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(&mut records, "degenerate_curve")?;
                native_point(
                    &mut records,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less F3D generation does not support this curve carrier".into(),
                ));
            }
        }
        records.push(0x11);
    }

    let ref_pcurve_start = native_record_index(pcurve_start, model.pcurves.len())?;
    let mut ref_pcurve_ordinal = 0usize;
    for pcurve in &model.pcurves {
        let companion_ref = pcurve_uses_ref_form(pcurve)?
            .then(|| native_record_index(ref_pcurve_start, ref_pcurve_ordinal))
            .transpose()?;
        native_pcurve(&mut records, pcurve, companion_ref)?;
        ref_pcurve_ordinal += usize::from(companion_ref.is_some());
        records.push(0x11);
    }
    for pcurve in model
        .pcurves
        .iter()
        .filter(|pcurve| pcurve_uses_ref_form(pcurve).is_ok_and(|value| value))
    {
        native_ref_pcurve_companion(&mut records, pcurve)?;
        records.push(0x11);
    }

    for (index, coedge) in coedges.iter().enumerate() {
        let edge_index = model
            .edges
            .iter()
            .position(|edge| edge.id == coedge.edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!("coedge references missing edge {}", coedge.edge))
            })?;
        let tolerant_range = tolerant_coedge_range(target, &coedge.id)?;
        native_ident(
            &mut records,
            if tolerant_range.is_some() {
                "tcoedge"
            } else {
                "coedge"
            },
        )?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(coedge_start, (index + 1) % coedges.len())?,
        );
        native_ref(
            &mut records,
            native_record_index(coedge_start, (index + coedges.len() - 1) % coedges.len())?,
        );
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(edge_start, edge_index)?);
        records.push(native_bool(coedge.sense == Sense::Reversed));
        native_ref(&mut records, 5);
        native_i64(&mut records, 0);
        let pcurve_ref = coedge
            .pcurves
            .first()
            .map(|use_| {
                let pcurve_id = &use_.pcurve;
                model
                    .pcurves
                    .iter()
                    .position(|pcurve| pcurve.id == *pcurve_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "coedge references missing pcurve {pcurve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(pcurve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        native_ref(&mut records, pcurve_ref);
        if let Some(range) = tolerant_range {
            native_f64(&mut records, range[0]);
            native_f64(&mut records, range[1]);
            native_tolerant_coedge_extension(&mut records, target, &coedge.id)?;
        }
        records.push(0x11);
    }

    let mut edge_owners = BTreeMap::new();
    apply_native_edge_owners(target, coedge_start, &mut edge_owners)?;
    for edge in &model.edges {
        let start = model
            .vertices
            .iter()
            .position(|vertex| vertex.id == edge.start)
            .ok_or_else(|| {
                CodecError::Malformed(format!("edge references missing vertex {}", edge.start))
            })?;
        let end = model
            .vertices
            .iter()
            .position(|vertex| vertex.id == edge.end)
            .ok_or_else(|| {
                CodecError::Malformed(format!("edge references missing vertex {}", edge.end))
            })?;
        let curve_ref = edge
            .curve
            .as_ref()
            .map(|curve_id| {
                model
                    .curves
                    .iter()
                    .position(|curve| curve.id == *curve_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("edge references missing curve {curve_id}"))
                    })
                    .and_then(|ordinal| native_record_index(curve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let mut range = edge.param_range.unwrap_or([0.0, 1.0]);
        // Conic edge parameters are angles in both the IR and the native
        // stream; line parameters are arc lengths, millimeters in the IR
        // and centimeters natively.
        if edge.curve.as_ref().is_some_and(|curve_id| {
            model.curves.iter().any(|curve| {
                curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Line { .. })
            })
        }) {
            range[0] /= LEN_TO_MM;
            range[1] /= LEN_TO_MM;
        }
        native_ident(
            &mut records,
            if edge.tolerance.is_some() {
                "tedge"
            } else {
                "edge"
            },
        )?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(vertex_start, start)?);
        native_f64(&mut records, range[0]);
        native_ref(&mut records, native_record_index(vertex_start, end)?);
        native_f64(&mut records, range[1]);
        native_ref(
            &mut records,
            edge_owners.get(&edge.id).copied().unwrap_or(-1),
        );
        native_ref(&mut records, curve_ref);
        let (sense, continuity) = edge_record_metadata(target, edge)?;
        records.push(native_bool(sense == Sense::Reversed));
        native_string(&mut records, &continuity)?;
        native_tolerant_edge_tail(&mut records, target, edge)?;
        records.push(0x11);
    }

    for vertex in &model.vertices {
        let point = model
            .points
            .iter()
            .position(|point| point.id == vertex.point)
            .ok_or_else(|| {
                CodecError::Malformed(format!("vertex references missing point {}", vertex.point))
            })?;
        let (owning_edge, endpoint_index) = vertex_ownership(target, vertex)?;
        native_ident(
            &mut records,
            if vertex.tolerance.is_some() {
                "tvertex"
            } else {
                "vertex"
            },
        )?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(edge_start, owning_edge)?);
        native_i64(&mut records, i64::from(endpoint_index));
        native_ref(&mut records, native_record_index(point_start, point)?);
        native_tolerant_vertex_tail(&mut records, target, vertex)?;
        records.push(0x11);
    }

    for point in &model.points {
        native_ident(&mut records, "point")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_point(
            &mut records,
            [
                point.position.x / LEN_TO_MM,
                point.position.y / LEN_TO_MM,
                point.position.z / LEN_TO_MM,
            ],
        );
        records.push(0x11);
    }
    native_history_tail(&mut records, target)?;

    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
struct NativeRecordPlan {
    body: i64,
    region: i64,
    shell: i64,
    wire: i64,
    face: i64,
    loop_: i64,
    surface: i64,
    curve: i64,
    pcurve: i64,
    coedge: i64,
    wire_coedge: i64,
    edge: i64,
    vertex: i64,
    point: i64,
    transform: i64,
    attribute: i64,
}

impl NativeRecordPlan {
    fn for_model(model: &cadmpeg_ir::document::Model) -> Result<Self, CodecError> {
        let body_start = 1;
        let region_start = native_record_index(body_start, model.bodies.len())?;
        let shell_start = native_record_index(region_start, model.regions.len())?;
        let wire_count = model.shells.iter().map(source_less_wire_count).sum();
        let wire_start = native_record_index(shell_start, model.shells.len())?;
        let face_start = native_record_index(wire_start, wire_count)?;
        let loop_start = native_record_index(face_start, model.faces.len())?;
        let surface_start = native_record_index(loop_start, model.loops.len())?;
        let curve_start = native_record_index(surface_start, model.surfaces.len())?;
        let pcurve_start = native_record_index(curve_start, model.curves.len())?;
        let ref_pcurve_count = model
            .pcurves
            .iter()
            .map(pcurve_uses_ref_form)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|uses_ref_form| *uses_ref_form)
            .count();
        let pcurve_record_count = model
            .pcurves
            .len()
            .checked_add(ref_pcurve_count)
            .ok_or_else(|| CodecError::Malformed("pcurve record count overflows usize".into()))?;
        let coedge_start = native_record_index(pcurve_start, pcurve_record_count)?;
        let wire_coedge_start = native_record_index(coedge_start, model.coedges.len())?;
        let wire_edge_count = model
            .shells
            .iter()
            .map(|shell| shell.wire_edges.len())
            .sum::<usize>();
        let edge_start = native_record_index(wire_coedge_start, wire_edge_count)?;
        let vertex_start = native_record_index(edge_start, model.edges.len())?;
        let point_start = native_record_index(vertex_start, model.vertices.len())?;
        let transform_start = native_record_index(point_start, model.points.len())?;
        let transform_count = model
            .bodies
            .iter()
            .filter(|body| body.transform.is_some())
            .count();
        let attribute_start = native_record_index(transform_start, transform_count)?;
        Ok(Self {
            body: body_start,
            region: region_start,
            shell: shell_start,
            wire: wire_start,
            face: face_start,
            loop_: loop_start,
            surface: surface_start,
            curve: curve_start,
            pcurve: pcurve_start,
            coedge: coedge_start,
            wire_coedge: wire_coedge_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            transform: transform_start,
            attribute: attribute_start,
        })
    }
}

fn source_less_wire_count(shell: &cadmpeg_ir::topology::Shell) -> usize {
    usize::from(!shell.wire_edges.is_empty()) + shell.free_vertices.len()
}

fn source_less_wire_record_for_shell(
    model: &cadmpeg_ir::document::Model,
    wire_start: i64,
    shell_ordinal: usize,
) -> Result<i64, CodecError> {
    let wire_ordinal = model.shells[..shell_ordinal]
        .iter()
        .map(source_less_wire_count)
        .sum();
    native_record_index(wire_start, wire_ordinal)
}

fn encode_source_less_wires(
    records: &mut Vec<u8>,
    wire_vertices: WireVerticesValidated<'_>,
    wire_start: i64,
    wire_coedge_start: i64,
    shell_start: i64,
    vertex_start: i64,
) -> Result<BTreeMap<VertexId, i64>, CodecError> {
    let target = wire_vertices.target();
    let model = &target.model;
    let vertex_ordinals = model
        .vertices
        .iter()
        .enumerate()
        .map(|(ordinal, vertex)| (&vertex.id, ordinal))
        .collect::<HashMap<_, _>>();
    let mut free_vertex_owners = BTreeMap::new();
    let mut edge_base = 0usize;
    let mut wire_ordinal = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        let shell_wire_count = source_less_wire_count(shell);
        if !shell.wire_edges.is_empty() {
            native_ident(records, "wire")?;
            native_ref(records, -1);
            native_i64(records, -1);
            native_ref(records, -1);
            native_ref(
                records,
                if shell_wire_count > 1 {
                    native_record_index(wire_start, wire_ordinal + 1)?
                } else {
                    -1
                },
            );
            native_ref(records, native_record_index(wire_coedge_start, edge_base)?);
            native_ref(records, native_record_index(shell_start, shell_ordinal)?);
            native_ref(records, -1);
            records.push(native_wire_side(
                target,
                &shell.id,
                &shell.wire_edges,
                None,
            )?);
            records.push(0x11);
            wire_ordinal += 1;
            edge_base += shell.wire_edges.len();
        }
        for (free_ordinal, vertex_id) in shell.free_vertices.iter().enumerate() {
            let vertex_ordinal = vertex_ordinals
                .get(vertex_id)
                .copied()
                .expect("free vertex existence was validated");
            let owner = native_record_index(wire_start, wire_ordinal)?;
            free_vertex_owners.insert(vertex_id.clone(), owner);
            native_ident(records, "wire")?;
            native_ref(records, -1);
            native_i64(records, -1);
            native_ref(records, -1);
            native_ref(
                records,
                if free_ordinal + 1 < shell.free_vertices.len() {
                    native_record_index(wire_start, wire_ordinal + 1)?
                } else {
                    -1
                },
            );
            native_ref(records, -1);
            native_ref(records, native_record_index(shell_start, shell_ordinal)?);
            native_ref(records, native_record_index(vertex_start, vertex_ordinal)?);
            records.push(native_wire_side(target, &shell.id, &[], Some(vertex_id))?);
            records.push(0x11);
            wire_ordinal += 1;
        }
    }
    Ok(free_vertex_owners)
}

fn encode_wire_body_smbh(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    let model = &target.model;
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || !model.faces.is_empty()
        || !model.loops.is_empty()
        || !model.coedges.is_empty()
        || !model.surfaces.is_empty()
        || !model.pcurves.is_empty()
        || model
            .shells
            .iter()
            .any(|shell| shell.wire_edges.is_empty() && shell.free_vertices.is_empty())
        || model
            .shells
            .iter()
            .flat_map(|shell| &shell.wire_edges)
            .zip(&model.edges)
            .any(|(id, edge)| *id != edge.id)
        || model
            .shells
            .iter()
            .map(|shell| shell.wire_edges.len())
            .sum::<usize>()
            != model.edges.len()
        || model
            .bodies
            .iter()
            .any(|body| body.kind != cadmpeg_ir::topology::BodyKind::Wire)
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D wire generation requires face-less wire bodies with nonempty wire shells"
                .into(),
        ));
    }
    for body in &model.bodies {
        if body.regions.is_empty()
            || body.regions.iter().any(|id| {
                !model
                    .regions
                    .iter()
                    .any(|region| region.id == *id && region.body == body.id)
            })
        {
            return Err(CodecError::Malformed(
                "source-less F3D wire ownership is inconsistent".into(),
            ));
        }
    }
    for region in &model.regions {
        if region.shells.is_empty()
            || !model
                .bodies
                .iter()
                .any(|body| body.id == region.body && body.regions.contains(&region.id))
            || region.shells.iter().any(|id| {
                !model
                    .shells
                    .iter()
                    .any(|shell| shell.id == *id && shell.region == region.id)
            })
        {
            return Err(CodecError::Malformed(
                "source-less F3D wire ownership is inconsistent".into(),
            ));
        }
    }
    if model.shells.iter().any(|shell| {
        !model
            .regions
            .iter()
            .any(|region| region.id == shell.region && region.shells.contains(&shell.id))
    }) {
        return Err(CodecError::Malformed(
            "source-less F3D wire ownership is inconsistent".into(),
        ));
    }
    let wire_vertices = validate_source_less_wire_vertices(target)?;
    let body_start = 1i64;
    let region_start = native_record_index(body_start, model.bodies.len())?;
    let shell_start = native_record_index(region_start, model.regions.len())?;
    let wire_start = native_record_index(shell_start, model.shells.len())?;
    let wire_count = model.shells.iter().map(source_less_wire_count).sum();
    let wire_coedge_start = native_record_index(wire_start, wire_count)?;
    let curve_start = native_record_index(wire_coedge_start, model.edges.len())?;
    let edge_start = native_record_index(curve_start, model.curves.len())?;
    let vertex_start = native_record_index(edge_start, model.edges.len())?;
    let point_start = native_record_index(vertex_start, model.vertices.len())?;
    let transform_start = native_record_index(point_start, model.points.len())?;
    let transform_count = model
        .bodies
        .iter()
        .filter(|body| body.transform.is_some())
        .count();
    let attribute_start = native_record_index(transform_start, transform_count)?;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for (ordinal, body) in model.bodies.iter().enumerate() {
        let first_region = body.regions.first().expect("wire ownership was validated");
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == *first_region)
            .expect("wire ownership was validated");
        let first_shell = model.regions[region_ordinal]
            .shells
            .first()
            .expect("wire ownership was validated");
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .expect("wire ownership was validated");
        let transform_ordinal = model.bodies[..ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_color_or_body_tag_ref(target, body, ordinal, attribute_start)?,
        );
        native_i64(&mut records, source_less_body_key(target, body, ordinal)?);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(
            &mut records,
            source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?,
        );
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for region in &model.regions {
        let body_ordinal = model
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .expect("wire ownership was validated");
        let body = &model.bodies[body_ordinal];
        let position = body
            .regions
            .iter()
            .position(|id| *id == region.id)
            .expect("wire ownership was validated");
        let next = body
            .regions
            .get(position + 1)
            .map(|id| {
                model
                    .regions
                    .iter()
                    .position(|candidate| candidate.id == *id)
                    .expect("wire ownership was validated")
            })
            .map(|position| native_record_index(region_start, position))
            .transpose()?
            .unwrap_or(-1);
        let first_shell = region.shells.first().expect("wire ownership was validated");
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .expect("wire ownership was validated");
        native_ident(&mut records, "region")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for (ordinal, shell) in model.shells.iter().enumerate() {
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == shell.region)
            .expect("wire ownership was validated");
        let region = &model.regions[region_ordinal];
        let position = region
            .shells
            .iter()
            .position(|id| *id == shell.id)
            .expect("wire ownership was validated");
        let next = region
            .shells
            .get(position + 1)
            .map(|id| {
                model
                    .shells
                    .iter()
                    .position(|candidate| candidate.id == *id)
                    .expect("wire ownership was validated")
            })
            .map(|position| native_record_index(shell_start, position))
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        for reference in [
            -1,
            next,
            -1,
            -1,
            source_less_wire_record_for_shell(model, wire_start, ordinal)?,
            native_record_index(region_start, region_ordinal)?,
        ] {
            native_ref(&mut records, reference);
        }
        records.push(0x11);
    }
    let free_vertex_owners = encode_source_less_wires(
        &mut records,
        wire_vertices,
        wire_start,
        wire_coedge_start,
        shell_start,
        vertex_start,
    )?;
    let mut edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        for ordinal in 0..shell.wire_edges.len() {
            let edge_ordinal = edge_base + ordinal;
            let next = edge_base + (ordinal + 1) % shell.wire_edges.len();
            let previous =
                edge_base + (ordinal + shell.wire_edges.len() - 1) % shell.wire_edges.len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(
                &mut records,
                source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?,
            );
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        edge_base += shell.wire_edges.len();
    }
    encode_source_less_curves(&mut records, target)?;
    let mut wire_edge_owners = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            native_record_index(wire_coedge_start, ordinal).map(|owner| (edge.id.clone(), owner))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    apply_native_edge_owners(target, wire_coedge_start, &mut wire_edge_owners)?;
    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        SourceLessRecordStarts {
            curve: curve_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            attribute: attribute_start,
        },
        Some(&wire_edge_owners),
        Some(&free_vertex_owners),
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, target, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(&mut records, target, attribute_start)?;
    native_history_tail(&mut records, target)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

fn encode_source_less_curves(records: &mut Vec<u8>, target: &CadIr) -> Result<(), CodecError> {
    let model = &target.model;
    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(records, "straight")?;
                native_point(
                    records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(records, "ellipse")?;
                native_point(
                    records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if major_radius != 0.0 => {
                native_curve_base(records, "ellipse")?;
                native_point(
                    records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        major_direction.x * major_radius / LEN_TO_MM,
                        major_direction.y * major_radius / LEN_TO_MM,
                        major_direction.z * major_radius / LEN_TO_MM,
                    ],
                );
                native_f64(records, minor_radius / major_radius);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(records, target, &carrier.id, curve)? {
                    native_curve_base(records, "intcurve")?;
                    native_nurbs_curve(records, curve)?;
                }
            }
            CurveGeometry::Procedural { .. } => {
                if !native_cacheless_procedural_curve(records, target, &carrier.id)? {
                    return Err(CodecError::Malformed(format!(
                        "procedural curve carrier {} has no construction",
                        carrier.id
                    )));
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(records, "degenerate_curve")?;
                native_point(
                    records,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less F3D wire curve carrier is unsupported".into(),
                ))
            }
        }
        records.push(0x11);
    }
    Ok(())
}

fn encode_multi_face_shell_smbh(
    target: &CadIr,
    wire_vertices: WireVerticesValidated<'_>,
) -> Result<Vec<u8>, CodecError> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let model = &target.model;
    let region_ordinals: HashMap<_, _> = model
        .regions
        .iter()
        .enumerate()
        .map(|(ordinal, region)| (&region.id, ordinal))
        .collect();
    let shell_ordinals: HashMap<_, _> = model
        .shells
        .iter()
        .enumerate()
        .map(|(ordinal, shell)| (&shell.id, ordinal))
        .collect();
    let coedge_ordinals: HashMap<_, _> = model
        .coedges
        .iter()
        .enumerate()
        .map(|(ordinal, coedge)| (&coedge.id, ordinal))
        .collect();
    let edge_ordinals: HashMap<_, _> = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| (&edge.id, ordinal))
        .collect();
    let loop_ordinals: HashMap<_, _> = model
        .loops
        .iter()
        .enumerate()
        .map(|(ordinal, lp)| (&lp.id, ordinal))
        .collect();
    let pcurve_ordinals: HashMap<_, _> = model
        .pcurves
        .iter()
        .enumerate()
        .map(|(ordinal, pcurve)| (&pcurve.id, ordinal))
        .collect();
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || model.faces.is_empty()
        || model.loops.len() < model.faces.len()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation requires owned face topology".into(),
        ));
    }
    validate_source_less_body_kinds(model)?;

    let plan = NativeRecordPlan::for_model(model)?;
    let NativeRecordPlan {
        body: body_start,
        region: region_start,
        shell: shell_start,
        wire: wire_start,
        face: face_start,
        loop_: loop_start,
        surface: surface_start,
        curve: curve_start,
        pcurve: pcurve_start,
        coedge: coedge_start,
        wire_coedge: wire_coedge_start,
        edge: edge_start,
        vertex: vertex_start,
        point: point_start,
        transform: transform_start,
        attribute: attribute_start,
    } = plan;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for (body_ordinal, body) in model.bodies.iter().enumerate() {
        let first_region = body
            .regions
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("body {} has no region", body.id)))?;
        let region_ordinal = region_ordinals.get(first_region).copied().ok_or_else(|| {
            CodecError::Malformed(format!("body references missing region {first_region}"))
        })?;
        let transform_ordinal = model.bodies[..body_ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        let first_wire = body
            .regions
            .iter()
            .filter_map(|region_id| {
                region_ordinals
                    .get(region_id)
                    .map(|ordinal| &model.regions[*ordinal])
            })
            .flat_map(|region| &region.shells)
            .find_map(|shell_id| {
                shell_ordinals
                    .get(shell_id)
                    .copied()
                    .filter(|ordinal| source_less_wire_count(&model.shells[*ordinal]) != 0)
            })
            .map(|shell_ordinal| {
                source_less_wire_record_for_shell(model, wire_start, shell_ordinal)
            })
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_color_or_body_tag_ref(target, body, body_ordinal, attribute_start)?,
        );
        native_i64(
            &mut records,
            source_less_body_key(target, body, body_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(&mut records, first_wire);
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for region in &model.regions {
        let body_ordinal = model
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .ok_or_else(|| CodecError::Malformed(format!("region {} has no body", region.id)))?;
        let body = &model.bodies[body_ordinal];
        let ordinal = body
            .regions
            .iter()
            .position(|id| *id == region.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("body does not own region {}", region.id))
            })?;
        let first_shell = region
            .shells
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("region {} has no shell", region.id)))?;
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .ok_or_else(|| {
                CodecError::Malformed(format!("region references missing shell {first_shell}"))
            })?;
        let next = if ordinal + 1 == body.regions.len() {
            -1
        } else {
            let id = &body.regions[ordinal + 1];
            let position = model
                .regions
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("body references missing region {id}"))
                })?;
            native_record_index(region_start, position)?
        };
        native_ident(&mut records, "region")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for shell in &model.shells {
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == shell.region)
            .ok_or_else(|| CodecError::Malformed(format!("shell {} has no region", shell.id)))?;
        let region = &model.regions[region_ordinal];
        let ordinal = region
            .shells
            .iter()
            .position(|id| *id == shell.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("region does not own shell {}", shell.id))
            })?;
        let first_face = shell
            .faces
            .first()
            .map(|first_face| {
                model
                    .faces
                    .iter()
                    .position(|face| face.id == *first_face)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("shell references missing face {first_face}"))
                    })
                    .and_then(|ordinal| native_record_index(face_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let next = if ordinal + 1 == region.shells.len() {
            -1
        } else {
            let id = &region.shells[ordinal + 1];
            let position = model
                .shells
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("region references missing shell {id}"))
                })?;
            native_record_index(shell_start, position)?
        };
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(&mut records, -1);
        native_ref(&mut records, first_face);
        native_ref(
            &mut records,
            if source_less_wire_count(shell) == 0 {
                -1
            } else {
                source_less_wire_record_for_shell(
                    model,
                    wire_start,
                    model
                        .shells
                        .iter()
                        .position(|item| item.id == shell.id)
                        .expect("current shell is present"),
                )?
            },
        );
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        records.push(0x11);
    }

    let free_vertex_owners = encode_source_less_wires(
        &mut records,
        wire_vertices,
        wire_start,
        wire_coedge_start,
        shell_start,
        vertex_start,
    )?;

    for (face_ordinal_global, face) in model.faces.iter().enumerate() {
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == face.shell)
            .ok_or_else(|| CodecError::Malformed(format!("face {} has no shell", face.id)))?;
        let shell = &model.shells[shell_ordinal];
        let ordinal = shell
            .faces
            .iter()
            .position(|id| *id == face.id)
            .ok_or_else(|| CodecError::Malformed(format!("shell does not own face {}", face.id)))?;
        if face.loops.is_empty() {
            return Err(CodecError::NotImplemented(
                "source-less multi-loop F3D requires every face to own a loop".into(),
            ));
        }
        let loop_position = model
            .loops
            .iter()
            .position(|loop_| loop_.id == face.loops[0])
            .ok_or_else(|| {
                CodecError::Malformed(format!("face references missing loop {}", face.loops[0]))
            })?;
        let surface_position = model
            .surfaces
            .iter()
            .position(|surface| surface.id == face.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!("face references missing surface {}", face.surface))
            })?;
        native_ident(&mut records, "face")?;
        native_ref(
            &mut records,
            owner_color_or_face_tag_ref(target, face, face_ordinal_global, attribute_start)?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            if ordinal + 1 == shell.faces.len() {
                -1
            } else {
                let id = &shell.faces[ordinal + 1];
                let position = model
                    .faces
                    .iter()
                    .position(|item| item.id == *id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("shell references missing face {id}"))
                    })?;
                native_record_index(face_start, position)?
            },
        );
        native_ref(
            &mut records,
            native_record_index(loop_start, loop_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(surface_start, surface_position)?,
        );
        records.push(native_bool(
            native_face_sense(target, face)? == Sense::Reversed,
        ));
        native_face_sidedness(&mut records, target, face)?;
        records.push(0x11);
    }

    for loop_ in &model.loops {
        let face_position = model
            .faces
            .iter()
            .position(|face| face.id == loop_.face)
            .ok_or_else(|| {
                CodecError::Malformed(format!("loop references missing face {}", loop_.face))
            })?;
        let first = loop_
            .coedges
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("loop {} has no coedges", loop_.id)))?;
        let coedge_position = model
            .coedges
            .iter()
            .position(|coedge| coedge.id == *first)
            .ok_or_else(|| {
                CodecError::Malformed(format!("loop references missing coedge {first}"))
            })?;
        native_ident(&mut records, "loop")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        let face = &model.faces[face_position];
        let ordinal = face
            .loops
            .iter()
            .position(|id| *id == loop_.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("face {} does not own loop {}", face.id, loop_.id))
            })?;
        let next_loop = if ordinal + 1 == face.loops.len() {
            -1
        } else {
            let next_id = &face.loops[ordinal + 1];
            let position = model
                .loops
                .iter()
                .position(|candidate| candidate.id == *next_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("face references missing loop {next_id}"))
                })?;
            native_record_index(loop_start, position)?
        };
        native_ref(&mut records, next_loop);
        native_ref(
            &mut records,
            native_record_index(coedge_start, coedge_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(face_start, face_position)?,
        );
        records.push(0x11);
    }

    for surface in &model.surfaces {
        match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                native_surface_base(&mut records, "plane")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [normal.x, normal.y, normal.z]);
                native_vector(&mut records, [u_axis.x, u_axis.y, u_axis.z]);
                records.push(0x0b);
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } => {
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, 1.0);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, 0.0);
                native_f64(&mut records, 1.0);
                native_f64(&mut records, radius / LEN_TO_MM);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } => {
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, ratio);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, half_angle.sin());
                native_f64(&mut records, half_angle.cos());
                native_f64(&mut records, radius / LEN_TO_MM);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_surface_base(&mut records, "sphere")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, radius / LEN_TO_MM);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Nurbs(ref nurbs) => {
                if !native_procedural_surface(&mut records, target, surface, nurbs)? {
                    native_surface_base(&mut records, "spline")?;
                    native_nurbs_surface(&mut records, nurbs)?;
                }
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                native_surface_base(&mut records, "torus")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_f64(&mut records, major_radius / LEN_TO_MM);
                native_f64(&mut records, minor_radius / LEN_TO_MM);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Polygonal { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less multi-face F3D does not support polygonal surface carrier {}",
                    surface.id
                )));
            }
            SurfaceGeometry::Transformed { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less multi-face F3D does not support transformed surface carrier {}",
                    surface.id
                )));
            }
            SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
                if !native_cacheless_procedural_surface(&mut records, target, surface)? {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less multi-face F3D does not support surface carrier {}",
                        surface.id
                    )));
                }
            }
        }
        records.push(0x11);
    }

    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(&mut records, "straight")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(&mut records, target, &carrier.id, curve)? {
                    native_curve_base(&mut records, "intcurve")?;
                    native_nurbs_curve(&mut records, curve)?;
                }
            }
            CurveGeometry::Procedural { .. } => {
                if !native_cacheless_procedural_curve(&mut records, target, &carrier.id)? {
                    return Err(CodecError::Malformed(format!(
                        "procedural curve carrier {} has no construction",
                        carrier.id
                    )));
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(&mut records, "degenerate_curve")?;
                native_point(
                    &mut records,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if major_radius == 0.0 {
                    return Err(CodecError::Malformed(
                        "source-less F3D ellipse has zero major radius".into(),
                    ));
                }
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        major_direction.x * major_radius / LEN_TO_MM,
                        major_direction.y * major_radius / LEN_TO_MM,
                        major_direction.z * major_radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, minor_radius / major_radius);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less multi-face F3D does not support this curve carrier".into(),
                ));
            }
        }
        records.push(0x11);
    }

    let ref_pcurve_start = native_record_index(pcurve_start, model.pcurves.len())?;
    let mut ref_pcurve_ordinal = 0usize;
    for pcurve in &model.pcurves {
        let companion_ref = pcurve_uses_ref_form(pcurve)?
            .then(|| native_record_index(ref_pcurve_start, ref_pcurve_ordinal))
            .transpose()?;
        native_pcurve(&mut records, pcurve, companion_ref)?;
        ref_pcurve_ordinal += usize::from(companion_ref.is_some());
        records.push(0x11);
    }
    for pcurve in model
        .pcurves
        .iter()
        .filter(|pcurve| pcurve_uses_ref_form(pcurve).is_ok_and(|value| value))
    {
        native_ref_pcurve_companion(&mut records, pcurve)?;
        records.push(0x11);
    }

    for (coedge_ordinal, coedge) in model.coedges.iter().enumerate() {
        if coedge.pcurves.len() > 1 {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} has an ordered pcurve collection",
                coedge.id
            )));
        }
        let next = coedge_ordinals.get(&coedge.next).copied();
        let previous = coedge_ordinals.get(&coedge.previous).copied();
        let radial = coedge_ordinals.get(&coedge.radial_next).copied();
        let edge = edge_ordinals.get(&coedge.edge).copied();
        let owner = loop_ordinals.get(&coedge.owner_loop).copied();
        let (Some(next), Some(previous), Some(radial), Some(edge), Some(owner)) =
            (next, previous, radial, edge, owner)
        else {
            return Err(CodecError::Malformed(format!(
                "coedge {} has an unresolved topology reference",
                coedge.id
            )));
        };
        let tolerant_range = tolerant_coedge_range(target, &coedge.id)?;
        native_ident(
            &mut records,
            if tolerant_range.is_some() {
                "tcoedge"
            } else {
                "coedge"
            },
        )?;
        native_ref(
            &mut records,
            sketch_link_attribute_ref(target, coedge, coedge_ordinal, attribute_start)?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(coedge_start, next)?);
        native_ref(&mut records, native_record_index(coedge_start, previous)?);
        native_ref(
            &mut records,
            if radial == coedge_ordinals.get(&coedge.id).copied().unwrap_or(radial) {
                -1
            } else {
                native_record_index(coedge_start, radial)?
            },
        );
        native_ref(&mut records, native_record_index(edge_start, edge)?);
        records.push(native_bool(coedge.sense == Sense::Reversed));
        native_ref(&mut records, native_record_index(loop_start, owner)?);
        native_i64(&mut records, 0);
        let pcurve_ref = coedge
            .pcurves
            .first()
            .map(|use_| {
                let pcurve_id = &use_.pcurve;
                pcurve_ordinals
                    .get(pcurve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "coedge references missing pcurve {pcurve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(pcurve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        native_ref(&mut records, pcurve_ref);
        if let Some(range) = tolerant_range {
            native_f64(&mut records, range[0]);
            native_f64(&mut records, range[1]);
            native_tolerant_coedge_extension(&mut records, target, &coedge.id)?;
        }
        records.push(0x11);
    }

    let mut wire_edge_owners = BTreeMap::new();
    let mut wire_edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        if shell.wire_edges.is_empty() {
            continue;
        }
        let wire_ref = source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?;
        for (ordinal, edge_id) in shell.wire_edges.iter().enumerate() {
            let edge_ordinal = edge_ordinals.get(edge_id).copied().ok_or_else(|| {
                CodecError::Malformed(format!("wire references missing edge {edge_id}"))
            })?;
            let coedge_ordinal = wire_edge_base + ordinal;
            let owner = native_record_index(wire_coedge_start, coedge_ordinal)?;
            if wire_edge_owners.insert(edge_id.clone(), owner).is_some() {
                return Err(CodecError::Malformed(format!(
                    "wire edge {edge_id} belongs to more than one shell"
                )));
            }
            let next = wire_edge_base + (ordinal + 1) % shell.wire_edges.len();
            let previous =
                wire_edge_base + (ordinal + shell.wire_edges.len() - 1) % shell.wire_edges.len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(&mut records, wire_ref);
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        wire_edge_base += shell.wire_edges.len();
    }
    apply_native_edge_owners(target, coedge_start, &mut wire_edge_owners)?;

    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        SourceLessRecordStarts {
            curve: curve_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            attribute: attribute_start,
        },
        Some(&wire_edge_owners),
        Some(&free_vertex_owners),
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, target, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(&mut records, target, attribute_start)?;
    native_history_tail(&mut records, target)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

/// Base record indices for the source-less stream sections that
/// `encode_source_less_edges_vertices_points` cross-references.
#[derive(Clone, Copy)]
struct SourceLessRecordStarts {
    curve: i64,
    edge: i64,
    vertex: i64,
    point: i64,
    attribute: i64,
}

fn encode_source_less_edges_vertices_points(
    records: &mut Vec<u8>,
    target: &CadIr,
    starts: SourceLessRecordStarts,
    edge_owners: Option<&BTreeMap<cadmpeg_ir::ids::EdgeId, i64>>,
    free_vertex_owners: Option<&BTreeMap<VertexId, i64>>,
) -> Result<(), CodecError> {
    let SourceLessRecordStarts {
        curve: curve_start,
        edge: edge_start,
        vertex: vertex_start,
        point: point_start,
        attribute: attribute_start,
    } = starts;
    let model = &target.model;
    let vertex_ordinals: HashMap<_, _> = model
        .vertices
        .iter()
        .enumerate()
        .map(|(ordinal, vertex)| (&vertex.id, ordinal))
        .collect();
    let curve_ordinals: HashMap<_, _> = model
        .curves
        .iter()
        .enumerate()
        .map(|(ordinal, curve)| (&curve.id, ordinal))
        .collect();
    let point_ordinals: HashMap<_, _> = model
        .points
        .iter()
        .enumerate()
        .map(|(ordinal, point)| (&point.id, ordinal))
        .collect();
    for (edge_ordinal, edge) in model.edges.iter().enumerate() {
        let start = vertex_ordinals.get(&edge.start).copied();
        let end = vertex_ordinals.get(&edge.end).copied();
        let (Some(start), Some(end)) = (start, end) else {
            return Err(CodecError::Malformed(format!(
                "edge {} has an unresolved vertex",
                edge.id
            )));
        };
        let curve_ref = edge
            .curve
            .as_ref()
            .map(|curve_id| {
                curve_ordinals
                    .get(curve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("edge references missing curve {curve_id}"))
                    })
                    .and_then(|ordinal| native_record_index(curve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let mut range = edge.param_range.unwrap_or([0.0, 1.0]);
        // Conic edge parameters are angles in both the IR and the native
        // stream; line parameters are arc lengths, millimeters in the IR
        // and centimeters natively.
        if edge.curve.as_ref().is_some_and(|curve_id| {
            curve_ordinals.get(curve_id).is_some_and(|ordinal| {
                matches!(model.curves[*ordinal].geometry, CurveGeometry::Line { .. })
            })
        }) {
            range[0] /= LEN_TO_MM;
            range[1] /= LEN_TO_MM;
        }
        native_ident(
            records,
            if edge.tolerance.is_some() {
                "tedge"
            } else {
                "edge"
            },
        )?;
        let persistent =
            edge_persistent_attribute_ref(target, edge, edge_ordinal, attribute_start)?;
        native_ref(
            records,
            if let Some(reference) = persistent {
                reference
            } else {
                timestamp_attribute_ref(
                    target,
                    &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
                    attribute_start,
                )?
                .unwrap_or(-1)
            },
        );
        native_i64(records, -1);
        native_ref(records, -1);
        native_ref(records, native_record_index(vertex_start, start)?);
        native_f64(records, range[0]);
        native_ref(records, native_record_index(vertex_start, end)?);
        native_f64(records, range[1]);
        native_ref(
            records,
            edge_owners
                .and_then(|owners| owners.get(&edge.id))
                .copied()
                .unwrap_or(-1),
        );
        native_ref(records, curve_ref);
        let (sense, continuity) = edge_record_metadata(target, edge)?;
        records.push(native_bool(sense == Sense::Reversed));
        native_string(records, &continuity)?;
        native_tolerant_edge_tail(records, target, edge)?;
        records.push(0x11);
    }
    for vertex in &model.vertices {
        let point = point_ordinals.get(&vertex.point).copied();
        let Some(point) = point else {
            return Err(CodecError::Malformed(format!(
                "vertex {} has an unresolved carrier",
                vertex.id
            )));
        };
        let ownership = free_vertex_owners
            .and_then(|owners| owners.get(&vertex.id))
            .copied();
        native_ident(
            records,
            if vertex.tolerance.is_some() {
                "tvertex"
            } else {
                "vertex"
            },
        )?;
        native_ref(
            records,
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Vertex(vertex.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1),
        );
        native_i64(records, -1);
        native_ref(records, -1);
        if let Some(wire) = ownership {
            native_ref(records, wire);
            native_i64(records, -1);
        } else {
            let (edge, endpoint_index) = vertex_ownership(target, vertex)?;
            native_ref(records, native_record_index(edge_start, edge)?);
            native_i64(records, i64::from(endpoint_index));
        }
        native_ref(records, native_record_index(point_start, point)?);
        native_tolerant_vertex_tail(records, target, vertex)?;
        records.push(0x11);
    }
    for point in &model.points {
        native_ident(records, "point")?;
        native_ref(records, -1);
        native_i64(records, -1);
        native_ref(records, -1);
        native_point(
            records,
            [
                point.position.x / LEN_TO_MM,
                point.position.y / LEN_TO_MM,
                point.position.z / LEN_TO_MM,
            ],
        );
        records.push(0x11);
    }
    Ok(())
}

fn vertex_ownership(
    target: &CadIr,
    vertex: &cadmpeg_ir::topology::Vertex,
) -> Result<(usize, u8), CodecError> {
    let model = &target.model;
    if let Some(metadata) = f3d_native(target)?.and_then(|native| {
        native
            .vertex_ownerships
            .into_iter()
            .find(|metadata| metadata.vertex == vertex.id)
    }) {
        let ordinal = model
            .edges
            .iter()
            .position(|edge| edge.id == metadata.owning_edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "vertex {} references missing owning edge {}",
                    vertex.id, metadata.owning_edge
                ))
            })?;
        let edge = &model.edges[ordinal];
        let valid = match metadata.endpoint_index {
            0 => edge.start == vertex.id,
            1 => edge.end == vertex.id,
            _ => false,
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "vertex {} endpoint slot {} conflicts with owning edge {}",
                vertex.id, metadata.endpoint_index, metadata.owning_edge
            )));
        }
        return Ok((ordinal, metadata.endpoint_index));
    }
    model
        .edges
        .iter()
        .enumerate()
        .find_map(|(ordinal, edge)| {
            if edge.start == vertex.id {
                Some((ordinal, 0))
            } else if edge.end == vertex.id {
                Some((ordinal, 1))
            } else {
                None
            }
        })
        .ok_or_else(|| CodecError::Malformed(format!("vertex {} has no edge", vertex.id)))
}

fn native_face_sidedness(
    records: &mut Vec<u8>,
    target: &CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<(), CodecError> {
    let containment = f3d_native(target)?.and_then(|native| {
        native
            .face_sidedness
            .into_iter()
            .find(|metadata| metadata.face == face.id)
            .and_then(|metadata| metadata.containment)
    });
    records.push(native_bool(containment.is_some()));
    if let Some(containment) = containment {
        records.push(match containment {
            crate::records::FaceContainment::In => 0x0a,
            crate::records::FaceContainment::Out => 0x0b,
        });
    }
    Ok(())
}

fn native_face_sense(
    target: &CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<Sense, CodecError> {
    Ok(f3d_native(target)?
        .and_then(|native| {
            native
                .face_sidedness
                .into_iter()
                .find(|metadata| metadata.face == face.id)
                .map(|metadata| {
                    normalized_face_sense_to_native(
                        face.sense,
                        metadata.native_sense,
                        metadata.normalized_sense,
                    )
                })
        })
        .unwrap_or(face.sense))
}

fn native_wire_side(
    target: &CadIr,
    shell: &ShellId,
    edges: &[cadmpeg_ir::ids::EdgeId],
    free_vertex: Option<&VertexId>,
) -> Result<u8, CodecError> {
    let matches = f3d_native(target)?
        .map(|native| {
            native
                .wire_topologies
                .into_iter()
                .filter(|wire| {
                    wire.shell == *shell
                        && wire.edges == edges
                        && wire.free_vertex.as_ref() == free_vertex
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let side = match matches.as_slice() {
        [] => crate::records::WireSide::Out,
        [wire] => wire.side,
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D generation has duplicate native wire metadata on shell {shell}"
            )))
        }
    };
    Ok(match side {
        crate::records::WireSide::In => 0x0a,
        crate::records::WireSide::Out => 0x0b,
    })
}

fn native_tolerant_vertex_tail(
    records: &mut Vec<u8>,
    target: &CadIr,
    vertex: &cadmpeg_ir::topology::Vertex,
) -> Result<(), CodecError> {
    let Some(tolerance) = vertex.tolerance else {
        return Ok(());
    };
    if !tolerance.is_finite() {
        return Err(CodecError::Malformed(format!(
            "F3D vertex {} tolerance must be finite",
            vertex.id
        )));
    }
    // The record stores three f64 tolerance slots: the two leading slots
    // verbatim (default: the -1 unevaluated sentinel) and the evaluated
    // tolerance last, followed by an integer 0. A negative tolerance is the
    // unevaluated sentinel, stored verbatim; a non-negative tolerance
    // converts from millimetres to centimetres.
    let leading = f3d_native(target)?
        .and_then(|native| {
            native
                .tolerant_vertex_tails
                .into_iter()
                .find(|tail| tail.vertex == vertex.id)
        })
        .map_or([-1.0; 2], |tail| tail.leading_tolerances);
    for value in leading {
        native_f64(records, value);
    }
    native_f64(
        records,
        if tolerance < 0.0 {
            tolerance
        } else {
            tolerance / LEN_TO_MM
        },
    );
    native_i64(records, 0);
    Ok(())
}

fn native_tolerant_edge_tail(
    records: &mut Vec<u8>,
    target: &CadIr,
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<(), CodecError> {
    let Some(tolerance) = edge.tolerance else {
        return Ok(());
    };
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(CodecError::Malformed(format!(
            "F3D edge {} tolerance must be finite and nonnegative",
            edge.id
        )));
    }
    native_f64(records, tolerance / LEN_TO_MM);
    let trailing = f3d_native(target)?
        .and_then(|native| {
            native
                .tolerant_edge_tails
                .into_iter()
                .find(|tail| tail.edge == edge.id)
        })
        .map_or([23100, 0], |tail| tail.trailing_integers);
    for value in trailing {
        native_i64(records, value);
    }
    Ok(())
}

fn edge_record_metadata(
    target: &CadIr,
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<(Sense, String), CodecError> {
    let metadata = f3d_native(target)?.and_then(|native| {
        native
            .edge_continuities
            .into_iter()
            .find(|metadata| metadata.edge == edge.id)
    });
    let sense = metadata
        .as_ref()
        .map_or(Sense::Forward, |metadata| metadata.sense);
    let continuity = metadata.map_or_else(|| "unknown".to_owned(), |metadata| metadata.continuity);
    if continuity != "tangent" && continuity != "unknown" {
        return Err(CodecError::Malformed(format!(
            "F3D edge {} has unsupported continuity token {continuity}",
            edge.id
        )));
    }
    Ok((sense, continuity))
}

fn apply_native_edge_owners(
    target: &CadIr,
    coedge_start: i64,
    owners: &mut BTreeMap<cadmpeg_ir::ids::EdgeId, i64>,
) -> Result<(), CodecError> {
    let metadata = f3d_native(target)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    for ownership in metadata {
        if !target
            .model
            .edges
            .iter()
            .any(|edge| edge.id == ownership.edge)
        {
            return Err(CodecError::Malformed(format!(
                "F3D edge ownership {} references missing edge {}",
                ownership.id, ownership.edge
            )));
        }
        let owner = match ownership.owner_coedge {
            None => -1,
            Some(owner) => {
                if let Some((ordinal, coedge)) = target
                    .model
                    .coedges
                    .iter()
                    .enumerate()
                    .find(|(_, coedge)| coedge.id == owner)
                {
                    if coedge.edge != ownership.edge {
                        return Err(CodecError::Malformed(format!(
                            "F3D edge ownership {} selects a coedge of another edge",
                            ownership.id
                        )));
                    }
                    native_record_index(coedge_start, ordinal)?
                } else if owners.contains_key(&ownership.edge) {
                    // Wire coedges are native-only and are reconstructed from
                    // the shell's wire-edge list before this override runs.
                    continue;
                } else {
                    return Err(CodecError::Malformed(format!(
                        "F3D edge ownership {} references missing coedge {owner}",
                        ownership.id
                    )));
                }
            }
        };
        owners.insert(ownership.edge, owner);
    }
    Ok(())
}
