// SPDX-License-Identifier: Apache-2.0
//! Current inward links select canonical edits without modifying native facts.

use super::*;
use cadmpeg_ir::ids::{AttributeId, Identity, PointId};
use cadmpeg_ir::native::bytes::NativeBytes;
use serde_json::json;

const PROPERTY: &str = "iges:property:record#source";

fn property(owners: &[String]) -> NativeRecord {
    NativeRecord::new(
        Identity::new(PROPERTY).expect("native identity"),
        serde_json::from_value(json!({
            "source_entity":"iges:entity:directory#3","form":27,"property_kind":"generic_data",
            "name":NativeBytes::from(b"NAME".to_vec()),"declared_value_count":4,"owners":owners,
            "values":[{"data_type":2,"value":{"kind":"real","value":3.25}}]
        }))
        .expect("native fields"),
    )
    .expect("native property")
}

#[test]
fn inward_property_link_preserves_native_values_and_writes_current_name_and_numeric_edits() {
    let mut ir = CadIr::empty();
    let point = PointId::mint("test:model:point#owner").expect("point identity");
    let id = AttributeId::mint("test:model:attribute#renamed").expect("attribute identity");
    ir.model.attributes.push(SourceAttribute {
        id: id.clone(),
        native_ref: Some(PROPERTY.into()),
        target: AttributeTarget::Point(point.clone()),
        name: "Current".into(),
        values: vec![
            Some(AttributeValue::Integer(7)),
            Some(AttributeValue::float(10.5).expect("finite value")),
        ],
    });
    ir.native
        .namespace_mut("iges")
        .arenas_mut()
        .insert("properties".into(), vec![property(&[])]);
    let before = ir.clone();
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert!(exported.regenerated.contains(PROPERTY));
    assert!(exported.preserved.contains(id.as_str()));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0].property_links, [1]);
    assert_eq!(
        entities[1].parameter_body,
        b"6,7HCurrent,2,1,7,2,1.0500000000000000D1;"
    );
    assert_eq!(ir, before, "canonical edits do not erase native values");
    assert_eq!(
        ir.native.namespace("iges").expect("namespace").arenas()["properties"][0]
            .field("neutral_values"),
        None
    );
}

#[test]
fn unlinked_typed_property_writes_under_full_scope_when_its_references_resolve() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("iges").arenas_mut().insert(
        "properties".into(),
        vec![property(&["iges:entity:directory#1".into()])],
    );
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push("iges:entity:directory#1".into());
    let mut entities = vec![owner.clone()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(ir.scope.model);
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert!(exported.written.contains(PROPERTY));
    assert!(!exported.regenerated.contains(PROPERTY));
    assert_eq!(entities.len(), 2);
    assert_eq!(
        entities[1].parameter_body,
        b"4,4HNAME,1,2,3.2500000000000000D0;"
    );
    ir.native.namespace_mut("iges").arenas_mut().insert(
        "properties".into(),
        vec![property(&["iges:entity:directory#missing".into()])],
    );
    let mut entities = vec![owner];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(entities.len(), 1);
    assert!(entities[0].property_links.is_empty());
    assert!(!exported.regenerated.contains(PROPERTY));
    assert!(!exported.written.contains(PROPERTY));
    assert!(exported
        .losses
        .iter()
        .any(|loss| loss.message.contains(PROPERTY)
            && loss.message.contains("current owner has no emitted entity")));
}

#[test]
fn duplicate_current_property_fields_refuse_the_group_instead_of_selecting_one() {
    let mut ir = CadIr::empty();
    let owner =
        AttributeTarget::Point(PointId::mint("test:model:point#owner").expect("point identity"));
    for ordinal in 0..2 {
        ir.model.attributes.push(SourceAttribute {
            id: AttributeId::mint(format!("test:model:attribute#{ordinal}"))
                .expect("attribute identity"),
            native_ref: Some(PROPERTY.into()),
            target: owner.clone(),
            name: "Current".into(),
            values: vec![Some(AttributeValue::Integer(ordinal))],
        });
    }
    let record = property(&[]);
    let attributes = BTreeMap::from([(PROPERTY, ir.model.attributes.iter().collect::<Vec<_>>())]);
    let error = fields::groups(&record, &attributes, fields::Family::Property)
        .err()
        .expect("ambiguous current group");
    assert!(error.contains("multiple values for the same current owner"));
}

