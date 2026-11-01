// SPDX-License-Identifier: Apache-2.0
//! Occurrences and product definitions keep their identities through a
//! write (spec §18).

use std::collections::BTreeSet;

use crate::test_support::{decode, encode};

#[test]
fn occurrences_and_definitions_keep_their_identities_through_a_write() {
    let text = std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/assembly_instances.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let first = decode(&text);
    let (written, _) = encode(first.ir());
    let second = decode(&written);
    let ids = |model: &cadmpeg_ir::document::Model| {
        let occurrences: BTreeSet<(String, String)> = model
            .occurrences
            .iter()
            .map(|occurrence| {
                let definition = match &occurrence.prototype {
                    cadmpeg_ir::products::PrototypeReference::Local { definition } => {
                        definition.to_string()
                    }
                    _ => String::new(),
                };
                (occurrence.id.to_string(), definition)
            })
            .collect();
        let definitions: BTreeSet<String> = model
            .product_definitions
            .iter()
            .map(|definition| definition.id.to_string())
            .collect();
        (occurrences, definitions)
    };
    assert_eq!(ids(&first.ir().model), ids(&second.ir().model), "{written}");
}
