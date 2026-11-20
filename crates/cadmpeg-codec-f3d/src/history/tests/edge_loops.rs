// SPDX-License-Identifier: Apache-2.0
//! Edge-to-loop index of `HistoricalBodyIndex`.

use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalTopology};

fn coedge(coedge: i64, owner_loop: i64, edge: i64) -> AsmHistoricalCoedge {
    AsmHistoricalCoedge {
        coedge,
        owner_loop,
        edge,
        next: coedge,
        previous: coedge,
        radial_next: coedge,
    }
}

#[test]
fn edge_loops_match_a_scan_of_the_coedge_rows() {
    let topology = AsmHistoricalTopology {
        coedge_topology: vec![
            coedge(33, 22, 41),
            coedge(30, 20, 40),
            coedge(34, 23, 42),
            coedge(31, 21, 40),
            coedge(32, 20, 41),
            coedge(35, 20, 40),
        ],
        ..AsmHistoricalTopology::default()
    };
    let index = crate::history::HistoricalBodyIndex::new(&topology);
    for edge in [40, 41, 42, 43] {
        let scanned = topology
            .coedge_topology
            .iter()
            .filter(|row| row.edge == edge)
            .map(|row| row.owner_loop)
            .collect::<Vec<_>>();
        let indexed = index.edge_loops.get(&edge).cloned().unwrap_or_default();
        assert_eq!(indexed, scanned, "edge {edge}");
    }
}
