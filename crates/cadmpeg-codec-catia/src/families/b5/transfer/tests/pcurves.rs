#![allow(unused_imports)]

use super::super::super::graph::{
    bounded_occurrence_range, edge_pcurve_parameters, loop_chain_closes, B5ExtrusionDirectrix,
    B5ExtrusionSurface, B5Face, B5Graph, B5Loop, B5LoopMetadata, B5OffsetSurface, B5OpaquePcurve,
    B5ParameterIncidence, B5Pcurve, B5PcurveParameterization, B5Profile, B5SphereGreatCirclePcurve,
    B5SupportedSurface, B5SupportedSurfaceParameters, B5Surface,
};
use super::super::edges::{
    b5_edge_support_definition, b5_supports_follow_edge, curve_cache_has_ordered_knots,
    merge_curve_plan, ordered_subrange, orient_b5_supports_to_edge,
};
use super::super::faces::{orient_loop_members, ownership_plan};
use super::super::pcurves::{
    cylinder_helix, cylinder_point, isocurve_endpoint_parameters, lifted_curve_geometry,
    neutral_pcurve_point, oriented_circle_plan, oriented_line_plan, oriented_nurbs_range,
    sphere_great_circle_geometry, sphere_great_circle_pcurve,
};
use super::super::surfaces::{rational_arc, revolution_surface, revolve_nurbs};
use super::super::unit;
use super::super::vertices::transfer_vertex_tolerances;
use super::super::*;
use super::*;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::surface_point;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::{SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn cylinder_pcurve_uses_independent_angular_scale_without_origin_rotation() {
    let surface = B5Surface::Cylinder {
        origin: [0.0, 0.0, 0.0],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 6.0,
        u_range: [1.0, 1.0 + 6.0 * std::f64::consts::PI],
        v_range: [-1.0, 1.0],
        angular_scale: 3.0,
        chart_origin: 1.0,
    };
    let point = neutral_pcurve_point([3.0 * std::f64::consts::PI, 3.0], &surface);
    assert_eq!(point.u, std::f64::consts::PI);
    assert_eq!(point.v, 3.0);
    let lifted = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        6.0,
        3.0,
        [3.0 * std::f64::consts::PI, 3.0],
    );
    assert!((lifted[0] + 6.0).abs() < 1.0e-12);
    assert!(lifted[1].abs() < 1.0e-12);
    assert_eq!(lifted[2], 3.0);
}

#[test]
fn revolution_cache_preserves_native_profile_and_arc_length_chart() {
    let profile = B5Profile::Line {
        point: [2.0, 0.0, 0.0],
        direction: [0.0, 0.0, 1.0],
        parameter_range: [-1.0, 1.0],
    };
    let (surface, plan) = revolution_surface(
        Some(&profile),
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        [[-1.0, 1.0], [0.0, 2.0 * std::f64::consts::PI]],
    )
    .expect("exact revolution cache");
    assert_eq!(plan.parameter_interval, [-1.0, 1.0]);
    assert_eq!(plan.angular_interval, [0.0, std::f64::consts::PI]);
    assert_eq!(
        plan.angular_parameter_interval,
        [0.0, 2.0 * std::f64::consts::PI]
    );
    let evaluated = surface_point(&SurfaceGeometry::Nurbs(surface), 0.5, std::f64::consts::PI)
        .expect("surface point");
    assert!(evaluated.x.abs() < 1.0e-12);
    assert!((evaluated.y - 2.0).abs() < 1.0e-12);
    assert!((evaluated.z - 0.5).abs() < 1.0e-12);
    assert!(revolution_surface(
        Some(&profile),
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        [[-0.5, 1.0], [0.0, 2.0 * std::f64::consts::PI]],
    )
    .is_none());
}

