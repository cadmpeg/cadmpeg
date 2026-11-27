// SPDX-License-Identifier: Apache-2.0
//! States that share a base: each resolves to its base with its own delta
//! applied (§5.4), so a state reads none of the changes of a state beside it.

use crate::design::evaluation::StateBase;
use crate::design::names::NameEntry;
use crate::design::refs::{EntityRef, RecordRef};
use crate::math::Point3;
use crate::validate::design::fixture::{design_findings, id};

use super::cost::history;

#[test]
fn a_state_reads_none_of_the_changes_of_a_state_on_the_same_base() {
    let mut ir = history(4, 4);
    for step in [0, 1] {
        ir.design.evaluations[step].content.as_mut().unwrap().base =
            StateBase::Evaluation(id("evaluation", "step-2"));
    }
    let corner = ir
        .geometry
        .points
        .iter()
        .find(|point| {
            point.id.as_str().contains(":cube-2:")
                && point.position().get() == Point3::new(0.0, 0.0, 0.0)
        })
        .unwrap()
        .id
        .clone();
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta.removed.push(RecordRef::Point(corner));
    delta.names.insert(
        EntityRef::Face(id("face", "missing")),
        Some(NameEntry {
            name: None,
            native: None,
        }),
    );
    let findings = design_findings(&ir);
    let of = |step: &str| -> Vec<&str> {
        let evaluation = format!("test:design:evaluation#{step}");
        findings
            .iter()
            .filter(|finding| finding.entity.as_deref() == Some(evaluation.as_str()))
            .map(|finding| finding.message.as_str())
            .collect()
    };
    let first = of("step-0");
    for fragment in ["which the state does not hold", "name entry names"] {
        assert!(
            first.iter().any(|message| message.contains(fragment)),
            "{fragment}: {first:#?}"
        );
    }
    assert_eq!(of("step-1"), Vec::<&str>::new());
}
