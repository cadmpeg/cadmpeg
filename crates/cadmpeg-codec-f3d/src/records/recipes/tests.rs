// SPDX-License-Identifier: Apache-2.0

use super::{ConstructionRecipe, ConstructionRecipeWire, CONSTRUCTION_RECIPE_CLONE_COUNT};

fn recipe(with_design: bool) -> ConstructionRecipe {
    let suffix = if with_design {
        r#","design_id":"301","design_selector":{"value":2}"#
    } else {
        ""
    };
    serde_json::from_str(&format!(
        r#"{{"id":"f3d:native:recipe#0","kind":"body"{suffix},"recipe_index":0,"record_index":7}}"#
    ))
    .unwrap()
}

#[test]
fn construction_recipe_borrowed_wire_matches_owned_wire_bytes() {
    for record in [recipe(false), recipe(true)] {
        let owned = ConstructionRecipeWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_recipe_native_retained_limit_refuses_before_record_clone() {
    let record = recipe(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "construction_recipes",
        || CONSTRUCTION_RECIPE_CLONE_COUNT.with(|count| count.set(0)),
        || CONSTRUCTION_RECIPE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn recipe_keeps_semantic_selection_without_source_positions() {
    let record = recipe(true);
    assert_eq!(record.byte_offset, None);
    let design = record.design.as_ref().unwrap();
    assert_eq!(design.id.value, "301");
    assert_eq!(design.id.offset, None);
    assert_eq!(design.selector.unwrap().value, 2);
    assert_eq!(design.selector.unwrap().byte_offset, None);
    assert_eq!(record.record_index.unwrap().value, 7);
    assert_eq!(record.record_index.unwrap().offset, None);
    let mut wire = serde_json::to_value(record).unwrap();
    wire["byte_offset"] = serde_json::json!(80);
    assert!(serde_json::from_value::<ConstructionRecipe>(wire).is_err());
}