#[test]
fn revolution_isocurve_keeps_its_native_trim_range() {
    let angular_range = [0.0, std::f64::consts::TAU];
    let graph = B5Graph {
        complete: true,
        faces: vec![B5Face {
            object_id: 1,
            surface: 10,
            loops: vec![2],
            terminal_control: None,
        }],
        face_records: BTreeMap::new(),
        loops: BTreeMap::from([(
            2,
            B5Loop {
                object_id: 2,
                pcurves: vec![20],
                edges: vec![30],
                metadata: test_loop_metadata(1),
                surface: 10,
            },
        )]),
        pcurves: BTreeMap::from([(
            20,
            B5Pcurve {
                object_id: 20,
                surface: 10,
                degree: 1,
                distinct_knots: angular_range.into_iter().collect(),
                multiplicities: vec![2, 2],
                control_points: vec![[0.5, angular_range[0]], [0.5, angular_range[1]]],
                weights: None,
                parameter_range: None,
                parameterization: B5PcurveParameterization::Native,
                class_21_suffix_scalar: None,
                lifted_endpoints: None,
            },
        )]),
        opaque_pcurves: BTreeMap::new(),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::from([(
            10,
            B5Surface::Revolution {
                profile_curve: 110,
                axis_origin: [0.0, 0.0, 0.0],
                reference_x: [1.0, 0.0, 0.0],
                reference_y: [0.0, 1.0, 0.0],
                axis_direction: [0.0, 0.0, 1.0],
                profile_range: [-1.0, 1.0],
                angular_range,
                angular_scale: 1.0,
            },
        )]),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::from([
            (
                40,
                B5ParameterIncidence {
                    object_id: 40,
                    curves: vec![20],
                    parameters: vec![angular_range[0]],
                    controls: vec![0],
                },
            ),
            (
                41,
                B5ParameterIncidence {
                    object_id: 41,
                    curves: vec![20],
                    parameters: vec![angular_range[1]],
                    controls: vec![0],
                },
            ),
        ]),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: vec![[2.0, 0.0, 0.5]],
        logical_vertex_refs: vec![50],
        edge_vertices: BTreeMap::from([(30, [0, 0])]),
        edge_parameter_incidences: BTreeMap::from([(30, [40, 41])]),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::from([(
            110,
            B5Profile::Line {
                point: [2.0, 0.0, 0.0],
                direction: [0.0, 0.0, 1.0],
                parameter_range: [-1.0, 1.0],
            },
        )]),
    };
    assert!(matches!(
        resolved_surface_carrier_in_graph(&graph, 10),
        Some(ResolvedPcurveSurface::Geometry(SurfaceGeometry::Nurbs(_)))
    ));
    let plan = build_plan(&graph, &UnknownId("catia:test-payload".to_string()))
        .expect("closed revolution graph");
    let curve = plan.edge_curve_plan.get(&30).expect("revolution isocurve");
    assert_eq!(curve.parameter_range, Some(angular_range));
    assert!(matches!(curve.geometry, CurveGeometry::Nurbs(_)));
}

#[test]
fn affine_and_isoparametric_pcurves_produce_exact_curve_carriers() {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 2.0], [3.0, 2.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let plane = B5Surface::Plane {
        origin: [1.0, 2.0, 3.0],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
        panic!("plane lift must be NURBS");
    };
    assert_eq!(curve.control_points()[0], Point3::new(1.0, 4.0, 3.0));
    assert_eq!(curve.control_points()[1], Point3::new(4.0, 4.0, 3.0));

    let cylinder = B5Surface::Cylinder {
        origin: [0.0, 0.0, 0.0],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 2.0,
        u_range: [0.0, 4.0 * std::f64::consts::PI],
        v_range: [-1.0, 1.0],
        angular_scale: 2.0,
        chart_origin: 0.0,
    };
    assert!(matches!(
        lifted_curve_geometry(&pcurve, &cylinder),
        Some(CurveGeometry::Circle { radius: 2.0, .. })
    ));
    let meridian = B5Pcurve {
        control_points: vec![[1.0, -2.0], [1.0, 4.0]],
        ..pcurve
    };
    assert!(matches!(
        lifted_curve_geometry(&meridian, &cylinder),
        Some(CurveGeometry::Line { .. })
    ));
}

#[test]
fn analytic_isocurves_accept_finite_nonzero_scales() {
    let scale = 1e-200;
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [0.5 * scale, 0.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let cylinder = B5Surface::Cylinder {
        origin: [0.0; 3],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: scale,
        u_range: [0.0, std::f64::consts::TAU * scale],
        v_range: [-scale, scale],
        angular_scale: scale,
        chart_origin: 0.0,
    };
    let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
    let edge_start = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        scale,
        scale,
        pcurve.control_points[0],
    );
    let edge_end = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        scale,
        scale,
        pcurve.control_points[1],
    );
    assert!(oriented_circle_plan(
        &pcurve,
        &cylinder,
        &geometry,
        [0.0, 1.0],
        edge_start,
        edge_end,
    )
    .is_some());

    let cone = B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle: std::f64::consts::FRAC_PI_6,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [0.0, scale],
        angular_scale: 1.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    let cone_pcurve = B5Pcurve {
        control_points: vec![[0.0, scale], [0.5, scale]],
        ..pcurve.clone()
    };
    assert!(matches!(
        lifted_curve_geometry(&cone_pcurve, &cone),
        Some(CurveGeometry::Circle { radius, .. }) if radius == scale * 0.5
    ));

    let torus = B5Surface::Torus {
        center: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        major_radius: scale,
        minor_radius: scale,
        major_angular_range: [0.0, std::f64::consts::TAU],
        major_angular_domain: [0.0, std::f64::consts::TAU],
        minor_angular_range: [0.0, std::f64::consts::TAU],
        minor_angular_domain: [0.0, std::f64::consts::TAU],
        major_scale: 1.0,
        minor_scale: 1.0,
    };
    let torus_pcurve = B5Pcurve {
        control_points: vec![[0.0, 0.0], [0.5, 0.0]],
        ..pcurve
    };
    assert!(matches!(
        lifted_curve_geometry(&torus_pcurve, &torus),
        Some(CurveGeometry::Circle { radius, .. }) if radius == 2.0 * scale
    ));
}

