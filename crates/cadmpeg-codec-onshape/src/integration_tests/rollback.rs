// SPDX-License-Identifier: Apache-2.0
//! The stored rollback bar positions the design timeline's rollback and
//! current state.
use cadmpeg_core::json;
use cadmpeg_ir::codec::DecodeResult;
use serde_json::Value;

use super::projection::stored_thin;
use crate::loss::OnshapeLossCode;
use crate::reader::id;
use crate::test_support::review::{decode, edit, validate, wire};

fn barred(index: Option<&Value>) -> DecodeResult {
    let bytes = edit(&stored_thin(true, false, false), |path, response| {
        if path.ends_with("features.json") {
            match index {
                Some(index) => response["rollbackIndex"] = index.clone(),
                None => {
                    response.as_object_mut().unwrap().remove("rollbackIndex");
                }
            }
        }
    });
    decode(&bytes)
}

fn member(name: &str) -> Value {
    json!({"kind":"feature","value":id("feature", name)})
}

fn unread(result: &DecodeResult) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|note| OnshapeLossCode::RollbackUnread.matches(note))
        .count()
}

#[test]
fn a_stored_rollback_index_places_the_bar_and_the_current_state() {
    let container = id("container", "Part Studio");
    for (index, rollback, at) in [
        (
            0,
            json!({"kind":"before","value":member("Base sketch")}),
            json!({"kind":"start","value":container}),
        ),
        (
            1,
            json!({"kind":"before","value":member("Plate")}),
            json!({"kind":"after","value":member("Base sketch")}),
        ),
        (
            2,
            json!({"kind":"end"}),
            json!({"kind":"after","value":member("Plate")}),
        ),
    ] {
        let result = barred(Some(&json!(index)));
        let wire = wire(&result);
        assert_eq!(result.ir().design.containers.len(), 1, "index {index}");
        assert_eq!(
            wire["design"]["containers"][0]["value"]["rollback"], rollback,
            "index {index}"
        );
        assert_eq!(
            wire["design"]["scopes"][0]["current"][0]["at"], at,
            "index {index}"
        );
        assert_eq!(unread(&result), 0, "index {index}");
        validate(result);
    }
}

#[test]
fn an_absent_or_invalid_rollback_index_states_the_end_with_a_loss() {
    for index in [None, Some(json!(3)), Some(json!(-1)), Some(json!("1"))] {
        let result = barred(index.as_ref());
        let wire = wire(&result);
        assert_eq!(
            wire["design"]["containers"][0]["value"]["rollback"],
            json!({"kind":"end"}),
            "{index:?}"
        );
        assert_eq!(unread(&result), 1, "{index:?}");
        validate(result);
    }
}