fn source_entity(
    entity_type: i64,
    form: i64,
    parameters: &str,
) -> crate::test_support::test_owned::OwnedTestEntity {
    crate::test_support::test_owned::OwnedTestEntity {
        entity_type,
        form,
        label: "ATTR".into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

#[test]
fn structured_numeric_property_uses_linked_current_values_without_overwriting_source_fields() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let bytes = crate::test_support::test_owned::owned_test_file(&[
        source_entity(116, 0, "116,1,2,3,0,0,1,3;"),
        source_entity(406, 5, "406,5,2.5,1,0,2,3.75;"),
    ]);
    let decoded = crate::IgesCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("numeric property source");
    let mut ir = decoded.ir().clone();
    let property = &ir
        .native
        .namespace("iges")
        .expect("native namespace")
        .arenas()["properties"][0];
    let native_id = property.id().to_owned();
    assert_eq!(property.field("width"), Some(json!(2.5)));
    let width = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "iges.property.line_widening.width")
        .expect("projected width");
    assert_eq!(width.native_ref.as_deref(), Some(native_id.as_str()));
    width.values = vec![Some(
        AttributeValue::float(6.5).expect("finite current width"),
    )];
    let before = ir.clone();
    let mut point = entity(116, 0, "1,2,3,0;".into());
    point.source_targets.push(ir.model.points[0].id.to_string());
    let mut entities = vec![point];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert!(exported.regenerated.contains(&native_id));
    assert_eq!(entities.len(), 2);
    assert_eq!(
        entities[1].parameter_body,
        b"5,6.5000000000000000D0,1,0,2,3.7500000000000000D0;"
    );
    assert_eq!(ir, before);
    assert_eq!(
        ir.native
            .namespace("iges")
            .expect("native namespace")
            .arenas()["properties"][0]
            .field("width"),
        Some(json!(2.5))
    );
}

#[test]
fn attribute_table_defaults_and_rows_emit_standalone_without_neutral_owners() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let bytes = crate::test_support::test_owned::owned_test_file_with_structures(
        &[
            source_entity(322, 1, "322,4HATTR,0,1,1,3,1,3HVAL;"),
            source_entity(322, 0, "322,4HMETA,1,1,10,3,1;"),
            source_entity(422, 1, "422,1,4HITEM;"),
        ],
        &[(5, -3)],
    );
    let decoded = crate::IgesCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("attribute table source");
    let mut ir = decoded.ir().clone();
    assert!(ir.model.attributes.is_empty());
    let namespace = ir.native.namespace("iges").expect("native namespace");
    let definition = &namespace.arenas()["attribute_table_definitions"][0];
    let instance = &namespace.arenas()["attribute_table_instances"][0];
    // Table defaults and rows project no neutral entity. Attribute tables
    // emit standalone, so the suite links them at dangling record targets
    // and pins the standalone bytes.
    let dangling = || AttributeTarget::Record {
        arena: cadmpeg_ir::document::ArenaName::parse("points")
            .expect("registered points arena"),
        id: "test:model:point#absent".into(),
    };
    for (name, value, native_ref) in [
        (
            "iges.attribute_table.default.attributes.0.values.0",
            "EDIT",
            definition.id(),
        ),
        (
            "iges.attribute_table.value.rows.0.0",
            "ROW",
            instance.id(),
        ),
    ] {
        ir.model.attributes.push(SourceAttribute {
            id: AttributeId::mint(format!("test:model:attribute#{name}"))
                .expect("attribute identity"),
            native_ref: Some(native_ref.to_owned()),
            target: dangling(),
            name: name.into(),
            values: vec![Some(AttributeValue::String(value.into()))],
        });
    }
    let before = ir.clone();
    let mut entities = Vec::new();
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].parameter_body, b"4HATTR,0,1,1,3,1,4HEDIT;");
    assert_eq!(entities[1].parameter_body, b"4HMETA,1,1,10,3,1;");
    assert_eq!(entities[2].parameter_body, b"1,3HROW;");
    assert!(ir
        .model
        .attributes
        .iter()
        .all(|attribute| exported.preserved.contains(attribute.id.as_str())));
    assert_eq!(ir, before, "table writes do not alter held native tokens");
}