#[test]
fn affine_plane_lift_preserves_pcurve_weights() {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 2,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![3, 3],
        control_points: vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let plane = B5Surface::Plane {
        origin: [0.0, 0.0, 2.0],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
        panic!("expected lifted rational curve");
    };
    assert_eq!(curve.weights(), pcurve.weights.as_deref());
    assert!(curve.control_points().iter().all(|point| point.z == 2.0));
}

#[test]
fn affine_lift_range_orients_and_trims_the_nurbs_carrier() {
    let geometry = CurveGeometry::Nurbs(
        NurbsCurve::new(
            1,
            vec![0.0, 0.0, 10.0, 10.0],
            vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
            None,
            false,
        )
        .expect("valid affine lift curve"),
    );
    let forward = oriented_nurbs_range(
        geometry.clone(),
        [2.0, 8.0],
        [2.0, 0.0, 2.0],
        [8.0, 0.0, 2.0],
    )
    .expect("forward trimmed range");
    assert_eq!(forward.geometry, geometry);
    assert_eq!(forward.parameter_range, Some([2.0, 8.0]));
    assert_eq!(forward.edge_tolerance, None);

    let reversed = oriented_nurbs_range(
        geometry.clone(),
        [8.0, 2.0],
        [8.0, 0.0, 2.0],
        [2.0, 0.0, 2.0],
    )
    .expect("reversed trimmed range");
    assert_eq!(reversed.parameter_range, Some([2.0, 8.0]));
    let CurveGeometry::Nurbs(reversed) = reversed.geometry else {
        unreachable!();
    };
    assert_eq!(
        reversed.control_points(),
        [Point3::new(10.0, 0.0, 2.0), Point3::new(0.0, 0.0, 2.0)]
    );
    assert!(oriented_nurbs_range(geometry, [2.0, 8.0], [3.0, 0.0, 2.0], [8.0, 0.0, 2.0]).is_none());

    let tolerant = oriented_nurbs_range(
        CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 10.0, 10.0],
                vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
                None,
                false,
            )
            .expect("valid tolerant lift curve"),
        ),
        [2.0, 8.0],
        [2.0, 0.0, 2.0 + 1e-4],
        [8.0, 0.0, 2.0],
    )
    .expect("tolerant trimmed range");
    assert!((tolerant.edge_tolerance.expect("edge tolerance") - (1e-4 + 1.0e-9)).abs() < 1e-15);
}

#[test]
fn isocurve_range_uses_monotone_varying_surface_coordinate() {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 2,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![3, 3],
        control_points: vec![[4.0, 2.0], [4.0, 6.0], [4.0, 10.0]],
        weights: Some(vec![1.0, 2.0, 1.0]),
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    assert_eq!(
        isocurve_endpoint_parameters(&pcurve, [0.25, 0.75]),
        Some([50.0 / 11.0, 82.0 / 11.0])
    );

    let decreasing = B5Pcurve {
        control_points: pcurve.control_points.iter().copied().rev().collect(),
        ..pcurve.clone()
    };
    assert_eq!(
        isocurve_endpoint_parameters(&decreasing, [0.25, 0.75]),
        Some([82.0 / 11.0, 50.0 / 11.0])
    );

    let turnback = B5Pcurve {
        control_points: vec![[4.0, 2.0], [4.0, 10.0], [4.0, 6.0]],
        ..pcurve.clone()
    };
    assert!(isocurve_endpoint_parameters(&turnback, [0.0, 1.0]).is_none());

    let nonpositive_weight = B5Pcurve {
        weights: Some(vec![1.0, 0.0, 1.0]),
        ..pcurve
    };
    assert!(isocurve_endpoint_parameters(&nonpositive_weight, [0.0, 1.0]).is_none());
}

