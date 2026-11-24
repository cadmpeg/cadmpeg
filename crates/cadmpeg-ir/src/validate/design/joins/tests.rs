// SPDX-License-Identifier: Apache-2.0
//! An `Apply` with `Join` needs a body to join in its stored input state.

use std::collections::BTreeMap;

use crate::design::evaluation::StateBase;
use crate::design::feature::FeatureOperation;
use crate::design::ops::swept::Extrude;
use crate::design::options::{BodyForm, Boolean, BooleanOp, SolidOutput};
use crate::design::selection::{Anchor, Picks, Selection};
use crate::document::CadIr;
use crate::validate::design::fixture::{assert_finding, design_findings, document, set_base};

/// The fixture with its fillet made a solid extrude that states `boolean`.
/// The stored state entering it is the evaluation after the first extrude.
fn extruding(boolean: Boolean) -> CadIr {
    let mut ir = document();
    ir.design.features[1].definition.operation = FeatureOperation::Extrude(Box::new(Extrude {
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
            body: Some(BodyForm::Solid),
            boolean: Some(boolean),
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

const JOIN: Boolean = Boolean::Apply {
    op: BooleanOp::Join,
    targets: None,
};

fn join_findings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.starts_with("an Apply with Join"))
        .collect()
}

#[test]
fn a_join_into_a_stored_state_with_no_solid_body_is_an_error() {
    let mut ir = extruding(JOIN);
    set_base(&mut ir, 0, StateBase::Empty);
    assert_finding(
        &ir,
        "an Apply with Join builds a solid whose stored input state holds no solid body; a first solid feature is NewBody",
    );
}

#[test]
fn a_join_into_a_solid_and_a_first_new_body_break_no_rule() {
    assert_eq!(join_findings(&extruding(JOIN)), Vec::<String>::new());
    let mut ir = extruding(Boolean::NewBody);
    set_base(&mut ir, 0, StateBase::Empty);
    assert_eq!(join_findings(&ir), Vec::<String>::new());
}
