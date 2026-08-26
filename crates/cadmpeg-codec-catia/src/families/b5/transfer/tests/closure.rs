#![allow(unused_imports)]

use super::super::super::graph::{
    bounded_occurrence_range, edge_pcurve_parameters, loop_chain_closes, pcurve_parameter_domain,
    B5ExtrusionDirectrix, B5ExtrusionSurface, B5Face, B5Graph, B5Loop, B5LoopMetadata,
    B5OffsetSurface, B5OpaquePcurve, B5ParameterIncidence, B5Pcurve, B5PcurveParameterization,
    B5Profile, B5SphereGreatCirclePcurve, B5SupportedSurface, B5SupportedSurfaceParameters,
    B5Surface,
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
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn unit_preserves_tiny_finite_direction() {
    assert_eq!(unit([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
    assert_eq!(unit([0.0, 0.0, 0.0]), None);
}

#[test]
fn affine_curve_ranges_reparameterize_without_changing_geometry() {
    let nurbs = NurbsCurve {
        degree: 1,
        knots: vec![10.0, 10.0, 20.0, 20.0],
        control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        weights: None,
        periodic: false,
    };
    let CurveGeometry::Nurbs(translated) = curve_on_parameter_range(
        CurveGeometry::Nurbs(nurbs.clone()),
        [10.0, 20.0],
        [0.0, 10.0],
    )
    .expect("equal-span NURBS translation") else {
        unreachable!();
    };
    assert_eq!(translated.knots, [0.0, 0.0, 10.0, 10.0]);
    assert_eq!(translated.control_points, nurbs.control_points);

    let line = CurveGeometry::Line {
        origin: Point3::new(10.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    assert_eq!(
        curve_on_parameter_range(line, [10.0, 20.0], [0.0, 10.0]),
        Some(CurveGeometry::Line {
            origin: Point3::new(20.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    assert_eq!(
        curve_on_parameter_range(
            CurveGeometry::Nurbs(nurbs.clone()),
            [10.0, 20.0],
            [12.0, 18.0],
        ),
        Some(CurveGeometry::Nurbs(nurbs.clone()))
    );
    let CurveGeometry::Nurbs(scaled) =
        curve_on_parameter_range(CurveGeometry::Nurbs(nurbs), [10.0, 20.0], [0.0, 2.0])
            .expect("positive affine NURBS mapping")
    else {
        unreachable!();
    };
    assert_eq!(scaled.knots, [0.0, 0.0, 2.0, 2.0]);
    assert_eq!(
        curve_on_parameter_range(
            CurveGeometry::Line {
                origin: Point3::new(10.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            [10.0, 20.0],
            [0.0, 2.0],
        ),
        Some(CurveGeometry::Line {
            origin: Point3::new(20.0, 0.0, 0.0),
            direction: Vector3::new(5.0, 0.0, 0.0),
        })
    );
}

#[test]
fn explicit_pcurve_range_must_be_a_subrange_of_its_knot_domain() {
    let mut pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![0.0, 10.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [1.0, 0.0]],
        weights: None,
        parameter_range: Some([2.0, 8.0]),
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    assert_eq!(pcurve_parameter_domain(&pcurve), Some([2.0, 8.0]));
    pcurve.parameter_range = None;
    assert_eq!(pcurve_parameter_domain(&pcurve), Some([0.0, 10.0]));
    pcurve.parameter_range = Some([-1.0, 8.0]);
    assert_eq!(pcurve_parameter_domain(&pcurve), None);
}

#[test]
fn support_bound_surface_closure_includes_carrier_supports_and_offsets() {
    let offsets = BTreeMap::from([(
        30,
        B5OffsetSurface {
            object_id: 30,
            carrier_surface: 31,
            source_surface: 50,
            distance: 1.0,
            carrier_kind: 2,
            parameter_bounds: [[0.0, 1.0], [0.0, 1.0]],
        },
    )]);
    let supported = BTreeMap::from([(
        10,
        B5SupportedSurface {
            object_id: 10,
            carrier_surface: 20,
            support_surfaces: [30, 40],
            support_pcurves: [60, 70],
            parameters: B5SupportedSurfaceParameters::Radius {
                controls: [1; 6],
                construction_radius: 2.0,
            },
        },
    )]);
    let extrusions = BTreeMap::from([(
        50,
        B5ExtrusionSurface {
            object_id: 50,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[0.0, 1.0], [0.0, 2.0]],
            directrix: B5ExtrusionDirectrix::Intersection {
                object_id: 80,
                supports: [(90, 91, [0.0, 1.0]), (100, 101, [0.0, 1.0])],
                parameter_range: [0.0, 1.0],
                cache_fit_tolerance: 1.0e-6,
            },
        },
    )]);

    assert_eq!(
        referenced_surface_ids([10], &offsets, &supported, &extrusions, &BTreeMap::new(),),
        HashSet::from([10, 20, 30, 31, 40, 50, 90, 100])
    );
}

#[test]
fn surface_closure_follows_aliases_to_native_constructions() {
    let offsets = BTreeMap::from([(
        20,
        B5OffsetSurface {
            object_id: 20,
            carrier_surface: 30,
            source_surface: 40,
            distance: 2.0,
            carrier_kind: 2,
            parameter_bounds: [[0.0, 1.0], [0.0, 2.0]],
        },
    )]);
    let aliases = BTreeMap::from([(10, 11), (11, 20)]);

    assert_eq!(
        referenced_surface_ids([10], &offsets, &BTreeMap::new(), &BTreeMap::new(), &aliases,),
        HashSet::from([10, 30, 40])
    );
}

#[test]
fn occurrence_interval_orders_and_bounds_native_stations() {
    assert_eq!(ordered_subrange([8.0, 2.0], [0.0, 10.0]), Some([2.0, 8.0]));
    assert_eq!(
        ordered_subrange([-5e-10, 10.0 + 5e-10], [0.0, 10.0]),
        Some([0.0, 10.0])
    );
    assert!(ordered_subrange([2.0, 2.0], [0.0, 10.0]).is_none());
    assert!(ordered_subrange([-2e-9, 8.0], [0.0, 10.0]).is_none());
    assert!(ordered_subrange([2.0, 12.0], [0.0, 10.0]).is_none());
    assert_eq!(
        bounded_occurrence_range([8.0, 2.0], [0.0, 10.0]),
        Some([8.0, 2.0])
    );

    let tiny = 1e-200_f64;
    assert_eq!(
        bounded_occurrence_range([0.0, tiny], [0.0, tiny]),
        Some([0.0, tiny])
    );
    assert!(bounded_occurrence_range([0.0, 2.0 * tiny], [0.0, tiny]).is_none());
    assert!(bounded_occurrence_range([0.0, tiny], [tiny, 0.0]).is_none());
}

#[test]
fn edge_parameters_follow_ordered_edge_refs_for_a_closed_vertex() {
    let mut graph = B5Graph {
        complete: false,
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
        parameter_incidences: BTreeMap::from([
            (
                40,
                B5ParameterIncidence {
                    object_id: 40,
                    curves: vec![20],
                    parameters: vec![0.0],
                    controls: vec![0],
                },
            ),
            (
                41,
                B5ParameterIncidence {
                    object_id: 41,
                    curves: vec![20],
                    parameters: vec![1.0],
                    controls: vec![0],
                },
            ),
        ]),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: vec![[0.0, 0.0, 0.0]],
        logical_vertex_refs: vec![50],
        edge_vertices: BTreeMap::from([(30, [0, 0])]),
        edge_parameter_incidences: BTreeMap::from([(30, [40, 41])]),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };

    assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([0.0, 1.0]));
    graph.edge_parameter_incidences.insert(30, [41, 40]);
    assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([1.0, 0.0]));
}

/// An incomplete graph keeps the face whose loop members all carry vertex
/// loci and excludes the face whose members carry none, so that a carrier
/// without recoverable geometry cannot pull invented vertices into the
/// neutral model.
#[test]
fn incomplete_graph_excludes_a_face_whose_members_have_no_vertex_loci() {
    let plane = |v_offset: f64| B5Surface::Plane {
        origin: [0.0, v_offset, 0.0],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let line_pcurve = |object_id: u32, surface: u32| B5Pcurve {
        object_id,
        surface,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [1.0, 0.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let incidence = |object_id: u32, curve: u32, parameter: f64| B5ParameterIncidence {
        object_id,
        curves: vec![curve],
        parameters: vec![parameter],
        controls: vec![0],
    };
    let graph = B5Graph {
        complete: false,
        faces: vec![
            B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
                terminal_control: None,
            },
            B5Face {
                object_id: 3,
                surface: 11,
                loops: vec![4],
                terminal_control: None,
            },
        ],
        face_records: BTreeMap::new(),
        loops: BTreeMap::from([
            (
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![20, 20, 20],
                    edges: vec![30, 31, 32],
                    metadata: test_loop_metadata(3),
                    surface: 10,
                },
            ),
            (
                4,
                B5Loop {
                    object_id: 4,
                    pcurves: vec![21, 21, 21],
                    edges: vec![33, 34, 35],
                    metadata: test_loop_metadata(3),
                    surface: 11,
                },
            ),
        ]),
        pcurves: BTreeMap::from([(20, line_pcurve(20, 10)), (21, line_pcurve(21, 11))]),
        opaque_pcurves: BTreeMap::new(),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::from([(10, plane(0.0)), (11, plane(5.0))]),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::from([
            (40, incidence(40, 20, 0.0)),
            (41, incidence(41, 20, 0.5)),
            (42, incidence(42, 20, 1.0)),
        ]),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_refs: vec![50, 51, 52],
        // Edges 33, 34, and 35 have no entry: their carrier resolves no
        // endpoint locus, which is what excludes face 3.
        edge_vertices: BTreeMap::from([(30, [0, 1]), (31, [1, 2]), (32, [2, 0])]),
        edge_parameter_incidences: BTreeMap::from([(30, [40, 41]), (31, [41, 42]), (32, [42, 40])]),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };
    let mut ir = CadIr::empty(Units::default());

    assert!(transfer(
        &mut ir,
        &mut AnnotationBuilder::new(),
        graph,
        &UnknownId("catia:test-payload".to_string()),
    ));
    assert_eq!(
        ir.model
            .faces
            .iter()
            .map(|face| face.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:face#1"]
    );
    assert_eq!(
        ir.model
            .loops
            .iter()
            .map(|loop_| loop_.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:loop#2"]
    );
    assert_eq!(ir.model.coedges.len(), 3);
    assert!(!ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.0.contains("#11")));
}

#[test]
fn repeated_source_pcurve_retains_occurrence_ranges_and_directions() {
    let mut graph = B5Graph {
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
                pcurves: vec![20, 20, 20],
                edges: vec![30, 31, 32],
                metadata: test_loop_metadata(3),
                surface: 10,
            },
        )]),
        pcurves: BTreeMap::from([(
            20,
            B5Pcurve {
                object_id: 20,
                surface: 10,
                degree: 1,
                distinct_knots: vec![0.0, 1.0],
                multiplicities: vec![2, 2],
                control_points: vec![[0.0, 0.0], [1.0, 0.0]],
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
            B5Surface::Plane {
                origin: [0.0, 0.0, 0.0],
                direction_u: [1.0, 0.0, 0.0],
                direction_v: [0.0, 1.0, 0.0],
                u_range: [-1.0, 1.0],
                v_range: [-1.0, 1.0],
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
                    parameters: vec![0.0],
                    controls: vec![0],
                },
            ),
            (
                41,
                B5ParameterIncidence {
                    object_id: 41,
                    curves: vec![20],
                    parameters: vec![0.5],
                    controls: vec![0],
                },
            ),
            (
                42,
                B5ParameterIncidence {
                    object_id: 42,
                    curves: vec![20],
                    parameters: vec![1.0],
                    controls: vec![0],
                },
            ),
        ]),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_refs: vec![50, 51, 52],
        edge_vertices: BTreeMap::from([(30, [0, 1]), (31, [1, 2]), (32, [2, 0])]),
        edge_parameter_incidences: BTreeMap::from([(30, [40, 41]), (31, [41, 42]), (32, [42, 40])]),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };
    graph
        .loops
        .get_mut(&2)
        .expect("required loop")
        .metadata
        .edge_controls[1][2] = -1;
    let mut ir = CadIr::empty(Units::default());

    assert!(transfer(
        &mut ir,
        &mut AnnotationBuilder::new(),
        graph,
        &UnknownId("catia:test-payload".to_string()),
    ));
    assert_eq!(ir.model.pcurves.len(), 3);
    assert_eq!(ir.model.coedges.len(), 3);
    assert_eq!(
        ir.model
            .points
            .iter()
            .map(|point| point
                .source_object
                .as_ref()
                .map(|source| source.object_id.as_str()))
            .collect::<Vec<_>>(),
        [
            Some("cgm-vertex:000032"),
            Some("cgm-vertex:000033"),
            Some("cgm-vertex:000034"),
        ]
    );
    assert_eq!(
        ir.model
            .pcurves
            .iter()
            .map(|pcurve| pcurve.parameter_range)
            .collect::<Vec<_>>(),
        [Some([0.0, 0.5]), Some([0.0, 1.0]), Some([0.5, 1.0])]
    );
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.as_str()))
            .collect::<Vec<_>>(),
        [
            "catia:b5:pcurve#20@0",
            "catia:b5:pcurve#20@2",
            "catia:b5:pcurve#20@1",
        ]
    );
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .map(|coedge| coedge.pcurves[0].parameter_range)
            .collect::<Vec<_>>(),
        [None, Some([1.0, 0.5]), None]
    );
    assert_eq!(ir.model.loops.len(), 1);
    assert_eq!(
        ir.model.loops[0]
            .vertex_uses
            .iter()
            .map(|use_| use_.vertex.0.as_str())
            .collect::<Vec<_>>(),
        [
            "catia:b5:vertex#1",
            "catia:b5:vertex#2",
            "catia:b5:vertex#0"
        ]
    );
    assert_eq!(
        ir.model.loops[0]
            .vertex_uses
            .iter()
            .map(|use_| use_.after.as_ref().map(|coedge| coedge.0.as_str()))
            .collect::<Vec<_>>(),
        [
            Some("catia:b5:coedge#2-0"),
            Some("catia:b5:coedge#2-1"),
            Some("catia:b5:coedge#2-2")
        ]
    );
}

#[test]
fn edge_supports_preserve_one_sided_and_intersection_constructions() {
    let surfaces = HashMap::from([
        (10, SurfaceId("surface-10".to_string())),
        (11, SurfaceId("surface-11".to_string())),
    ]);
    let pcurve_20 = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let pcurve_21 = PcurveGeometry::Line {
        origin: Point2::new(0.0, 1.0),
        direction: Point2::new(1.0, 0.0),
    };
    let pcurves = BTreeMap::from([
        (20, (pcurve_20.clone(), false, [2.0, 4.0])),
        (21, (pcurve_21.clone(), false, [2.0, 5.0])),
    ]);
    let (_, _, one_sided) =
        b5_edge_support_definition(&[(10, 20, [2.0, 4.0])], &surfaces, &pcurves, None)
            .expect("one-sided surface curve");
    assert!(matches!(
        one_sided,
        ProceduralCurveDefinition::SurfaceCurve { context, .. }
            if context.parameter_range == [2.0, 4.0]
                && context.sides[0].surface == Some(surfaces[&10].clone())
                && context.sides[0].pcurve == Some(pcurve_20)
                && context.sides[1].surface.is_none()
    ));

    let (_, _, intersection) = b5_edge_support_definition(
        &[(10, 20, [2.0, 4.0]), (11, 21, [2.0, 4.0])],
        &surfaces,
        &pcurves,
        None,
    )
    .expect("two-sided intersection");
    assert!(matches!(
        intersection,
        ProceduralCurveDefinition::Intersection { context, .. }
            if context.parameter_range == [2.0, 4.0]
                && context.sides[1].surface == Some(surfaces[&11].clone())
                && context.sides[1].pcurve == Some(pcurve_21)
                && context.sides.iter().all(|side| side.pcurve_parameter_range.is_none())
    ));
    let (_, _, independently_parameterized) = b5_edge_support_definition(
        &[(10, 20, [2.0, 4.0]), (11, 21, [5.0, 2.0])],
        &surfaces,
        &pcurves,
        None,
    )
    .expect("independently parameterized intersection");
    assert!(matches!(
        independently_parameterized,
        ProceduralCurveDefinition::Intersection { context, .. }
            if context.parameter_range == [0.0, 1.0]
                && context.sides[0].pcurve_parameter_range == Some([2.0, 4.0])
                && context.sides[1].pcurve_parameter_range == Some([5.0, 2.0])
    ));
    let (_, _, distance_parameterized) = b5_edge_support_definition(
        &[(10, 20, [2.0, 4.0])],
        &surfaces,
        &pcurves,
        Some([0.0, 8.0]),
    )
    .expect("distance-parameterized surface curve");
    assert!(matches!(
        distance_parameterized,
        ProceduralCurveDefinition::SurfaceCurve { context, .. }
            if context.parameter_range == [0.0, 8.0]
                && context.sides[0].pcurve_parameter_range == Some([2.0, 4.0])
    ));
}

#[test]
fn procedural_support_requires_physical_edge_endpoint_agreement() {
    let plane = || SurfacePlan {
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        procedure: None,
    };
    let surfaces = BTreeMap::from([(10, plane()), (11, plane())]);
    let pcurves = BTreeMap::from([
        (
            20,
            (
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
                false,
                [0.0, 1.0],
            ),
        ),
        (
            21,
            (
                PcurveGeometry::Line {
                    origin: Point2::new(1.0, 0.0),
                    direction: Point2::new(-1.0, 0.0),
                },
                false,
                [0.0, 1.0],
            ),
        ),
    ]);
    let supports = [(10, 20, [0.0, 1.0])];
    assert!(b5_supports_follow_edge(
        &supports,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [1.5e-3; 2],
        &surfaces,
        &pcurves,
    ));
    assert!(!b5_supports_follow_edge(
        &supports,
        [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        [1.5e-3; 2],
        &surfaces,
        &pcurves,
    ));
    assert!(!b5_supports_follow_edge(
        &supports,
        [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        [1.5e-3; 2],
        &surfaces,
        &pcurves,
    ));
    let mut reversed_supports = [(10, 20, [1.0, 0.0])];
    orient_b5_supports_to_edge(
        &mut reversed_supports,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [1.5e-3; 2],
        &surfaces,
        &pcurves,
    );
    assert_eq!(reversed_supports[0].2, [0.0, 1.0]);
    assert!(b5_supports_follow_edge(
        &reversed_supports,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [1.5e-3; 2],
        &surfaces,
        &pcurves,
    ));
    let mut tolerance_ambiguous_supports = [(10, 20, [1.0, 0.0])];
    orient_b5_supports_to_edge(
        &mut tolerance_ambiguous_supports,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [1.01; 2],
        &surfaces,
        &pcurves,
    );
    assert_eq!(tolerance_ambiguous_supports[0].2, [0.0, 1.0]);
    let mut oppositely_parameterized_supports = [(10, 20, [0.0, 1.0]), (11, 21, [0.0, 1.0])];
    orient_b5_supports_to_edge(
        &mut oppositely_parameterized_supports,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [1.01; 2],
        &surfaces,
        &pcurves,
    );
    assert_eq!(oppositely_parameterized_supports[1].2, [1.0, 0.0]);
    assert!(b5_supports_agree(
        &oppositely_parameterized_supports,
        &surfaces,
        &pcurves,
    ));
    assert!(b5_supports_follow_edge(
        &supports,
        [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        [1.01; 2],
        &surfaces,
        &pcurves,
    ));
}

#[test]
fn descending_nurbs_knots_are_not_promoted_as_curve_caches() {
    let geometry = CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots: vec![1.0, 1.0, 0.0, 0.0],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        weights: None,
        periodic: false,
    });
    assert!(!curve_cache_has_ordered_knots(&geometry));
}

#[test]
fn exact_revolution_builders_reject_unbounded_subdivision_counts() {
    assert!(rational_arc(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        1.0e-300,
        [0.0, 1.0],
    )
    .is_none());
    let profile = cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0)],
        weights: None,
        periodic: false,
    };
    assert!(revolve_nurbs(
        &profile,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [0.0, 1.0e300],
        [0.0, 1.0],
    )
    .is_none());
    let mut wide_profile = profile;
    wide_profile.control_points = vec![Point3::new(1.0, 0.0, 0.0); 123];
    assert!(revolve_nurbs(
        &wide_profile,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [0.0, 4096.0 * std::f64::consts::FRAC_PI_2],
        [0.0, 1.0],
    )
    .is_none());
}

#[test]
fn body_kind_requires_unique_complete_loop_ownership() {
    let mut graph = B5Graph {
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
                pcurves: vec![4],
                edges: vec![3],
                metadata: test_loop_metadata(1),
                surface: 10,
            },
        )]),
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
        vertex_points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
        logical_vertex_points: Vec::new(),
        logical_vertex_refs: Vec::new(),
        edge_vertices: BTreeMap::from([(3, [0, 1])]),
        edge_parameter_incidences: BTreeMap::new(),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };

    assert_eq!(
        ownership_plan(&graph)
            .expect("required invariant")
            .body_kind,
        BodyKind::Sheet
    );
    graph.faces[0].loops.push(2);
    assert!(ownership_plan(&graph).is_none());
    graph.faces[0].loops.pop();
    graph.faces.push(B5Face {
        object_id: 5,
        surface: 10,
        loops: vec![2],
        terminal_control: None,
    });
    assert!(ownership_plan(&graph).is_none());
    graph.faces.pop();

    graph.faces.push(B5Face {
        object_id: 5,
        surface: 10,
        loops: vec![6],
        terminal_control: None,
    });
    graph.loops.insert(
        6,
        B5Loop {
            object_id: 6,
            pcurves: vec![8],
            edges: vec![7],
            metadata: test_loop_metadata(1),
            surface: 10,
        },
    );
    graph.edge_vertices.insert(7, [0, 1]);
    let ownership = ownership_plan(&graph).expect("required invariant");
    assert_eq!(ownership.face_components, vec![0, 1]);
    assert_eq!(ownership.components.len(), 2);
    assert_eq!(ownership.body_kind, BodyKind::Sheet);
    assert_eq!(ownership.loop_owners.get(&2), Some(&0));
    assert_eq!(ownership.loop_owners.get(&6), Some(&1));

    graph
        .loops
        .get_mut(&2)
        .expect("required invariant")
        .edges
        .push(3);
    assert_eq!(
        ownership_plan(&graph)
            .expect("required invariant")
            .body_kind,
        BodyKind::General
    );
    graph
        .loops
        .get_mut(&2)
        .expect("required invariant")
        .edges
        .pop();

    graph.loops.get_mut(&6).expect("required invariant").edges[0] = 3;
    let ownership = ownership_plan(&graph).expect("required invariant");
    assert_eq!(ownership.face_components, vec![0, 0]);
    assert_eq!(ownership.components.len(), 1);
    assert_eq!(ownership.body_kind, BodyKind::Solid);

    graph.faces.pop();
    graph.loops.remove(&6);
    graph.edge_vertices.remove(&7);
    graph.edge_vertices.insert(3, [0, 2]);
    assert!(ownership_plan(&graph).is_none());
}

