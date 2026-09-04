// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use cadmpeg_ir::eval::nurbs_curve_point;

#[test]
fn edge_parameter_range_rejects_reversed_nonperiodic_interval() {
    let line = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    assert_eq!(edge_parameter_range(&line, 2.0, 5.0), Some([2.0, 5.0]));
    assert_eq!(edge_parameter_range(&line, 5.0, 2.0), None);
    assert_eq!(edge_parameter_range(&line, 2.0, 2.0), None);
}

#[test]
fn edge_parameter_range_normalizes_periodic_interval_in_constant_time() {
    let circle = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    let start = 1.5 + 20_000.0 * std::f64::consts::TAU;
    let end = 0.5 - 20_000.0 * std::f64::consts::TAU;
    let range = edge_parameter_range(&circle, start, end).expect("periodic interval");
    assert!((range[0] - 1.5).abs() < 1.0e-10);
    assert!((range[1] - (0.5 + std::f64::consts::TAU)).abs() < 1.0e-10);
}

#[test]
fn nonperiodic_nurbs_endpoint_seed_selects_the_terminal_branch() {
    let nurbs = NurbsCurve::new(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let geometry = CurveGeometry::Nurbs(nurbs.clone());
    let start_point = nurbs_curve_point(
        nurbs.degree(),
        nurbs.knots(),
        nurbs.control_points(),
        None,
        0.0,
    )
    .expect("start point");
    let end_point = nurbs_curve_point(
        nurbs.degree(),
        nurbs.knots(),
        nurbs.control_points(),
        None,
        1.0,
    )
    .expect("end point");
    let start_seed = curve_endpoint_seed(&geometry, false, 0.0);
    let start = nurbs_curve_parameter_near_point(&nurbs, start_point, 1.0e-6, start_seed)
        .expect("start witness");
    let start_seed_end = nurbs_curve_parameter_near_point(&nurbs, end_point, 1.0e-6, start)
        .expect("unanchored end witness");
    assert!((start_seed_end - 1.0).abs() > 0.1);
    let end_seed = curve_endpoint_seed(&geometry, true, start);
    let end =
        nurbs_curve_parameter_near_point(&nurbs, end_point, 1.0e-6, end_seed).expect("end witness");

    assert!(start.abs() < 1.0e-12);
    assert!((end - 1.0).abs() < 1.0e-12);
}

#[test]
fn surface_parameter_units_follow_the_surface_chart() {
    let ir = CadIr::empty();
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let transformed = SurfaceGeometry::Transformed {
        basis: Box::new(cylinder.clone()),
        transform: Transform::identity(),
    };
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId("plane".into()),
            &plane,
            10.0,
            0.25,
            &BTreeMap::new(),
        ),
        Some([10.0, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId("cylinder".into()),
            &cylinder,
            10.0,
            0.25,
            &BTreeMap::new(),
        ),
        Some([0.25, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId("sphere".into()),
            &sphere,
            10.0,
            0.25,
            &BTreeMap::new(),
        ),
        Some([0.25, 0.25])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId("transformed".into()),
            &transformed,
            10.0,
            0.25,
            &BTreeMap::new(),
        ),
        Some([0.25, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId("unknown".into()),
            &SurfaceGeometry::Unknown { record: None },
            10.0,
            0.25,
            &BTreeMap::new(),
        ),
        None
    );
}

#[test]
fn procedural_surface_units_follow_the_evaluated_parameter_order() {
    let mut ir = CadIr::empty();
    let directrix = CurveId("line".into());
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let sweep = SurfaceId("sweep".into());
    let revolution = SurfaceId("revolution".into());
    ir.model.surfaces.extend([
        Surface {
            id: sweep.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: revolution.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
    ]);
    let _attached = ir.model.add_procedural_surface(
        sweep.clone(),
        ProceduralSurface::new(
            ProceduralSurfaceId("sweep-construction".into()),
            ProceduralSurfaceDefinition::LinearSweep {
                directrix: directrix.clone(),
                direction: Vector3::new(0.0, 1.0, 0.0),
            },
            None,
        ),
    );
    let _attached = ir.model.add_procedural_surface(
        revolution.clone(),
        ProceduralSurface::new(
            ProceduralSurfaceId("revolution-construction".into()),
            ProceduralSurfaceDefinition::AxisRevolution {
                directrix,
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
            },
            None,
        ),
    );
    let length_scale = 0.001;
    let angle_scale = std::f64::consts::PI / 180.0;

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &sweep,
            &ir.model.surfaces[0].geometry,
            length_scale,
            angle_scale,
            &BTreeMap::new(),
        ),
        Some([length_scale, 1.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &revolution,
            &ir.model.surfaces[1].geometry,
            length_scale,
            angle_scale,
            &BTreeMap::new(),
        ),
        Some([angle_scale, length_scale])
    );
}

#[test]
fn directrix_parameter_units_follow_step_curve_equations() {
    let ir = CadIr::empty();
    let angle_scale = std::f64::consts::PI / 180.0;
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 2.0,
    };
    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 1.0,
    };
    let polyline = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        parameters: None,
        chordal_deflection: 0.0,
    };
    let mut active = BTreeSet::new();

    assert_eq!(
        directrix_geometry_parameter_scale(&ir, &parabola, 0.001, angle_scale, &mut active),
        Some(1.0)
    );
    assert_eq!(
        directrix_geometry_parameter_scale(&ir, &hyperbola, 0.001, angle_scale, &mut active),
        Some(1.0)
    );
    assert_eq!(
        directrix_geometry_parameter_scale(&ir, &polyline, 0.001, angle_scale, &mut active),
        Some(1.0)
    );
}

