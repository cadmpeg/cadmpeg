// SPDX-License-Identifier: Apache-2.0

use std::panic::{catch_unwind, AssertUnwindSafe};

use cadmpeg_ir::CadIr;

use super::{assert_measures_agree, measures_decide};
use crate::test_support::cuboid;

const LABEL: &str = "fixture";

/// The message of the panic that `check` raises.
fn panic_message(check: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(check)).expect_err("the check panics");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .expect("a text panic payload")
}

/// The message names no body identity and prints no number.
fn assert_states_no_document_text(message: &str, documents: &[&CadIr]) {
    assert!(
        !message.chars().any(|character| character.is_ascii_digit()),
        "the failure message prints a number"
    );
    for document in documents {
        for body in &document.model.bodies {
            assert!(
                !message.contains(body.id.as_str()),
                "the failure message prints a body identity"
            );
        }
    }
}

#[test]
fn a_document_agrees_with_itself_over_one_body_pair() {
    let cube = cuboid([0.0; 3], [1.0; 3]);
    let (comparison, before, after) = assert_measures_agree(LABEL, &cube, &cube);
    assert_eq!(comparison.pairs().len(), 1);
    assert_eq!(before.bodies().len(), 1);
    assert_eq!(after.bodies().len(), 1);
    assert!(measures_decide(LABEL, &cube));
}

#[test]
fn a_changed_body_fails_with_fixed_text_only() {
    let small = cuboid([0.0; 3], [1.0; 3]);
    let large = cuboid([0.0; 3], [2.0; 3]);
    let message = panic_message(|| {
        assert_measures_agree(LABEL, &small, &large);
    });
    assert!(message.starts_with("fixture: the round trip does not keep its measured geometry; "));
    assert_states_no_document_text(&message, &[&small, &large]);
}

#[test]
fn a_document_with_no_body_decides_nothing() {
    let empty = CadIr::empty();
    assert!(!measures_decide(LABEL, &empty));
    let message = panic_message(|| {
        assert_measures_agree(LABEL, &empty, &empty);
    });
    assert!(message.starts_with("fixture: "));
    assert_states_no_document_text(&message, &[&empty]);
}