#[test]
fn equal_current_property_is_written_and_a_changed_counterpart_is_regenerated() {
    let mut ir = CadIr::empty();
    let attribute = AttributeId::mint("test:model:attribute#equal").expect("attribute identity");
    ir.model.attributes.push(SourceAttribute {
        id: attribute.clone(),
        native_ref: Some(PROPERTY.into()),
        target: AttributeTarget::Point(
            PointId::mint("test:model:point#owner").expect("point identity"),
        ),
        name: "NAME".into(),
        values: vec![Some(
            AttributeValue::float(3.25).expect("native finite value"),
        )],
    });
    ir.native.namespace_mut("iges").arenas_mut().insert(
        "properties".into(),
        vec![property(&["iges:entity:directory#1".into()])],
    );
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets = vec![
        "test:model:point#owner".into(),
        "iges:entity:directory#1".into(),
    ];
    let mut entities = vec![owner.clone()];
    let equal = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(equal.losses.is_empty());
    assert!(equal.written.contains(PROPERTY));
    assert!(!equal.regenerated.contains(PROPERTY));
    let declaration = super::super::super::synthesized_native_dispositions(
        &ir,
        &equal.losses,
        &equal.regenerated,
        &equal.written,
    );
    assert_eq!(
        declaration.record_disposition("iges", "properties", PROPERTY),
        cadmpeg_ir::codec::write::NativeDisposition::Written
    );
    ir.model.attributes[0].values = vec![Some(
        AttributeValue::float(4.5).expect("edited finite value"),
    )];
    let mut entities = vec![owner.clone()];
    let changed = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(changed.losses.is_empty());
    assert!(!changed.written.contains(PROPERTY));
    assert!(changed.regenerated.contains(PROPERTY));
    let declaration = super::super::super::synthesized_native_dispositions(
        &ir,
        &changed.losses,
        &changed.regenerated,
        &changed.written,
    );
    assert_eq!(
        declaration.record_disposition("iges", "properties", PROPERTY),
        cadmpeg_ir::codec::write::NativeDisposition::Regenerated
    );
    ir.model.attributes.clear();
    let mut entities = vec![owner.clone()];
    let missing = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(missing.losses.is_empty());
    assert!(missing.written.contains(PROPERTY));
    assert_eq!(
        entities[1].parameter_body,
        b"4,4HNAME,1,2,3.2500000000000000D0;"
    );
}

#[test]
fn removing_the_last_counterparts_keeps_one_shared_typed_property_with_all_owners() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let bytes = crate::test_support::test_owned::owned_test_file(&[
        source_entity(116, 0, "116,1,2,3,0,0,1,5;"),
        source_entity(116, 0, "116,4,5,6,0,0,1,5;"),
        source_entity(406, 15, "406,1,4HSAME;"),
    ]);
    let decoded = crate::IgesCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("shared typed property source");
    let mut ir = decoded.ir().clone();
    let native_id = ir.native.namespace("iges").expect("namespace").arenas()["properties"][0]
        .id()
        .to_owned();
    assert_eq!(
        ir.model
            .attributes
            .iter()
            .filter(|attribute| attribute.native_ref.as_deref() == Some(native_id.as_str()))
            .count(),
        2
    );
    ir.model
        .attributes
        .retain(|attribute| attribute.native_ref.as_deref() != Some(native_id.as_str()));
    let mut entities = ir
        .model
        .points
        .iter()
        .map(|point| {
            let mut owner = entity(116, 0, "0,0,0;".into());
            owner.source_targets.push(point.id.to_string());
            owner
        })
        .collect::<Vec<_>>();
    assert_eq!(entities.len(), 2);
    let before = ir.clone();
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert!(exported.written.contains(&native_id));
    assert!(!exported.regenerated.contains(&native_id));
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].property_links, [2]);
    assert_eq!(entities[1].property_links, [2]);
    assert_eq!(entities[2].parameter_body, b"1,4HSAME;");
    assert_eq!(ir, before);
}

