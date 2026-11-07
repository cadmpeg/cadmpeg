// SPDX-License-Identifier: Apache-2.0

use super::{attach, replaceable};
use crate::families::FamilyEntityAdmission;
use crate::wire::records::consolidated_records;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, PlaneSurface, SphereSurface};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Coedge, Edge, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const RADIUS: f64 = 2.5;
const EPSILON: f64 = 1.0e-12;
const ACTIVE: [f64; 2] = [2.4, 4.7];

fn frame(class: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        3,
        class,
        u8::try_from(payload.len()).expect("small frame"),
        5,
    ];
    bytes.extend(payload);
    bytes
}
fn compact(value: u8) -> u8 {
    value * 4 + 1
}
fn source() -> Vec<u8> {
    source_with_fit(0.0, 10)
}

fn source_without_map() -> Vec<u8> {
    let mut sphere = Vec::new();
    for value in [
        0.0,
        0.0,
        0.0,
        RADIUS,
        0.0,
        0.0,
        0.0,
        RADIUS,
        0.0,
        0.0,
        0.0,
        RADIUS,
        RADIUS,
        0.0,
        std::f64::consts::TAU,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
        RADIUS,
        0.0,
    ] {
        sphere.extend(value.to_le_bytes());
    }
    let mut jet = vec![compact(1), compact(5), compact(2), 0x0c];
    for parameter in [2.0_f64, 5.0] {
        jet.extend(parameter.to_le_bytes());
    }
    jet.extend([compact(2), 0]);
    for lane in [
        [0.2 * RADIUS, 0.8 * RADIUS],
        [0.0, 0.0],
        [0.2 * RADIUS, 0.2 * RADIUS],
        [0.0, 0.0],
    ] {
        for value in lane {
            jet.extend(value.to_le_bytes());
        }
    }
    jet.push(5);
    for value in [0.0_f64; 4].into_iter().chain(ACTIVE) {
        jet.extend(value.to_le_bytes());
    }
    jet.push(7);
    let mut jet_frame = vec![0xa5, 3, 0x20];
    jet_frame.extend(u32::try_from(jet.len()).expect("bounded jet").to_le_bytes());
    jet_frame.push(5);
    jet_frame.extend(jet);
    [
        frame(0x2a, &sphere),
        jet_frame,
        frame(0x27, &[]),
        frame(0x18, &[compact(1)]),
        frame(0x5d, &[]),
        frame(0x5d, &[]),
        frame(0x5e, &[compact(3), compact(2), compact(1), 3, 3, 0x21]),
        frame(0x5f, &[0x82, compact(7), 3, 5]),
        frame(
            0x62,
            &[
                0x85,
                compact(7),
                compact(2),
                compact(7),
                compact(8),
                compact(8),
                1,
            ],
        ),
        frame(0x5f, &[0x82, compact(7), 3, 5]),
        frame(
            0x62,
            &[
                0x85,
                compact(7),
                compact(4),
                compact(7),
                compact(10),
                compact(8),
                1,
            ],
        ),
    ]
    .concat()
}

fn fixture() -> CadIr {
    let mut ir = CadIr::empty();
    for (at, parameter) in ACTIVE.into_iter().enumerate() {
        let angle = 0.2 + 0.2 * (parameter - 2.0);
        let point = PointId::mint(format!("catia:test:point#{at}")).expect("point id");
        let vertex = VertexId::mint(format!("catia:test:vertex#{at}")).expect("vertex id");
        ir.model.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(Point3::new(RADIUS * angle.cos(), RADIUS * angle.sin(), 0.0))
                .expect("finite point"),
            Some(
                crate::assemble::cgm_source("vertex", 5 + u32::try_from(at).expect("port"))
                    .expect("source tag"),
            ),
        ));
        ir.model.vertices.push(Vertex {
            id: vertex,
            point,
            tolerance: None,
        });
    }
    let curve = CurveId::mint("catia:test:curve#edge").expect("curve id");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                RADIUS,
            )
            .expect("circle"),
        )),
        source_object: Some(crate::assemble::cgm_source("edge-support", 7).expect("source tag")),
    });
    let edge = EdgeId::mint("catia:test:edge#one").expect("edge id");
    ir.model.edges.push(Edge {
        id: edge.clone(),
        start: ir.model.vertices[0].id.clone(),
        end: ir.model.vertices[1].id.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.28, 0.74]))
            .expect("curve interval"),
        tolerance: None,
    });
    for side in 0..2 {
        let surface = SurfaceId::mint(format!("catia:test:surface#{side}")).expect("surface id");
        let face = FaceId::mint(format!("catia:test:face#{side}")).expect("face id");
        let owner_loop = LoopId::mint(format!("catia:test:loop#{side}")).expect("loop id");
        let coedge = CoedgeId::mint(format!("catia:test:coedge#{side}")).expect("coedge id");
        let geometry = if side == 0 {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                SphereSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    RADIUS,
                )
                .expect("sphere"),
            ))
        } else {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane"),
            ))
        };
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry,
            source_object: Some(
                crate::assemble::cgm_source("carrier", if side == 0 { 8 } else { 10 })
                    .expect("face tag"),
            ),
        });
        ir.model.faces.push(Face {
            id: face.clone(),
            shell: ShellId::mint("catia:test:shell#one").expect("shell id"),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(vec![owner_loop.clone()]),
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: owner_loop.clone(),
            face,
            boundary: LoopBoundary::Ring(
                LoopRing::new(vec![coedge.clone()], Vec::new()).expect("ring"),
            ),
        });
        ir.model.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop,
            edge: edge.clone(),
            radial_next: CoedgeId::mint(format!("catia:test:coedge#{}", 1 - side))
                .expect("radial id"),
            sense: if side == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir
}

