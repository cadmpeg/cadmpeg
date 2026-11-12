// SPDX-License-Identifier: Apache-2.0
use super::feature_outputs::{base_feature, scope_between, state};
use crate::history::bind_feature_outputs;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology,
};

/// A closed body of two triangular faces that share their three edges, with
/// its vertices at `x`, `x + 2` and `x + 1`.
fn pillow(x: f64) -> AsmHistoricalTopology {
    use cadmpeg_ir::math::Point3;

    let relation = |owner_ref, member_refs: &[i64]| AsmHistoricalRelation {
        owner_ref,
        member_refs: member_refs.to_vec(),
    };
    let mut topology = AsmHistoricalTopology {
        bodies: vec![100],
        regions: vec![101],
        shells: vec![102],
        faces: vec![110, 120],
        loops: vec![111, 121],
        edges: vec![1000, 1001, 1002],
        vertices: vec![1, 2, 3],
        points: vec![11, 12, 13],
        body_regions: vec![relation(100, &[101])],
        region_shells: vec![relation(101, &[102])],
        shell_faces: vec![relation(102, &[110, 120])],
        face_loops: vec![relation(110, &[111]), relation(120, &[121])],
        loop_coedges: vec![
            relation(111, &[1110, 1111, 1112]),
            relation(121, &[1210, 1211, 1212]),
        ],
        edge_vertices: [(1000, 1, 2), (1001, 2, 3), (1002, 3, 1)]
            .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
                edge,
                start_vertex,
                end_vertex,
            })
            .to_vec(),
        vertex_points: [(1, 11), (2, 12), (3, 13)]
            .map(|(entity, carrier)| AsmHistoricalCarrierBinding { entity, carrier })
            .to_vec(),
        point_positions: [(11, x), (12, x + 2.0), (13, x + 1.0)]
            .map(|(point, px)| AsmHistoricalPoint {
                point,
                position: cadmpeg_ir::math::Point3::new(
                    px,
                    0.0,
                    if point == 13 { 2.0 } else { 0.0 },
                ),
            })
            .to_vec(),
        ..AsmHistoricalTopology::default()
    };
    for (loop_, base) in [(111, 1110), (121, 1210)] {
        for (offset, edge) in [1000, 1001, 1002].into_iter().enumerate() {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: base + i64::try_from(offset).unwrap(),
                owner_loop: loop_,
                edge,
                next: 0,
                previous: 0,
                radial_next: 0,
            });
        }
    }
    topology.edge_samples = [
        (1000, Point3::new(x, 0., 0.), Point3::new(x + 2., 0., 0.)),
        (
            1001,
            Point3::new(x + 2., 0., 0.),
            Point3::new(x + 1., 0., 2.),
        ),
        (1002, Point3::new(x + 1., 0., 2.), Point3::new(x, 0., 0.)),
    ]
    .map(
        |(edge, start, end)| crate::history_records::AsmHistoricalEdgeSample {
            edge,
            interior: Point3::new((start.x + end.x) * 0.5, 0., (start.z + end.z) * 0.5),
            tangent: None,
            bounds: Some([
                Point3::new(start.x.min(end.x), 0., start.z.min(end.z)),
                Point3::new(start.x.max(end.x), 0., start.z.max(end.z)),
            ]),
        },
    )
    .to_vec();
    topology
}

fn pillow_history(id: &str, x: f64) -> crate::history_records::AsmHistory {
    let mut states = vec![
        state(3, 3, Some(2), pillow(x)),
        state(2, 2, Some(0), pillow(x)),
        state(0, 0, None, AsmHistoricalTopology::default()),
    ];
    for (state, _, _) in &mut states {
        state.parent = id.into();
        state.id = format!("{id}:state#{}", state.state_id);
    }
    crate::test_support::history::history(id.into(), 0, None, false, states)
}

#[test]
fn an_unbound_feature_reads_the_history_of_the_blob_that_holds_its_outputs() {
    let first_scope = scope_between(3, 2);
    let mut second_scope = first_scope.clone();
    second_scope.id = "f3d:Other/BulkStream.dat:design-parameter-scope#3".into();
    let blob = |guid: &str| format!("{guid}.smbh");
    let feature = |scope: &crate::records::feature::scope::DesignParameterScope, guid: &str| {
        let mut feature = base_feature();
        feature.id = crate::ids::neutral_feature_id(scope);
        feature.native_ref = Some(scope.id.clone());
        feature.evaluation.set_outputs(
            cadmpeg_ir::features::DistinctMembers::try_from(vec![cadmpeg_ir::ids::BodyId::mint(
                format!("f3d:brep/{}/brep:entity#1", blob(guid)),
            )
            .unwrap()])
            .unwrap(),
        );
        feature
    };
    let mut features = [
        feature(&first_scope, "aaaa"),
        feature(&second_scope, "bbbb"),
    ];
    let history_id = |guid: &str| {
        format!(
            "f3d:Breps.BlobParts/BREP.{}:asm-history#0000000001",
            blob(guid)
        )
    };
    // The second blob's history comes first, so a reading by state id alone
    // would give both features the same centre.
    let mut histories = [
        pillow_history(&history_id("bbbb"), 10.0),
        pillow_history(&history_id("aaaa"), 0.0),
    ];
    let losses = bind_feature_outputs(
        &mut features,
        &[first_scope, second_scope],
        &mut histories,
        &[],
        &std::collections::HashMap::new(),
        None,
    )
    .unwrap();
    // Each feature's history is inferred from its output blob, so each one
    // reports the inference.
    assert_eq!(
        losses
            .iter()
            .filter(|note| {
                note.code == crate::loss::F3dLossCode::HistoricalOutputBlobHistoryInferred.kind()
            })
            .count(),
        2
    );
    for (feature, expected) in features.iter().zip(["100:1,0,1", "100:11,0,1"]) {
        assert!(!feature
            .source_properties
            .contains_key(&cadmpeg_core::nonblank_literal!("body_centers_incomplete")));
        assert_eq!(
            feature
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some(expected)
        );
    }
}

