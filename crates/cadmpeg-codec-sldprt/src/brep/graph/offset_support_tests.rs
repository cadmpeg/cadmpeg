// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::parasolid::sequence::{sequence_of, F, NULL_REAL};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;

fn prefix(sense: u8) -> Vec<F> {
    vec![
        F::I(1),
        F::P(1),
        F::P(1),
        F::P(1),
        F::P(1),
        F::P(1),
        F::C(sense),
    ]
}

fn offset(support: u16, distance: f64) -> Vec<F> {
    let mut fields = prefix(b'+');
    fields.extend([
        F::C(b'U'),
        F::L(false),
        F::P(support),
        F::R(distance),
        F::R(NULL_REAL),
    ]);
    fields
}

fn indexed_support(nodes: &[(u16, u16, Vec<F>)], profile: Option<CurveCarrier>) -> CarrierIndex {
    let (bytes, start) = sequence_of(nodes);
    let records = topology::nodes::sequence(&bytes).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut carriers = super::super::index::index_carriers_with_nodes(
        &ctx,
        &bytes[start..],
        super::super::index::NodeCarriers {
            analytic: super::super::carrier_nodes::analytic(&records, start, &mut Vec::new()),
            offsets: super::super::offset::from_nodes(&records, start),
            sweeps: super::super::sweep::from_nodes(&records, start),
            ..Default::default()
        },
        None,
        &HashSet::new(),
    )
    .unwrap();
    if let Some(profile) = profile {
        carriers.insert(super::super::Carrier::Curve(profile));
    }
    assert!(!carriers.has_unvalidated_offset());
    carriers
}

fn emit_support(
    nodes: &[(u16, u16, Vec<F>)],
    profile: Option<CurveCarrier>,
) -> (cadmpeg_ir::CadIr, SurfaceId) {
    let carriers = indexed_support(nodes, profile);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut out = Brep::default();
    let mut annotations = AnnotationBuilder::new();
    let source = StreamHandle::new(cadmpeg_ir::stream_name!("offset-fixture"));
    let mut sink = BrepSink {
        ctx: &ctx,
        out: &mut out,
    };
    let surface = ensure_surface_support(
        &mut sink,
        3,
        &carriers,
        &HashMap::new(),
        &mut annotations,
        &source,
        &mut HashSet::new(),
    )
    .unwrap()
    .unwrap();
    let count = sink.out.surfaces.len();
    assert_eq!(
        ensure_surface_support(
            &mut sink,
            3,
            &carriers,
            &HashMap::new(),
            &mut annotations,
            &source,
            &mut HashSet::new()
        )
        .unwrap(),
        Some(surface.clone())
    );
    assert_eq!(sink.out.surfaces.len(), count);
    prune_rejected_topology(&mut out);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces = out.surfaces;
    ir.model.curves = out.curves;
    ir.model.procedural_surfaces = out.procedural_surfaces;
    (ir, surface)
}

fn point(ir: &cadmpeg_ir::CadIr, surface: &SurfaceId, u: f64, v: f64) -> cadmpeg_ir::math::Point3 {
    cadmpeg_ir::eval::model_surface_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &cadmpeg_ir::index::ModelIndex::build(ir),
        surface,
        u,
        v,
    )
    .unwrap()
    .get()
}

#[test]
fn unchecked_offsets_preserve_both_distance_polarities_and_nested_supports() {
    for distance in [-0.0025, 0.0025] {
        let mut plane = prefix(b'+');
        plane.extend([F::V([0.0; 3]), F::V([0.0, 0.0, 1.0]), F::V([1.0, 0.0, 0.0])]);
        let (ir, surface) = emit_support(
            &[
                (60, 3, offset(4, distance)),
                (60, 4, offset(20, distance)),
                (50, 20, plane),
            ],
            None,
        );
        assert_eq!(
            point(&ir, &surface, 0.25, 0.75),
            cadmpeg_ir::math::Point3::new(0.25, 0.75, distance * 2000.0)
        );
    }
}