#[test]
fn analytic_line_range_uses_oriented_signed_distance() {
    let line = CurveGeometry::Line {
        origin: Point3::new(1.0, 2.0, 3.0),
        direction: Vector3::new(0.0, 0.0, 2.0),
    };
    let forward =
        oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 9.0]).expect("forward line range");
    assert_eq!(forward.parameter_range, Some([2.0, 6.0]));
    assert!(matches!(
        forward.geometry,
        CurveGeometry::Line { direction, .. }
            if direction == Vector3::new(0.0, 0.0, 1.0)
    ));

    let reversed =
        oriented_line_plan(&line, [1.0, 2.0, 9.0], [1.0, 2.0, 5.0]).expect("reversed line range");
    assert_eq!(reversed.parameter_range, Some([-6.0, -2.0]));
    assert!(matches!(
        reversed.geometry,
        CurveGeometry::Line { direction, .. }
            if direction == Vector3::new(0.0, 0.0, -1.0)
    ));
    let tolerant = oriented_line_plan(&line, [1.001, 2.0, 5.0], [1.0, 2.0, 9.0])
        .expect("tolerant line endpoints");
    assert!(tolerant.edge_tolerance.is_some_and(|value| value > 0.001));
    assert_eq!(tolerant.cache_fit_tolerance, None);
    assert!(oriented_line_plan(&line, [1.01, 2.0, 5.0], [1.0, 2.0, 9.0]).is_none());
    assert!(oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 5.0]).is_none());

    let tiny_direction = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1e-200, 0.0, 0.0),
    };
    let tiny = oriented_line_plan(&tiny_direction, [2.0, 0.0, 0.0], [3.0, 0.0, 0.0])
        .expect("finite nonzero line direction");
    assert!(matches!(
        tiny.geometry,
        CurveGeometry::Line { direction, .. }
            if direction == Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn isoparametric_circle_range_preserves_winding_and_seams() {
    let cylinder = B5Surface::Cylinder {
        origin: [0.0, 0.0, 0.0],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 2.0,
        u_range: [0.0, 4.0 * std::f64::consts::PI],
        v_range: [-1.0, 1.0],
        angular_scale: 2.0,
        chart_origin: 0.0,
    };
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[11.0, 3.0], [13.0, 3.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
    let edge_start = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        2.0,
        pcurve.control_points[0],
    );
    let edge_end = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        2.0,
        pcurve.control_points[1],
    );
    let forward = oriented_circle_plan(
        &pcurve,
        &cylinder,
        &geometry,
        [0.0, 1.0],
        edge_start,
        edge_end,
    )
    .expect("seam-crossing circle range");
    assert_eq!(forward.parameter_range, Some([5.5, 6.5]));

    let tiny_sweep = 1e-14;
    let tiny_pcurve = B5Pcurve {
        control_points: vec![[0.0, 3.0], [2.0 * tiny_sweep, 3.0]],
        ..pcurve.clone()
    };
    let tiny_geometry =
        lifted_curve_geometry(&tiny_pcurve, &cylinder).expect("tiny cylinder latitude");
    let tiny_end = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        2.0,
        tiny_pcurve.control_points[1],
    );
    let tiny = oriented_circle_plan(
        &tiny_pcurve,
        &cylinder,
        &tiny_geometry,
        [0.0, 1.0],
        [2.0, 0.0, 3.0],
        tiny_end,
    )
    .expect("tiny circle sweep");
    assert_eq!(tiny.parameter_range, Some([0.0, tiny_sweep]));

    let reversed_pcurve = B5Pcurve {
        control_points: pcurve.control_points.iter().copied().rev().collect(),
        ..pcurve.clone()
    };
    let reversed = oriented_circle_plan(
        &reversed_pcurve,
        &cylinder,
        &geometry,
        [0.0, 1.0],
        edge_end,
        edge_start,
    )
    .expect("reversed circle range");
    let [start, end] = reversed.parameter_range.expect("canonical range");
    assert!(start >= 0.0 && end > start && end - start == 1.0);
    assert!(matches!(
        reversed.geometry,
        CurveGeometry::Circle { axis, .. } if axis == Vector3::new(0.0, 0.0, -1.0)
    ));

    let turnback = B5Pcurve {
        degree: 2,
        multiplicities: vec![3, 3],
        control_points: vec![[0.0, 3.0], [4.0, 3.0], [2.0, 3.0]],
        ..pcurve
    };
    let turnback_geometry =
        lifted_curve_geometry(&turnback, &cylinder).expect("turnback latitude locus");
    let turnback_end = cylinder_point(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        2.0,
        [2.0, 3.0],
    );
    assert!(oriented_circle_plan(
        &turnback,
        &cylinder,
        &turnback_geometry,
        [0.0, 1.0],
        [2.0, 0.0, 3.0],
        turnback_end,
    )
    .is_none());

    let half_angle = std::f64::consts::FRAC_PI_6;
    let cone = B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [-4.0, 0.0],
        angular_scale: 2.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    let cone_pcurve = B5Pcurve {
        control_points: vec![[0.0, -4.0], [2.0, -4.0]],
        ..reversed_pcurve
    };
    let cone_geometry = lifted_curve_geometry(&cone_pcurve, &cone).expect("signed cone latitude");
    let cone_point = |angle: f64| {
        [
            -4.0 * half_angle.sin() * angle.cos(),
            -4.0 * half_angle.sin() * angle.sin(),
            -4.0 * half_angle.cos(),
        ]
    };
    let signed = oriented_circle_plan(
        &cone_pcurve,
        &cone,
        &cone_geometry,
        [0.0, 1.0],
        cone_point(0.0),
        cone_point(1.0),
    )
    .expect("normalized signed-radius circle");
    assert!(matches!(
        signed.geometry,
        CurveGeometry::Circle { radius, ref_direction, .. }
            if radius == 2.0 && ref_direction == Vector3::new(-1.0, 0.0, 0.0)
    ));
}

