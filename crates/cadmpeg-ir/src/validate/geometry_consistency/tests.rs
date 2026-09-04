// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::document::CadIr;
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsSurface, Pcurve,
    PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::topology::{Coedge, Edge, Face, Loop, LoopBoundaryRole, PcurveUse, Sense, Vertex};
use crate::validate::validate_neutral;

macro_rules! procedural_surface {
    (
            id: $id:expr,
            definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr,
        record_bounds: $record_bounds:expr $(,)?
    ) => {
        ProceduralSurface::try_new($id, $definition, $cache_fit_tolerance, $record_bounds)
            .expect("valid procedural surface fixture")
    };
}

macro_rules! procedural_curve {
    (
            id: $id:expr,
            definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr $(,)?
    ) => {
        ProceduralCurve::try_new($id, $definition, $cache_fit_tolerance)
            .expect("valid procedural curve fixture")
    };
}

fn mapped_surface_curve(mapping: [f64; 2]) -> CadIr {
    let mut ir = CadIr::empty();
    let curve = CurveId("curve".to_string());
    let surface = SurfaceId("surface".to_string());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let construction = procedural_curve! {
        id: ProceduralCurveId("surface-curve".to_string()),
        definition: ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(surface),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                        pcurve_parameter_range: Some(mapping),
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                tail: None,
            },
        },
        cache_fit_tolerance: None,
    };
    ir.model.add_procedural_curve(curve, construction).unwrap();
    ir
}

fn mapped_surface_offset() -> CadIr {
    let mut ir = mapped_surface_curve([2.0, 3.0]);
    let base = CurveId("base".to_string());
    *ir.model.curves[0]
        .geometry
        .solved_cache_mut()
        .expect("mapped curve has a solved cache") = CurveGeometry::Line {
        origin: Point3::new(2.0, 0.0, 25.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    ir.model.curves.push(Curve {
        id: base.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let ProceduralCurveDefinition::SurfaceCurve { family } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!();
    };
    let context = family.context().clone();
    ir.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag: false,
        base_u_range: [0.0, 1.0],
        base_v_range: [0.0, 1.0],
        base,
        base_range: [2.0, 3.0],
        base_endpoints: [Some(2.0), Some(3.0)],
        cache_first: None,
        distance: 25.0,
        shift: 0.0,
        scale: 1.0,
    });
    ir
}

fn untrimmed_surface_curve() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.points.extend([
        crate::topology::Point {
            id: "point-start".into(),
            position: Point3::new(0.0, 1.0, 0.0),
            source_object: None,
        },
        crate::topology::Point {
            id: "point-end".into(),
            position: Point3::new(-1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: "vertex-start".into(),
            point: "point-start".into(),
            tolerance: None,
        },
        Vertex {
            id: "vertex-end".into(),
            point: "point-end".into(),
            tolerance: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: "curve".into(),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        },
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: "edge".into(),
        curve: Some("curve".into()),
        start: "vertex-start".into(),
        end: "vertex-end".into(),
        param_range: None,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: "surface".into(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.pcurves.push(Pcurve {
        id: "pcurve".into(),
        geometry: PcurveGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 1.0,
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    });
    ir.model.coedges.push(Coedge {
        id: "coedge".into(),
        owner_loop: "loop".into(),
        edge: "edge".into(),
        next: "coedge".into(),
        previous: "coedge".into(),
        radial_next: "coedge".into(),
        sense: Sense::Forward,
        pcurves: vec![PcurveUse {
            pcurve: "pcurve".into(),
            isoparametric: None,
            parameter_range: None,
        }],
        use_curve: None,
    });
    ir.model.loops.push(Loop {
        id: "loop".into(),
        face: "face".into(),
        boundary_role: LoopBoundaryRole::Outer,
        boundary: crate::topology::LoopBoundary::Ring {
            coedges: vec!["coedge".into()],
            vertex_uses: Vec::new(),
        },
    });
    ir.model.faces.push(Face {
        id: "face".into(),
        shell: "shell".into(),
        surface: "surface".into(),
        sense: Sense::Forward,
        loops: vec!["loop".into()],
        name: None,
        color: None,
        tolerance: None,
    });
    ir
}

#[test]
fn procedural_support_endpoints_honor_the_per_side_parameter_mapping() {
    let mut findings = Vec::new();
    check_procedural_support_consistency(&mapped_surface_curve([2.0, 3.0]), &mut findings);
    assert!(findings.is_empty());

    check_procedural_support_consistency(&mapped_surface_curve([3.0, 2.0]), &mut findings);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("support side 0"));
}

#[test]
fn surface_offset_support_constrains_the_embedded_base_curve() {
    let mut findings = Vec::new();
    check_procedural_support_consistency(&mapped_surface_offset(), &mut findings);
    assert!(findings.is_empty());

    let mut context_first = mapped_surface_offset();
    context_first.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::SurfaceOffset { base_endpoints, .. } = definition else {
            unreachable!();
        };
        *base_endpoints = [None, None];
    });
    check_procedural_support_consistency(&context_first, &mut findings);
    assert!(findings.is_empty());

    let mut ir = mapped_surface_offset();
    let CurveGeometry::Line { origin, .. } = &mut ir.model.curves[1].geometry else {
        unreachable!();
    };
    origin.y = 2.0;
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(findings.len(), 2);
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("base offset distance")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("support side 0")));
}

