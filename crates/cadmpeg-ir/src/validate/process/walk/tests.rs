// SPDX-License-Identifier: Apache-2.0

use super::walk;
use crate::design::dimension::ParamRef;
use crate::validate::process::fixture::document;

/// The deepest nesting of `[` and `{` in JSON text, outside strings.
fn json_depth(text: &str) -> usize {
    let (mut depth, mut deepest, mut in_string, mut escaped) = (0_usize, 0, false, false);
    for byte in text.bytes() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b']' | b'}' => depth -= 1,
            _ => {}
        }
    }
    deepest
}

/// The walk measures the nesting `serde_json` writes, for every record of
/// the fixture.
#[test]
fn the_walk_measures_the_json_nesting_of_a_record() {
    let ir = document();
    let process = &ir.process;
    let check = |found: usize, text: String| assert_eq!(found, json_depth(&text), "{text}");
    for setup in &process.setups {
        check(walk(setup).depth, serde_json::to_string(setup).unwrap());
    }
    for operation in &process.operations {
        check(
            walk(operation).depth,
            serde_json::to_string(operation).unwrap(),
        );
    }
    for program in &process.programs {
        check(walk(program).depth, serde_json::to_string(program).unwrap());
    }
    for state in &process.states {
        check(walk(state).depth, serde_json::to_string(state).unwrap());
    }
}

/// The walk finds every `Choice`, `ParamRef`, workpiece index, and anchor
/// the record holds.
#[test]
fn the_walk_finds_the_values_a_record_holds() {
    let ir = document();
    let mill = &ir.process.operations[2];
    let found = walk(mill);
    let at: Vec<&str> = found
        .choices
        .iter()
        .map(|choice| choice.at.as_str())
        .collect();
    assert_eq!(at, ["/heights/bottom/from"]);
    assert!(matches!(
        found.references.as_slice(),
        [(at, ParamRef::Process(id))] if at == "/heights/bottom/offset/value" && id.as_str() == "test:process:parameter#depth"
    ));
    assert_eq!(
        found.workpieces,
        [("/geometry/0/source/value/0".to_owned(), Some(0))]
    );
    assert_eq!(found.anchors.len(), 1);
}