#[test]
fn edge_curve_plans_merge_proofs_and_discard_conflicting_carriers() {
    let geometry = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut plans = HashMap::new();
    let mut conflicts = HashSet::new();
    merge_curve_plan(
        &mut plans,
        &mut conflicts,
        4,
        CurvePlan {
            geometry: geometry.clone(),
            parameter_range: None,
            edge_tolerance: None,
            cache_fit_tolerance: None,
        },
    );
    merge_curve_plan(
        &mut plans,
        &mut conflicts,
        4,
        CurvePlan {
            geometry,
            parameter_range: Some([2.0, 8.0]),
            edge_tolerance: None,
            cache_fit_tolerance: None,
        },
    );
    assert_eq!(plans[&4].parameter_range, Some([2.0, 8.0]));

    let conflicting = CurvePlan {
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 1.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        parameter_range: Some([2.0, 8.0]),
        edge_tolerance: None,
        cache_fit_tolerance: None,
    };
    merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting.clone());
    assert!(!plans.contains_key(&4));
    assert!(conflicts.contains(&4));
    merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting);
    assert!(!plans.contains_key(&4));
}

#[test]
fn cone_chart_normalizes_arc_length_and_slant_coordinates() {
    let half_angle = std::f64::consts::FRAC_PI_6;
    let cone = B5Surface::Cone {
        apex: [0.0, 0.0, 0.0],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [2.0, 8.0],
        angular_scale: 3.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 3.0 * std::f64::consts::PI],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 4.0], [3.0 * std::f64::consts::PI, 4.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    assert_eq!(
        pcurve
            .control_points
            .iter()
            .map(|point| neutral_pcurve_point(*point, &cone))
            .collect::<Vec<_>>(),
        [
            Point2::new(0.0, 2.0 * half_angle.cos()),
            Point2::new(std::f64::consts::PI, 2.0 * half_angle.cos()),
        ]
    );
    let mut opposite_handed = cone.clone();
    let B5Surface::Cone { axis, .. } = &mut opposite_handed else {
        unreachable!();
    };
    *axis = [0.0, 0.0, -1.0];
    assert_eq!(
        neutral_pcurve_point([3.0 * std::f64::consts::PI, 4.0], &opposite_handed),
        Point2::new(-std::f64::consts::PI, 2.0 * half_angle.cos())
    );
    let Some(CurveGeometry::Circle {
        center,
        radius,
        axis,
        ..
    }) = lifted_curve_geometry(&pcurve, &cone)
    else {
        panic!("expected cone latitude circle");
    };
    assert_eq!(center, Point3::new(0.0, 0.0, 4.0 * half_angle.cos()));
    assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
    assert!((radius - 2.0).abs() < 1.0e-12);
}