fn add_existing(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    exactness: Exactness,
    tag: &str,
) {
    let id = PcurveId::mint("catia:test:pcurve#prior").expect("pcurve id");
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(9.0, 8.0), Point2::new(1.0, 2.0)).expect("line"),
        ),
        metadata: PcurveMetadata::default(),
    });
    ir.model.coedges[0]
        .pcurves
        .push(cadmpeg_ir::topology::PcurveUse {
            pcurve: id.clone(),
            isoparametric: None,
            parameter_range: None,
        });
    crate::assemble::annotate(
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        0,
        tag,
        exactness,
    )
    .expect("provenance");
    annotations
        .derived(&ir.model.coedges[0].id, "pcurves")
        .expect("generated use");
    annotations
        .field_exactness(&id, "geometry", exactness)
        .expect("geometry exactness");
}

#[test]
fn source_closed_sphere_jet_keeps_its_native_interval_and_coedge_direction() {
    let data = source();
    let records = consolidated_records(&data);
    for (swap, sense) in [
        (false, Sense::Forward),
        (true, Sense::Forward),
        (false, Sense::Reversed),
    ] {
        let mut ir = fixture();
        if swap {
            let edge = &mut ir.model.edges[0];
            std::mem::swap(&mut edge.start, &mut edge.end);
        }
        ir.model.coedges[0].sense = sense;
        let mut annotations = AnnotationBuilder::new();
        let mut refusal = crate::nurbs::LaneRefusals::new();
        crate::test_support::with_service_context(|ctx| {
            let mut admission = FamilyEntityAdmission::new(ctx);
            assert_eq!(
                attach(
                    &mut ir,
                    &mut annotations,
                    &data,
                    &records,
                    &mut refusal,
                    &mut admission
                )
                .expect("source transfer")
                .pcurves,
                1
            );
            assert_eq!(admission.admitted(), 1);
        });
        let pcurve = &ir.model.pcurves[0];
        let range = pcurve
            .metadata
            .parameter_range()
            .expect("native range")
            .get();
        assert!(range
            .into_iter()
            .zip(ACTIVE)
            .all(|(a, b)| (a - b).abs() < EPSILON));
        let mut expected = ACTIVE;
        if swap ^ (sense == Sense::Reversed) {
            expected.reverse();
        }
        let use_range = ir.model.coedges[0].pcurves[0]
            .parameter_range
            .expect("directed source use")
            .endpoints();
        assert!(use_range
            .into_iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < EPSILON));
        assert!(pcurve.metadata.fit_tolerance().is_none());
        assert_eq!(refusal.note_count(), 0);
    }
}

#[test]
fn source_jet_rebinds_generated_use_and_removes_its_orphaned_curve() {
    let data = source();
    let records = consolidated_records(&data);
    let mut ir = fixture();
    let mut annotations = AnnotationBuilder::new();
    add_existing(
        &mut ir,
        &mut annotations,
        Exactness::Derived,
        super::GENERATED_TAG,
    );
    let prior = ir.model.pcurves[0].clone();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = FamilyEntityAdmission::new(ctx);
        assert_eq!(
            attach(
                &mut ir,
                &mut annotations,
                &data,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
                &mut admission
            )
            .expect("source transfer")
            .pcurves,
            1
        );
    });
    assert_ne!(ir.model.coedges[0].pcurves[0].pcurve, prior.id);
    assert!(ir.model.pcurves.iter().all(|pcurve| pcurve.id != prior.id));
    assert!(!annotations
        .annotations()
        .provenance
        .contains_key(prior.id.as_str()));
    assert!(!annotations
        .annotations()
        .exactness()
        .contains_key(prior.id.as_str()));
    let source_id = &ir.model.pcurves[0].id;
    assert_eq!(
        annotations.annotations().provenance[source_id.as_str()]
            .tag
            .as_deref(),
        Some(super::TAG)
    );
}

