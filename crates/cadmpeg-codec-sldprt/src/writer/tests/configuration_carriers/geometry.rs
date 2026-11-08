// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn periodic_nurbs_surface_descriptor_carries_each_axis_flag() {
    use cadmpeg_core::decode::View;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::math::Point3;
    let point = |x, y| FinitePoint3::new(Point3::new(x, y, 0.0)).unwrap();
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], true),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![point(0.0, 0.0), point(0.0, 1.0)],
                vec![point(1.0, 0.0), point(1.0, 1.0)],
            ],
        },
        false,
    )
    .unwrap();
    let mut bytes = Vec::new();
    crate::writer::write_nurbs_surface(&mut bytes, 10, &surface, &mut 20, 0.001).unwrap();
    let descriptor = bytes.windows(2).position(|tag| tag == [0, 0x7e]).unwrap() + 2;
    assert_eq!(
        View::u16_be_at(
            &bytes,
            descriptor + crate::layout::bspline_surface_descriptor::U_PERIODIC
        ),
        Some(0x0100)
    );
}

#[test]
fn rational_nurbs_curve_descriptor_carries_its_flag_and_knot_count() {
    use cadmpeg_core::decode::View;
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::math::Point3;

    let poles = NurbsPoles3::from_lanes(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
    )
    .unwrap();
    let curve = NurbsCurve::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], poles, false).unwrap();
    let mut bytes = Vec::new();
    crate::writer::write_nurbs_curve(&mut bytes, 10, &curve, &mut 20, 0.001).unwrap();
    let descriptor = bytes.windows(2).position(|tag| tag == [0, 0x88]).unwrap() + 2;
    let flags = View::u16_be_at(
        &bytes,
        descriptor + crate::layout::bspline_curve_descriptor::RATIONAL,
    )
    .unwrap();
    assert_eq!(flags >> 8, 1);
    assert_eq!(
        View::u32_be_at(
            &bytes,
            descriptor + crate::layout::bspline_curve_descriptor::KNOT_COUNT
        ),
        Some(3)
    );
}

#[test]
fn encoder_bakes_rigid_body_transform() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let original_point = ir.model.points[0].position().get();
    let original_normal = ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
                if {
                    let normal = *plane_surface.frame().axis().as_raw();
                    normal.x == 1.0
                } =>
            {
                let normal = *plane_surface.frame().axis().as_raw();
                Some(normal)
            }
            _ => None,
        })
        .unwrap();
    ir.model.bodies[0].transform = Some(
        Transform::affine([
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
        ])
        .expect("affine transform"),
    );
    let expected_point = Point3::new(
        -original_point.y + 10.0,
        original_point.x + 20.0,
        original_point.z + 30.0,
    );
    let expected_normal = Vector3::new(-original_normal.y, original_normal.x, original_normal.z);

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir().model.points.iter().any(|point| {
        (point.position().get().x - expected_point.x).abs() < 1.0e-9
            && (point.position().get().y - expected_point.y).abs() < 1.0e-9
            && (point.position().get().z - expected_point.z).abs() < 1.0e-9
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
        if {
            let normal = plane_surface.frame().axis().as_raw();
            *normal == expected_normal
        })
    }));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
}

#[test]
fn semantic_writer_regenerates_modified_planar_brep() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let mut result = EditableDecodeResult::from(result);
    let moved = result.ir_mut().model.points[0].position().get();
    result.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        result.ir(),
        result.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let mut regenerated = Cursor::new(encoded);
    let decoded = SldprtCodec
        .decode(&mut regenerated, &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get().x == 1.0));
}

#[test]
fn semantic_writer_uses_schema_specific_face_families() {
    let solid = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut solid = EditableDecodeResult::from(solid);
    let moved = solid.ir_mut().model.points[0].position().get();
    solid.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut solid_bytes = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        solid.ir(),
        solid.source_fidelity(),
        &mut solid_bytes,
    )
    .unwrap();
    let solid_scan = crate::test_support::container::scan(&solid_bytes);
    let solid_payload = &solid_scan.blocks[0].payload;
    assert!(count_entity51_family(solid_payload, 2, 0x0013) >= 1);
    assert!(count_entity51_family(solid_payload, 1, 0x0015) >= 1);

    let sheet_body = owned_triangle_with_kind(0, 701, 0.0, 3);
    let sheet = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sheet_body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut sheet = EditableDecodeResult::from(sheet);
    let moved = sheet.ir_mut().model.points[0].position().get();
    sheet.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut sheet_bytes = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        sheet.ir(),
        sheet.source_fidelity(),
        &mut sheet_bytes,
    )
    .unwrap();
    let sheet_scan = crate::test_support::container::scan(&sheet_bytes);
    let sheet_payload = &sheet_scan.blocks[0].payload;
    assert!(count_entity51_family(sheet_payload, 2, 0x0015) >= 1);
    assert!(count_entity51_family(sheet_payload, 1, 0x001f) >= 1);
}

