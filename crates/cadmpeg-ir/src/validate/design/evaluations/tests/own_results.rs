// SPDX-License-Identifier: Apache-2.0
//! The own result of a member of an unordered container (§5.3, part 10
//! "Containers, positions, evaluations, current geometry").

use crate::design::configuration::FeatureStateOverride;
use crate::design::container::{Container, UnorderedKind};
use crate::design::evaluation::StateBase;
use crate::design::feature::{FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::refs::{Evaluated, Member, Position};
use crate::document::CadIr;
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, design_findings, document, feature, id,
};

/// The fixture with its timeline an unordered group of the extrude and the
/// fillet, each of which has a result of its own.
fn grouped() -> CadIr {
    let mut ir = document();
    ir.design.containers[0] = Container::Unordered {
        id: id("container", "timeline"),
        kind: UnorderedKind::Group,
        members: [
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ]
        .into_iter()
        .collect(),
    };
    ir
}

fn places(ir: &mut CadIr, index: usize, family: OperationFamily) {
    ir.design.features[index].definition.operation = FeatureOperation::Unresolved { family };
}

#[track_caller]
fn assert_no_message(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings
            .iter()
            .all(|finding| !finding.message.contains(fragment)),
        "{findings:#?}"
    );
}

#[test]
fn group_members_with_results_of_their_own_name_states() {
    assert_no_design_finding(&grouped());
}

#[test]
fn a_current_state_after_a_placing_member_is_an_error() {
    let mut ir = grouped();
    places(&mut ir, 1, OperationFamily::Place);
    assert_finding(&ir, "has no result of its own in the active configuration");
}

#[test]
fn a_state_base_after_a_member_with_no_result_in_any_configuration_is_an_error() {
    let mut ir = grouped();
    places(&mut ir, 1, OperationFamily::Place);
    assert_finding(&ir, "with no result of its own in any configuration");

    let mut ir = grouped();
    places(&mut ir, 1, OperationFamily::Place);
    ir.design.configurations[1].overrides.features.insert(
        feature("fillet"),
        FeatureStateOverride {
            suppressed: None,
            definition: Some(FeatureDefinition {
                operation: FeatureOperation::Unresolved {
                    family: OperationFamily::Fillet,
                },
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            }),
        },
    );
    assert_no_message(&ir, "in any configuration");
}

#[test]
fn an_evaluation_after_a_member_with_no_result_in_its_configuration_is_an_error() {
    let mut ir = grouped();
    places(&mut ir, 0, OperationFamily::Joint);
    assert_finding(
        &ir,
        "has no result of its own in the evaluation's configuration",
    );
}

#[test]
fn a_native_member_states_nothing_that_decides_its_result() {
    let mut ir = grouped();
    ir.design.features[1].definition.operation = FeatureOperation::Native {
        kind: crate::design::names::CodecName::try_from("test:op".to_owned()).unwrap(),
        native_options: std::collections::BTreeMap::default(),
        source_settings: std::collections::BTreeMap::default(),
    };
    assert_no_message(&ir, "no result of its own");
}

#[test]
fn after_an_unordered_container_names_no_state() {
    let mut ir = grouped();
    ir.design.evaluations[1].of = Evaluated::State(Position::After(Member::Container(id(
        "container",
        "timeline",
    ))));
    assert_finding(&ir, "names unordered container");

    let mut ir = document();
    ir.design.containers.push(Container::Unordered {
        id: id("container", "group"),
        kind: UnorderedKind::Group,
        members: std::collections::BTreeSet::new(),
    });
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.push(Member::Container(id("container", "group")));
    ir.design.evaluations[0].content = Some(crate::validate::design::fixture::stored(
        StateBase::Current(Position::After(Member::Container(id("container", "group")))),
    ));
    assert_finding(&ir, "names unordered container");
}
