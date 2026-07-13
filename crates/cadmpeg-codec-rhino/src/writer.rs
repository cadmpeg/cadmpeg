// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use std::io::Write;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::chunks::{MAGIC, TCODE_ENDOFFILE, TCODE_SHORT};

const TCODE_PROPERTIES_TABLE: u32 = 0x1000_0014;
const TCODE_SETTINGS_TABLE: u32 = 0x1000_0015;
const TCODE_OBJECT_TABLE: u32 = 0x1000_0013;
const TCODE_ENDOFTABLE: u32 = 0xffff_ffff;
const TCODE_UNITS_AND_TOLERANCES: u32 = 0x2000_8031;
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const TCODE_OBJECT_RECORD_TYPE: u32 = 0x0200_0071;
const TCODE_OBJECT_RECORD_END: u32 = 0x0200_007f;
const TCODE_CLASS_WRAPPER: u32 = 0x0002_7ffa;
const TCODE_CLASS_UUID: u32 = 0x0002_fffb;
const TCODE_CLASS_DATA: u32 = 0x0002_fffc;
const TCODE_CLASS_END: u32 = 0x0002_7fff;

const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const POINT_CLOUD_CLASS: [u8; 16] = [
    0x47, 0xf3, 0x88, 0x24, 0xfa, 0xf8, 0xd3, 0x11, 0xbf, 0xec, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const LINE_CLASS: [u8; 16] = [
    0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const BREP_CLASS: [u8; 16] = [
    0xc5, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const ARC_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_CURVE_CLASS: [u8; 16] = [
    0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_SURFACE_CLASS: [u8; 16] = [
    0xde, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const PLANE_SURFACE_CLASS: [u8; 16] = [
    0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const CHANNEL_UV: u32 = 0x5248_0001;
const CHANNEL_COLOR: u32 = 0x5248_0002;
const CHANNEL_SURFACE_PARAMETERS: u32 = 0x5248_0003;
const CHANNEL_CURVATURE: u32 = 0x5248_0004;

pub(crate) fn write(ir: &CadIr, version: u64, output: &mut dyn Write) -> Result<(), CodecError> {
    check_representable(ir)?;
    if let Some(payload) = planar_sheet_brep_payload(ir)? {
        return write_archive(
            ir,
            version,
            vec![object_record(0x10, BREP_CLASS, &payload)],
            output,
        );
    }
    let (topology_points, point_groups) = free_vertex_groups(ir)?;

    let mut objects = ir
        .model
        .points
        .iter()
        .filter(|point| !topology_points.contains(&point.id.0))
        .map(|point| {
            let position = point.position;
            let mut payload = vec![0x10];
            payload.extend(position.x.to_le_bytes());
            payload.extend(position.y.to_le_bytes());
            payload.extend(position.z.to_le_bytes());
            object_record(1, POINT_CLASS, &payload)
        })
        .collect::<Vec<_>>();
    for points in point_groups {
        if points.len() == 1 {
            let point = points[0];
            let mut payload = vec![0x10];
            payload.extend(point.x.to_le_bytes());
            payload.extend(point.y.to_le_bytes());
            payload.extend(point.z.to_le_bytes());
            objects.push(object_record(1, POINT_CLASS, &payload));
        } else {
            objects.push(object_record(
                2,
                POINT_CLOUD_CLASS,
                &point_cloud_payload(&points),
            ));
        }
    }
    for curve in &ir.model.curves {
        let (class, payload) = match &curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => (
                ARC_CLASS,
                circle_payload(*center, *axis, *ref_direction, *radius),
            ),
            CurveGeometry::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)),
            _ => unreachable!("representability checked before serialization"),
        };
        objects.push(object_record(4, class, &payload));
    }
    for surface in &ir.model.surfaces {
        let (class, payload) = match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => (
                PLANE_SURFACE_CLASS,
                plane_surface_payload(*origin, *normal, *u_axis),
            ),
            SurfaceGeometry::Nurbs(nurbs) => (NURBS_SURFACE_CLASS, nurbs_surface_payload(nurbs)),
            _ => unreachable!("representability checked before serialization"),
        };
        objects.push(object_record(8, class, &payload));
    }
    for mesh in &ir.model.tessellations {
        objects.push(object_record(
            0x20,
            MESH_CLASS,
            &mesh_payload(mesh, version),
        ));
    }

    write_archive(ir, version, objects, output)
}

fn write_archive(
    ir: &CadIr,
    version: u64,
    objects: Vec<Vec<u8>>,
    output: &mut dyn Write,
) -> Result<(), CodecError> {
    let mut bytes = header(version)?;
    bytes.extend(long_chunk(1, b"cadmpeg"));
    bytes.extend(table(TCODE_PROPERTIES_TABLE, &[]));
    bytes.extend(table(
        TCODE_SETTINGS_TABLE,
        &[units_record(ir.tolerances.linear, ir.tolerances.angular)],
    ));
    bytes.extend(table(TCODE_OBJECT_TABLE, &objects));
    let final_size = bytes
        .len()
        .checked_add(20)
        .ok_or_else(|| CodecError::Malformed("3DM output size overflow".into()))?;
    bytes.extend(long_chunk(
        TCODE_ENDOFFILE,
        &(final_size as u64).to_le_bytes(),
    ));
    output.write_all(&bytes)?;
    Ok(())
}

fn check_representable(ir: &CadIr) -> Result<(), CodecError> {
    if ir.native.namespace("rhino").is_some() {
        return Err(CodecError::NotImplemented(
            "Rhino native records require explicit survival handling".into(),
        ));
    }
    let model = &ir.model;
    let unsupported = [
        ("subds", model.subds.len()),
        ("pcurves", model.pcurves.len()),
        ("procedural_surfaces", model.procedural_surfaces.len()),
        ("procedural_curves", model.procedural_curves.len()),
        ("features", model.features.len()),
        ("configurations", model.configurations.len()),
        ("parameters", model.parameters.len()),
        ("sketches", model.sketches.len()),
        ("sketch_entities", model.sketch_entities.len()),
        ("sketch_constraints", model.sketch_constraints.len()),
        ("appearances", model.appearances.len()),
        ("appearance_bindings", model.appearance_bindings.len()),
        ("attributes", model.attributes.len()),
    ]
    .into_iter()
    .filter(|(_, count)| *count != 0)
    .map(|(name, _)| name)
    .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "Rhino writer cannot yet represent arenas: {}",
            unsupported.join(", ")
        )));
    }
    if model.points.len() > i32::MAX as usize
        || model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::Malformed(
            "point arena exceeds native counts or contains non-finite coordinates".into(),
        ));
    }
    if model
        .bodies
        .iter()
        .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Sheet)
    {
        planar_sheet_brep_payload(ir)?.ok_or_else(|| {
            CodecError::NotImplemented("sheet topology is not a writable planar polygon".into())
        })?;
        return Ok(());
    } else {
        free_vertex_groups(ir)?;
    }
    for curve in &model.curves {
        let CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } = &curve.geometry
        else {
            if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
                check_nurbs_curve(&curve.id.0, nurbs)?;
                continue;
            }
            return Err(CodecError::NotImplemented(format!(
                "Rhino writer cannot represent curve {} as a native object",
                curve.id.0
            )));
        };
        let axis_norm = axis.norm();
        let reference_norm = ref_direction.norm();
        let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
        if !center.x.is_finite()
            || !center.y.is_finite()
            || !center.z.is_finite()
            || !radius.is_finite()
            || *radius <= 0.0
            || !axis_norm.is_finite()
            || !reference_norm.is_finite()
            || (axis_norm - 1.0).abs() > 1.0e-10
            || (reference_norm - 1.0).abs() > 1.0e-10
            || dot.abs() > 1.0e-10
        {
            return Err(CodecError::Malformed(format!(
                "curve {} has an invalid circle frame",
                curve.id.0
            )));
        }
    }
    for surface in &model.surfaces {
        match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                check_frame(&surface.id.0, *origin, *normal, *u_axis, "plane")?;
            }
            SurfaceGeometry::Nurbs(nurbs) => check_nurbs_surface(&surface.id.0, nurbs)?,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "Rhino writer cannot represent surface {} as a native object",
                    surface.id.0
                )))
            }
        }
    }
    for mesh in &model.tessellations {
        check_mesh(mesh)?;
    }
    Ok(())
}