#[test]
fn sphere_class_1d_fields_lift_to_the_exact_great_circle_plane() {
    let sphere = B5Surface::Sphere {
        center: [1.0, 2.0, 3.0],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 5.0,
        azimuth_range: [0.0, std::f64::consts::TAU],
        latitude_range: [-1.0, 1.0],
        construction_radius: 8.0,
        chart_origin: 0.0,
    };
    let pcurve = B5SphereGreatCirclePcurve {
        chart_bounds: [[0.0, 8.0], [0.0, std::f64::consts::TAU * 8.0]],
        chart_shift: 0.0,
        chart_scale: 8.0,
        slope: -1.0,
        phase: -std::f64::consts::FRAC_PI_2,
    };
    let Some(CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    }) = sphere_great_circle_geometry(&pcurve, &sphere)
    else {
        panic!("expected great circle");
    };
    assert_eq!(center, Point3::new(1.0, 2.0, 3.0));
    assert!((radius - 5.0).abs() < 1.0e-12);
    assert!((axis.x * axis.x + axis.y * axis.y + axis.z * axis.z - 1.0).abs() < 1.0e-12);
    assert!(
        (axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z).abs()
            < 1.0e-12
    );
    assert!((axis.y + std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-12);
    assert!((axis.z - std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-12);

    let (geometry, range) =
        sphere_great_circle_pcurve(&pcurve).expect("exact parameter-space curve");
    assert_eq!(range, [0.0, 8.0]);
    let uv = cadmpeg_ir::eval::pcurve_uv(&geometry, 8.0).expect("chart endpoint");
    assert_eq!(uv.u, 1.0);
    assert!((uv.v - (-(1.0 + std::f64::consts::FRAC_PI_2).cos()).atan()).abs() < 1.0e-12);

    let tiny = 1e-200;
    let mut tiny_sphere = sphere;
    let B5Surface::Sphere {
        construction_radius,
        radius,
        ..
    } = &mut tiny_sphere
    else {
        unreachable!()
    };
    *construction_radius = tiny;
    *radius = tiny;
    let tiny_pcurve = B5SphereGreatCirclePcurve {
        chart_bounds: [[0.0, tiny], [0.0, std::f64::consts::TAU * tiny]],
        chart_shift: 0.0,
        chart_scale: tiny,
        slope: -1.0,
        phase: 0.0,
    };
    assert!(sphere_great_circle_geometry(&tiny_pcurve, &tiny_sphere).is_some());
    let (geometry, range) =
        sphere_great_circle_pcurve(&tiny_pcurve).expect("tiny parameter-space curve");
    assert_eq!(range, [0.0, tiny]);
    let uv = cadmpeg_ir::eval::pcurve_uv(&geometry, tiny).expect("tiny chart endpoint");
    assert_eq!(uv.u, 1.0);
}

#[test]
fn owned_sphere_class_1d_pcurve_enters_the_transfer_plan() {
    let chart_scale = 8.0;
    let parameter_range = [0.0, 4.0 * std::f64::consts::PI];
    let graph = B5Graph {
        complete: true,
        faces: vec![B5Face {
            object_id: 1,
            surface: 2,
            loops: vec![3],
            terminal_control: None,
        }],
        face_records: BTreeMap::new(),
        loops: BTreeMap::from([(
            3,
            B5Loop {
                object_id: 3,
                pcurves: vec![4, 4, 4],
                edges: vec![5, 6, 7],
                metadata: test_loop_metadata(3),
                surface: 2,
            },
        )]),
        pcurves: BTreeMap::new(),
        opaque_pcurves: BTreeMap::from([(
            4,
            B5OpaquePcurve {
                object_id: 4,
                surface: 2,
                class: 0x1d,
                payload: Vec::new(),
                sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                    chart_bounds: [parameter_range, [0.0, std::f64::consts::TAU * chart_scale]],
                    chart_shift: 0.0,
                    chart_scale,
                    slope: 0.0,
                    phase: 0.0,
                }),
            },
        )]),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::from([(
            2,
            B5Surface::Sphere {
                center: [0.0, 0.0, 0.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 5.0,
                azimuth_range: [0.0, std::f64::consts::TAU],
                latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
                construction_radius: chart_scale,
                chart_origin: 0.0,
            },
        )]),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::new(),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: vec![[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [-5.0, 0.0, 0.0]],
        logical_vertex_points: Vec::new(),
        logical_vertex_refs: Vec::new(),
        edge_vertices: BTreeMap::from([(5, [0, 1]), (6, [1, 2]), (7, [2, 0])]),
        edge_parameter_incidences: BTreeMap::new(),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };
    let payload = UnknownId("catia:test-payload".to_string());

    assert!(ownership_plan(&graph).is_some());
    assert!(loop_chain_closes(&graph.loops[&3], &graph.edge_vertices));
    let senses = graph.loops[&3].edge_senses();
    assert!(orient_loop_members(&graph, BTreeMap::from([(3, senses)])).is_some());
    let plan = build_plan(&graph, &payload).expect("complete owned graph");

    assert_eq!(
        plan.pcurve_plan.get(&4),
        Some(&(
            PcurveGeometry::SphericalGreatCircle {
                azimuth_origin: 0.0,
                azimuth_rate: chart_scale.recip(),
                plane_phase: 0.0,
                plane_slope: 0.0,
            },
            false,
            parameter_range,
        ))
    );
    assert_eq!(
        plan.edge_support_plan.get(&5),
        Some(&vec![(2, 4, parameter_range)])
    );
    assert!(plan.exact_support_edges.contains(&5));

    let mut ir = CadIr::empty();
    assert!(transfer(
        &mut ir,
        &mut AnnotationBuilder::new(),
        graph,
        &payload,
    ));
    assert_eq!(ir.model.pcurves.len(), 1);
    assert!(matches!(
        ir.model.pcurves[0].geometry,
        PcurveGeometry::SphericalGreatCircle { .. }
    ));
}

/// One closed spherical component of a synthetic B5 graph. `face`, `loop_`,
/// `pcurve`, and `surface` are persistent object ids; `edges` names the three
/// edge object ids and `vertices` the three vertex-point rows.
struct SyntheticSphericalComponent {
    face: u32,
    loop_: u32,
    pcurve: u32,
    surface: u32,
    edges: [u32; 3],
    vertices: [usize; 3],
    center: [f64; 3],
}

/// Build a B5 graph of independent closed spherical components. Each
/// component contributes one face carrying one three-member loop over a
/// single class-`1d` great-circle pcurve, as in
/// [`owned_sphere_class_1d_pcurve_enters_the_transfer_plan`].
fn synthetic_spherical_graph(components: &[SyntheticSphericalComponent]) -> B5Graph {
    let chart_scale = 8.0;
    let parameter_range = [0.0, 4.0 * std::f64::consts::PI];
    let radius = 5.0;
    let mut graph = B5Graph {
        complete: true,
        faces: Vec::new(),
        face_records: BTreeMap::new(),
        loops: BTreeMap::new(),
        pcurves: BTreeMap::new(),
        opaque_pcurves: BTreeMap::new(),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::new(),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: Vec::new(),
        logical_vertex_refs: Vec::new(),
        edge_vertices: BTreeMap::new(),
        edge_parameter_incidences: BTreeMap::new(),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };
    for component in components {
        graph.faces.push(B5Face {
            object_id: component.face,
            surface: component.surface,
            loops: vec![component.loop_],
            terminal_control: None,
        });
        graph.loops.insert(
            component.loop_,
            B5Loop {
                object_id: component.loop_,
                pcurves: vec![component.pcurve; 3],
                edges: component.edges.to_vec(),
                metadata: test_loop_metadata(3),
                surface: component.surface,
            },
        );
        graph.opaque_pcurves.insert(
            component.pcurve,
            B5OpaquePcurve {
                object_id: component.pcurve,
                surface: component.surface,
                class: 0x1d,
                payload: Vec::new(),
                sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                    chart_bounds: [parameter_range, [0.0, std::f64::consts::TAU * chart_scale]],
                    chart_shift: 0.0,
                    chart_scale,
                    slope: 0.0,
                    phase: 0.0,
                }),
            },
        );
        graph.surfaces.insert(
            component.surface,
            B5Surface::Sphere {
                center: component.center,
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius,
                azimuth_range: [0.0, std::f64::consts::TAU],
                latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
                construction_radius: chart_scale,
                chart_origin: 0.0,
            },
        );
        let points = [
            [
                component.center[0] + radius,
                component.center[1],
                component.center[2],
            ],
            [
                component.center[0],
                component.center[1] + radius,
                component.center[2],
            ],
            [
                component.center[0] - radius,
                component.center[1],
                component.center[2],
            ],
        ];
        for (row, point) in component.vertices.into_iter().zip(points) {
            assert_eq!(row, graph.vertex_points.len(), "contiguous vertex rows");
            graph.vertex_points.push(point);
        }
        for (position, edge) in component.edges.into_iter().enumerate() {
            graph.edge_vertices.insert(
                edge,
                [
                    component.vertices[position],
                    component.vertices[(position + 1) % 3],
                ],
            );
        }
    }
    graph
}

