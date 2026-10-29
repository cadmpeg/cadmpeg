// SPDX-License-Identifier: Apache-2.0
use crate::attributes::{AttributeTarget, SourceAttribute};
use crate::schema::native_links::visit_native_references;

#[test]
fn typed_accessor_keeps_labels_distinct_and_visits_unlinked_entities() {
    let mut ir = crate::examples::unit_cube().expect("cube");
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#link"
            .try_into()
            .expect("attribute id"),
        native_ref: Some("test:native:record#one".into()),
        target: AttributeTarget::Body(ir.model.bodies[0].id.clone()),
        name: "test:native:record#two".into(),
        values: Vec::new(),
    });
    let mut links = Vec::new();
    visit_native_references(&ir.model, &mut |arena, owner, id, native| {
        links.push((arena, owner, id.to_owned(), native.map(str::to_owned)));
    });
    assert!(links.iter().any(|(arena, owner, id, native)| {
        *arena == "attributes"
            && *owner == std::any::type_name::<SourceAttribute>()
            && id == "test:model:attribute#link"
            && native.as_deref() == Some("test:native:record#one")
    }));
    assert!(links
        .iter()
        .any(|(arena, _, _, native)| *arena == "bodies" && native.is_none()));
    assert!(!links
        .iter()
        .any(|(_, _, _, native)| native.as_deref() == Some("test:native:record#two")));
}

#[test]
fn empty_model_has_no_placeholder_projection() {
    let mut count = 0;
    visit_native_references(&crate::document::Model::default(), &mut |_, _, _, _| {
        count += 1;
    });
    assert_eq!(count, 0);
}

mod geometry;