fn planar_sheet_brep_payload(ir: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let model = &ir.model;
    let Some(body) = model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Sheet)
    else {
        return Ok(None);
    };
    let edge_count = model.coedges.len();
    if model.bodies.len() != 1
        || model.regions.len() != 1
        || model.shells.len() != 1
        || model.faces.len() != 1
        || model.loops.len() != 1
        || edge_count < 3
        || model.edges.len() != edge_count
        || model.vertices.len() != edge_count
        || model.points.len() != edge_count
        || model.curves.len() != edge_count
        || model.surfaces.len() != 1
        || !model.tessellations.is_empty()
    {
        return Err(CodecError::NotImplemented(
            "planar sheet writing currently requires one polygonal face".into(),
        ));
    }
    if body.regions.len() != 1
        || body.transform.is_some()
        || body.name.is_some()
        || body.color.is_some()
        || body.visible.is_some()
    {
        return Err(CodecError::NotImplemented(
            "planar sheet body display state or placement is not writable".into(),
        ));
    }
    let region = &model.regions[0];
    let shell = &model.shells[0];
    let face = &model.faces[0];
    let loop_ = &model.loops[0];
    if region.id != body.regions[0]
        || region.body != body.id
        || region.shells != [shell.id.clone()]
        || shell.region != region.id
        || shell.faces != [face.id.clone()]
        || !shell.wire_edges.is_empty()
        || !shell.free_vertices.is_empty()
        || face.shell != shell.id
        || face.loops != [loop_.id.clone()]
        || face.name.is_some()
        || face.color.is_some()
        || loop_.face != face.id
        || loop_.coedges.len() != edge_count
    {
        return Err(CodecError::Malformed(
            "planar sheet ownership graph is inconsistent".into(),
        ));
    }
    let surface = model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .ok_or_else(|| CodecError::Malformed("planar triangle surface is missing".into()))?;
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface.geometry
    else {
        return Err(CodecError::NotImplemented(
            "triangle sheet surface must be planar".into(),
        ));
    };
    if surface.source_object.is_some() {
        return Err(CodecError::NotImplemented(
            "sheet surface source-object state is not writable".into(),
        ));
    }
    check_frame(&surface.id.0, origin, normal, u_axis, "plane")?;
    let v_axis = cross(normal, u_axis);

    let mut ordered_coedges = Vec::with_capacity(edge_count);
    for id in &loop_.coedges {
        let coedge = model
            .coedges
            .iter()
            .find(|coedge| coedge.id == *id)
            .ok_or_else(|| CodecError::Malformed(format!("coedge {} is missing", id.0)))?;
        if coedge.owner_loop != loop_.id || coedge.pcurve.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} ownership or attributed pcurve is not writable",
                coedge.id.0
            )));
        }
        ordered_coedges.push(coedge);
    }
    for index in 0..edge_count {
        let current = ordered_coedges[index];
        if current.next != ordered_coedges[(index + 1) % edge_count].id
            || current.previous != ordered_coedges[(index + edge_count - 1) % edge_count].id
            || current.radial_next != current.id
        {
            return Err(CodecError::Malformed(format!(
                "coedge {} ring is inconsistent",
                current.id.0
            )));
        }
    }

    let mut ordered_edges = Vec::with_capacity(edge_count);
    let mut traversal_vertices = Vec::with_capacity(edge_count);
    for coedge in &ordered_coedges {
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .ok_or_else(|| CodecError::Malformed(format!("edge {} is missing", coedge.edge.0)))?;
        if ordered_edges
            .iter()
            .any(|existing: &&cadmpeg_ir::topology::Edge| existing.id == edge.id)
        {
            return Err(CodecError::NotImplemented(
                "planar sheet cannot reuse an edge in one loop".into(),
            ));
        }
        let from_id = if coedge.sense == Sense::Forward {
            &edge.start
        } else {
            &edge.end
        };
        traversal_vertices.push(from_id.clone());
        ordered_edges.push(edge);
    }
    for index in 0..edge_count {
        let edge = ordered_edges[index];
        let traversal_end = if ordered_coedges[index].sense == Sense::Forward {
            &edge.end
        } else {
            &edge.start
        };
        if *traversal_end != traversal_vertices[(index + 1) % edge_count] {
            return Err(CodecError::Malformed(
                "planar coedge traversal does not close".into(),
            ));
        }
    }

    let mut ordered_vertices = Vec::with_capacity(edge_count);
    let mut ordered_points = Vec::with_capacity(edge_count);
    for id in &traversal_vertices {
        let vertex = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *id)
            .ok_or_else(|| CodecError::Malformed(format!("vertex {} is missing", id.0)))?;
        if ordered_vertices
            .iter()
            .any(|existing: &&cadmpeg_ir::topology::Vertex| existing.id == vertex.id)
        {
            return Err(CodecError::Malformed(
                "planar loop has repeated traversal vertices".into(),
            ));
        }
        let point = model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| CodecError::Malformed(format!("point {} is missing", vertex.point.0)))?;
        ordered_vertices.push(vertex);
        ordered_points.push(point.position);
    }
    for edge in &ordered_edges {
        validate_planar_edge(model, edge, ir.tolerances.linear)?;
    }

    let mut payload = vec![0x32];
    let c2 = (0..edge_count)
        .map(|index| {
            let from = plane_uv(ordered_points[index], origin, u_axis, v_axis);
            let to = plane_uv(
                ordered_points[(index + 1) % edge_count],
                origin,
                u_axis,
                v_axis,
            );
            (LINE_CLASS, bounded_line_payload(from, to, [0.0, 1.0], 2))
        })
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(&c2));
    let c3 = ordered_edges
        .iter()
        .map(|edge| {
            let from = vertex_point(model, &edge.start).expect("validated edge start");
            let to = vertex_point(model, &edge.end).expect("validated edge end");
            (
                LINE_CLASS,
                bounded_line_payload([from.x, from.y, from.z], [to.x, to.y, to.z], [0.0, 1.0], 3),
            )
        })
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(&c3));
    payload.extend(polymorphic_array(&[(
        PLANE_SURFACE_CLASS,
        plane_surface_payload(origin, normal, u_axis),
    )]));

    let edge_index = ordered_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| (edge.id.0.clone(), index as i32))
        .collect::<std::collections::BTreeMap<_, _>>();
    let vertex_index = ordered_vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| (vertex.id.0.clone(), index as i32))
        .collect::<std::collections::BTreeMap<_, _>>();
    let vertex_records = ordered_vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let point = ordered_points[index];
            let incident = ordered_edges
                .iter()
                .filter(|edge| edge.start == vertex.id || edge.end == vertex.id)
                .map(|edge| edge_index[&edge.id.0])
                .collect::<Vec<_>>();
            let mut record = (index as i32).to_le_bytes().to_vec();
            for value in [point.x, point.y, point.z] {
                record.extend(value.to_le_bytes());
            }
            record.extend(indexes(&incident));
            record.extend(vertex.tolerance.unwrap_or(0.0).to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&vertex_records));
    let edge_records = ordered_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
            record.extend(vertex_index[&edge.start.0].to_le_bytes());
            record.extend(vertex_index[&edge.end.0].to_le_bytes());
            record.extend(indexes(&[index as i32]));
            record.extend(edge.tolerance.unwrap_or(0.0).to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&edge_records));
    let trim_records = ordered_coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge = ordered_edges[index];
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
            record.extend((index as i32).to_le_bytes());
            let (from, to) = if coedge.sense == Sense::Forward {
                (&edge.start, &edge.end)
            } else {
                (&edge.end, &edge.start)
            };
            record.extend(vertex_index[&from.0].to_le_bytes());
            record.extend(vertex_index[&to.0].to_le_bytes());
            record.extend(i32::from(coedge.sense == Sense::Reversed).to_le_bytes());
            record.extend(1_i32.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            record.extend([0_u8; 48]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&trim_records));
    let mut loop_record = 0_i32.to_le_bytes().to_vec();
    loop_record.extend(indexes(&(0..edge_count as i32).collect::<Vec<_>>()));
    loop_record.extend(1_i32.to_le_bytes());
    loop_record.extend(0_i32.to_le_bytes());
    payload.extend(raw_array(&[loop_record]));
    let mut face_record = 0_i32.to_le_bytes().to_vec();
    face_record.extend(indexes(&[0]));
    face_record.extend(0_i32.to_le_bytes());
    face_record.extend(i32::from(face.sense == Sense::Reversed).to_le_bytes());
    face_record.extend(0_i32.to_le_bytes());
    payload.extend(raw_array(&[face_record]));
    let min = ordered_points.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = ordered_points.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(crc_chunk(0x4000_8000, &[0, 0]));
    payload.extend(crc_chunk(0x4000_8000, &[0, 0]));
    payload.extend(3_i32.to_le_bytes());
    Ok(Some(payload))
}

