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

#[test]
fn a_definition_keeps_its_product_data_and_sketches_through_a_write() {
    let fixture = |name: &str| {
        std::fs::read_to_string(format!(
            "{}/tests/golden/fixtures/{name}.usda",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture")
    };
    let source = decode(&fixture("brep_array_box"));
    let sketches = decode(&fixture("sketch_curved"));
    let mut ir = source.ir().clone();
    let model = &sketches.ir().model;
    ir.model.sketches.clone_from(&model.sketches);
    ir.model.sketch_entities.clone_from(&model.sketch_entities);
    ir.model.sketch_constraints.clone_from(&model.sketch_constraints);
    ir.model.parameters.clone_from(&model.parameters);
    let sketch = ir.model.sketches[0].id.clone();
    let definition = &mut ir.model.product_definitions[0];
    definition.label = Some("Bracket, left".to_owned());
    definition.part_number = Some("BR-1001".to_owned());
    definition.description = Some("Laser-cut bracket".to_owned());
    definition.bom_properties.insert(
        cadmpeg_core::text::NonBlankString::new("Material").expect("a name"),
        "6061-T6".to_owned(),
    );
    definition.sketches = vec![sketch];
    let expected = definition.clone();
    let (written, _) = encode(&ir);
    let read = decode(&written);
    let found = read
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|candidate| candidate.id == expected.id)
        .unwrap_or_else(|| panic!("the definition\n{written}"));
    assert_eq!(found.label, expected.label);
    assert_eq!(found.part_number, expected.part_number);
    assert_eq!(found.description, expected.description);
    assert_eq!(found.bom_properties, expected.bom_properties);
    assert_eq!(found.source_name, expected.source_name);
    assert_eq!(found.sketches, expected.sketches);
}