#[test]
fn semantic_writer_preserves_outer_header() {
    let mut source = sldprt_with_body(&triangle_body());
    source[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    source[4..8].copy_from_slice(&7u32.to_be_bytes());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();

    assert_eq!(
        u32::from_le_bytes(encoded[..4].try_into().unwrap()),
        0x1234_5678
    );
    assert_eq!(u32::from_be_bytes(encoded[4..8].try_into().unwrap()), 7);
}

#[test]
fn semantic_writer_regenerates_modified_analytic_breps() {
    for body in [closed_cylinder_body(), sphere_patch_body()] {
        let source = sldprt_with_body(&body);
        let mut cur = Cursor::new(source);
        let result = SldprtCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap();
        let mut result = EditableDecodeResult::from(result);
        translate_model_x(&mut result.ir_mut(), 1.0);

        let mut encoded = Vec::new();
        crate::test_support::plan_fixture_write_with_native_envelope(
            result.ir(),
            result.source_fidelity(),
            &mut encoded,
        )
        .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();

        assert_eq!(
            decoded.ir().model.faces.len(),
            result.ir().model.faces.len()
        );
        assert_eq!(
            decoded.ir().model.curves.len(),
            result.ir().model.curves.len()
        );
        assert_eq!(
            decoded
                .ir()
                .model
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>(),
            result
                .ir()
                .model
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn semantic_writer_preserves_sheet_body_classification() {
    let body = owned_triangle_with_kind(0, 701, 0.0, 3);
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.bodies.len(), 1);
    assert_eq!(
        regenerated.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(regenerated.ir().model.faces.len(), 1);
    assert_eq!(
        regenerated
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("parasolid_schema"))
            .map(String::as_str),
        Some("SCH_SW_32001_11000")
    );
}

#[test]
fn semantic_writer_rejects_invalid_ir_without_panicking() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.faces[0].surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#missing").expect("identity grammar");
    // The face needing a missing surface cascades under W1 instead of
    // refusing (#237): the write succeeds without panicking.
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .expect("a face needing a missing surface cascades");
    SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cascaded output decodes");
}

#[test]
fn semantic_writer_reports_unrepresented_typed_fields() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let edge = &mut ir.model.edges[0];
        edge.carrier =
            cadmpeg_ir::topology::EdgeCarrier::new(edge.curve().cloned(), Some([0.0, 1.0]))
                .unwrap();
    }
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.fields-omitted"));
}

#[test]
pub(crate) fn semantic_writer_drops_subds_with_loss() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.subds.push(cadmpeg_ir::SubdSurface {
        native_ref: None,
        id: cadmpeg_ir::ids::SubdId::mint("test:sldprt:subd#0").expect("identity grammar"),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        source_object: None,
        cage: cadmpeg_ir::subd::SubdCage::default(),
    });

    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.arena-omitted"));
}

#[test]
fn semantic_writer_rejects_unsupported_conic_curves() {
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let major_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    for geometry in [
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
            cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis,
                major_direction,
                1.0,
            )
            .unwrap(),
        )),
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis,
                major_direction,
                2.0,
                1.0,
            )
            .unwrap(),
        )),
    ] {
        assert!(matches!(
            crate::writer::curve_values(&geometry, 0.001),
            Err(cadmpeg_core::CodecError::InvalidInput(_))
        ));
    }
}