fn validate_planar_edge(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
    document_tolerance: f64,
) -> Result<(), CodecError> {
    let curve_id = edge.curve.as_ref().ok_or_else(|| {
        CodecError::NotImplemented(format!("edge {} has no writable curve", edge.id.0))
    })?;
    let curve = model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .ok_or_else(|| CodecError::Malformed(format!("curve {} is missing", curve_id.0)))?;
    if curve.source_object.is_some() {
        return Err(CodecError::NotImplemented(format!(
            "edge curve {} source-object state is not writable",
            curve.id.0
        )));
    }
    let CurveGeometry::Line { origin, direction } = curve.geometry else {
        return Err(CodecError::NotImplemented(format!(
            "edge curve {} is not a line",
            curve.id.0
        )));
    };
    let [start_parameter, end_parameter] = edge.param_range.ok_or_else(|| {
        CodecError::NotImplemented(format!("edge {} has no parameter range", edge.id.0))
    })?;
    if !start_parameter.is_finite()
        || !end_parameter.is_finite()
        || start_parameter >= end_parameter
        || (direction.norm() - 1.0).abs() > 1.0e-10
    {
        return Err(CodecError::Malformed(format!(
            "edge {} has an invalid line parameterization",
            edge.id.0
        )));
    }
    let expected_start = cadmpeg_ir::math::Point3::new(
        origin.x + direction.x * start_parameter,
        origin.y + direction.y * start_parameter,
        origin.z + direction.z * start_parameter,
    );
    let expected_end = cadmpeg_ir::math::Point3::new(
        origin.x + direction.x * end_parameter,
        origin.y + direction.y * end_parameter,
        origin.z + direction.z * end_parameter,
    );
    let start = vertex_point(model, &edge.start)
        .ok_or_else(|| CodecError::Malformed(format!("edge {} start is missing", edge.id.0)))?;
    let end = vertex_point(model, &edge.end)
        .ok_or_else(|| CodecError::Malformed(format!("edge {} end is missing", edge.id.0)))?;
    let tolerance = edge.tolerance.unwrap_or(document_tolerance).max(1.0e-10);
    if !close_point(start, expected_start, tolerance) || !close_point(end, expected_end, tolerance)
    {
        return Err(CodecError::Malformed(format!(
            "edge {} endpoints disagree with its line curve",
            edge.id.0
        )));
    }
    Ok(())
}

