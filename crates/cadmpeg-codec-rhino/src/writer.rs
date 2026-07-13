// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use std::io::Write;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;

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
        ("surfaces", model.surfaces.len()),
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
        ("tessellations", model.tessellations.len()),
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
    if ir.native.namespace("rhino").is_some() {
        return Err(CodecError::NotImplemented(
            "Rhino native records require explicit survival handling".into(),
        ));
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
}