#[test]
fn native_unknown_or_differently_tagged_pcurves_keep_all_model_and_annotation_values() {
    for (exactness, tag) in [
        (Exactness::ByteExact, super::GENERATED_TAG),
        (Exactness::Unknown, super::GENERATED_TAG),
        (Exactness::Derived, "native_jet"),
    ] {
        let data = source();
        let records = consolidated_records(&data);
        let mut ir = fixture();
        let mut annotations = AnnotationBuilder::new();
        add_existing(&mut ir, &mut annotations, exactness, tag);
        let before = ir.clone();
        let notes = annotations.annotations().clone();
        assert!(!replaceable(
            &ir.model.coedges[0],
            annotations.annotations()
        ));
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                attach(
                    &mut ir,
                    &mut annotations,
                    &data,
                    &records,
                    &mut crate::nurbs::LaneRefusals::new(),
                    &mut FamilyEntityAdmission::new(ctx)
                )
                .expect("native preservation")
                .pcurves,
                0
            );
        });
        assert_eq!(ir, before);
        assert_eq!(annotations.annotations(), &notes);
    }
}

#[test]
fn resource_refusals_preserve_the_model() {
    let data = source();
    let records = consolidated_records(&data);
    for limit in [0_u64, 1] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        if limit == 0 {
            policy.limits.max_entities = 0;
        } else {
            policy.limits.max_retained_bytes = 0;
        }
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
            .expect("bounded source");
        let mut ir = fixture();
        let before = ir.clone();
        let mut annotations = AnnotationBuilder::new();
        let result = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(&ctx),
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(ir, before);
        assert_eq!(annotations.build(), cadmpeg_ir::Annotations::default());
    }
}

#[test]
fn source_owner_port_face_and_chart_mismatches_withhold_the_jet() {
    let original = source();
    let records = consolidated_records(&original);
    for case in 0..4 {
        let mut data = original.clone();
        let mut ir = fixture();
        match case {
            0 => {
                let payload = records[1].payload().expect("jet payload");
                data[payload.start] = compact(2);
            }
            1 => {
                ir.model.points[0].source_object =
                    Some(crate::assemble::cgm_source("vertex", 6).expect("tag"));
            }
            2 => {
                ir.model.surfaces[0].source_object =
                    Some(crate::assemble::cgm_source("carrier", 9).expect("tag"));
            }
            _ => {
                ir.model.surfaces[0].geometry =
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        SphereSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(0.0, 1.0, 0.0),
                            RADIUS,
                        )
                        .expect("different chart"),
                    ));
            }
        }
        let records = consolidated_records(&data);
        let before = ir.clone();
        let mut annotations = AnnotationBuilder::new();
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                attach(
                    &mut ir,
                    &mut annotations,
                    &data,
                    &records,
                    &mut crate::nurbs::LaneRefusals::new(),
                    &mut FamilyEntityAdmission::new(ctx)
                )
                .expect("closed-source rejection")
                .pcurves,
                0
            );
        });
        assert_eq!(ir, before);
        assert_eq!(annotations.build(), cadmpeg_ir::Annotations::default());
    }
}

#[test]
fn sphere_chart_scales_positions_and_both_derivative_channels_in_the_native_gauge() {
    use crate::wire::records::{ConsolidatedPcurve, ConsolidatedPcurveSite};
    let radius = 3.5;
    let sites = [-2.0_f64, 1.0]
        .into_iter()
        .map(|t| ConsolidatedPcurveSite {
            knot: cadmpeg_ir::scalar::FiniteReal::new(t).expect("knot"),
            point: cadmpeg_ir::units::FiniteVector::new([
                radius * (0.2 + 0.3 * t + 0.05 * t * t),
                radius * (0.1 - 0.2 * t + 0.025 * t * t),
            ])
            .expect("point"),
            first_derivatives: cadmpeg_ir::units::FiniteVector::new([
                radius * (0.3 + 0.1 * t),
                radius * (-0.2 + 0.05 * t),
            ])
            .expect("first derivative"),
            second_derivatives: cadmpeg_ir::units::FiniteVector::new([radius * 0.1, radius * 0.05])
                .expect("second derivative"),
        })
        .collect::<Vec<_>>();
    let jet = ConsolidatedPcurve {
        pos: 0,
        support_id: 1,
        extrapolation_sites: 0,
        sites: sites.try_into().expect("ordered sites"),
        range: cadmpeg_ir::topology::IncreasingParameterInterval::new([-1.5, 0.75])
            .expect("active range"),
        tail: vec![7],
    };
    let geometry = super::super::consolidated_jet_pcurve(
        &jet,
        &super::super::ConsolidatedCarrierChart::Sphere { radius },
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("scaled jet");
    crate::test_support::with_service_context(|ctx| {
        for t in [-2.0_f64, -0.7, 0.4, 1.0] {
            let uv = cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, t)
                .expect("UV point")
                .get();
            assert!((uv.u - (0.2 + 0.3 * t + 0.05 * t * t)).abs() < EPSILON);
            assert!((uv.v - (0.1 - 0.2 * t + 0.025 * t * t)).abs() < EPSILON);
        }
    });
}

