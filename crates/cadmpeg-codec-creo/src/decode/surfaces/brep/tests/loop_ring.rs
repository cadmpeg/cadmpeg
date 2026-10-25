// SPDX-License-Identifier: Apache-2.0

use super::super::{native_coedge_sense, native_loop_ring};

fn native_loop() -> crate::topology::Loop {
    crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 10,
                side: crate::topology::Side::Zero,
            },
            crate::topology::HalfEdgeId {
                curve_id: 11,
                side: crate::topology::Side::One,
            },
            crate::topology::HalfEdgeId {
                curve_id: 12,
                side: crate::topology::Side::Zero,
            },
        ],
    )
}

#[test]
fn brep_native_loop_ring_reverses_native_traversal() {
    let ring = (native_loop_ring(&native_loop(), 5)).expect("build the native loop ring");
    assert_eq!(
        ring.coedges()
            .iter()
            .map(cadmpeg_ir::ids::CoedgeId::as_str)
            .collect::<Vec<_>>(),
        [
            "creo:visibgeom:coedge#12:0",
            "creo:visibgeom:coedge#11:1",
            "creo:visibgeom:coedge#10:0"
        ]
    );
}

#[test]
fn clockwise_native_triangle_transfers_to_a_continuous_counterclockwise_ring() {
    let native = native_loop();
    let ring = native_loop_ring(&native, 5).expect("triangle ring");
    let edges = [
        [[0.0, 0.0], [0.0, 2.0]],
        [[3.0, 0.0], [0.0, 2.0]],
        [[3.0, 0.0], [0.0, 0.0]],
    ];
    let directed: Vec<_> = ring
        .coedges()
        .iter()
        .map(|id| {
            let half_edge = native
                .half_edges()
                .iter()
                .find(|half_edge| {
                    id.as_str()
                        == format!(
                            "creo:visibgeom:coedge#{}:{}",
                            half_edge.curve_id,
                            half_edge.side.index()
                        )
                })
                .expect("native edge");
            let mut endpoints =
                edges[usize::try_from(half_edge.curve_id - 10).expect("edge index")];
            if native_coedge_sense(half_edge.side) == cadmpeg_ir::topology::Sense::Reversed {
                endpoints.reverse();
            }
            endpoints
        })
        .collect();
    let mut twice_area = 0.0;
    for (edge, next) in directed.iter().zip(directed.iter().cycle().skip(1)) {
        assert_eq!(edge[1], next[0]);
        twice_area += edge[0][0] * edge[1][1] - edge[1][0] * edge[0][1];
    }
    assert_eq!(twice_area, 6.0);
}

#[test]
fn brep_loop_ring_error_refuses_empty_source_ring_before_projection() {
    assert!(crate::topology::Loop::new(std::num::NonZeroU32::new(5), Vec::new(), &[]).is_none());
}
