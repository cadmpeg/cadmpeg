// SPDX-License-Identifier: Apache-2.0

use crate::test_support::parasolid::sequence::{face_fields, sequence_of, F};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn decode(nodes: &[(u16, u16, Vec<F>)]) -> super::Brep {
    let (payload, _) = sequence_of(nodes);
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::decode_bodies(
        &ctx,
        &[(&payload, &header)],
        &cadmpeg_ir::stream_name!("closed-face"),
    )
    .unwrap()
}

fn loopless_nodes(kind: u16) -> Vec<(u16, u16, Vec<F>)> {
    use F::{B, C, I, P, R, V};
    let mut face = face_fields();
    face[5] = P(1);
    let mut surface = vec![I(7), P(1), P(1), P(1), P(1), P(1), C(b'+')];
    match kind {
        53 => surface.extend([
            V([0.0; 3]),
            R(0.007),
            V([0.0, 0.0, 1.0]),
            V([1.0, 0.0, 0.0]),
        ]),
        54 => surface.extend([
            V([0.0; 3]),
            V([0.0, 0.0, 1.0]),
            R(0.007),
            R(0.002),
            V([1.0, 0.0, 0.0]),
        ]),
        50 => surface.extend([V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]),
        _ => unreachable!(),
    }
    #[rustfmt::skip]
    let nodes = vec![
        (12, 2, vec![I(7), P(1), P(1), P(1), P(1), P(1), P(1), R(1000.0), R(1.0e-8), P(1), P(1), P(1), B(1), P(1), B(1), B(0), P(4), P(1), P(1), P(1), P(3), P(1), P(1)]),
        (19, 3, vec![I(2), P(1), P(2), P(1), P(1), P(4), C(b'S')]),
        (13, 4, vec![I(3), P(1), P(2), P(1), P(5), P(1), P(1), P(3), P(1)]),
        (14, 5, face),
        (kind, 20, surface),
    ];
    nodes
}

#[test]
fn typed_loopless_sphere_and_torus_keep_their_ownership() {
    for kind in [53, 54] {
        let out = decode(&loopless_nodes(kind));
        assert_eq!(
            (out.bodies.len(), out.shells.len(), out.faces.len()),
            (1, 1, 1)
        );
        assert!(out.faces[0].loops.is_empty());
        assert!(out.loops.is_empty());
        assert!(out.edges.is_empty());
        assert!(out.vertices.is_empty());
        assert!(out.losses.is_empty());
        assert_eq!(
            out.shells[0].faces(),
            std::slice::from_ref(&out.faces[0].id)
        );
    }
}

#[test]
fn a_loopless_plane_stays_withheld_with_ownership_loss() {
    let out = decode(&loopless_nodes(50));
    assert!(out.faces.is_empty());
    assert!(out
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.ownership-withheld"));
}

#[test]
fn a_missing_spherical_loop_is_withheld() {
    let mut nodes = loopless_nodes(53);
    nodes[3].2[5] = F::P(6);
    let out = decode(&nodes);
    assert!(out.faces.is_empty());
    assert_eq!(out.stats.withheld_faces, 1);
}

fn indexed(geometry: SolvedCurveGeometry) -> super::IndexedCurve {
    super::IndexedCurve::Exact(super::CurveCarrier {
        attr: 20,
        offset: 0,
        end: 0,
        geometry: CurveGeometry::Solved(geometry),
        marker_reversed: None,
        parameter_range: None,
    })
}

#[test]
fn an_ellipse_ring_traverses_one_revolution_from_its_major_axis() {
    let ellipse = indexed(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            7.0,
            2.0,
        )
        .unwrap(),
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::closed_edge_start(&ctx, Some(&ellipse)).unwrap(),
        Some((Point3::new(7.0, 0.0, 0.0), [0.0, std::f64::consts::TAU]))
    );
}

fn ellipse_sheet(partner: bool) -> Vec<(u16, u16, Vec<F>)> {
    use F::{C, I, P, R, V};
    let mut nodes = loopless_nodes(50);
    nodes[0].2[14] = F::B(3);
    nodes[3].2[5] = P(6);
    #[rustfmt::skip]
    nodes.extend([
        (15, 6, vec![I(5), P(1), P(7), P(5), P(1)]),
        (17, 7, vec![P(1), P(6), P(7), P(7), P(1), P(if partner {11} else {1}), P(8), P(1), P(1), C(b'+')]),
        (17, 11, vec![P(1), P(1), P(1), P(1), P(1), P(7), P(8), P(1), P(1), C(b'-')]),
        (16, 8, vec![I(6), P(1), R(crate::test_support::parasolid::sequence::NULL_REAL), P(7), P(1), P(1), P(21), P(1), P(1), P(2)]),
        (32, 21, vec![I(7), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]), R(0.007), R(0.002)]),
    ]);
    nodes
}

#[test]
fn a_reciprocal_ellipse_ring_keeps_its_sheet_face() {
    let out = decode(&ellipse_sheet(true));
    assert_eq!(
        (
            out.faces.len(),
            out.loops.len(),
            out.edges.len(),
            out.vertices.len()
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(out.edges[0].start, out.edges[0].end);
    assert_eq!(
        out.edges[0].param_range().unwrap().get(),
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(out.stats.withheld_ring_edges, 0);
}

#[test]
fn an_ellipse_ring_without_a_mutual_partner_stays_withheld() {
    let out = decode(&ellipse_sheet(false));
    assert!(out.faces.is_empty());
    assert!(out.edges.is_empty());
    assert_eq!(out.stats.withheld_ring_edges, 1);
}

#[test]
fn a_clamped_spline_ring_requires_coincident_domain_ends() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    for gap in [0.0, 1.0e-7, 2.0] {
        let closes = gap == 0.0;
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 2.0, 0.0),
            Point3::new(0.0, gap, 0.0),
        ];
        let curve = indexed(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(1, vec![2.0, 2.0, 3.0, 4.0, 5.0, 5.0], points, None, false)
                .unwrap(),
        ));
        let placed = indexed(SolvedCurveGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedCurve::try_new(
                Box::new(curve.carrier().geometry.solved().unwrap().clone()),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .unwrap(),
        ));
        for candidate in [&curve, &placed] {
            let start = super::closed_edge_start(&ctx, Some(candidate)).unwrap();
            assert_eq!(start.is_some(), closes);
            if let Some((point, range)) = start {
                assert_eq!(point, Point3::new(0.0, 0.0, 0.0));
                assert_eq!(range, [2.0, 5.0]);
            }
        }
    }
}