/// B5 object ids carry an unpadded decimal key, so a face pair such as
/// `#9`/`#10` reaches the neutral model in ascending native order while
/// sorting the other way. The route must still produce an admissible model.
#[test]
fn decimal_object_id_keys_transfer_to_an_admissible_model() {
    let graph = synthetic_spherical_graph(&[
        SyntheticSphericalComponent {
            face: 9,
            loop_: 29,
            pcurve: 39,
            surface: 2,
            edges: [49, 50, 51],
            vertices: [0, 1, 2],
            center: [0.0, 0.0, 0.0],
        },
        SyntheticSphericalComponent {
            face: 10,
            loop_: 200,
            pcurve: 300,
            surface: 12,
            edges: [400, 401, 402],
            vertices: [3, 4, 5],
            center: [100.0, 0.0, 0.0],
        },
    ]);

    let mut ir = CadIr::empty();
    assert!(transfer(
        &mut ir,
        &mut AnnotationBuilder::new(),
        graph,
        &UnknownId("catia:payload:unknown#test".to_string()),
    ));

    // Native traversal order, which the arena-order check reads as unsorted.
    assert_eq!(
        ir.model
            .faces
            .iter()
            .map(|face| face.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:face#9", "catia:b5:face#10"]
    );
    assert_eq!(ir.model.loops.len(), 2);
    assert_eq!(ir.model.shells.len(), 2);
    assert_eq!(ir.model.regions.len(), 2);
    assert_eq!(ir.model.edges.len(), 6);
    assert_eq!(ir.model.coedges.len(), 6);
    assert_eq!(ir.model.vertices.len(), 6);
    assert_eq!(ir.model.pcurves.len(), 2);
    let unsorted_arenas = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .filter(|finding| finding.check == cadmpeg_ir::report::Check::ArenaOrder)
        .count();
    assert!(
        unsorted_arenas >= 6,
        "one component cannot unsort this many arenas: {unsorted_arenas}"
    );

    assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    assert_eq!(
        ir.model
            .faces
            .iter()
            .map(|face| face.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:face#10", "catia:b5:face#9"]
    );
}

#[test]
fn torus_chart_lifts_meridians_and_latitudes_exactly() {
    let torus = B5Surface::Torus {
        center: [0.0, 0.0, 0.0],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        major_radius: 5.0,
        minor_radius: 2.0,
        major_angular_range: [0.0, std::f64::consts::TAU],
        major_angular_domain: [0.0, std::f64::consts::TAU],
        minor_angular_range: [0.0, std::f64::consts::TAU],
        minor_angular_domain: [0.0, std::f64::consts::TAU],
        major_scale: 5.0,
        minor_scale: 2.0,
    };
    let base = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [0.0, 4.0 * std::f64::consts::PI]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    assert_eq!(
        neutral_pcurve_point([5.0 * std::f64::consts::PI, 2.0], &torus),
        Point2::new(std::f64::consts::PI, 1.0)
    );
    let Some(CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    }) = lifted_curve_geometry(&base, &torus)
    else {
        panic!("expected meridian circle");
    };
    assert_eq!(center, Point3::new(5.0, 0.0, 0.0));
    assert_eq!(axis, Vector3::new(0.0, -1.0, 0.0));
    assert_eq!(radius, 2.0);

    let latitude = B5Pcurve {
        control_points: vec![[0.0, 0.0], [10.0 * std::f64::consts::PI, 0.0]],
        ..base
    };
    let Some(CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    }) = lifted_curve_geometry(&latitude, &torus)
    else {
        panic!("expected latitude circle");
    };
    assert_eq!(center, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(radius, 7.0);
}

#[test]
fn tensor_surface_contraction_preserves_exact_isocurve() {
    let surface = cadmpeg_ir::geometry::NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 2.0),
        ],
        None,
        false,
        false,
        false,
    )
    .expect("valid tensor surface");
    let curve = crate::nurbs::nurbs_surface_isocurve(&surface, 0.25, true).expect("u isocurve");
    assert_eq!(curve.degree(), 1);
    assert_eq!(curve.knots(), surface.v_knots());
    assert_eq!(curve.control_points()[0], Point3::new(0.5, 0.0, 0.0));
    assert_eq!(curve.control_points()[1], Point3::new(0.5, 1.0, 0.5));
}

