// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of a Join Combine whose tools are the bodies of a
//! three-element rectangular body pattern.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The operands of `Combine 1` and the decode loss codes of a corpus file.
fn combine_operands(bytes: &[u8]) -> (serde_json::Value, Vec<String>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    let ir = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let mut combines = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == "Combine 1");
    let combine = combines.next().expect("Combine 1").clone();
    assert!(combines.next().is_none(), "one Combine");
    (
        combine["definition"]["value"]["value"]["operands"].clone(),
        codes,
    )
}

/// The generating feature of each generated tool body.
fn tool_generators(operands: &serde_json::Value) -> Vec<String> {
    let tools = &operands["tools"];
    assert_eq!(tools["kind"], "generated", "{tools}");
    tools["value"]["bodies"]
        .as_array()
        .expect("generated bodies")
        .iter()
        .map(|body| body["feature"].as_str().expect("generator").to_owned())
        .collect()
}

#[test]
fn pattern_copies_bind_as_bodies_that_the_pattern_generates() {
    let (operands, codes) = combine_operands(include_bytes!(
        "../../../../corpus/f3d/combine_pattern_copies.f3d"
    ));
    assert_eq!(operands["target"]["kind"], "resolved", "{operands}");
    let generators = tool_generators(&operands);
    assert_eq!(generators.len(), 2);
    assert!(
        generators
            .iter()
            .all(|feature| feature.contains(":R-Pattern1:")),
        "{generators:?}"
    );
    assert!(
        !codes
            .iter()
            .any(|code| code == "feature.combine-tool-body-unbound"
                || code == "feature.combine-operand-body-unbound"),
        "{codes:?}"
    );
}

#[test]
fn a_repeated_seed_tool_selects_the_seed_body_once() {
    // Fusion takes the tools Seed, Copy1, Copy2 and Seed; the result equals
    // the Combine of Seed, Copy1 and Copy2.
    let (operands, codes) = combine_operands(include_bytes!(
        "../../../../corpus/f3d/combine_pattern_seed_repeated.f3d"
    ));
    assert_eq!(operands["target"]["kind"], "resolved", "{operands}");
    let generators = tool_generators(&operands);
    assert_eq!(generators.len(), 3, "{generators:?}");
    assert!(generators[0].contains(":Extrude2:"), "{generators:?}");
    assert!(
        generators[1..]
            .iter()
            .all(|feature| feature.contains(":R-Pattern1:")),
        "{generators:?}"
    );
    assert!(
        !codes
            .iter()
            .any(|code| code == "feature.combine-tool-body-unbound"
                || code == "feature.combine-operand-body-unbound"
                || code == "feature.combine-operand-list-refused"),
        "{codes:?}"
    );
}