#[test]
fn unresolved_procedural_directrix_has_no_assumed_parameter_units() {
    let mut ir = CadIr::empty();
    let directrix = CurveId("composite".into());
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Composite {
            segments: Vec::new(),
            self_intersect: None,
        },
        source_object: None,
    });
    let surface = SurfaceId("sweep".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_surface(
        surface.clone(),
        ProceduralSurface::new(
            ProceduralSurfaceId("sweep-construction".into()),
            ProceduralSurfaceDefinition::LinearSweep {
                directrix,
                direction: Vector3::new(0.0, 1.0, 0.0),
            },
            None,
        ),
    );

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &surface,
            &ir.model.surfaces[0].geometry,
            0.001,
            std::f64::consts::PI / 180.0,
            &BTreeMap::new(),
        ),
        None
    );
}

#[test]
fn axis_revolution_surface_parameter_units_use_plane_angle_for_u() {
    let surface_id = SurfaceId("surface".into());
    let directrix = CurveId("directrix".into());
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_surface(
        surface_id.clone(),
        ProceduralSurface::new(
            ProceduralSurfaceId("construction".into()),
            ProceduralSurfaceDefinition::AxisRevolution {
                directrix,
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
            },
            None,
        ),
    );

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &surface_id,
            &ir.model.surfaces[0].geometry,
            10.0,
            std::f64::consts::PI / 180.0,
            &BTreeMap::new(),
        ),
        Some([std::f64::consts::PI / 180.0, 10.0])
    );
}

#[test]
fn anisotropic_circle_scaling_preserves_its_native_parameterization() {
    let original = PcurveGeometry::Circle {
        center: Point2::new(1.0, -2.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, 1.0),
        radius: 3.0,
    };
    let mut scaled = original.clone();
    assert!(scale_pcurve_geometry(&mut scaled, [2.0, 3.0]));
    assert!(matches!(scaled, PcurveGeometry::Harmonic { .. }));
    for parameter in [0.0, 0.25, 1.0, 2.0] {
        let expected = cadmpeg_ir::eval::pcurve_uv(&original, parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&scaled, parameter).unwrap();
        assert!((actual.u - expected.u * 2.0).abs() < 1.0e-12);
        assert!((actual.v - expected.v * 3.0).abs() < 1.0e-12);
    }
}

#[test]
fn anisotropic_replica_scaling_conjugates_the_parent_map() {
    let original = PcurveGeometry::Transformed {
        basis: Box::new(PcurveGeometry::Line {
            origin: Point2::new(1.0, 2.0),
            direction: Point2::new(3.0, 4.0),
        }),
        transform: Transform2 {
            rows: [[0.0, -2.0, 10.0], [2.0, 0.0, 20.0], [0.0, 0.0, 1.0]],
        },
    };
    let mut scaled = original.clone();
    assert!(scale_pcurve_geometry(&mut scaled, [2.0, 3.0]));
    for parameter in [0.0, 0.5, 1.0] {
        let expected = cadmpeg_ir::eval::pcurve_uv(&original, parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&scaled, parameter).unwrap();
        assert!((actual.u - expected.u * 2.0).abs() < 1.0e-12);
        assert!((actual.v - expected.v * 3.0).abs() < 1.0e-12);
    }
}

#[test]
fn unsupported_anisotropic_pcurve_forms_are_not_reshaped_by_scalar_scaling() {
    let mut parabola = PcurveGeometry::Parabola {
        vertex: Point2::new(0.0, 0.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, 1.0),
        focal_distance: 1.0,
    };
    assert!(!scale_pcurve_geometry(&mut parabola, [2.0, 3.0]));
    assert!(matches!(parabola, PcurveGeometry::Parabola { .. }));
}