#[test]
fn untrimmed_pcurve_uses_a_vertex_derived_parameter_interval() {
    let ir = untrimmed_surface_curve();
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");

    let mut mismatched = ir;
    let PcurveGeometry::Circle { radius, .. } = &mut mismatched.model.pcurves[0].geometry else {
        unreachable!();
    };
    *radius = 2.0;
    super::check_pcurve_surface_consistency(&mismatched, &mut findings);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("pcurve mapped through"));
}

#[test]
fn trimmed_surface_pcurve_uses_the_local_parameterization_for_validation() {
    let mut ir = untrimmed_surface_curve();
    let base_id = SurfaceId("base-surface".into());
    let base_geometry = ir.model.surfaces[0].geometry.clone();
    ir.model.surfaces[0].id = base_id.clone();
    ir.model.surfaces.push(Surface {
        id: "surface".into(),
        geometry: base_geometry,
        source_object: None,
    });
    let construction = procedural_surface! {
        id: ProceduralSurfaceId("trimmed-surface".into()),
        definition: ProceduralSurfaceDefinition::Subset {
            support: base_id,
            parameter_ranges: [[2.0, 0.0], [0.0, 2.0]],
            u_sense: Some(false),
            v_sense: Some(true),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    };
    ir.model
        .add_procedural_surface(SurfaceId("surface".into()), construction)
        .unwrap();
    ir.model.points[0].position = Point3::new(1.0, 2.0, 0.0);
    ir.model.points[1].position = Point3::new(2.0, 1.0, 0.0);
    ir.model.curves[0].geometry = CurveGeometry::Circle {
        center: Point3::new(1.0, 1.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    ir.model.pcurves[0].geometry = PcurveGeometry::Circle {
        center: Point2::new(1.0, 1.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, 1.0),
        radius: 1.0,
    };
    ir.model.pcurves[0].parameter_range = Some([0.0, std::f64::consts::PI]);

    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn untrimmed_nurbs_pcurve_uses_its_own_endpoint_parameters() {
    let mut ir = untrimmed_surface_curve();
    ir.model.pcurves[0].geometry = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 1.0),
            Point2::new(-1.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0_f64.sqrt() / 2.0, 1.0]),
        periodic: false,
    };
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn stale_trimmed_pcurve_range_can_use_a_vertex_derived_interval() {
    let mut ir = untrimmed_surface_curve();
    ir.model.points[0].position = Point3::new(0.25, 0.0, 0.0);
    ir.model.points[1].position = Point3::new(0.0, 0.0, 0.0);
    ir.model.pcurves[0].geometry = PcurveGeometry::Trimmed {
        parameter_range: [0.0, 1.0],
        same_sense: true,
        basis: Box::new(PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(1.0, 0.0),
        }),
    };
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn raw_nurbs_domain_is_not_treated_as_edge_trim() {
    let pcurve = Pcurve {
        id: "pcurve".into(),
        geometry: PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![-1.0, 0.0, 0.0, 1.0, 1.0, 2.0],
            control_points: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(2.0, 0.0),
            ],
            weights: None,
            periodic: false,
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    };
    assert_eq!(pcurve_parameter_domain(&pcurve.geometry), Some([0.0, 1.0]));
    assert!(pcurve_parameter_ranges(&pcurve, None, None).is_none());
}

#[test]
fn collapsed_trimmed_pcurve_falls_back_to_its_basis_domain() {
    let geometry = PcurveGeometry::Trimmed {
        parameter_range: [1.0, 1.0],
        same_sense: true,
        basis: Box::new(PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            weights: None,
            periodic: true,
        }),
    };
    assert_eq!(pcurve_parameter_domain(&geometry), Some([0.0, 1.0]));
}

