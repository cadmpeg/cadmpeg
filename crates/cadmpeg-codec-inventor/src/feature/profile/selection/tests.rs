// SPDX-License-Identifier: Apache-2.0

use super::*;

fn edge(start: f64, end: f64) -> ProfileEdge {
    ProfileEdge {
        name: None,
        start: Point3::new(start, 0.0, 0.0),
        end: Point3::new(end, 0.0, 0.0),
        middle: None,
    }
}

#[test]
fn complete_open_path_retains_its_endpoints_with_reversed_edge_carriers() {
    let endpoints = path_endpoints(&[edge(1.0, 0.0), edge(1.0, 2.0), edge(3.0, 2.0)])
        .expect("connected open path");
    assert_eq!(
        endpoints,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)]
    );
    assert!(path_endpoints(&[]).is_none());
    assert!(path_endpoints(&[edge(0.0, 1.0), edge(2.0, 3.0)]).is_none());
    assert!(path_endpoints(&[edge(0.0, 1.0), edge(1.0, 0.0)]).is_none());
}
