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

    let mut objects = ir
        .model
        .points
        .iter()
        .map(|point| {
            let position = point.position;
            let mut payload = vec![0x10];
            payload.extend(position.x.to_le_bytes());
            payload.extend(position.y.to_le_bytes());
            payload.extend(position.z.to_le_bytes());
            object_record(1, POINT_CLASS, &payload)
        })
        .collect::<Vec<_>>();
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
    let model = &ir.model;
    let unsupported = [
        ("bodies", model.bodies.len()),
        ("regions", model.regions.len()),
        ("shells", model.shells.len()),
        ("faces", model.faces.len()),
        ("loops", model.loops.len()),
        ("coedges", model.coedges.len()),
        ("edges", model.edges.len()),
        ("vertices", model.vertices.len()),
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
    if ir.native.namespace("rhino").is_some() {
        return Err(CodecError::NotImplemented(
            "Rhino native records require explicit survival handling".into(),
        ));
    }
    Ok(())
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
}
