// SPDX-License-Identifier: Apache-2.0
//! Reciprocity failures identify the inconsistent relation, not a missing ID.

use crate::examples::unit_cube;
use crate::report::Severity;

#[test]
fn cube_topology_is_reciprocal_and_each_broken_direction_is_an_error() {
    let cube = unit_cube().unwrap();
    let mut findings = Vec::new();
    super::check(&cube.model, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    let mut omitted = cube.clone();
    omitted.model.bodies[0].regions.clear();
    super::check(&omitted.model, &mut findings);
    assert!(findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding.message.contains("region parent pointer")
            && finding.message.contains("child list omits")));
    findings.clear();
    let mut wrong_owner = cube.clone();
    wrong_owner.model.loops[0].face = wrong_owner.model.faces[1].id.clone();
    super::check(&wrong_owner.model, &mut findings);
    assert!(findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding.message.contains("face lists loop")
            && finding.message.contains("parent pointer names")));
    findings.clear();
    let mut duplicate = cube;
    duplicate.model.bodies[0]
        .regions
        .push(duplicate.model.regions[0].id.clone());
    super::check(&duplicate.model, &mut findings);
    assert!(findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding.message.contains("body lists region")
            && finding.message.contains("more than once")));
}
