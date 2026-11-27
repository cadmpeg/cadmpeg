// SPDX-License-Identifier: Apache-2.0

use crate::loss::SldprtLossCode;
use crate::test_support::parasolid::sequence::{face_fields, loopless_nodes, sequence_of, F};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

/// How many records `out` gives up for one of `causes`.
fn given_up(out: &super::Brep, causes: &[&str]) -> usize {
    out.dropped
        .iter()
        .filter(|dropped| {
            matches!(dropped, super::DroppedRecord::GivenUp { error, .. } if causes.contains(&error.as_str()))
        })
        .count()
}

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

#[test]
fn typed_loopless_sphere_and_torus_keep_their_ownership() {
    for kind in [53, 54] {
        let out = decode(&loopless_nodes(kind));
        assert_eq!(
            (out.bodies.len(), out.shells.len(), out.faces.len()),
            (1, 1, 1)
        );
        assert!(out.faces[0].loops.stated().next().is_none());
        assert!(out.loops.is_empty());
        assert!(out.edges.is_empty());
        assert!(out.vertices.is_empty());
        assert!(out.losses.is_empty());
        assert_eq!(out.faces[0].shell, out.shells[0].id);
    }
}

#[test]
fn a_compact_trimmed_face_supplies_topology_for_its_null_loop_ownership_face() {
    use crate::test_support::parasolid::{
        parasolid_with_body, sphere_carrier, sphere_patch_body, typed_single_face_ownership,
    };
    for (trimmed, ownership_first, separate_streams) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, false, true),
        (true, true, true),
    ] {
        let mut body = if trimmed {
            sphere_patch_body()
        } else {
            sphere_carrier(100, [0.0; 3], 1.0)
        };
        let topology_len = body.len();
        typed_single_face_ownership(&mut body, 900, 901, 902, 10, 100, 1);
        let mut parts = if separate_streams {
            let ownership = body.split_off(topology_len);
            vec![body, ownership]
        } else {
            if ownership_first {
                body.rotate_left(topology_len);
            }
            vec![body]
        };
        if separate_streams && ownership_first {
            parts.reverse();
        }
        let payloads = parts
            .iter()
            .map(|part| {
                let payload = parasolid_with_body("sphere ownership", "SCH_SW_33103_11000", part);
                let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
                (payload, header)
            })
            .collect::<Vec<_>>();
        let streams = payloads
            .iter()
            .map(|(payload, header)| (payload.as_slice(), header))
            .collect::<Vec<_>>();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let out = super::decode_bodies(
            &ctx,
            &streams,
            &cadmpeg_ir::stream_name!("sphere-ownership"),
        )
        .unwrap();
        assert_eq!(
            (out.bodies.len(), out.shells.len(), out.faces.len()),
            (1, 1, 1)
        );
        assert_eq!(out.faces[0].loops.stated().count(), usize::from(trimmed));
        assert_eq!(out.loops.len(), usize::from(trimmed));
        assert_eq!(out.faces[0].shell, out.shells[0].id);
        assert!(out.losses.is_empty(), "{:?}", out.losses);
    }
}