#[test]
fn line_pcurve_recovers_vertices_from_nurbs_surface_domain_seeds() {
    // At v=0 the quadratic surface has a zero derivative. The ordinary
    // seed at t=0 therefore cannot start Newton recovery; the finite
    // surface domain supplies an interior seed on the same branch.
    let surface = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 2,
        v_count: 3,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 1.0),
        ],
        weights: None,
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    });
    let surface_id = SurfaceId("surface".to_string());
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface.clone(),
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let pcurve = Pcurve {
        id: "pcurve".into(),
        geometry: PcurveGeometry::Line {
            origin: Point2::new(1.0, 0.0),
            direction: Point2::new(0.0, 1.0),
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    };
    let context = SurfacePcurveContext {
        index: &index,
        surface_id: &surface_id,
        geometry: &surface,
    };
    let seeds = pcurve_parameter_seeds_on_surface(&context, &pcurve);
    assert!(seeds.contains(&0.5));

    let ranges = edge_pcurve_parameter_ranges(
        &context,
        None,
        Point3::new(1.0, 0.0, 0.5625),
        Point3::new(1.0, 0.0, 0.0625),
        &pcurve,
        &pcurve,
        1.0e-12,
    )
    .expect("finite NURBS surface domain should provide recovery seeds");
    assert!(
        ranges.iter().any(|[start, end]| {
            (start - 0.75).abs() <= 1.0e-10 && (end - 0.25).abs() <= 1.0e-10
        }),
        "{ranges:?}"
    );
}

#[test]
fn procedural_surface_carrier_requires_its_exact_owner() {
    let mut ir = unit_cube();
    let construction = ProceduralSurfaceId("synthetic:cube:procedural-surface#0".into());
    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction.clone(),
        definition: ProceduralSurfaceDefinition::Exact {
            spline: crate::geometry::ExactSpline::Legacy {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
            },
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId("synthetic:missing".into()),
        cache: None,
    };
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding
                .message
                .contains("references missing procedural surface construction")
        }));
}

#[test]
fn procedural_curve_carrier_requires_its_exact_owner() {
    let mut ir = unit_cube();
    let construction = ProceduralCurveId("synthetic:cube:procedural-curve#0".into());
    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.model.procedural_curves.push(procedural_curve! {
        id: construction.clone(),
        definition: ProceduralCurveDefinition::Helix {
            angle_range: [0.0, std::f64::consts::TAU],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        cache_fit_tolerance: None,
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: ProceduralCurveId("synthetic:missing".into()),
        cache: None,
    };
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding
                .message
                .contains("references missing procedural curve construction")
        }));
}

#[test]
fn self_referential_composite_curve_is_invalid() {
    use crate::geometry::{CompositeCurveSegment, CompositeCurveTransition};

    let mut ir = unit_cube();
    let id = CurveId("synthetic:test:curve#recursive".into());
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: id,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }],
            self_intersect: Some(false),
        },
        source_object: None,
    });

    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));
}

#[test]
fn edge_endpoint_mismatch_is_flagged() {
    let mut ir = unit_cube();
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "worked cube must be geometrically consistent, got: {:?}",
        report.findings
    );

    let mut source_tolerant = unit_cube();
    source_tolerant.model.points[0].position.z += 0.015;
    source_tolerant.tolerances.linear = 0.02;
    let report = validate_neutral(&source_tolerant, Vec::new());
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "document tolerance must qualify a small endpoint mismatch: {:?}",
        report.findings
    );

    // Displace one corner: the point no longer lies on its edges' curves at
    // the stored parameter values.
    ir.model.points[0].position.z += 1.0;
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.severity == Severity::Error
                && f.entity.as_deref().is_some_and(|e| e.contains("edge"))),
        "displaced vertex must fail edge endpoint consistency, got: {:?}",
        report.findings
    );

    let curve = ir.model.edges[0].curve.clone().expect("cube edge curve");
    let procedural = procedural_curve! {
        id: ProceduralCurveId("synthetic:cube:curve-cache#0".into()),
        definition: ProceduralCurveDefinition::Intersection {
            context: crate::geometry::IntcurveSupportContext {
                sides: std::array::from_fn(|_| crate::geometry::IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                    pcurve_parameter_range: None,
                }),
                parameter_range: ir.model.edges[0].param_range.expect("cube edge range"),
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.99),
    };
    ir.model.add_procedural_curve(curve, procedural).unwrap();
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.entity.as_deref() == Some("synthetic:cube:edge#0")),
        "cache tolerance below the endpoint mismatch must still fail"
    );

    ir.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(1.0))
        .unwrap();
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.entity.as_deref() == Some("synthetic:cube:edge#0")),
        "curve mismatch within its cache fit tolerance must validate, got: {:?}",
        report.findings
    );
}