fn spline_pillow(cached: bool) -> AsmHistoricalTopology {
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let mut topology = pillow(0.0);
    let mut brep = AsmBrep::default();
    for (edge, poles) in [
        (
            1000,
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(1., 8., 0.),
                Point3::new(2., 0., 0.),
            ],
        ),
        (1001, vec![Point3::new(2., 0., 0.), Point3::new(1., 0., 2.)]),
        (1002, vec![Point3::new(1., 0., 2.), Point3::new(0., 0., 0.)]),
    ] {
        let degree = if edge == 1000 { 2 } else { 1 };
        let knots = if degree == 2 {
            vec![0., 0., 0., 1., 1., 1.]
        } else {
            vec![0., 0., 1., 1.]
        };
        let geometry = SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(degree, knots, poles, None, false).unwrap(),
        );
        let geometry = if cached {
            CurveGeometry::Procedural {
                construction: ProceduralCurveId::mint(format!("f3d:test:construction#{edge}"))
                    .unwrap(),
                cache: Some(geometry),
            }
        } else {
            CurveGeometry::Solved(geometry)
        };
        let id = CurveId::mint(format!("f3d:test:curve#{edge}")).unwrap();
        brep.curves.push(Curve {
            id: id.clone(),
            geometry,
            parameter_range: None,
            source_object: None,
        });
        brep.edges.push(Edge {
            id: EdgeId::mint(format!("f3d:test:edge#{edge}")).unwrap(),
            carrier: EdgeCarrier::new(Some(id), Some([0., 1.])).unwrap(),
            start: VertexId::mint("f3d:test:vertex#1").unwrap(),
            end: VertexId::mint("f3d:test:vertex#2").unwrap(),
            tolerance: None,
        });
    }
    topology.edge_samples = crate::history::edge_samples::historical_edge_samples(&brep).unwrap();
    topology
}

#[test]
fn spline_side_edge_extends_the_body_box_beyond_its_vertices() {
    use cadmpeg_ir::math::Point3;
    for cached in [false, true] {
        let topology = spline_pillow(cached);
        let index = crate::history::HistoricalBodyIndex::new(&topology);
        let evidence = index.body_bounds(100, &[1000, 1001, 1002]);
        let bounds = evidence.bounds.unwrap();
        assert!(evidence.missing_evidence.is_empty());
        // The quadratic Bezier has y(t) = 16t(1-t), whose maximum is 4.
        assert_eq!(bounds, (Point3::new(0., 0., 0.), Point3::new(2., 4., 2.)));
        assert!(!evidence.exact);
    }
}

#[test]
fn an_incomplete_sample_box_keeps_its_midpoint_in_the_partial_center() {
    let mut topology = spline_pillow(true);
    topology
        .edge_samples
        .iter_mut()
        .find(|sample| sample.edge == 1000)
        .unwrap()
        .bounds = None;
    assert_incomplete_center(topology, Some("100:1,2,1"), "edge 1000");
}

#[test]
fn an_unsampleable_edge_keeps_the_partial_body_center() {
    let mut topology = spline_pillow(true);
    topology.edge_samples.retain(|sample| sample.edge != 1000);
    assert_incomplete_center(topology, Some("100:1,0,1"), "edge 1000");
}

#[test]
fn a_missing_vertex_keeps_the_partial_body_center() {
    let mut topology = pillow(0.0);
    topology.point_positions.retain(|point| point.point != 11);
    assert_incomplete_center(topology, Some("100:1,0,1"), "vertex 1");
}

#[test]
fn a_body_without_edge_evidence_has_an_incomplete_center_entry() {
    let mut topology = pillow(0.0);
    topology.point_positions.clear();
    topology.edge_samples.clear();
    assert_incomplete_center(topology, None, "edge 1000");
}

fn assert_incomplete_center(topology: AsmHistoricalTopology, expected: Option<&str>, detail: &str) {
    let scope = scope_between(3, 2);
    let mut feature = base_feature();
    feature.id = crate::ids::neutral_feature_id(&scope);
    feature.native_ref = Some(scope.id.clone());
    let history = crate::test_support::history::history(
        "f3d:asm-history#incomplete-edge".into(),
        0,
        None,
        false,
        vec![
            state(3, 3, Some(2), topology.clone()),
            state(2, 2, None, topology),
        ],
    );
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        &[],
        &std::collections::HashMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("body_centers"))
            .map(String::as_str),
        expected
    );
    assert_eq!(
        feature
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("body_centers_incomplete"))
            .map(String::as_str),
        Some("100")
    );
    let code = if expected.is_some() {
        crate::loss::F3dLossCode::HistoricalBodyCenterPartial
    } else {
        crate::loss::F3dLossCode::HistoricalBodyBoundsUnresolved
    };
    assert!(losses
        .iter()
        .any(|note| note.code == code.kind()
            && (expected.is_none() || note.message.contains(detail))));
}