#[test]
fn a_loopless_plane_keeps_its_shell_region_and_body_marked() {
    use cadmpeg_ir::topology::{BodyKind, Unstated};
    let out = decode(&loopless_nodes(50));
    assert!(out.faces.is_empty());
    assert_eq!(given_up(&out, &[super::FACE_WITHHELD]), 1);
    assert!(!out
        .dropped
        .iter()
        .any(|dropped| matches!(dropped, super::DroppedRecord::With { .. })));
    // The shell holds no stated face and says so; its region and body stay.
    let [shell] = out.shells.as_slice() else {
        panic!("one shell: {:#?}", out.dropped);
    };
    assert_eq!(shell.id.as_str(), "sldprt:brep:shell#4");
    assert_eq!(shell.unstated, Some(Unstated::Members));
    let [region] = out.regions.as_slice() else {
        panic!("one region");
    };
    assert_eq!(region.id.as_str(), "sldprt:brep:region#3");
    assert_eq!(region.unstated, None);
    assert_eq!(region.stated_shells().collect::<Vec<_>>(), [&shell.id]);
    let [body] = out.bodies.as_slice() else {
        panic!("one body");
    };
    assert_eq!(body.id.as_str(), "sldprt:brep:body#2");
    assert_eq!((body.kind, body.unstated), (BodyKind::Solid, None));
    // The records stated in partial form are not counted as withheld.
    assert!(!out
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.ownership-withheld"));
}

#[test]
fn a_derived_shell_states_no_members_once_a_face_is_given_up() {
    use cadmpeg_ir::topology::Unstated;
    use F::{C, I, P, V};
    let mut nodes = loopless_nodes(53);
    // The region does not name its body, so the decode derives the hierarchy.
    nodes[1].2[2] = P(9);
    let kept = decode(&nodes);
    assert!(kept.stats.synthetic_body_grouping);
    assert!(kept.shells.iter().all(|shell| shell.unstated.is_none()));
    // A loopless plane beside the sphere in its shell is given up.
    nodes[3].2[3] = P(30);
    let mut plane = face_fields();
    plane[0] = I(9);
    plane[4] = P(5);
    plane[5] = P(1);
    plane[7] = P(21);
    nodes.push((14, 30, plane));
    #[rustfmt::skip]
    nodes.push((50, 21, vec![I(8), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]));
    let out = decode(&nodes);
    assert!(out.stats.synthetic_body_grouping);
    let [face] = out.faces.as_slice() else {
        panic!("the sphere face is kept: {:#?}", out.dropped);
    };
    assert_eq!(face.id.as_str(), "sldprt:brep:face#5");
    assert!(!out.shells.is_empty());
    assert!(out
        .shells
        .iter()
        .all(|shell| shell.unstated == Some(Unstated::Members)));
}

/// A typed FACE node that the decode does not state, with no record given
/// up, marks each derived shell: no node says which shell held that face.
#[test]
fn a_derived_shell_states_no_members_once_a_typed_face_is_not_stated() {
    use cadmpeg_ir::ids::{RegionId, ShellId};
    use cadmpeg_ir::topology::{Shell, Unstated};
    let region = RegionId::try_from("sldprt:brep:region#0").unwrap();
    let mut out = super::Brep::default();
    out.stats.synthetic_body_grouping = true;
    out.shells.push(Shell::new(
        ShellId::try_from("sldprt:brep:shell#0").unwrap(),
        region,
    ));
    super::mark_derived_shells(&mut out, false);
    assert_eq!(out.shells[0].unstated, None);
    super::mark_derived_shells(&mut out, true);
    assert_eq!(out.shells[0].unstated, Some(Unstated::Members));
}

#[test]
fn a_spherical_face_whose_loop_record_is_missing_keeps_a_partial_loop_list() {
    let mut nodes = loopless_nodes(53);
    nodes[3].2[5] = F::P(6);
    let out = decode(&nodes);
    let [face] = &out.faces[..] else {
        panic!("the face is kept");
    };
    assert!(face.loops.is_partial());
    assert_eq!(face.loops.stated().count(), 0);
    assert_eq!(given_up(&out, &[crate::brep::graph::FACE_WITHHELD]), 0);
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
    assert_eq!(
        out.edges[0].ends.as_ref().unwrap().start,
        out.edges[0].ends.as_ref().unwrap().end
    );
    assert_eq!(
        out.edges[0].param_range().unwrap().get(),
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(given_up(&out, &[crate::brep::graph::RING_EDGE_WITHHELD]), 0);
}

#[test]
fn an_ellipse_ring_without_a_mutual_partner_stays_withheld() {
    let out = decode(&ellipse_sheet(false));
    let [face] = &out.faces[..] else {
        panic!("the face is kept");
    };
    assert!(face.loops.is_partial());
    assert!(out.edges.is_empty());
    assert_eq!(given_up(&out, &[crate::brep::graph::RING_EDGE_WITHHELD]), 1);
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
        let [face] = &out.faces[..] else {
            panic!("the face is kept");
        };
        assert_eq!(face.loops.is_partial(), !admitted);
        assert_eq!(
            given_up(&out, &[crate::brep::graph::RING_EDGE_WITHHELD]),
            usize::from(!admitted)
        );
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
    assert_eq!(
        given_up(
            &out,
            &[
                crate::brep::graph::FIN_PARTNERS_UNCORROBORATED,
                crate::brep::graph::LOOP_FIN_UNREAD,
            ]
        ),
        0
    );
    assert_eq!(given_up(&out, &[crate::brep::graph::RING_EDGE_WITHHELD]), 0);
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
            assert!(face.loops.stated().next().is_none());
            assert_eq!(face.shell, out.shells[0].id);
        }
    }
}

