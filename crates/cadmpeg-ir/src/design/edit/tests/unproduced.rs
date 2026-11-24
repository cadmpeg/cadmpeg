// SPDX-License-Identifier: Apache-2.0
//! Configuration exchange keeps a scope's unproduced bodies.

use super::*;

#[test]
fn switching_configurations_preserves_unproduced_bodies_and_their_records() {
    let mut document = configured();
    document.design.scopes[0].unproduced.insert(id("body", 9));
    document.model.product_definitions[0]
        .bodies
        .push(id("body", 9));
    document
        .geometry
        .regions
        .push(Region::new(id("region", 9), id("body", 9), Vec::new()));
    let native: NameEntry = serde_json::from_value(json!({
        "native": {"codec": "test", "value": {"kind": "text", "value": "unproduced"}}
    }))
    .unwrap();
    let mut names = document.design.names.entries().clone();
    names.insert(EntityRef::Body(id("body", 9)), native.clone());
    document.design.names = names.into();
    document
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();
    assert_eq!(
        document.design.scopes[0].unproduced,
        BTreeSet::from([id("body", 9)])
    );
    assert!(document.geometry.bodies.contains(&body(9)));
    assert!(document
        .geometry
        .regions
        .iter()
        .any(|region| region.id == id("region", 9)));
    assert!(document.model.product_definitions[0]
        .bodies
        .contains(&id("body", 9)));
    assert_eq!(
        document
            .design
            .names
            .entries()
            .get(&EntityRef::Body(id("body", 9))),
        Some(&native)
    );
    assert!(document
        .design
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref())
        .all(|content| content
            .delta
            .records
            .bodies
            .iter()
            .all(|body| body.id != id("body", 9))));
}
