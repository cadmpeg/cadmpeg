// SPDX-License-Identifier: Apache-2.0
//! `Tool(f)` names a feature that has a tool body in the evaluation's
//! configuration (§5.4).

use std::collections::BTreeMap;

use crate::design::feature::FeatureOperation;
use crate::design::ops::swept::Extrude;
use crate::design::options::{Boolean, BooleanOp, SolidOutput};
use crate::design::refs::Evaluated;
use crate::design::selection::{Anchor, Picks, Selection};
use crate::document::CadIr;
use crate::validate::design::fixture::{assert_finding, design_findings, document, feature};

const NONE_BUILT: &str = "evaluation evaluates the tool body of";

/// The fixture with its second evaluation of the extrude's tool body, the
/// extrude stating `boolean`.
fn tool_of_extrude(boolean: Option<Boolean>) -> CadIr {
    let mut ir = document();
    ir.design.evaluations[1].of = Evaluated::Tool(feature("extrude"));
    ir.design.features[0].definition.operation = FeatureOperation::Extrude(Box::new(Extrude {
        profile: Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        },
        loops: None,
        direction: None,
        origin: None,
        span: None,
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: None,
            boolean,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

fn tool_findings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.starts_with(NONE_BUILT))
        .collect()
}

#[test]
fn a_tool_evaluation_names_a_feature_with_a_tool_body() {
    let ir = tool_of_extrude(Some(Boolean::NewBody));
    assert_finding(
        &ir,
        "evaluation evaluates the tool body of test:design:feature#extrude, whose operation in the evaluation's configuration builds none",
    );
}

#[test]
fn a_tool_evaluation_of_an_applied_or_unstated_boolean_breaks_no_rule() {
    let joined = tool_of_extrude(Some(Boolean::Apply {
        op: BooleanOp::Join,
        targets: None,
    }));
    assert_eq!(tool_findings(&joined), Vec::<String>::new());
    let unstated = tool_of_extrude(None);
    assert_eq!(tool_findings(&unstated), Vec::<String>::new());
    let mut unresolved = document();
    unresolved.design.evaluations[1].of = Evaluated::Tool(feature("extrude"));
    assert_eq!(tool_findings(&unresolved), Vec::<String>::new());
}