#[test]
fn spun_circle_with_one_stored_bound_revolves_the_whole_profile() {
    use crate::brep::{index, topology};
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::topology::Sense;
    use std::collections::HashSet;

    let mut body =
        crate::test_support::parasolid::circle_carrier(6, [0.0; 3], [0.0, 1.0, 0.0], 0.002);
    body.extend([0x00, 0x44]);
    body.extend(12_u16.to_be_bytes());
    body.extend(7_u32.to_be_bytes());
    for value in [1_u16, 2, 3, 4, 1] {
        body.extend(value.to_be_bytes());
    }
    body.push(b'+');
    body.extend(6_u16.to_be_bytes());
    for value in [
        0.0_f64,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        -0.002,
        -3.14158e13,
        -3.14158e13,
        -3.14158e13,
        std::f64::consts::FRAC_PI_2,
        -3.14158e13,
    ] {
        body.extend(value.to_be_bytes());
    }
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let carriers = index::scan_carriers(&ctx, &body).unwrap();
    let face = super::WalkedFace {
        bridge_attr: 20,
        surface_attr: 12,
        sense: Sense::Forward,
        loops: Vec::new(),
        vertex_loops: Vec::new(),
        unresolved_loop: None,
        unresolved_fins: Vec::new(),
        unlinked_loops: HashSet::new(),
        lost_loops: Vec::new(),
        chain_cut: None,
    };
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let resolved = super::resolve_sweep_surface(
        &ctx,
        &carriers,
        &topology::Tables::default(),
        &face,
        &mut refusal,
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    let SolvedSurfaceGeometry::Nurbs(surface) = resolved.geometry else {
        panic!("spun NURBS surface")
    };
    assert_eq!(surface.u_knots().as_slice().first(), Some(&0.0));
    assert_eq!(
        surface.u_knots().as_slice().last(),
        Some(&std::f64::consts::TAU)
    );
    assert_eq!(surface.u_count(), 9);
    assert!(refusal.take_records().is_empty());
}

#[test]
fn ignored_spun_profile_intervals_keep_typed_surfaces_and_report_loss() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use F::{C, I, P, R, V};
    for [low, high] in [
        [0.0, 4.0 * std::f64::consts::PI],
        [
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        ],
        [1.0, 1.0],
        [0.0, f64::INFINITY],
        [f64::NAN, 1.0],
    ] {
        let mut nodes = ellipse_sheet(true);
        nodes[4] = (
            68,
            20,
            vec![
                I(7),
                P(1),
                P(1),
                P(1),
                P(1),
                P(1),
                C(b'+'),
                P(22),
                V([0.0; 3]),
                V([1.0, 0.0, 0.0]),
                V([0.007, 0.0, 0.0]),
                V([-0.007, 0.0, 0.0]),
                R(low),
                R(high),
                V([0.0, 1.0, 0.0]),
                R(crate::test_support::parasolid::sequence::NULL_REAL),
            ],
        );
        let mut profile = nodes
            .iter()
            .find(|(kind, _, _)| *kind == 32)
            .unwrap()
            .clone();
        profile.1 = 22;
        nodes.push(profile);
        let out = decode(&nodes);
        assert_eq!(out.faces.len(), 1);
        assert_eq!(out.stats.unknown_surface_faces, 0);
        let surface = out
            .surfaces
            .iter()
            .find(|surface| surface.id == out.faces[0].surface)
            .unwrap();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = &surface.geometry
        else {
            panic!("typed spun surface")
        };
        assert_eq!(surface.u_count(), 9);
        assert_eq!(surface.u_knots().as_slice().first(), Some(&0.0));
        assert_eq!(
            surface.u_knots().as_slice().last(),
            Some(&std::f64::consts::TAU)
        );
        let notes = out
            .losses
            .iter()
            .filter(|note| note.code.local_code() == "geometry.spun-profile-interval-ignored")
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains(&format!("[{low}, {high}]")));
    }
}