fn close_point(
    left: cadmpeg_ir::math::Point3,
    right: cadmpeg_ir::math::Point3,
    tolerance: f64,
) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn vertex_point(
    model: &cadmpeg_ir::document::Model,
    vertex_id: &cadmpeg_ir::ids::VertexId,
) -> Option<cadmpeg_ir::math::Point3> {
    let vertex = model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)?;
    model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .map(|point| point.position)
}

fn plane_uv(
    point: cadmpeg_ir::math::Point3,
    origin: cadmpeg_ir::math::Point3,
    u: cadmpeg_ir::math::Vector3,
    v: cadmpeg_ir::math::Vector3,
) -> [f64; 3] {
    let delta = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    [
        delta[0] * u.x + delta[1] * u.y + delta[2] * u.z,
        delta[0] * v.x + delta[1] * v.y + delta[2] * v.z,
        0.0,
    ]
}

fn bounded_line_payload(from: [f64; 3], to: [f64; 3], domain: [f64; 2], dimension: i32) -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in from.into_iter().chain(to).chain(domain) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(dimension.to_le_bytes());
    payload
}

fn polymorphic_array(children: &[([u8; 16], Vec<u8>)]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((children.len() as i32).to_le_bytes());
    for (class, payload) in children {
        body.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper(*class, payload));
    }
    crc_chunk(0x4000_8000, &body)
}

fn raw_array(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((records.len() as i32).to_le_bytes());
    body.extend(records.concat());
    crc_chunk(0x4000_8000, &body)
}

fn indexes(values: &[i32]) -> Vec<u8> {
    let mut bytes = (values.len() as i32).to_le_bytes().to_vec();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    bytes
}

fn class_wrapper(class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body);
    let data = crc_chunk(TCODE_CLASS_DATA, payload);
    let end = short_chunk(TCODE_CLASS_END, 0);
    long_chunk(TCODE_CLASS_WRAPPER, &[uuid, data, end].concat())
}

fn check_mesh(mesh: &cadmpeg_ir::tessellation::Tessellation) -> Result<(), CodecError> {
    let vertex_count = mesh.vertices.len();
    if vertex_count == 0 || vertex_count > (1 << 24) || mesh.triangles.len() > (1 << 24) {
        return Err(CodecError::Malformed(format!(
            "mesh {} has invalid native counts",
            mesh.id
        )));
    }
    if mesh.body.is_some() || !mesh.strip_lengths.is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} uses body binding or strips not yet writable",
            mesh.id
        )));
    }
    if !mesh.normals.is_empty() && mesh.normals.len() != vertex_count {
        return Err(CodecError::Malformed(format!(
            "mesh {} normal count mismatch",
            mesh.id
        )));
    }
    if mesh.vertices.iter().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || !p.z.is_finite()
            || !(p.x as f32).is_finite()
            || !(p.y as f32).is_finite()
            || !(p.z as f32).is_finite()
    }) || mesh.normals.iter().any(|n| {
        !n.x.is_finite()
            || !n.y.is_finite()
            || !n.z.is_finite()
            || !(n.x as f32).is_finite()
            || !(n.y as f32).is_finite()
            || !(n.z as f32).is_finite()
    }) {
        return Err(CodecError::Malformed(format!(
            "mesh {} contains non-finite native values",
            mesh.id
        )));
    }
    if mesh
        .triangles
        .iter()
        .flatten()
        .any(|index| *index as usize >= vertex_count)
    {
        return Err(CodecError::Malformed(format!(
            "mesh {} index is out of range",
            mesh.id
        )));
    }
    let mut kinds = std::collections::BTreeSet::new();
    for channel in &mesh.channels {
        let expected = match channel.kind {
            CHANNEL_UV => 8,
            CHANNEL_COLOR => 4,
            CHANNEL_SURFACE_PARAMETERS | CHANNEL_CURVATURE => 16,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "mesh {} channel kind {:#x} is not writable",
                    mesh.id, channel.kind
                )))
            }
        };
        if !kinds.insert(channel.kind)
            || channel.flags != 0
            || channel.item_size != expected
            || channel.count as usize != vertex_count
            || channel.data.len() != vertex_count * expected as usize
        {
            return Err(CodecError::Malformed(format!(
                "mesh {} channel {:#x} has invalid metadata",
                mesh.id, channel.kind
            )));
        }
    }
    Ok(())
}

type PointGroups = (
    std::collections::BTreeSet<String>,
    Vec<Vec<cadmpeg_ir::math::Point3>>,
);