#[test]
fn every_iso_si_prefix_resolves_to_its_exact_factor() {
    let expected = [
        ("EXA", 1e18),
        ("PETA", 1e15),
        ("TERA", 1e12),
        ("GIGA", 1e9),
        ("MEGA", 1e6),
        ("KILO", 1e3),
        ("HECTO", 1e2),
        ("DECA", 1e1),
        ("DECI", 1e-1),
        ("CENTI", 1e-2),
        ("MILLI", 1e-3),
        ("MICRO", 1.0e-6),
        ("NANO", 1.0e-9),
        ("PICO", 1.0e-12),
        ("FEMTO", 1e-15),
        ("ATTO", 1e-18),
    ];
    for (prefix, factor) in expected {
        assert_eq!(si_prefix(prefix), Some(factor), "prefix {prefix}");
    }
}

#[test]
fn prefixed_plane_angle_units_scale_to_radians() {
    let (exchange, _) = crate::parse::parse(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=(NAMED_UNIT(*) SI_UNIT(.MILLI.,.RADIAN.));\
#2=(NAMED_UNIT(*) SI_UNIT($,.RADIAN.));\
ENDSEC;END-ISO-10303-21;",
    )
    .expect("parse plane-angle units");
    let mut active = BTreeSet::new();
    assert_eq!(unit_scale_radians(1, &exchange, &mut active), Some(1.0e-3));
    assert!(active.is_empty());
    assert_eq!(unit_scale_radians(2, &exchange, &mut active), Some(1.0));
    assert!(active.is_empty());
}

#[test]
fn conversion_based_plane_angle_units_multiply_prefixed_base_scales() {
    let (exchange, _) = crate::parse::parse(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT(.MILLI.,.RADIAN.));\
#2=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(2.),#1);\
#3=(CONVERSION_BASED_UNIT('two milli-radians',#2) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\
#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\
#5=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(2.),#4);\
#6=(CONVERSION_BASED_UNIT('two radians',#5) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\
ENDSEC;END-ISO-10303-21;",
    )
    .expect("parse conversion-based plane-angle units");
    let mut active = BTreeSet::new();
    assert_eq!(unit_scale_radians(3, &exchange, &mut active), Some(2.0e-3));
    assert!(active.is_empty());
    assert_eq!(unit_scale_radians(6, &exchange, &mut active), Some(2.0));
    assert!(active.is_empty());
}

#[test]
fn recursive_unit_and_pcurve_failures_release_active_ids() {
    let (exchange, _) = crate::parse::parse(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=CONVERSION_BASED_UNIT('',#2);\
#2=UNKNOWN_FACTOR();\
#3=LINE('',#4,#5);\
#4=UNKNOWN_POINT();\
#5=UNKNOWN_VECTOR();\
#6=CURVE_REPLICA('',#6,#7);\
#7=UNKNOWN_OPERATOR();\
ENDSEC;END-ISO-10303-21;",
    )
    .expect("parse recursive failure graph");
    let mut active = BTreeSet::new();
    assert!(unit_scale_mm(1, &exchange, &mut active).is_none());
    assert!(active.is_empty());
    assert!(unit_scale_radians(1, &exchange, &mut active).is_none());
    assert!(active.is_empty());

    let mut warnings = Vec::new();
    assert!(decode_pcurve_geometry(
        3,
        &exchange,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1.0,
        &mut warnings,
        &mut active,
        0,
    )
    .is_none());
    assert!(active.is_empty());
    assert!(decode_pcurve_geometry(
        6,
        &exchange,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1.0,
        &mut warnings,
        &mut active,
        0,
    )
    .is_none());
    assert!(active.is_empty());
}

#[test]
fn pcurve_trim_select_ignores_cartesian_point_coordinates() {
    let value = Value::List(vec![
        Value::Typed(
            "CARTESIAN_POINT".into(),
            Box::new(Value::List(vec![Value::Real(17.0), Value::Real(23.0)])),
        ),
        Value::Real(0.25),
    ]);
    assert_eq!(pcurve_trim_parameter(&value), Some(0.25));
}

#[test]
fn pcurve_trim_select_prefers_parameter_value() {
    let value = Value::List(vec![
        Value::Real(17.0),
        Value::Typed("PARAMETER_VALUE".into(), Box::new(Value::Real(0.25))),
    ]);
    assert_eq!(pcurve_trim_parameter(&value), Some(0.25));
}