/// The ring of `ellipse_sheet` whose edge has no EDGE record and whose FINs
/// name a vertex that no record states.
fn unresolved_end_sheet() -> Vec<(u16, u16, Vec<F>)> {
    let mut nodes = ellipse_sheet(true);
    nodes.retain(|(kind, _, _)| *kind != 16);
    for (kind, _, fields) in &mut nodes {
        if *kind == 17 {
            fields[4] = F::P(9);
        }
    }
    nodes
}

#[test]
fn a_loop_whose_unrecorded_edge_has_an_unresolved_end_names_the_vertex() {
    let out = decode(&unresolved_end_sheet());
    assert!(out.loops.is_empty());
    assert!(out.edges.is_empty());
    let loop_charge = "an end vertex of an edge of the loop does not resolve";
    assert!(
        out.dropped.iter().any(|dropped| matches!(
            dropped,
            super::DroppedRecord::GivenUp { record, error, .. }
                if record.contains("loop") && error == loop_charge
        )),
        "{:#?}",
        out.dropped
    );
    assert_eq!(
        given_up(
            &out,
            &[
                crate::brep::graph::FIN_PARTNERS_UNCORROBORATED,
                crate::brep::graph::LOOP_FIN_UNREAD,
            ]
        ),
        0
    );
}

