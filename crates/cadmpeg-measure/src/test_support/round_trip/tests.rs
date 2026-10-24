// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::CadIr;

use super::{measures_agree, measures_decide, RoundTripError, RoundTripFailure};
use crate::test_support::builders::cuboid;

const LABEL: &str = "fixture";

/// The `Display` text and the `Debug` text that `expect` prints.
fn messages(error: &RoundTripError) -> [String; 2] {
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert_eq!(display, debug, "Debug prints the Display text");
    [display, debug]
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
    let (comparison, before, after) =
        measures_agree(LABEL, &cube, &cube).expect("the cube agrees with itself");
    assert_eq!(comparison.pairs().len(), 1);
    assert_eq!(before.bodies().len(), 1);
    assert_eq!(after.bodies().len(), 1);
    assert!(measures_decide(LABEL, &cube).expect("the cube is measured"));
}

#[test]
fn a_changed_body_fails_with_fixed_text_only() {
    let small = cuboid([0.0; 3], [1.0; 3]);
    let large = cuboid([0.0; 3], [2.0; 3]);
    let error = measures_agree(LABEL, &small, &large).expect_err("the cubes disagree");
    assert!(matches!(error.failure(), RoundTripFailure::Disagrees(_)));
    for message in messages(&error) {
        assert!(
            message.starts_with("fixture: the round trip does not keep its measured geometry; ")
        );
        assert_states_no_document_text(&message, &[&small, &large]);
    }
}

#[test]
fn a_document_with_no_body_decides_nothing() {
    let empty = CadIr::empty();
    assert!(!measures_decide(LABEL, &empty).expect("the empty document is measured"));
    let error = measures_agree(LABEL, &empty, &empty).expect_err("no body pair agrees");
    for message in messages(&error) {
        assert!(message.starts_with("fixture: "));
        assert_states_no_document_text(&message, &[&empty]);
    }
}