#[test]
fn another_model_arena_cannot_supply_the_generated_pcurve_identity() {
    let data = source();
    let records = consolidated_records(&data);
    let mut admitted = fixture();
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(
            attach(
                &mut admitted,
                &mut AnnotationBuilder::new(),
                &data,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
                &mut FamilyEntityAdmission::new(ctx)
            )
            .expect("source transfer")
            .pcurves,
            1
        );
    });
    let id = admitted.model.pcurves[0].id.as_str();
    let mut ir = fixture();
    ir.model.points.push(Point::new(
        PointId::mint(id).expect("cross-arena identity"),
        FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("point"),
        None,
    ));
    let before = ir.clone();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let result = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
        .expect("collision preservation");
        assert_eq!(result.pcurves, 0);
        assert_eq!(result.unresolved_identity_collisions, 1);
        assert_eq!(result.unresolved_existing_uses, 0);
    });
    assert_eq!(ir, before);
    assert_eq!(annotations.build(), cadmpeg_ir::Annotations::default());
}

#[test]
fn a_multi_use_chain_is_preserved_and_counted_as_an_unsupported_replacement() {
    let data = source();
    let records = consolidated_records(&data);
    let mut ir = fixture();
    let mut annotations = AnnotationBuilder::new();
    add_existing(
        &mut ir,
        &mut annotations,
        Exactness::Derived,
        super::GENERATED_TAG,
    );
    let mut second = ir.model.coedges[0].pcurves[0].clone();
    ir.model.coedges[0].pcurves[0].parameter_range =
        Some(cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 0.5]).expect("first segment"));
    second.parameter_range = Some(
        cadmpeg_ir::geometry::DirectedParameterRange::new([0.5, 1.0]).expect("second segment"),
    );
    ir.model.coedges[0].pcurves.push(second);
    let before = ir.clone();
    let notes = annotations.annotations().clone();
    crate::test_support::with_service_context(|ctx| {
        let result = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
        .expect("preserve chain");
        assert_eq!(result.pcurves, 0);
        assert_eq!(result.unresolved_existing_uses, 1);
        assert_eq!(result.unresolved_identity_collisions, 0);
    });
    assert_eq!(ir, before);
    assert_eq!(annotations.annotations(), &notes);
}

#[test]
fn an_identical_source_bound_use_is_idempotent() {
    let data = source();
    let records = consolidated_records(&data);
    let mut ir = fixture();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = FamilyEntityAdmission::new(ctx);
        let first = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
        .expect("source transfer");
        assert_eq!(first.pcurves, 1);
        let before = ir.clone();
        let notes = annotations.annotations().clone();
        let second = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
        .expect("identical source use");
        assert_eq!(second.pcurves, 0);
        assert_eq!(second.unresolved_existing_uses, 0);
        assert_eq!(second.unresolved_identity_collisions, 0);
        assert_eq!(ir, before);
        assert_eq!(annotations.annotations(), &notes);
    });
}

fn source_with_fit(fit: f64, second_operand: u8) -> Vec<u8> {
    const SAMPLE_COUNT: u8 = 15;
    let mut data = source_without_map();
    let records = consolidated_records(&data);
    let payload = records[6].payload().expect("edge payload");
    data[payload.start] = 4 * 4 + 3;
    let mut map = vec![0x82, 0x08, 8, 0, 0x08, second_operand, 0, 0x0a];
    map.extend(0x1587_u16.to_le_bytes());
    for value in [0.0_f64, 1.0, 0.0, ACTIVE[0], ACTIVE[1]] {
        map.extend(value.to_le_bytes());
    }
    map.push(0x80 + SAMPLE_COUNT);
    for [lower, upper] in [[0.0_f64, 1.0], ACTIVE] {
        for at in 0..SAMPLE_COUNT {
            let value = lower + (upper - lower) * f64::from(at) / f64::from(SAMPLE_COUNT - 1);
            map.extend(value.to_le_bytes());
        }
    }
    map.extend(fit.to_le_bytes());
    data.extend([0xa5, 3, 0x25]);
    data.extend(u32::try_from(map.len()).expect("bounded map").to_le_bytes());
    data.push(5);
    data.extend(map);
    data
}