/// A body kept after all its faces are lost keeps its native identity
/// values and is the target of its last modifying feature.
#[test]
fn a_body_whose_every_face_is_lost_keeps_its_identity_and_modifier() {
    let (payload, offset) = sequence_of(&loopless_nodes(50));
    let records = super::topology::nodes::sequence(&payload).unwrap();
    let tables = super::topology::nodes::tables(&records, &payload, offset).unwrap();
    let facts = super::typed::nodes::facts(&records, offset).unwrap();
    let entity_facts = super::entity::Facts {
        body_payloads: vec![(2, vec![77])],
        body_modifiers: vec![super::attrib::BodyModifier {
            body_attr: 2,
            feature_source_id: crate::brep::feature_source::FeatureSourceId::try_from(5).unwrap(),
            target: None,
        }],
        ..super::entity::Facts::default()
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let out = super::decode_graph(
        &ctx,
        &mut super::CarrierIndex::default(),
        &tables,
        entity_facts,
        &facts,
        &cadmpeg_ir::stream_name!("lost-faces"),
    )
    .unwrap();
    assert!(out.faces.is_empty());
    let [body] = out.bodies.as_slice() else {
        panic!("one body: {:#?}", out.dropped);
    };
    assert_eq!(body.id.as_str(), "sldprt:brep:body#2");
    let identities: Vec<_> = out
        .native_body_identities
        .iter()
        .map(|identity| (identity.target.as_str(), identity.values.as_slice()))
        .collect();
    assert_eq!(identities, [("sldprt:brep:body#2", &[77][..])]);
    let targets: Vec<_> = out
        .body_modifiers
        .iter()
        .map(|modifier| modifier.target.as_deref())
        .collect();
    assert_eq!(targets, [Some("sldprt:brep:body#2")]);
}

/// The nodes of `nodes` without their BODY record, whose body the decode
/// then derives.
fn without_body(mut nodes: Vec<(u16, u16, Vec<F>)>) -> Vec<(u16, u16, Vec<F>)> {
    nodes.retain(|(kind, _, _)| *kind != 12);
    nodes
}

#[test]
fn a_derived_body_is_a_solid_only_where_its_faces_close() {
    use cadmpeg_ir::topology::BodyKind;
    let mut partial = loopless_nodes(53);
    partial[3].2[5] = F::P(6);
    for (nodes, kind) in [
        (loopless_nodes(53), BodyKind::Solid),
        (partial, BodyKind::Sheet),
        (ellipse_sheet(true), BodyKind::Sheet),
    ] {
        let out = decode(&without_body(nodes));
        assert!(out.stats.synthetic_body_grouping);
        assert_eq!(out.faces.len(), 1, "{:#?}", out.dropped);
        let [body] = out.bodies.as_slice() else {
            panic!("one derived body");
        };
        assert_eq!(body.kind, kind);
    }
}

#[test]
fn body_records_that_do_not_link_up_still_state_the_derived_body_kind() {
    use cadmpeg_ir::topology::BodyKind;
    let mut nodes = loopless_nodes(53);
    // A sheet BODY record that its region names, whose region chain does not
    // reach that region.
    nodes[0].2[14] = F::B(3);
    nodes[0].2[20] = F::P(9);
    let out = decode(&nodes);
    assert!(out.stats.synthetic_body_grouping);
    let kind = out.stats.derived_kind;
    assert!(kind.agreed && !kind.disagree && !kind.absent);
    let [body] = out.bodies.as_slice() else {
        panic!("one derived body");
    };
    assert_eq!(body.kind, BodyKind::Sheet);
}

/// A BODY record that names a region as the head of its region chain, where
/// that region does not name the body back, does not link to it: the
/// derived body takes the kind of its faces.
#[test]
fn a_body_record_its_region_does_not_name_states_no_derived_kind() {
    use cadmpeg_ir::topology::BodyKind;
    let mut nodes = loopless_nodes(53);
    nodes[0].2[14] = F::B(3);
    nodes[1].2[2] = F::P(9);
    let out = decode(&nodes);
    assert!(out.stats.synthetic_body_grouping);
    let kind = out.stats.derived_kind;
    assert!(!kind.agreed && !kind.disagree && kind.absent);
    let [body] = out.bodies.as_slice() else {
        panic!("one derived body");
    };
    assert_eq!(body.kind, BodyKind::Solid);
}

/// A derived hierarchy beside a typed BODY, REGION, and SHELL that hold its
/// faces counts each of them as withheld.
#[test]
fn a_derived_hierarchy_counts_the_typed_region_it_withholds() {
    use F::{C, I, P, R, V};
    let mut nodes = loopless_nodes(53);
    // FACE 7 in shell 4, which the typed graph does not read: its tolerance
    // is negative.
    nodes[3].2[3] = P(7);
    let mut face = face_fields();
    face[0] = I(12);
    face[2] = R(-1.0);
    face[4] = P(5);
    face[5] = P(1);
    face[6] = P(4);
    face[7] = P(21);
    nodes.push((14, 7, face));
    #[rustfmt::skip]
    nodes.push((53, 21, vec![I(13), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.0, 0.0, 0.05]), R(0.007), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]));
    let out = decode(&nodes);
    assert!(out.stats.synthetic_body_grouping);
    let notes: Vec<_> = out
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::TopologyOwnershipWithheld.kind())
        .map(|loss| loss.message.as_str())
        .collect();
    assert_eq!(
        notes,
        ["3 current typed ownership node(s) are withheld: 1 body node(s), 1 region node(s), 1 shell node(s), and 0 face node(s)."]
    );
}

#[test]
fn a_face_whose_loop_chain_is_cut_is_noted() {
    let mut nodes = loopless_nodes(53);
    nodes[3].2[5] = F::P(6);
    let out = decode(&nodes);
    let notes: Vec<_> = out
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::TopologyLoopChainCut.kind())
        .collect();
    let [note] = notes.as_slice() else {
        panic!("one chain-cut note: {:#?}", out.losses);
    };
    assert!(
        note.message.contains("no loop record states"),
        "{}",
        note.message
    );
}
