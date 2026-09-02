// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::units::Units;
use sha2::{Digest, Sha256};

use super::*;
use crate::layout::file_header;
use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn source_less_points_round_trip_across_target_versions() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("point:a".into()),
        position: Point3::new(1.25, -2.5, 3.75),
        source_object: None,
    });

    for (version, value) in [
        (RhinoArchiveVersion::V5, "50"),
        (RhinoArchiveVersion::V6, "60"),
        (RhinoArchiveVersion::V7, "70"),
        (RhinoArchiveVersion::V8, "80"),
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        assert_eq!(
            std::str::from_utf8(&bytes[file_header::ARCHIVE_VERSION..file_header::LEN])
                .expect("required invariant")
                .trim(),
            value
        );
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.points.len(), 1);
        assert_eq!(
            decoded.ir().model.points[0].position,
            Point3::new(1.25, -2.5, 3.75)
        );
    }
}

#[test]
fn coarse_absolute_tolerance_writes_valid_independent_relative_tolerance() {
    let mut ir = CadIr::empty(Units::default());
    ir.tolerances.linear = 2.0;
    ir.model.points.push(Point {
        id: PointId("point:coarse-tolerance".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });

    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("coarse absolute tolerance is writable");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("generated settings record remains valid");

    assert_eq!(decoded.ir().tolerances.linear, 2.0);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("relative tolerance")));
}

#[test]
fn invalid_archive_tolerances_are_rejected_before_output() {
    for (linear, angular) in [
        (0.0, 1.0e-10),
        (f64::INFINITY, 1.0e-10),
        (1.0e-6, 0.0),
        (1.0e-6, std::f64::consts::PI.next_up()),
    ] {
        let mut ir = CadIr::empty(Units::default());
        ir.tolerances.linear = linear;
        ir.tolerances.angular = angular;
        let mut output = vec![0xaa];
        let error = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut output))
            .expect_err("invalid tolerance must not be serialized");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert_eq!(output, [0xaa]);
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
    assert!(RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str())
        )
        .and_then(|plan| plan.write_to(&mut output))
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
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert_eq!(
        decoded.ir().model.curves[0].geometry,
        ir.model.curves[0].geometry
    );
    let digest = Sha256::digest(b"curve:circle");
    let expected =
        crate::wire::Uuid::from_wire(digest[..16].try_into().expect("required invariant"))
            .to_string();
    assert_eq!(
        decoded.ir().model.curves[0]
            .source_object
            .as_ref()
            .expect("generated object identity")
            .object_id,
        expected
    );
}

#[test]
fn rational_nurbs_curve_round_trips_homogeneous_poles() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: cadmpeg_ir::ids::CurveId("curve:nurbs".into()),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        }),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(
        decoded.ir().model.curves[0].geometry,
        ir.model.curves[0].geometry
    );
}

#[test]
fn reversed_unclamped_nurbs_knots_are_native_canonical() {
    let mut curve = cadmpeg_ir::geometry::NurbsCurve {
        degree: 2,
        knots: vec![-3.0, 0.0, 1.0, 5.0, 8.0, 9.0, 10.0, 11.0, 14.0],
        control_points: (0..6)
            .map(|index| Point3::new(f64::from(index), 0.0, 0.0))
            .collect(),
        weights: None,
        periodic: false,
    };
    super::canonicalize_native_curve_knots(&mut curve, "reversed")
        .expect("reflected stored knots reconstruct");

    assert_eq!(
        curve.knots,
        [-1.0, 0.0, 1.0, 5.0, 8.0, 9.0, 10.0, 11.0, 12.0]
    );
    super::check_knot_roundtrip("reversed", "curve", &curve.knots, 3, 6, curve.periodic)
        .expect("canonicalized knots serialize without another change");
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
                normal_reversed: false,
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
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let actual = decoded
            .ir()
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
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(0.0, 3.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: vec![cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0); 3],
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .all(|loss| !loss.message.contains("CRC mismatch")),
            "{version:?}: {:?}",
            decoded.report().losses
        );
        assert_eq!(decoded.ir().model.tessellations.len(), 1);
        let actual = &decoded.ir().model.tessellations[0];
        assert_eq!(actual.vertices, ir.model.tessellations[0].vertices);
        assert_eq!(actual.triangles, ir.model.tessellations[0].triangles);
        assert_eq!(actual.normals, ir.model.tessellations[0].normals);
    }

    ir.model.tessellations[0].triangle_groups.push(
        cadmpeg_ir::tessellation::TessellationTriangleGroup {
            source_id: Some("synthetic:test:group#0".into()),
            triangles: vec![0],
        },
    );
    assert!(matches!(
        RhinoCodec.plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str())
        ),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn mesh_precision_is_target_specific_and_reported() {
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "cadir:model:tessellation#precision".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.1, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });
    let mut v5 = Vec::new();
    let v5_report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V5.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut v5))
        .expect("required invariant");
    assert_eq!(v5_report.losses.len(), 1);
    let decoded_v5 = RhinoCodec
        .decode(&mut Cursor::new(v5), &DecodeOptions::default())
        .expect("required invariant");
    assert_ne!(decoded_v5.ir().model.tessellations[0].vertices[0].x, 0.1);
    let mut v8 = Vec::new();
    let v8_report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut v8))
        .expect("required invariant");
    assert!(v8_report.losses.is_empty());
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(v8), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(decoded.ir().model.tessellations[0].vertices[0].x, 0.1);
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
            domain: cadmpeg_ir::tessellation::TessellationChannelDomain::default(),
            item_size,
            kind,
            flags: 0,
            count: 3,
            data,
            indices: Vec::new(),
        },
    )
    .collect::<Vec<_>>();
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "cadir:model:tessellation#channels".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices,
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: channels.clone(),
        });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let actual = &decoded.ir().model.tessellations[0].channels;
    for expected in channels {
        assert_eq!(
            actual.iter().find(|channel| channel.kind == expected.kind),
            Some(&expected)
        );
    }
}

