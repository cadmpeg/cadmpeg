// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::validate::validate_neutral;

#[test]
fn directed_subd_sum_fixture_round_trips_and_validates() {
    let ir = directed_subd_sum().unwrap();
    let report = crate::validate::validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let json = ir.to_canonical_json().expect("serialize fixture");
    assert_eq!(CadIr::from_json(&json).expect("parse fixture"), ir);
}

#[cfg(feature = "schema")]
#[test]
fn directed_subd_sum_fixture_matches_schema_shape() {
    let schema = serde_json::to_value(crate::cadir_json_schema()).expect("serialize schema");
    let schema_text = schema.to_string();
    assert!(schema_text.contains("procedural_surfaces"));
    assert!(schema_text.contains("sharpness"));
    assert!(schema_text.contains("\"sum\""));
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube();
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_eq!(ir.model.edges.len(), 12);
    assert_eq!(ir.model.vertices.len(), 8);
    assert_eq!(ir.model.points.len(), 8);
    assert_eq!(ir.model.surfaces.len(), 6);
    assert_eq!(ir.model.curves.len(), 12);
}

#[test]
fn unit_cube_validates_clean() {
    let ir = unit_cube();
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "cube should have no error findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.entity_counts.get("coedges"), Some(&24));
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube();
    for edge in &ir.model.edges {
        let coedges: Vec<_> = ir
            .model
            .coedges
            .iter()
            .filter(|c| c.edge == edge.id)
            .collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].radial_next, coedges[1].id);
        assert_eq!(coedges[1].radial_next, coedges[0].id);
    }
}
