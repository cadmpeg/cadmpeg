// SPDX-License-Identifier: Apache-2.0

#[test]
fn topology_findings_keep_message_entity_check_and_order() {
    let mut findings = Vec::new();
    super::super::ref_error(
        &mut findings,
        "test:model:feature#owner",
        format_args!("historical {}", "face"),
        "test:model:face#missing",
    );
    super::super::geometry_error(
        &mut findings,
        "test:model:feature#owner",
        "invalid geometry",
    );
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings[0].check,
        crate::report::check::Check::ReferentialIntegrity
    );
    assert_eq!(
        findings[0].message,
        "references missing historical face `test:model:face#missing`"
    );
    assert_eq!(
        findings[1].check,
        crate::report::check::Check::GeometricConsistency
    );
    assert_eq!(findings[1].message, "invalid geometry");
    for finding in findings {
        assert_eq!(finding.entity.as_deref(), Some("test:model:feature#owner"));
        assert_eq!(finding.severity, crate::report::Severity::Error);
    }
}
