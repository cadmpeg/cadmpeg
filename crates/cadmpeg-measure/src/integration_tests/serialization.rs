// SPDX-License-Identifier: Apache-2.0
//! The serialized report: finite numbers and members in contract order.

use crate::select::Selection;
use crate::test_support::{measure, plate_with_hole};

/// The keys of the top-level object of a JSON text, in text order.
fn keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0_i32;
    let mut chars = text.chars().peekable();
    let mut string = String::new();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
                string.push(c);
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                while chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
                if depth == 1 && chars.peek() == Some(&':') {
                    keys.push(std::mem::take(&mut string));
                }
                string.clear();
            } else {
                string.push(c);
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    keys
}

#[test]
fn a_face_report_serializes_members_in_contract_order() {
    let ir = plate_with_hole(true);
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let text = serde_json::to_string(&report).expect("finite report");
    assert_eq!(
        keys(&text),
        [
            "tolerances",
            "caveats",
            "bodies",
            "faces",
            "edges",
            "document"
        ]
    );
    let face = &report.faces()[0];
    assert_eq!(
        keys(&serde_json::to_string(face).unwrap()),
        [
            "face",
            "name",
            "body",
            "shell",
            "sense",
            "carrier",
            "measures",
            "orientation",
            "caveats",
            "status"
        ]
    );
    assert_eq!(
        keys(&serde_json::to_string(face.measures().area()).unwrap()),
        [
            "status",
            "value",
            "unit",
            "method",
            "numeric_error",
            "model_uncertainty"
        ]
    );
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let face = &value["faces"][0];
    assert_eq!(face["carrier"]["kind"], "plane");
    assert_eq!(face["carrier"]["parameters"]["origin"]["method"], "source");
    assert_eq!(
        value["caveats"][0]["code"],
        "measure.provenance-not-recorded"
    );
}