fn free_vertex_groups(ir: &CadIr) -> Result<PointGroups, CodecError> {
    use cadmpeg_ir::topology::BodyKind;

    let model = &ir.model;
    let mut regions = std::collections::BTreeSet::new();
    let mut shells = std::collections::BTreeSet::new();
    let mut vertices = std::collections::BTreeSet::new();
    let mut points = std::collections::BTreeSet::new();
    let mut groups = Vec::with_capacity(model.bodies.len());
    for body in &model.bodies {
        if body.kind != BodyKind::General
            || body.regions.len() != 1
            || body.transform.is_some()
            || body.name.is_some()
            || body.color.is_some()
            || body.visible.is_some()
        {
            return Err(CodecError::NotImplemented(format!(
                "body {} is not a plain free-vertex body",
                body.id.0
            )));
        }
        let region = model
            .regions
            .iter()
            .find(|region| region.id == body.regions[0])
            .ok_or_else(|| {
                CodecError::Malformed(format!("body {} region is missing", body.id.0))
            })?;
        if region.body != body.id
            || region.shells.len() != 1
            || !regions.insert(region.id.0.clone())
        {
            return Err(CodecError::Malformed(format!(
                "body {} region graph is invalid",
                body.id.0
            )));
        }
        let shell = model
            .shells
            .iter()
            .find(|shell| shell.id == region.shells[0])
            .ok_or_else(|| CodecError::Malformed(format!("body {} shell is missing", body.id.0)))?;
        if shell.region != region.id
            || !shell.faces.is_empty()
            || !shell.wire_edges.is_empty()
            || shell.free_vertices.is_empty()
            || !shells.insert(shell.id.0.clone())
        {
            return Err(CodecError::Malformed(format!(
                "body {} shell graph is invalid",
                body.id.0
            )));
        }
        let mut group = Vec::with_capacity(shell.free_vertices.len());
        for vertex_id in &shell.free_vertices {
            let vertex = model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("vertex {} is missing", vertex_id.0))
                })?;
            if vertex.tolerance.is_some() || !vertices.insert(vertex.id.0.clone()) {
                return Err(CodecError::NotImplemented(format!(
                    "vertex {} has tolerance or multiple ownership",
                    vertex.id.0
                )));
            }
            let point = model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("point {} is missing", vertex.point.0))
                })?;
            if !points.insert(point.id.0.clone()) {
                return Err(CodecError::NotImplemented(format!(
                    "point {} is shared by multiple free vertices",
                    point.id.0
                )));
            }
            group.push(point.position);
        }
        groups.push(group);
    }
    if regions.len() != model.regions.len()
        || shells.len() != model.shells.len()
        || vertices.len() != model.vertices.len()
    {
        return Err(CodecError::NotImplemented(
            "orphan region, shell, or vertex topology is not writable".into(),
        ));
    }
    Ok((points, groups))
}

fn check_frame(
    id: &str,
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    family: &str,
) -> Result<(), CodecError> {
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if !origin.x.is_finite()
        || !origin.y.is_finite()
        || !origin.z.is_finite()
        || (normal.norm() - 1.0).abs() > 1.0e-10
        || (x.norm() - 1.0).abs() > 1.0e-10
        || dot.abs() > 1.0e-10
    {
        return Err(CodecError::Malformed(format!(
            "{family} {id} has an invalid frame"
        )));
    }
    Ok(())
}

fn check_nurbs_surface(
    id: &str,
    surface: &cadmpeg_ir::geometry::NurbsSurface,
) -> Result<(), CodecError> {
    let u_order = surface.u_degree as usize + 1;
    let v_order = surface.v_degree as usize + 1;
    let u_count = surface.u_count as usize;
    let v_count = surface.v_count as usize;
    let pole_count = u_count.checked_mul(v_count);
    if u_order < 2
        || v_order < 2
        || u_count < u_order
        || v_count < v_order
        || i32::try_from(u_order).is_err()
        || i32::try_from(v_order).is_err()
        || i32::try_from(u_count).is_err()
        || i32::try_from(v_count).is_err()
        || surface.u_knots.len() != u_count + u_order
        || surface.v_knots.len() != v_count + v_order
        || pole_count != Some(surface.control_points.len())
    {
        return Err(CodecError::Malformed(format!(
            "surface {id} has inconsistent NURBS counts"
        )));
    }
    if surface
        .u_knots
        .iter()
        .chain(&surface.v_knots)
        .any(|v| !v.is_finite())
        || surface
            .u_knots
            .windows(2)
            .chain(surface.v_knots.windows(2))
            .any(|v| v[0] > v[1])
        || surface
            .control_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
        || surface.weights.as_ref().is_some_and(|w| {
            w.len() != surface.control_points.len() || w.iter().any(|v| !v.is_finite() || *v == 0.0)
        })
    {
        return Err(CodecError::Malformed(format!(
            "surface {id} has invalid NURBS data"
        )));
    }
    check_knot_roundtrip(
        id,
        "surface U",
        &surface.u_knots,
        u_order,
        u_count,
        surface.u_periodic,
    )?;
    check_knot_roundtrip(
        id,
        "surface V",
        &surface.v_knots,
        v_order,
        v_count,
        surface.v_periodic,
    )?;
    Ok(())
}

fn check_nurbs_curve(id: &str, curve: &cadmpeg_ir::geometry::NurbsCurve) -> Result<(), CodecError> {
    let order = curve.degree as usize + 1;
    let count = curve.control_points.len();
    if i32::try_from(order).is_err()
        || i32::try_from(count).is_err()
        || order < 2
        || count < order
        || curve.knots.len() != count + order
    {
        return Err(CodecError::Malformed(format!(
            "curve {id} has inconsistent NURBS counts"
        )));
    }
    if curve.knots.iter().any(|v| !v.is_finite())
        || curve.knots.windows(2).any(|v| v[0] > v[1])
        || curve
            .control_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
        || curve
            .weights
            .as_ref()
            .is_some_and(|w| w.len() != count || w.iter().any(|v| !v.is_finite() || *v == 0.0))
    {
        return Err(CodecError::Malformed(format!(
            "curve {id} has invalid NURBS data"
        )));
    }
    check_knot_roundtrip(id, "curve", &curve.knots, order, count, curve.periodic)?;
    Ok(())
}

fn check_knot_roundtrip(
    id: &str,
    direction: &str,
    full: &[f64],
    order: usize,
    count: usize,
    declared_periodic: bool,
) -> Result<(), CodecError> {
    let stored = &full[1..full.len() - 1];
    if stored[order - 2] >= stored[count - 1] {
        return Err(CodecError::Malformed(format!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed = crate::surfaces::reconstruct_knots(stored, order, count)
        .map_err(|error| CodecError::Malformed(format!("{direction} {id}: {error}")))?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(CodecError::Malformed(format!(
            "{direction} {id} knot endpoints or periodic flag are not native-canonical"
        )));
    }
    Ok(())
}

fn header(version: u64) -> Result<Vec<u8>, CodecError> {
    let text = version.to_string();
    if text.len() > 8 {
        return Err(CodecError::Malformed(
            "3DM archive version exceeds header field".into(),
        ));
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend(std::iter::repeat_n(b' ', 8 - text.len()));
    bytes.extend(text.bytes());
    Ok(bytes)
}

fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend((body.len() as i64).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(typecode, &payload)
}

fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

fn table(typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(TCODE_ENDOFTABLE, 0));
    long_chunk(typecode, &body)
}

fn units_record(linear: f64, angular: f64) -> Vec<u8> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes()); // millimeters
    body.extend(linear.to_le_bytes());
    body.extend(angular.to_le_bytes());
    body.extend(linear.to_le_bytes());
    crc_chunk(TCODE_UNITS_AND_TOLERANCES, &body)
}