#[test]
fn hidden_swept_offset_support_keeps_native_travel_units_and_normal_side() {
    for distance in [-0.0025, 0.0025] {
        let mut sweep = prefix(b'+');
        sweep.extend([F::P(21), F::V([0.0, 0.0, 1.0]), F::R(NULL_REAL)]);
        let profile = CurveCarrier {
            attr: 21,
            offset: 0,
            end: 0,
            marker_reversed: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(4.0, 0.0, 0.0),
                    ],
                    None,
                    false,
                )
                .unwrap(),
            )),
        };
        let (ir, surface) = emit_support(
            &[(60, 3, offset(20, distance)), (67, 20, sweep)],
            Some(profile),
        );
        assert_eq!(ir.model.curves.len(), 1);
        let actual = point(&ir, &surface, 0.25, 0.002);
        assert!(
            actual.distance(cadmpeg_ir::math::Point3::new(1.0, -distance * 1000.0, 2.0)) < 1.0e-12
        );
    }
}

#[test]
fn unchecked_cylinder_offset_radius_follows_the_stored_distance_sign() {
    for distance in [-0.0025, 0.0025] {
        let mut cylinder = prefix(b'+');
        cylinder.extend([
            F::V([0.0; 3]),
            F::V([0.0, 0.0, 1.0]),
            F::R(0.00725),
            F::V([1.0, 0.0, 0.0]),
        ]);
        let (ir, surface) =
            emit_support(&[(60, 3, offset(20, distance)), (51, 20, cylinder)], None);
        assert!(
            point(&ir, &surface, 0.0, 3.0).distance(cadmpeg_ir::math::Point3::new(
                7.25 + distance * 1000.0,
                0.0,
                3.0
            )) < 1.0e-12
        );
    }
}

#[test]
fn bounded_offset_faces_require_every_boundary_edge() {
    let (bytes, start) = crate::test_support::parasolid::sequence::topology_sequence();
    let records = topology::nodes::sequence(&bytes).unwrap();
    let tables = topology::nodes::tables(&records, &bytes, start).unwrap();
    let mut face = WalkedFace {
        bridge_attr: 5,
        surface_attr: 20,
        sense: Sense::Forward,
        loops: vec![(6, vec![7])],
        vertex_loops: Vec::new(),
        unresolved_loop: None,
        unresolved_fins: Vec::new(),
        withheld_forward_links: 0,
        unlinked_loops: HashSet::new(),
    };
    let exact = HashSet::from([8]);
    assert!(face_has_exact_boundary(&face, &tables, &exact));
    assert!(!face_has_exact_boundary(&face, &tables, &HashSet::new()));
    assert!(!face_has_exact_boundary(
        &face,
        &tables,
        &HashSet::from([9])
    ));
    face.unresolved_fins.push(7);
    assert!(!face_has_exact_boundary(&face, &tables, &exact));
    face.unresolved_fins.clear();
    face.vertex_loops.push((6, 9));
    assert!(!face_has_exact_boundary(&face, &tables, &exact));
    face.vertex_loops.clear();
    face.loops.clear();
    assert!(!face_has_exact_boundary(&face, &tables, &exact));
}

#[test]
fn offset_support_alias_keeps_a_resolved_sweep_with_chart_boundaries() {
    let mut sweep = prefix(b'+');
    sweep.extend([F::P(21), F::V([0.0, 0.0, 1.0]), F::R(NULL_REAL)]);
    let profile = CurveCarrier {
        attr: 21,
        offset: 0,
        end: 0,
        marker_reversed: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        )),
    };
    let carriers = indexed_support(&[(67, 20, sweep)], Some(profile));
    let (bytes, start) = crate::test_support::parasolid::sequence::topology_sequence();
    let records = topology::nodes::sequence(&bytes).unwrap();
    let tables = topology::nodes::tables(&records, &bytes, start).unwrap();
    let mut face = WalkedFace {
        bridge_attr: 5,
        surface_attr: 20,
        sense: Sense::Forward,
        loops: vec![(6, vec![7])],
        vertex_loops: Vec::new(),
        unresolved_loop: None,
        unresolved_fins: Vec::new(),
        withheld_forward_links: 0,
        unlinked_loops: HashSet::new(),
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(!face_has_exact_boundary(&face, &tables, &HashSet::new()));
    assert!(sweep_face_can_alias(&ctx, &face, &tables, &carriers, &HashSet::new()).unwrap());
    face.loops = vec![(6, vec![70])];
    assert!(!sweep_face_can_alias(&ctx, &face, &tables, &carriers, &HashSet::new()).unwrap());
    face.loops = vec![(6, vec![7])];
    assert!(sweep_face_can_alias(&ctx, &face, &tables, &carriers, &HashSet::from([8])).unwrap());
}