#[test]
fn generic_holes_keep_actual_native_no_value_fields_without_restoring_unread_values() {
    let mut ir = CadIr::empty();
    let point =
        PointId::mint("test:model:point#source").expect("the authored point identity is valid");
    let id = AttributeId::mint("test:model:attribute#source")
        .expect("the authored attribute identity is valid");
    ir.model.attributes.push(SourceAttribute {
        id: id.clone(),
        native_ref: Some(PROPERTY.into()),
        target: AttributeTarget::Point(point.clone()),
        name: "Current".into(),
        values: vec![None, Some(AttributeValue::String("B".into()))],
    });
    let record = NativeRecord::new(
        Identity::new(PROPERTY).expect("the native property identity is valid"),
        serde_json::from_value(json!({
            "source_entity":"iges:entity:directory#3", "form":27, "property_kind":"generic_data",
            "name":NativeBytes::from(b"NAME".to_vec()), "declared_value_count":8, "owners":[],
            "values":[
                {"data_type":0,"value":{"kind":"omitted"}},
                {"data_type":1,"value":{"kind":"integer","value":91}},
                {"data_type":3,"value":{"kind":"string","value":NativeBytes::from(b"source".to_vec())}}
            ]
        })).expect("the native property fields deserialize"),
    ).expect("the native property record builds");
    ir.native
        .namespace_mut("iges")
        .arenas_mut()
        .insert("properties".into(), vec![record]);
    let before = ir.clone();
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let output = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(ir, before);
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].parameter_body, b"6,7HCurrent,2,0,,3,1HB;");
    assert!(output.regenerated.contains(PROPERTY));
    assert!(output.preserved.contains(id.as_str()));
    assert_eq!(output.losses.len(), 1);
    assert!(output.losses[0].message.contains(id.as_str()));
    assert!(output.losses[0].message.contains("None positions [0]"));
}

#[test]
fn scalar_hole_is_not_replayed_from_native_or_rehomed_as_generic_data() {
    let mut ir = CadIr::empty();
    let point =
        PointId::mint("test:model:point#scalar").expect("the authored point identity is valid");
    let id = AttributeId::mint("test:model:attribute#scalar")
        .expect("the authored attribute identity is valid");
    ir.model.attributes.push(SourceAttribute {
        id: id.clone(),
        native_ref: Some(PROPERTY.into()),
        target: AttributeTarget::Point(point.clone()),
        name: "Current".into(),
        values: vec![None],
    });
    let record = NativeRecord::new(
        Identity::new(PROPERTY).expect("the native property identity is valid"),
        serde_json::from_value(json!({
            "source_entity":"iges:entity:directory#3", "form":15, "property_kind":"name",
            "value":NativeBytes::from(b"SOURCE".to_vec()), "owners":[]
        }))
        .expect("the native property fields deserialize"),
    )
    .expect("the native property record builds");
    ir.native
        .namespace_mut("iges")
        .arenas_mut()
        .insert("properties".into(), vec![record]);
    let before = ir.clone();
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let output = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(ir, before);
    assert_eq!(entities.len(), 1);
    assert!(entities[0].property_links.is_empty());
    assert!(!output.preserved.contains(id.as_str()));
    assert!(!output.written.contains(PROPERTY));
    assert!(!output.regenerated.contains(PROPERTY));
    assert!(output.losses.iter().any(|loss| loss.code
        == IgesLossCode::WriterAttributeOmitted.note("").code
        && loss.message.contains(id.as_str())
        && loss.message.contains("None positions [0]")));
}