#[test]
fn mesh_channel_bytes_cannot_impersonate_nested_chunk_framing() {
    let mut ir = CadIr::empty(Units::default());
    let mut uv_data = vec![0_u8; 24];
    uv_data[..4].copy_from_slice(&0x4000_8000_u32.to_le_bytes());
    uv_data[4..12].copy_from_slice(&160_i64.to_le_bytes());
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "cadir:model:tessellation#chunk-like-channel".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: vec![cadmpeg_ir::tessellation::TessellationChannel {
                domain: cadmpeg_ir::tessellation::TessellationChannelDomain::default(),
                kind: CHANNEL_UV,
                item_size: 8,
                flags: 0,
                count: 3,
                data: uv_data.clone(),
                indices: Vec::new(),
            }],
        });

    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("channel bytes are opaque to chunk framing");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("generated mesh remains decodable");
    assert_eq!(
        decoded.ir().model.tessellations[0].channels[0].data,
        uv_data
    );
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
        name: Some("survey points".into()),
        color: Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 128.0 / 255.0,
            a: 1.0,
        }),
        visible: Some(false),
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
            source_object: None,
        });
    }
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 2);
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(
        decoded.ir().model.bodies[0].name.as_deref(),
        Some("survey points")
    );
    assert_eq!(decoded.ir().model.bodies[0].color, ir.model.bodies[0].color);
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));
}

#[test]
fn supported_decoded_geometry_can_be_edited_and_rewritten() {
    let mut source = CadIr::empty(Units::default());
    source.model.points.push(Point {
        id: PointId("cadir:model:point#retained".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(decoded.ir().native.namespace("rhino").is_some());
    decoded.ir_mut().model.points[0].position = Point3::new(4.0, 5.0, 6.0);

    let mut output = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect("required invariant");
    let rewritten = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(
        rewritten.ir().model.points[0].position,
        Point3::new(4.0, 5.0, 6.0)
    );
}

#[test]
fn unsupported_retained_native_records_are_refused_before_output() {
    let mut source = CadIr::empty(Units::default());
    source.model.points.push(Point {
        id: PointId("cadir:model:point#retained".into()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&source, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("required invariant");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("required invariant");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded
        .ir_mut()
        .native
        .namespace_mut("rhino")
        .arenas
        .entry("materials".into())
        .or_default()
        .push(cadmpeg_ir::NativeRecord::new(
            "rhino:presentation:material#unsupported",
            serde_json::Map::new(),
        ));

    let mut output = vec![0xaa];
    let error = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(error.to_string().contains("survival handling"));
    assert_eq!(output, [0xaa]);
}

#[test]
fn noncanonical_nurbs_periodicity_is_rejected_atomically() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: cadmpeg_ir::ids::CurveId("cadir:model:curve#periodic".into()),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: true,
        }),
        source_object: None,
    });
    let mut output = vec![0xaa];
    assert!(RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str())
        )
        .and_then(|plan| plan.write_to(&mut output))
        .is_err());
    assert_eq!(output, [0xaa]);
}