#[test]
fn a_periodic_spline_ring_uses_its_effective_knot_domain() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    let curve = indexed(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![-1.0, 0.0, 1.0, 2.0, 3.0, 4.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
                Point3::new(0.0, 3.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            None,
            true,
        )
        .unwrap(),
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::closed_edge_start(&ctx, Some(&curve)).unwrap(),
        Some((Point3::new(0.0, 0.0, 0.0), [0.0, 3.0]))
    );
}

fn bound_ellipse(nodes: &mut Vec<(u16, u16, Vec<F>)>, [low, high]: [f64; 2]) {
    use F::{C, I, P, R, V};
    nodes.iter_mut().find(|(kind, _, _)| *kind == 32).unwrap().1 = 22;
    let point = |t: f64| [0.007 * t.cos(), 0.002 * t.sin(), 0.0];
    #[rustfmt::skip]
    nodes.push((133, 21, vec![I(8), P(1), P(1), P(1), P(1), P(1), C(b'+'), P(22), V(point(low)), V(point(high)), R(low), R(high)]));
}

#[test]
fn a_bounded_ellipse_ring_requires_one_complete_revolution() {
    for (range, admitted) in [
        ([0.0, std::f64::consts::TAU], true),
        ([std::f64::consts::PI, 3.0 * std::f64::consts::PI], true),
        ([0.0, std::f64::consts::PI], false),
        ([0.0, 2.0 * std::f64::consts::TAU], false),
    ] {
        let mut nodes = ellipse_sheet(true);
        bound_ellipse(&mut nodes, range);
        let out = decode(&nodes);
        assert_eq!(out.edges.len(), usize::from(admitted));
        assert_eq!(out.faces.len(), usize::from(admitted));
        assert_eq!(out.stats.withheld_ring_edges, usize::from(!admitted));
    }
}

#[test]
fn a_vertexless_ring_recovers_a_canonical_boundary_dummy() {
    let mut nodes = ellipse_sheet(true);
    nodes
        .iter_mut()
        .find(|(kind, index, _)| (*kind, *index) == (17, 7))
        .unwrap()
        .2[9] = F::C(b'-');
    nodes
        .iter_mut()
        .find(|(kind, index, _)| (*kind, *index) == (17, 11))
        .unwrap()
        .2[9] = F::C(b'+');
    nodes.iter_mut().find(|(kind, _, _)| *kind == 16).unwrap().2[3] = F::P(11);
    nodes[3].2[8] = F::C(b'-');
    let out = decode(&nodes);
    assert_eq!(
        (out.faces.len(), out.edges.len(), out.vertices.len()),
        (1, 1, 1)
    );
    assert_eq!(out.stats.withheld_fin_partners, 0);
    assert_eq!(out.stats.withheld_ring_edges, 0);
}

#[test]
fn a_loopless_nurbs_face_requires_both_periodic_parameters() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    for periodic in [[true, true], [true, false], [false, true], [false, false]] {
        let (payload, offset) = sequence_of(&loopless_nodes(53));
        let records = super::topology::nodes::sequence(&payload).unwrap();
        let tables = super::topology::nodes::tables(&records, &payload, offset).unwrap();
        let facts = super::typed::nodes::facts(&records, offset).unwrap();
        let meridian = [(7.0, 0.0), (5.0, 2.0), (3.0, 0.0), (7.0, 0.0)];
        let h = 3.0_f64.sqrt() * 0.5;
        let longitudes = [(1.0, 0.0), (-0.5, h), (-0.5, -h), (1.0, 0.0)];
        let grid = meridian
            .iter()
            .map(|(r, z)| {
                longitudes
                    .iter()
                    .map(|(x, y)| Point3::new(r * x, r * y, *z))
                    .collect()
            })
            .collect();
        let axis = |p| NurbsSurfaceAxis::new(1, vec![-1.0, 0.0, 1.0, 2.0, 3.0, 4.0], p);
        let surface = NurbsSurface::from_lanes(
            axis(periodic[0]),
            axis(periodic[1]),
            NurbsSurfaceLanes::new(grid, None),
            false,
        )
        .unwrap();
        let mut carriers = super::CarrierIndex::default();
        carriers.insert(super::super::Carrier::Surface(
            super::super::SurfaceCarrier {
                attr: 20,
                offset: 0,
                end: 0,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                orientation_reversed: false,
                marker_reversed: false,
            },
        ));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let out = super::decode_graph(
            &ctx,
            &mut carriers,
            &tables,
            super::entity::Facts::default(),
            &facts,
            &cadmpeg_ir::stream_name!("periodic-face"),
        )
        .unwrap();
        assert_eq!(out.faces.len(), usize::from(periodic == [true, true]));
        if let Some(face) = out.faces.first() {
            assert!(face.loops.is_empty());
            assert_eq!(out.shells[0].faces(), std::slice::from_ref(&face.id));
        }
    }
}