#[test]
fn loop_orientation_reverses_member_order_and_rejects_frustrated_parity() {
    let loop_ = |object_id: u32, edges: Vec<u32>| B5Loop {
        object_id,
        pcurves: vec![0; edges.len()],
        metadata: test_loop_metadata(edges.len()),
        edges,
        surface: 10,
    };
    let mut graph = B5Graph {
        complete: true,
        faces: Vec::new(),
        face_records: BTreeMap::new(),
        loops: BTreeMap::from([(1, loop_(1, vec![3])), (2, loop_(2, vec![4, 5, 3]))]),
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
    graph
        .loops
        .get_mut(&2)
        .expect("required loop")
        .metadata
        .edge_controls[1][2] = -1;
    let orientation = orient_loop_members(
        &graph,
        BTreeMap::from([(1, vec![false]), (2, vec![false; 3])]),
    )
    .expect("required invariant");
    assert_eq!(orientation[&1].member_order, vec![0]);
    assert_eq!(orientation[&2].member_order, vec![2, 1, 0]);
    assert_eq!(orientation[&1].reversed, vec![false]);
    assert_eq!(orientation[&2].reversed, vec![true; 3]);
    assert_eq!(orientation[&1].pcurve_reversed, vec![false]);
    assert_eq!(orientation[&2].pcurve_reversed, vec![true, false, true]);

    graph.loops = BTreeMap::from([
        (1, loop_(1, vec![1, 3])),
        (2, loop_(2, vec![1, 2])),
        (3, loop_(3, vec![2, 3])),
    ]);
    assert!(orient_loop_members(
        &graph,
        BTreeMap::from([
            (1, vec![false; 2]),
            (2, vec![false; 2]),
            (3, vec![false; 2]),
        ]),
    )
    .is_none());
}

#[test]
fn emitted_carriers_determine_logical_vertex_tolerance() {
    let graph = B5Graph {
        complete: true,
        faces: Vec::new(),
        face_records: BTreeMap::new(),
        loops: BTreeMap::from([(
            1,
            B5Loop {
                object_id: 1,
                pcurves: vec![2],
                edges: vec![3],
                metadata: test_loop_metadata(1),
                surface: 4,
            },
        )]),
        pcurves: BTreeMap::new(),
        opaque_pcurves: BTreeMap::new(),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::from([
            (
                20,
                B5ParameterIncidence {
                    object_id: 20,
                    curves: vec![2],
                    parameters: vec![0.25],
                    controls: vec![0],
                },
            ),
            (
                21,
                B5ParameterIncidence {
                    object_id: 21,
                    curves: vec![2],
                    parameters: vec![0.75],
                    controls: vec![0],
                },
            ),
        ]),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: vec![[0.25, 0.0, 1e-4], [0.75, 0.0, 0.0]],
        logical_vertex_refs: vec![10, 11],
        edge_vertices: BTreeMap::from([(3, [0, 1])]),
        edge_parameter_incidences: BTreeMap::from([(3, [20, 21])]),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::new(),
    };
    let pcurves = BTreeMap::from([(
        2,
        (
            PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                weights: None,
                periodic: false,
            },
            false,
            [0.0, 1.0],
        ),
    )]);
    let surfaces = BTreeMap::from([(
        4,
        SurfacePlan {
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            procedure: None,
        },
    )]);
    let supports = HashMap::from([(3, vec![(4, 2, [0.25, 0.75])])]);

    let tolerances = transfer_vertex_tolerances(&graph, &supports, &surfaces, &pcurves);
    assert!((tolerances[&0] - (1e-4 + 1.0e-9)).abs() < 1.0e-12);
    assert!(!tolerances.contains_key(&1));
}
