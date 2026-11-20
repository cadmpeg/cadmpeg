// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of a Join Combine with one, two and three tool bodies.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The Combine operands, decode loss codes and neutral bodies of a corpus file.
fn combine_operands(bytes: &[u8]) -> (serde_json::Value, Vec<String>, serde_json::Value) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    let ir = serde_json::to_value(result.ir()).expect("serialize the IR");
    let mut combines = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == "Combine 1");
    let combine = combines.next().expect("Combine 1").clone();
    assert!(combines.next().is_none(), "one Combine");
    let bodies = ir["geometry"]["bodies"].clone();
    (
        combine["definition"]["value"]["value"]["operands"].clone(),
        codes,
        bodies,
    )
}

#[test]
fn a_join_combine_selects_its_target_and_tools_in_the_state_before_it() {
    // Fusion joins one target box with N tool boxes; the result is one body.
    let files: [(&[u8], usize); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/combine_tools_n1.f3d"),
            1,
        ),
        (
            include_bytes!("../../../../corpus/f3d/combine_tools_n2.f3d"),
            2,
        ),
        (
            include_bytes!("../../../../corpus/f3d/combine_tools_n3.f3d"),
            3,
        ),
    ];
    for (bytes, tools) in files {
        let (operands, codes, bodies) = combine_operands(bytes);
        assert_eq!(bodies.as_array().map(Vec::len), Some(1), "{tools} tools");
        let target = &operands["target"];
        assert_eq!(target["kind"], "historical", "{tools} tools: {target}");
        assert_eq!(target["value"]["bodies"].as_array().map(Vec::len), Some(1));
        let selected = &operands["tools"];
        assert_eq!(
            selected["kind"], "historical_set",
            "{tools} tools: {selected}"
        );
        assert_eq!(
            selected["value"]["members"].as_array().map(Vec::len),
            Some(tools),
            "{tools} tools: {selected}"
        );
        assert_eq!(selected["value"]["state"], target["value"]["state"]);
        assert!(
            !codes
                .iter()
                .any(|code| code == "feature.combine-selection-native"),
            "{tools} tools: {codes:?}"
        );
    }
}