#[test]
fn pcurve_surface_mismatch_is_flagged() {
    // The bottom face's plane is `origin (0,0,0), normal (0,0,-1)`, whose
    // derived u/v frame maps `(u, v) -> (u, -v, 0)`. Edge #0 runs from
    // `(0,0,0)` to `(10,0,0)`, so its parameter image is the line
    // `(0,0) -> (10,0)`.
    let checked = |u_end: f64, v_end: f64, fit_tolerance: Option<f64>| {
        let mut ir = unit_cube();
        ir.model.pcurves.push(crate::geometry::Pcurve {
            id: crate::ids::PcurveId("synthetic:cube:pcurve#0".into()),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(u_end, v_end),
                ],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance,
        });
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| {
                coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0"
            })
            .expect("bottom face uses edge #0");
        coedge.pcurves = vec![crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#0".into()),
            isoparametric: None,
            parameter_range: None,
        }];
        validate_neutral(&ir, Vec::new())
    };

    let consistent = checked(10.0, 0.0, None);
    assert!(
        !consistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "matching pcurve must validate, got: {:?}",
        consistent.findings
    );

    let inconsistent = checked(10.0, 5.0, Some(4.99));
    assert!(
        inconsistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.entity.as_deref().is_some_and(|e| e.contains("coedge"))),
        "off-surface-image pcurve must be flagged, got: {:?}",
        inconsistent.findings
    );
    let tolerance_qualified = checked(10.0, 5.0, Some(5.0));
    assert!(
        !tolerance_qualified
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "pcurve mismatch within its fit tolerance must validate, got: {:?}",
        tolerance_qualified.findings
    );

    let mut procedural = unit_cube();
    procedural.model.pcurves.push(crate::geometry::Pcurve {
        id: crate::ids::PcurveId("synthetic:cube:pcurve#procedural".into()),
        geometry: crate::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(10.0, 5.0),
            ],
            weights: None,
            periodic: false,
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    });
    let coedge = procedural
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0")
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#procedural".into()),
        isoparametric: None,
        parameter_range: None,
    }];
    let owner_loop = coedge.owner_loop.clone();
    let surface = procedural
        .model
        .loops
        .iter()
        .find(|lp| lp.id == owner_loop)
        .and_then(|lp| {
            procedural
                .model
                .faces
                .iter()
                .find(|face| face.id == lp.face)
                .map(|face| face.surface.clone())
        })
        .expect("coedge owner face");
    let construction = procedural_surface! {
    id: ProceduralSurfaceId("synthetic:cube:procedural-surface#0".into()),
    definition: ProceduralSurfaceDefinition::Revolution {
            directrix: procedural.model.curves[0].id.clone(),
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            angular_interval: [0.0, std::f64::consts::TAU],
            angular_parameter_interval: None,
            parameter_interval: Some([0.0, 1.0]),
            transposed: false,
            revision_form: None,
        },
        cache_fit_tolerance: Some(0.01),
        record_bounds: None,
    };
    procedural
        .model
        .add_procedural_surface(surface, construction)
        .unwrap();
    let procedural_report = validate_neutral(&procedural, Vec::new());
    assert!(
        !procedural_report
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "procedural UVs must not be evaluated on the solved cache, got: {:?}",
        procedural_report.findings
    );
    procedural.model.procedural_surfaces[0].replace_definition(
        ProceduralSurfaceDefinition::Exact {
            spline: crate::geometry::ExactSpline::Legacy {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
            },
        },
    );
    let exact_report = validate_neutral(&procedural, Vec::new());
    assert!(
        !exact_report
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "exact procedural UVs must not be evaluated on the solved cache, got: {:?}",
        exact_report.findings
    );

    let mut negative_parameterization = unit_cube();
    negative_parameterization
        .model
        .pcurves
        .push(crate::geometry::Pcurve {
            id: crate::ids::PcurveId("synthetic:cube:pcurve#negative".into()),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![-10.0, -10.0, 0.0, 0.0],
                control_points: vec![
                    crate::math::Point2::new(10.0, 0.0),
                    crate::math::Point2::new(0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        });
    let coedge = negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0")
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#negative".into()),
        isoparametric: None,
        parameter_range: Some([-10.0, 0.0]),
    }];
    let ranged_coedge_id = coedge.id.clone();
    let negative = validate_neutral(&negative_parameterization, Vec::new());
    assert!(
        !negative
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "opposite-sign pcurve parameterization must validate, got: {:?}",
        negative.findings
    );

    negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == ranged_coedge_id)
        .expect("ranged coedge")
        .pcurves[0]
        .parameter_range = Some([-11.0, 0.0]);
    let invalid_range = validate_neutral(&negative_parameterization, Vec::new());
    assert!(invalid_range.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain && finding.message.contains("coedge pcurve range")
    }));
}
