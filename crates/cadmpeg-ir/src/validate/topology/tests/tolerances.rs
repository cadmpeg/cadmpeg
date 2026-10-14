// SPDX-License-Identifier: Apache-2.0

use crate::report::{check::Check, Severity};

#[test]
fn tolerance_validation_preserves_warning_owners_and_arena_order() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.tolerances = crate::units::Tolerances::new(2.0e6, 1.0).unwrap();
    let tolerance = crate::scalar::PositiveReal::new(2.0e6);
    ir.model.vertices[0].tolerance = tolerance;
    ir.model.edges[0].tolerance = tolerance;
    ir.model.faces[0].tolerance = tolerance;
    let mut findings = Vec::new();
    super::super::check_tolerances(&ir, &mut findings);
    super::super::check_topology_tolerances(&ir, &mut findings);
    assert_eq!(findings.len(), 4);
    for finding in &findings {
        assert_eq!(finding.check, Check::Tolerances);
        assert_eq!(finding.severity, Severity::Warning);
    }
    assert_eq!(findings[0].entity, None);
    assert_eq!(
        findings[0].message,
        "document tolerance is outside a sane canonical range"
    );
    for (finding, owner) in findings[1..].iter().zip([
        ir.model.vertices[0].id.as_str(),
        ir.model.edges[0].id.as_str(),
        ir.model.faces[0].id.as_str(),
    ]) {
        assert_eq!(finding.entity.as_deref(), Some(owner));
        assert_eq!(
            finding.message,
            "topology tolerance is outside a sane canonical range"
        );
    }
}
