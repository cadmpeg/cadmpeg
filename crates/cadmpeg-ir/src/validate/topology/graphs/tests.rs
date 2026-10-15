// SPDX-License-Identifier: Apache-2.0

use crate::report::{check::Check, Severity};

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.model.coedges[0].id.clone();
    let other = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge != ir.model.coedges[0].edge)
        .unwrap()
        .id
        .clone();
    ir.model.coedges[0].radial_next = other;
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ir, &mut findings).unwrap();
    assert!(findings.len() >= 2);
    for finding in &findings[..2] {
        assert_eq!(finding.check, Check::CoedgePairing);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(findings[0].message, "radial ring crosses edges");
    assert_eq!(findings[1].message, "radial ring does not close");
}