fn point_cloud_payload(points: &[cadmpeg_ir::math::Point3]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend((points.len() as i32).to_le_bytes());
    for point in points {
        payload.extend(point.x.to_le_bytes());
        payload.extend(point.y.to_le_bytes());
        payload.extend(point.z.to_le_bytes());
    }
    for value in [
        0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = points.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = points.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(0_i32.to_le_bytes());
    payload
}

fn circle_payload(
    center: cadmpeg_ir::math::Point3,
    axis: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    radius: f64,
) -> Vec<u8> {
    let y = cadmpeg_ir::math::Vector3::new(
        axis.y * x.z - axis.z * x.y,
        axis.z * x.x - axis.x * x.z,
        axis.x * x.y - axis.y * x.x,
    );
    let equation_d = -(axis.x * center.x + axis.y * center.y + axis.z * center.z);
    let mut payload = vec![0x10];
    for value in [
        center.x,
        center.y,
        center.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        axis.x,
        axis.y,
        axis.z,
        axis.x,
        axis.y,
        axis.z,
        equation_d,
        radius,
        center.x + radius * x.x,
        center.y + radius * x.y,
        center.z + radius * x.z,
        center.x + radius * y.x,
        center.y + radius * y.y,
        center.z + radius * y.z,
        center.x - radius * x.x,
        center.y - radius * x.y,
        center.z - radius * x.z,
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::TAU,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    payload
}

fn nurbs_curve_payload(curve: &cadmpeg_ir::geometry::NurbsCurve) -> Vec<u8> {
    let rational = i32::from(curve.weights.is_some());
    let order = (curve.degree + 1) as i32;
    let count = curve.control_points.len() as i32;
    let mut payload = vec![0x10];
    for value in [3, rational, order, count, 0, 0] {
        payload.extend(value.to_le_bytes());
    }
    let min = curve
        .control_points
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = curve
        .control_points
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(((curve.knots.len() - 2) as i32).to_le_bytes());
    for knot in &curve.knots[1..curve.knots.len() - 1] {
        payload.extend(knot.to_le_bytes());
    }
    payload.extend(count.to_le_bytes());
    for (index, point) in curve.control_points.iter().enumerate() {
        let weight = curve.weights.as_ref().map_or(1.0, |weights| weights[index]);
        payload.extend((point.x * weight).to_le_bytes());
        payload.extend((point.y * weight).to_le_bytes());
        payload.extend((point.z * weight).to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    payload
}

fn plane_surface_payload(
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
) -> Vec<u8> {
    let y = cross(normal, x);
    let d = -(normal.x * origin.x + normal.y * origin.y + normal.z * origin.z);
    let mut payload = vec![0x10];
    for value in [
        origin.x, origin.y, origin.z, x.x, x.y, x.z, y.x, y.y, y.z, normal.x, normal.y, normal.z,
        normal.x, normal.y, normal.z, d, -1.0, 1.0, -1.0, 1.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload
}

fn nurbs_surface_payload(surface: &cadmpeg_ir::geometry::NurbsSurface) -> Vec<u8> {
    let rational = i32::from(surface.weights.is_some());
    let mut payload = vec![0x10];
    for value in [
        3,
        rational,
        (surface.u_degree + 1) as i32,
        (surface.v_degree + 1) as i32,
        surface.u_count as i32,
        surface.v_count as i32,
        0,
        0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = surface
        .control_points
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = surface
        .control_points
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    for knots in [&surface.u_knots, &surface.v_knots] {
        payload.extend(((knots.len() - 2) as i32).to_le_bytes());
        for knot in &knots[1..knots.len() - 1] {
            payload.extend(knot.to_le_bytes());
        }
    }
    payload.extend((surface.control_points.len() as i32).to_le_bytes());
    for (index, point) in surface.control_points.iter().enumerate() {
        let weight = surface
            .weights
            .as_ref()
            .map_or(1.0, |weights| weights[index]);
        payload.extend((point.x * weight).to_le_bytes());
        payload.extend((point.y * weight).to_le_bytes());
        payload.extend((point.z * weight).to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    payload
}

fn cross(a: cadmpeg_ir::math::Vector3, b: cadmpeg_ir::math::Vector3) -> cadmpeg_ir::math::Vector3 {
    cadmpeg_ir::math::Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn mesh_payload(mesh: &cadmpeg_ir::tessellation::Tessellation, archive_version: u64) -> Vec<u8> {
    let minor = if archive_version == 50 { 5_u8 } else { 7_u8 };
    let mut payload = vec![0x30 | minor];
    payload.extend((mesh.vertices.len() as i32).to_le_bytes());
    payload.extend((mesh.triangles.len() as i32).to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0_u8; 16]);
    payload.extend([0_u8; 16 * 4]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 5]);

    let width = if mesh.vertices.len() < 256 {
        1_i32
    } else if mesh.vertices.len() < 65_536 {
        2_i32
    } else {
        4_i32
    };
    payload.extend(width.to_le_bytes());
    for triangle in &mesh.triangles {
        for index in [triangle[0], triangle[1], triangle[2], triangle[2]] {
            match width {
                1 => payload.push(index as u8),
                2 => payload.extend((index as u16).to_le_bytes()),
                4 => payload.extend(index.to_le_bytes()),
                _ => unreachable!(),
            }
        }
    }

    let float_vertices = mesh
        .vertices
        .iter()
        .flat_map(|point| {
            [point.x as f32, point.y as f32, point.z as f32]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let normals = mesh
        .normals
        .iter()
        .flat_map(|normal| {
            [normal.x as f32, normal.y as f32, normal.z as f32]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    for data in [
        &float_vertices[..],
        &normals[..],
        mesh_channel(mesh, CHANNEL_UV),
        mesh_channel(mesh, CHANNEL_CURVATURE),
        mesh_channel(mesh, CHANNEL_COLOR),
    ] {
        payload.extend(mesh_buffer(data));
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 16]);
    payload.extend(mesh_buffer(mesh_channel(mesh, CHANNEL_SURFACE_PARAMETERS)));
    payload.extend([0_u8; 3]);
    if minor >= 6 {
        payload.push(0);
    }
    if minor >= 7 {
        payload.push(1);
        let doubles = mesh
            .vertices
            .iter()
            .flat_map(|point| {
                [point.x, point.y, point.z]
                    .into_iter()
                    .flat_map(f64::to_le_bytes)
            })
            .collect::<Vec<_>>();
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend((mesh.vertices.len() as u32).to_le_bytes());
        body.extend(mesh_buffer(&doubles));
        payload.extend(crc_chunk(0x4000_8000, &body));
    }
    payload
}

fn mesh_channel(mesh: &cadmpeg_ir::tessellation::Tessellation, kind: u32) -> &[u8] {
    mesh.channels
        .iter()
        .find(|channel| channel.kind == kind)
        .map_or(&[], |channel| channel.data.as_slice())
}

fn mesh_buffer(data: &[u8]) -> Vec<u8> {
    let mut result = (data.len() as u32).to_le_bytes().to_vec();
    if !data.is_empty() {
        result.extend(crc32fast::hash(data).to_le_bytes());
        result.push(0);
        result.extend(data);
    }
    result
}

fn object_record(object_type: i64, class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let object_type = short_chunk(TCODE_OBJECT_RECORD_TYPE, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body);
    let class_data = crc_chunk(TCODE_CLASS_DATA, payload);
    let class_end = short_chunk(TCODE_CLASS_END, 0);
    let class = long_chunk(TCODE_CLASS_WRAPPER, &[uuid, class_data, class_end].concat());
    let object_end = short_chunk(TCODE_OBJECT_RECORD_END, 0);
    crc_chunk(
        TCODE_OBJECT_RECORD,
        &[object_type, class, object_end].concat(),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;
    use cadmpeg_ir::units::Units;

    use super::{CHANNEL_COLOR, CHANNEL_CURVATURE, CHANNEL_SURFACE_PARAMETERS, CHANNEL_UV};
    use crate::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};

    #[test]
    fn source_less_points_round_trip_across_target_versions() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: PointId("point:a".into()),
            position: Point3::new(1.25, -2.5, 3.75),
        });

        for (version, value) in [
            (RhinoArchiveVersion::V5, "50"),
            (RhinoArchiveVersion::V6, "60"),
            (RhinoArchiveVersion::V7, "70"),
            (RhinoArchiveVersion::V8, "80"),
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            assert_eq!(std::str::from_utf8(&bytes[24..32]).unwrap().trim(), value);
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.points.len(), 1);
            assert_eq!(
                decoded.ir.model.points[0].position,
                Point3::new(1.25, -2.5, 3.75)
            );
        }
    }

    #[test]
    fn rejection_occurs_before_output() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:a".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Degenerate {
                point: Point3::new(0.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let mut output = vec![0xaa];
        assert!(RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .is_err());
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn source_less_circle_round_trips_with_its_frame() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:circle".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Circle {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 4.0,
            },
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.curves.len(), 1);
        assert_eq!(
            decoded.ir.model.curves[0].geometry,
            ir.model.curves[0].geometry
        );
    }

    #[test]
    fn rational_nurbs_curve_round_trips_homogeneous_poles() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:nurbs".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsCurve {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 2.0, 0.0),
                        Point3::new(3.0, 0.0, 0.0),
                    ],
                    weights: Some(vec![1.0, 0.5, 1.0]),
                    periodic: false,
                },
            ),
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            decoded.ir.model.curves[0].geometry,
            ir.model.curves[0].geometry
        );
    }

    #[test]
    fn free_plane_and_rational_nurbs_surface_round_trip() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:plane".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        });
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:nurbs".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsSurface {
                    u_degree: 1,
                    v_degree: 1,
                    u_knots: vec![0.0, 0.0, 1.0, 1.0],
                    v_knots: vec![2.0, 2.0, 5.0, 5.0],
                    u_count: 2,
                    v_count: 2,
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.0, 2.0, 0.0),
                        Point3::new(3.0, 0.0, 1.0),
                        Point3::new(3.0, 2.0, 1.0),
                    ],
                    weights: Some(vec![1.0, 0.75, 0.5, 1.0]),
                    u_periodic: false,
                    v_periodic: false,
                },
            ),
            source_object: None,
        });
        ir.finalize();
        let expected = ir
            .model
            .surfaces
            .iter()
            .map(|s| s.geometry.clone())
            .collect::<Vec<_>>();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let actual = decoded
                .ir
                .model
                .surfaces
                .iter()
                .map(|s| s.geometry.clone())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn standalone_mesh_round_trips_across_archive_versions() {
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#mesh".into(),
                body: None,
                source_object: None,
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                    Point3::new(0.0, 3.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: vec![cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0); 3],
                channels: Vec::new(),
            });
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.tessellations.len(), 1);
            let actual = &decoded.ir.model.tessellations[0];
            assert_eq!(actual.vertices, ir.model.tessellations[0].vertices);
            assert_eq!(actual.triangles, ir.model.tessellations[0].triangles);
            assert_eq!(actual.normals, ir.model.tessellations[0].normals);
        }
    }

    #[test]
    fn mesh_precision_is_target_specific_and_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#precision".into(),
                body: None,
                source_object: None,
                vertices: vec![
                    Point3::new(0.1, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                channels: Vec::new(),
            });
        let mut v5 = Vec::new();
        let v5_report = RhinoEncoder::new(RhinoArchiveVersion::V5)
            .encode(&ir, &mut v5)
            .unwrap();
        assert_eq!(v5_report.losses.len(), 1);
        let mut v8 = Vec::new();
        let v8_report = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut v8)
            .unwrap();
        assert!(v8_report.losses.is_empty());
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(v8), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.tessellations[0].vertices[0].x, 0.1);
    }

    #[test]
    fn mesh_auxiliary_channels_round_trip_by_kind() {
        let mut ir = CadIr::empty(Units::default());
        let vertices = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let channels = [
            (CHANNEL_UV, 8_u32, vec![0_u8; 24]),
            (CHANNEL_COLOR, 4, vec![0x7f; 12]),
            (CHANNEL_SURFACE_PARAMETERS, 16, vec![0x11; 48]),
            (CHANNEL_CURVATURE, 16, vec![0x22; 48]),
        ]
        .into_iter()
        .map(
            |(kind, item_size, data)| cadmpeg_ir::tessellation::TessellationChannel {
                item_size,
                kind,
                flags: 0,
                count: 3,
                data,
            },
        )
        .collect::<Vec<_>>();
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#channels".into(),
                body: None,
                source_object: None,
                vertices,
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                channels: channels.clone(),
            });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let actual = &decoded.ir.model.tessellations[0].channels;
        for expected in channels {
            assert_eq!(
                actual.iter().find(|channel| channel.kind == expected.kind),
                Some(&expected)
            );
        }
    }

    #[test]
    fn free_vertex_body_preserves_point_cloud_grouping() {
        let mut ir = CadIr::empty(Units::default());
        let body_id: cadmpeg_ir::ids::BodyId = "cadir:model:body#cloud".into();
        let region_id: cadmpeg_ir::ids::RegionId = "cadir:model:region#cloud".into();
        let shell_id: cadmpeg_ir::ids::ShellId = "cadir:model:shell#cloud".into();
        let vertex_ids = [
            cadmpeg_ir::ids::VertexId("cadir:model:vertex#cloud.0".into()),
            cadmpeg_ir::ids::VertexId("cadir:model:vertex#cloud.1".into()),
        ];
        let point_ids = [
            cadmpeg_ir::ids::PointId("cadir:model:point#cloud.0".into()),
            cadmpeg_ir::ids::PointId("cadir:model:point#cloud.1".into()),
        ];
        ir.model.bodies.push(cadmpeg_ir::topology::Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::General,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(cadmpeg_ir::topology::Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(cadmpeg_ir::topology::Shell {
            id: shell_id,
            region: region_id,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: vertex_ids.to_vec(),
        });
        for (index, (vertex, point)) in vertex_ids.into_iter().zip(point_ids).enumerate() {
            ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
                id: vertex,
                point: point.clone(),
                tolerance: None,
            });
            ir.model.points.push(cadmpeg_ir::topology::Point {
                id: point,
                position: Point3::new(index as f64, index as f64 + 2.0, 3.0),
            });
        }
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.bodies.len(), 1);
        assert_eq!(decoded.ir.model.vertices.len(), 2);
        assert_eq!(decoded.ir.model.points.len(), 2);
    }

    #[test]
    fn retained_native_records_are_refused_before_output() {
        let mut source = CadIr::empty(Units::default());
        source.model.points.push(Point {
            id: PointId("cadir:model:point#retained".into()),
            position: Point3::new(1.0, 2.0, 3.0),
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&source, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(decoded.ir.native.namespace("rhino").is_some());

        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&decoded.ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("survival handling"));
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn noncanonical_nurbs_periodicity_is_rejected_atomically() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("cadir:model:curve#periodic".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsCurve {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                        Point3::new(2.0, 0.0, 0.0),
                    ],
                    weights: None,
                    periodic: true,
                },
            ),
            source_object: None,
        });
        let mut output = vec![0xaa];
        assert!(RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .is_err());
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn planar_triangle_sheet_round_trips_connected_topology() {
        let ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        assert_planar_sheet_round_trip(&ir, 3);
    }

    #[test]
    fn planar_quad_sheet_round_trips_connected_topology() {
        let ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        assert_planar_sheet_round_trip(&ir, 4);
    }

    fn assert_planar_sheet_round_trip(ir: &CadIr, edge_count: usize) {
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.faces.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.loops.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.coedges.len(), edge_count, "{version:?}");
            assert_eq!(decoded.ir.model.edges.len(), edge_count, "{version:?}");
            assert_eq!(decoded.ir.model.vertices.len(), edge_count, "{version:?}");
            assert!(
                cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok(),
                "{version:?}"
            );
        }
    }

    fn polygon_sheet(points: &[Point3]) -> CadIr {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::*;
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::*;

        let mut ir = CadIr::empty(Units::default());
        let body: BodyId = "cadir:model:body#polygon".into();
        let region: RegionId = "cadir:model:region#polygon".into();
        let shell: ShellId = "cadir:model:shell#polygon".into();
        let face: FaceId = "cadir:model:face#polygon".into();
        let loop_id: LoopId = "cadir:model:loop#polygon".into();
        let surface: SurfaceId = "cadir:model:surface#polygon".into();
        let point_ids = (0..points.len())
            .map(|index| PointId(format!("cadir:model:point#polygon.{index}")))
            .collect::<Vec<_>>();
        let vertex_ids = (0..points.len())
            .map(|index| VertexId(format!("cadir:model:vertex#polygon.{index}")))
            .collect::<Vec<_>>();
        let edge_ids = (0..points.len())
            .map(|index| EdgeId(format!("cadir:model:edge#polygon.{index}")))
            .collect::<Vec<_>>();
        let curve_ids = (0..points.len())
            .map(|index| CurveId(format!("cadir:model:curve#polygon.{index}")))
            .collect::<Vec<_>>();
        let coedge_ids = (0..points.len())
            .map(|index| CoedgeId(format!("cadir:model:coedge#polygon.{index}")))
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Sheet,
            regions: vec![region.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell.clone(),
            region,
            faces: vec![face.clone()],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.faces.push(Face {
            id: face.clone(),
            shell,
            surface: surface.clone(),
            sense: Sense::Forward,
            loops: vec![loop_id.clone()],
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face,
            coedges: coedge_ids.to_vec(),
        });
        ir.model.surfaces.push(Surface {
            id: surface,
            geometry: SurfaceGeometry::Plane {
                origin: points[0],
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        for index in 0..points.len() {
            let next = points[(index + 1) % points.len()];
            let delta = Vector3::new(
                next.x - points[index].x,
                next.y - points[index].y,
                next.z - points[index].z,
            );
            let length = delta.norm();
            let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
            ir.model.points.push(Point {
                id: point_ids[index].clone(),
                position: points[index],
            });
            ir.model.vertices.push(Vertex {
                id: vertex_ids[index].clone(),
                point: point_ids[index].clone(),
                tolerance: None,
            });
            ir.model.curves.push(Curve {
                id: curve_ids[index].clone(),
                geometry: CurveGeometry::Line {
                    origin: points[index],
                    direction,
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_ids[index].clone(),
                curve: Some(curve_ids[index].clone()),
                start: vertex_ids[index].clone(),
                end: vertex_ids[(index + 1) % points.len()].clone(),
                param_range: Some([0.0, length]),
                tolerance: None,
            });
            ir.model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_id.clone(),
                edge: edge_ids[index].clone(),
                next: coedge_ids[(index + 1) % points.len()].clone(),
                previous: coedge_ids[(index + points.len() - 1) % points.len()].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: Sense::Forward,
                pcurve: None,
            });
        }
        ir.finalize();
        ir
    }
}