#[test]
fn source_owner_without_a_closed_a25_map_does_not_emit_a_pcurve() {
    let data = source_without_map();
    let records = consolidated_records(&data);
    let mut ir = fixture();
    let before = ir.clone();
    crate::test_support::with_service_context(|ctx| {
        let result = attach(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
        .expect("withhold source map without a fit bound");
        assert_eq!(result.pcurves, 0);
        assert_eq!(result.unresolved_source_uses, 1);
    });
    assert_eq!(ir, before);
}

#[test]
fn a_generated_pcurve_shared_by_another_coedge_is_preserved() {
    let data = source();
    let records = consolidated_records(&data);
    let mut ir = fixture();
    let mut annotations = AnnotationBuilder::new();
    add_existing(
        &mut ir,
        &mut annotations,
        Exactness::Derived,
        super::GENERATED_TAG,
    );
    let prior = ir.model.pcurves[0].clone();
    ir.model.coedges[1].pcurves = ir.model.coedges[0].pcurves.clone();
    let before = ir.clone();
    let prior_annotations = annotations.annotations().clone();
    crate::test_support::with_service_context(|ctx| {
        let result = attach(
            &mut ir,
            &mut annotations,
            &data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
        .expect("preserve shared generated pcurve");
        assert_eq!(result.pcurves, 0);
        assert_eq!(result.unresolved_existing_uses, 1);
    });
    assert_eq!(ir, before);
    assert_eq!(annotations.annotations(), &prior_annotations);
    assert_eq!(ir.model.pcurves, [prior]);
}

#[test]
fn source_map_fit_admits_rounded_endpoints_and_withholds_residuals_outside_its_bound() {
    const SOURCE_MODEL_FIT_MM: f64 = 1.0e-4;
    for (residual, operand, expected) in [
        (SOURCE_MODEL_FIT_MM * 0.5, 10, 1),
        (SOURCE_MODEL_FIT_MM * 2.0, 10, 0),
        (SOURCE_MODEL_FIT_MM * 0.5, 9, 0),
    ] {
        let data = source_with_fit(SOURCE_MODEL_FIT_MM, operand);
        let records = consolidated_records(&data);
        let mut ir = fixture();
        let point = &mut ir.model.points[0];
        let exact = point.position().get();
        let rounded = |value| f64::from(cadmpeg_core::convert::f32_from_f64(value).expect("f32"));
        point.set_position(
            FinitePoint3::new(Point3::new(
                rounded(exact.x),
                rounded(exact.y),
                rounded(residual),
            ))
            .expect("rounded point"),
        );
        let before = ir.clone();
        let mut annotations = AnnotationBuilder::new();
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                attach(
                    &mut ir,
                    &mut annotations,
                    &data,
                    &records,
                    &mut crate::nurbs::LaneRefusals::new(),
                    &mut FamilyEntityAdmission::new(ctx),
                )
                .expect("source fit admission")
                .pcurves,
                expected,
            );
        });
        assert_eq!(ir.model.points, before.model.points);
        assert_eq!(ir.model.vertices, before.model.vertices);
        if expected == 0 {
            assert_eq!(ir, before);
        } else {
            assert!(ir.model.pcurves[0].metadata.fit_tolerance().is_none());
        }
    }
}

#[test]
fn source_endpoint_allowance_uses_the_exact_f32_rounding_cell() {
    const SOURCE_MODEL_FIT_MM: f64 = 1.0e-5;
    let stored = Point3::new(128.0, 0.0, 0.0);
    let cell_upper = f64::from(128.0_f32.next_up()).midpoint(128.0);
    assert!(super::endpoint_matches(
        Point3::new(cell_upper + SOURCE_MODEL_FIT_MM * 0.5, 0.0, 0.0),
        stored,
        SOURCE_MODEL_FIT_MM,
    ));
    assert!(!super::endpoint_matches(
        Point3::new(cell_upper + SOURCE_MODEL_FIT_MM * 2.0, 0.0, 0.0),
        stored,
        SOURCE_MODEL_FIT_MM,
    ));
}
