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
    bind_feature_outputs(
        &mut features,
        &[first_scope, second_scope],
        &mut histories,
        &[],
        &std::collections::HashMap::new(),
        None,
    )
    .unwrap();
    for (feature, expected) in features.iter().zip(["100:1,0,1", "100:11,0,1"]) {
        assert_eq!(
            feature
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some(expected)
        );
    }
}