#[test]
fn affine_cylinder_pcurve_preserves_exact_helix_construction() {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 3.0], [4.0, 7.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let cylinder = B5Surface::Cylinder {
        origin: [0.0, 0.0, 0.0],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 2.0,
        u_range: [0.0, 4.0 * std::f64::consts::PI],
        v_range: [-1.0, 1.0],
        angular_scale: 2.0,
        chart_origin: 0.0,
    };
    let end = [2.0 * 2.0_f64.cos(), 2.0 * 2.0_f64.sin(), 7.0];
    let Some(plan) = cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], [2.0, 0.0, 3.0], end) else {
        panic!("degree-one cylinder helix");
    };
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        pitch,
        apex_factor,
        ..
    } = &plan.definition
    else {
        unreachable!();
    };
    assert_eq!(*angle_range, [0.0, 2.0]);
    assert_eq!(*center, Point3::new(0.0, 0.0, 3.0));
    assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1.0e-12);
    assert_eq!(*apex_factor, 0.0);
    assert_eq!(plan.parameter_range, [0.0, 2.0]);
    assert!(plan.fit_tolerance <= 1e-4);
    assert_eq!(
        plan.cache.control_points().first(),
        Some(&Point3::new(2.0, 0.0, 3.0))
    );

    assert!(
        cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], end, [2.0, 0.0, 3.0]).is_none(),
        "the native edge endpoint order is authoritative"
    );

    let trimmed_start = [2.0 * 0.5_f64.cos(), 2.0 * 0.5_f64.sin(), 4.0];
    let trimmed_end = [2.0 * 1.5_f64.cos(), 2.0 * 1.5_f64.sin(), 6.0];
    let trimmed = cylinder_helix(&pcurve, &cylinder, [0.25, 0.75], trimmed_start, trimmed_end)
        .expect("trimmed physical edge helix");
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        pitch,
        ..
    } = trimmed.definition
    else {
        unreachable!();
    };
    assert_eq!(angle_range, [0.0, 1.0]);
    assert_eq!(center.z, 4.0);
    assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1.0e-12);

    let tiny = 1e-14;
    let tiny_pcurve = B5Pcurve {
        control_points: vec![[0.0, 0.0], [2.0 * tiny, 2.0 * tiny]],
        ..pcurve
    };
    let tiny_end = [2.0 * tiny.cos(), 2.0 * tiny.sin(), 2.0 * tiny];
    let tiny_plan = cylinder_helix(
        &tiny_pcurve,
        &cylinder,
        [0.0, 1.0],
        [2.0, 0.0, 0.0],
        tiny_end,
    )
    .expect("tiny helix sweep");
    let ProceduralCurveDefinition::Helix {
        angle_range, pitch, ..
    } = tiny_plan.definition
    else {
        unreachable!();
    };
    assert_eq!(angle_range, [0.0, tiny]);
    assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1.0e-12);
}
