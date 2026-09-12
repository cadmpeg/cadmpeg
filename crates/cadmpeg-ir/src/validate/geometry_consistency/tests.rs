// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::document::CadIr;
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    NurbsSurface, Pcurve, PcurveGeometry, PcurveMetadata, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SupportPcurve, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::topology::{Coedge, Edge, Face, Loop, PcurveUse, Sense, Vertex};
use crate::validate::validate_neutral;

macro_rules! procedural_surface {
    (
            id: $id:expr,
            definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr,
        record_bounds: $record_bounds:expr $(,)?
    ) => {{
        let mut definition = $definition;
        definition
            .set_legacy_cache($cache_fit_tolerance.map(|value: f64| {
                crate::geometry::LegacyCache::try_new(value)
                    .expect("admissible fit tolerance fixture")
            }))
            .expect("valid procedural surface cache fixture");
        ProceduralSurface::new($id, definition, $record_bounds)
            .expect("valid procedural surface fixture")
    }};
}

macro_rules! procedural_curve {
    (
            id: $id:expr,
            definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr $(,)?
    ) => {{
        let mut definition = $definition;
        definition
            .set_legacy_cache($cache_fit_tolerance.map(|value: f64| {
                crate::geometry::LegacyCache::try_new(value)
                    .expect("admissible fit tolerance fixture")
            }))
            .expect("valid procedural curve cache fixture");
        ProceduralCurve::new($id, definition).expect("valid procedural curve fixture")
    }};
}