#[test]
fn semantic_writer_reports_unrepresentable_analytic_surface_parameterizations() {
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(
                &mut Cursor::new(sldprt_with_body(&triangle_body())),
                &DecodeOptions::default(),
            )
            .unwrap(),
    );
    let origin = cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0);
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    axis,
                    reference,
                    2.0,
                    0.5,
                    std::f64::consts::FRAC_PI_4,
                )
                .unwrap(),
            )),
            "elliptical cone ratio 0.5",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    axis,
                    reference,
                    2.0,
                    1.0,
                    -std::f64::consts::FRAC_PI_4,
                )
                .unwrap(),
            )),
            "cone half-angle -0.7853981633974483",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                    origin, axis, reference, -2.0,
                )
                .unwrap(),
            )),
            "signed sphere radius -2",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                    origin, axis, reference, 2.0, -0.5,
                )
                .unwrap(),
            )),
            "torus radii (2, -0.5)",
        ),
    ];

    for (geometry, expected) in cases {
        let mut ir = decoded.ir().clone();
        let surface_id = ir.model.surfaces[0].id.as_str().to_owned();
        ir.model.surfaces[0].geometry = geometry;

        let plan = SldprtCodec
            .plan(
                EncodeInput::new(&ir, Some(decoded.source_fidelity())),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        assert!(
            plan.report().losses.iter().any(|loss| loss.code
                == crate::loss::SldprtLossCode::WriterEntityOmitted.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains(&surface_id)
                && loss.message.contains(expected)),
            "{:?}",
            plan.report().losses
        );
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(written.ir().model.bodies.is_empty());
    }
}

#[test]
fn semantic_writer_converts_millimetres_to_native_metres() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            50.8, moved.y, moved.z,
        ))
        .expect("a finite position is a point"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir()
        .model
        .points
        .iter()
        .any(|point| (point.position().get().x - 50.8).abs() < 1e-5));
}

#[test]
fn semantic_writer_preserves_multiple_body_ownership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap(),
    );

    assert_eq!(regenerated.ir().model.bodies.len(), 2);
    assert_eq!(regenerated.ir().model.regions.len(), 2);
    assert_eq!(regenerated.ir().model.shells.len(), 2);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces().len() == 1));
    assert!(regenerated.ir().model.regions.iter().all(|region| {
        regenerated.source_fidelity().annotations.provenance[region.id.as_str()]
            .tag
            .as_deref()
            == Some("00_51_region")
    }));
    assert!(regenerated.ir().model.shells.iter().all(|shell| {
        regenerated.source_fidelity().annotations.provenance[shell.id.as_str()]
            .tag
            .as_deref()
            == Some("00_51_shell")
    }));
}

#[test]
fn semantic_writer_regenerates_modified_nurbs_carriers() {
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let body = crate::test_support::parasolid::nurbs_triangle_body();
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let (expected_curve, expected_surface) = {
        let mut ir_edit = decoded.ir_mut();
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) =
            &mut ir_edit.model.curves[0].geometry
        else {
            panic!("expected NURBS curve");
        };
        curve
            .try_map_control_points(|index, point| {
                let mut point = point.get();
                if index == 1 {
                    point.y += 250.0;
                }
                cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                    cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                        "control_points contains a non-finite point".into(),
                    )
                })
            })
            .unwrap();
        let expected_curve = curve.clone();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &mut ir_edit.model.surfaces[0].geometry
        else {
            panic!("expected NURBS surface");
        };
        let target = surface.v_count() + 1;
        surface
            .try_map_control_points(|index, pole| {
                let mut pole = pole.get();
                if index == target {
                    pole.z += 500.0;
                }
                cadmpeg_ir::features::FinitePoint3::new(pole).ok_or_else(|| {
                    cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                        "control_points contains a non-finite point".into(),
                    )
                })
            })
            .unwrap();
        let expected_surface = surface.clone();
        (expected_curve, expected_surface)
    };

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir().model.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(value)) if value == &expected_curve)
    ), "curves: {:?}; expected: {expected_curve:?}; losses: {:?}", regenerated.ir().model.curves, regenerated.report().losses);
    assert!(regenerated.ir().model.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(value)) if value == &expected_surface)
    ));
}

#[test]
fn duplicate_configuration_partition_indices_are_reassigned_with_a_loss() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration, DistinctMembers};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let bodies = DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()]).unwrap();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 0,
            source_index: Some(0),
            name: Some(format!("Config {index}")),
            material: None,
            properties: std::collections::BTreeMap::default(),
            bodies: Some(bodies.clone()),
            parameter_values: std::collections::BTreeMap::default(),
            parameter_overrides: std::collections::BTreeMap::default(),
            feature_states: std::collections::BTreeMap::default(),
            native_ref: None,
        })
        .collect();
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let replaced = ir.model.configurations[1].id.as_str();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(replaced) && loss.message.contains("source index")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let indices = decoded
        .ir()
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(indices.len(), 2);
}