fn mapped_surface_curve(mapping: [f64; 2]) -> CadIr {
    let mut ir = CadIr::empty();
    let curve = CurveId::mint("test:model:curve#curve".to_string()).expect("valid identity");
    let surface =
        SurfaceId::mint("test:model:surface#surface".to_string()).expect("valid identity");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let construction = procedural_curve! {
       id: ProceduralCurveId::mint("test:model:entity#surface-curve".to_string()).expect("valid identity"),
       definition: ProceduralCurveDefinition::SurfaceCurve {
           family: SurfaceCurveFamily::Parametric {
               context: IntcurveSupportContext::try_new([
                   IntcurveSupportSide {
                       surface: Some(surface),
                       pcurve: Some(SupportPcurve::new(
                           PcurveGeometry::Line(crate::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap()),
                           Some(DirectedParameterRange::new(mapping).unwrap()),
                       )),
                   },
                   IntcurveSupportSide {
                       surface: None,
                       pcurve: None,
                   },
               ], [0.0, 1.0], std::array::from_fn(|_| Vec::new())).unwrap(),
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
    let base = CurveId::mint("test:model:entity#base".to_string()).expect("valid identity");
    *ir.model.curves[0]
        .geometry
        .solved_cache_mut()
        .expect("mapped curve has a solved cache") = SolvedCurveGeometry::Line(
        crate::geometry::LineCurve::try_new(
            Point3::new(2.0, 0.0, 25.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    ir.model.curves.push(Curve {
        id: base.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let ProceduralCurveDefinition::SurfaceCurve { family } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!();
    };
    let context = family.context().clone();
    ir.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::SurfaceOffset(
        crate::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
            context,
            false,
            [[0.0, 1.0], [0.0, 1.0]],
            (base, [2.0, 3.0], [Some(2.0), Some(3.0)]),
            None,
            25.0,
            [0.0, 1.0],
        )
        .unwrap(),
    ));
    ir
}

fn untrimmed_surface_curve() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.points.extend([
        crate::topology::Point {
            id: "test:model:point#point-start"
                .try_into()
                .expect("valid identity"),
            position: Point3::new(0.0, 1.0, 0.0),
            source_object: None,
        },
        crate::topology::Point {
            id: "test:model:point#point-end"
                .try_into()
                .expect("valid identity"),
            position: Point3::new(-1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: "test:model:vertex#vertex-start"
                .try_into()
                .expect("valid identity"),
            point: "test:model:point#point-start"
                .try_into()
                .expect("valid identity"),
            tolerance: None,
        },
        Vertex {
            id: "test:model:vertex#vertex-end"
                .try_into()
                .expect("valid identity"),
            point: "test:model:point#point-end"
                .try_into()
                .expect("valid identity"),
            tolerance: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: "test:model:curve#curve".try_into().expect("valid identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            crate::geometry::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: "test:model:edge#edge".try_into().expect("valid identity"),
        carrier: crate::topology::EdgeCarrier::unbounded(Some(
            "test:model:curve#curve".try_into().expect("valid identity"),
        )),
        start: "test:model:vertex#vertex-start"
            .try_into()
            .expect("valid identity"),
        end: "test:model:vertex#vertex-end"
            .try_into()
            .expect("valid identity"),
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: "test:model:surface#surface"
            .try_into()
            .expect("valid identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.pcurves.push(Pcurve {
        id: "test:model:pcurve#pcurve"
            .try_into()
            .expect("valid identity"),
        geometry: PcurveGeometry::Circle(
            crate::geometry::CirclePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                1.0,
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::default(),
    });
    ir.model.coedges.push(Coedge {
        id: "test:model:coedge#coedge"
            .try_into()
            .expect("valid identity"),
        owner_loop: "test:model:loop#loop".try_into().expect("valid identity"),
        edge: "test:model:edge#edge".try_into().expect("valid identity"),
        radial_next: "test:model:coedge#coedge"
            .try_into()
            .expect("valid identity"),
        sense: Sense::Forward,
        pcurves: vec![PcurveUse {
            pcurve: "test:model:pcurve#pcurve"
                .try_into()
                .expect("valid identity"),
            isoparametric: None,
            parameter_range: None,
        }],
        use_curve: None,
    });
    ir.model.loops.push(Loop {
        id: "test:model:loop#loop".try_into().expect("valid identity"),
        face: "test:model:face#face".try_into().expect("valid identity"),
        boundary: crate::topology::LoopBoundary::Ring(
            crate::topology::LoopRing::new(
                vec!["test:model:coedge#coedge"
                    .try_into()
                    .expect("valid identity")],
                Vec::new(),
            )
            .expect("valid loop ring"),
        ),
    });
    ir.model.faces.push(Face {
        id: "test:model:face#face".try_into().expect("valid identity"),
        shell: "test:model:shell#shell".try_into().expect("valid identity"),
        surface: "test:model:surface#surface"
            .try_into()
            .expect("valid identity"),
        sense: Sense::Forward,
        loops: vec!["test:model:loop#loop".try_into().expect("valid identity")].into(),
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
        let ProceduralCurveDefinition::SurfaceOffset(definition_payload) = definition else {
            unreachable!();
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            crate::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
                definition_payload.context().clone(),
                *definition_payload.discontinuity_flag(),
                [
                    *definition_payload.base_u_range(),
                    *definition_payload.base_v_range(),
                ],
                (
                    definition_payload.base().clone(),
                    *definition_payload.base_range(),
                    [None, None],
                ),
                definition_payload.cache_first().cloned(),
                *definition_payload.distance(),
                [*definition_payload.shift(), *definition_payload.scale()],
            )
            .unwrap();
        definition_payload.set_legacy_cache(restored_cache);
    });
    check_procedural_support_consistency(&context_first, &mut findings);
    assert!(findings.is_empty());

    let mut ir = mapped_surface_offset();
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) =
        &mut ir.model.curves[1].geometry
    else {
        unreachable!();
    };
    let origin = line_curve.origin();
    let direction = line_curve.direction();
    let mut origin = *origin;
    origin.y = 2.0;
    *line_curve = crate::geometry::LineCurve::try_new(origin, *direction).unwrap();
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
    let PcurveGeometry::Circle(circle_pcurve) = &mut mismatched.model.pcurves[0].geometry else {
        unreachable!();
    };
    let center = circle_pcurve.center();
    let x_axis = circle_pcurve.x_axis();
    let y_axis = circle_pcurve.y_axis();
    *circle_pcurve =
        crate::geometry::CirclePcurve::try_new(*center, *x_axis, *y_axis, 2.0).unwrap();
    super::check_pcurve_surface_consistency(&mismatched, &mut findings);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("pcurve mapped through"));
}

#[test]
fn trimmed_surface_pcurve_uses_the_local_parameterization_for_validation() {
    let mut ir = untrimmed_surface_curve();
    let base_id = SurfaceId::mint("test:model:entity#base-surface").expect("valid identity");
    let base_geometry = ir.model.surfaces[0].geometry.clone();
    ir.model.surfaces[0].id = base_id.clone();
    ir.model.surfaces.push(Surface {
        id: "test:model:surface#surface"
            .try_into()
            .expect("valid identity"),
        geometry: base_geometry,
        source_object: None,
    });
    let construction = procedural_surface! {
        id: ProceduralSurfaceId::mint("test:model:entity#trimmed-surface").expect("valid identity"),
        definition: ProceduralSurfaceDefinition::Subset(crate::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(base_id, [[2.0, 0.0], [0.0, 2.0]], Some(false), Some(true)).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    };
    ir.model
        .add_procedural_surface(
            SurfaceId::mint("test:model:surface#surface").expect("valid identity"),
            construction,
        )
        .unwrap();
    ir.model.points[0].position = Point3::new(1.0, 2.0, 0.0);
    ir.model.points[1].position = Point3::new(2.0, 1.0, 0.0);
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        crate::geometry::CircleCurve::try_new(
            Point3::new(1.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    ir.model.pcurves[0].geometry = PcurveGeometry::Circle(
        crate::geometry::CirclePcurve::try_new(
            Point2::new(1.0, 1.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            1.0,
        )
        .unwrap(),
    );
    let PcurveMetadata::General { form: metadata } = &mut ir.model.pcurves[0].metadata else {
        panic!("fixture uses general pcurve metadata")
    };
    metadata
        .set_parameter_range(Some([0.0, std::f64::consts::PI]))
        .unwrap();

    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn untrimmed_nurbs_pcurve_uses_its_own_endpoint_parameters() {
    let mut ir = untrimmed_surface_curve();
    ir.model.pcurves[0].geometry = PcurveGeometry::Nurbs {
        nurbs: crate::geometry::PcurveNurbs::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 1.0),
                Point2::new(-1.0, 1.0),
                Point2::new(-1.0, 0.0),
            ],
            Some(vec![1.0, 2.0_f64.sqrt() / 2.0, 1.0]),
            false,
        )
        .unwrap(),
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
    ir.model.pcurves[0].geometry = PcurveGeometry::Trimmed(
        crate::geometry::TrimmedPcurve::try_new(
            [0.0, 1.0],
            true,
            Box::new(PcurveGeometry::Line(
                crate::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                    .unwrap(),
            )),
        )
        .unwrap(),
    );
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn raw_nurbs_domain_is_not_treated_as_edge_trim() {
    let pcurve = Pcurve {
        id: "test:model:pcurve#pcurve"
            .try_into()
            .expect("valid identity"),
        geometry: PcurveGeometry::Nurbs {
            nurbs: crate::geometry::PcurveNurbs::new(
                2,
                vec![-1.0, 0.0, 0.0, 1.0, 1.0, 2.0],
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(2.0, 0.0),
                ],
                None,
                false,
            )
            .unwrap(),
        },
        metadata: PcurveMetadata::default(),
    };
    assert_eq!(pcurve_parameter_domain(&pcurve.geometry), Some([0.0, 1.0]));
    assert!(pcurve_parameter_ranges(&pcurve, None, None).is_none());
}

#[test]
fn collapsed_trimmed_pcurve_falls_back_to_its_basis_domain() {
    let geometry = PcurveGeometry::Trimmed(
        crate::geometry::TrimmedPcurve::try_new(
            [1.0, 1.0],
            true,
            Box::new(PcurveGeometry::Nurbs {
                nurbs: crate::geometry::PcurveNurbs::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    None,
                    true,
                )
                .unwrap(),
            }),
        )
        .unwrap(),
    );
    assert_eq!(pcurve_parameter_domain(&geometry), Some([0.0, 1.0]));
}

#[test]
fn line_pcurve_recovers_vertices_from_nurbs_surface_domain_seeds() {
    // At v=0 the quadratic surface has a zero derivative. The ordinary
    // seed at t=0 therefore cannot start Newton recovery; the finite
    // surface domain supplies an interior seed on the same branch.
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            1,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 0.0, 1.0),
                ],
                vec![
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 1.0),
                ],
            ],
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    ));
    let surface_id =
        SurfaceId::mint("test:model:surface#surface".to_string()).expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface.clone(),
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let pcurve = Pcurve {
        id: "test:model:pcurve#pcurve"
            .try_into()
            .expect("valid identity"),
        geometry: PcurveGeometry::Line(
            crate::geometry::LinePcurve::try_new(Point2::new(1.0, 0.0), Point2::new(0.0, 1.0))
                .unwrap(),
        ),
        metadata: PcurveMetadata::default(),
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
    let construction =
        ProceduralSurfaceId::mint("synthetic:cube:procedural-surface#0").expect("valid identity");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction.clone(),
        definition: ProceduralSurfaceDefinition::Exact(crate::geometry::surface_payloads::ExactSurfacePayload::try_new(crate::geometry::ExactSpline::Legacy {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
    cache: None,
}).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint("test:model:entity#synthetic:missing")
            .expect("valid identity"),
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
    let construction =
        ProceduralCurveId::mint("synthetic:cube:procedural-curve#0").expect("valid identity");
    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.model.procedural_curves.push(procedural_curve! {
        id: construction.clone(),
        definition: ProceduralCurveDefinition::Helix(crate::geometry::HelixCurveConstruction::try_new(
            [0.0, std::f64::consts::TAU],
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            0.0,
            Vector3::new(0.0, 0.0, 1.0),
        ).unwrap()),
        cache_fit_tolerance: None,
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: ProceduralCurveId::mint("test:model:entity#synthetic:missing")
            .expect("valid identity"),
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
    let id = CurveId::mint("synthetic:test:curve#recursive").expect("valid identity");
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: crate::geometry::CompositeCurveSegments::try_from(vec![
                CompositeCurveSegment {
                    curve: id,
                    same_sense: true,
                    transition: CompositeCurveTransition::Continuous,
                },
            ])
            .unwrap(),
            self_intersect: Some(false),
        }),
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
    source_tolerant.tolerances.linear =
        crate::units::PositiveScalar::new(0.02).expect("positive finite tolerance");
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

    let curve = ir.model.edges[0].curve().clone().expect("cube edge curve");
    let procedural = procedural_curve! {
        id: ProceduralCurveId::mint("synthetic:cube:curve-cache#0").expect("valid identity"),
        definition: ProceduralCurveDefinition::Intersection {
            context: crate::geometry::IntcurveSupportContext::try_new(std::array::from_fn(|_| crate::geometry::IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                }), ir.model.edges[0].param_range().expect("cube edge range"), std::array::from_fn(|_| Vec::new())).unwrap(),
            discontinuity_flag: false,
            cache: None,
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
            id: crate::ids::PcurveId::mint("synthetic:cube:pcurve#0").expect("valid identity"),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                nurbs: crate::geometry::PcurveNurbs::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![
                        crate::math::Point2::new(0.0, 0.0),
                        crate::math::Point2::new(u_end, v_end),
                    ],
                    None,
                    false,
                )
                .unwrap(),
            },
            metadata: PcurveMetadata::try_general(None, None, fit_tolerance).unwrap(),
        });
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| {
                coedge.id.as_str().contains("bottom")
                    && coedge.edge.as_str() == "synthetic:cube:edge#0"
            })
            .expect("bottom face uses edge #0");
        coedge.pcurves = vec![crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId::mint("synthetic:cube:pcurve#0").expect("valid identity"),
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
        id: crate::ids::PcurveId::mint("synthetic:cube:pcurve#procedural").expect("valid identity"),
        geometry: crate::geometry::PcurveGeometry::Nurbs {
            nurbs: crate::geometry::PcurveNurbs::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(10.0, 5.0),
                ],
                None,
                false,
            )
            .unwrap(),
        },
        metadata: PcurveMetadata::default(),
    });
    let coedge = procedural
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId::mint("synthetic:cube:pcurve#procedural")
            .expect("valid identity"),
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
    id: ProceduralSurfaceId::mint("synthetic:cube:procedural-surface#0").expect("valid identity"),
    definition: ProceduralSurfaceDefinition::Revolution(crate::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(procedural.model.curves[0].id.clone(), (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)), [0.0, std::f64::consts::TAU], None, Some([0.0, 1.0]), false, None).unwrap()),
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
    procedural.model.procedural_surfaces[0].replace_definition(ProceduralSurfaceDefinition::Exact(
        crate::geometry::surface_payloads::ExactSurfacePayload::try_new(
            crate::geometry::ExactSpline::Legacy {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
                cache: None,
            },
        )
        .unwrap(),
    ));
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
            id: crate::ids::PcurveId::mint("synthetic:cube:pcurve#negative")
                .expect("valid identity"),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                nurbs: crate::geometry::PcurveNurbs::new(
                    1,
                    vec![-10.0, -10.0, 0.0, 0.0],
                    vec![
                        crate::math::Point2::new(10.0, 0.0),
                        crate::math::Point2::new(0.0, 0.0),
                    ],
                    None,
                    false,
                )
                .unwrap(),
            },
            metadata: PcurveMetadata::default(),
        });
    let coedge = negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId::mint("synthetic:cube:pcurve#negative")
            .expect("valid identity"),
        isoparametric: None,
        parameter_range: Some(crate::geometry::DirectedParameterRange::new([-10.0, 0.0]).unwrap()),
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

    let pcurve_use = &mut negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == ranged_coedge_id)
        .expect("ranged coedge")
        .pcurves[0];
    pcurve_use.parameter_range =
        Some(crate::geometry::DirectedParameterRange::new([-11.0, 0.0]).unwrap());
    let invalid_range = validate_neutral(&negative_parameterization, Vec::new());
    assert!(invalid_range.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain && finding.message.contains("coedge pcurve range")
    }));
}
