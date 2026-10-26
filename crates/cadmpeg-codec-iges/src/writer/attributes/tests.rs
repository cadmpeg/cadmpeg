// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};

#[test]
fn generic_attribute_uses_current_name_values_and_exact_owner_identity() {
    let mut ir = CadIr::empty();
    let point: cadmpeg_ir::ids::PointId = "test:model:point#owner"
        .try_into()
        .expect("valid authored test identity");
    let attribute: cadmpeg_ir::ids::AttributeId = "test:model:attribute#material"
        .try_into()
        .expect("valid authored test identity");
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: attribute.clone(),
        target: AttributeTarget::Point(point.clone()),
        name: "finish".into(),
        values: vec![
            Some(AttributeValue::String("polished".into())),
            Some(AttributeValue::Boolean(true)),
        ],
    });
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.status = EntityStatus::Independent;
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.preserved.contains(attribute.as_str()));
    assert_eq!(entities[0].property_links, [1]);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body)
            .expect("generic attribute parameter body is UTF-8"),
        "6,6Hfinish,2,3,8Hpolished,6,1;"
    );
    super::super::resolve_entity_references(&mut entities)
        .expect("current attribute owner references resolve");
    assert_eq!(
        std::str::from_utf8(&entities[0].parameter_body).expect("point parameter body is UTF-8"),
        "0,0,0,0,1,3;"
    );
}

#[test]
fn missing_owner_omits_only_the_attribute_with_a_named_loss() {
    let mut ir = CadIr::empty();
    let id: cadmpeg_ir::ids::AttributeId = "test:model:attribute#orphan"
        .try_into()
        .expect("valid authored test identity");
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: id.clone(),
        target: AttributeTarget::Point(
            "test:model:point#missing"
                .try_into()
                .expect("valid authored test identity"),
        ),
        name: "finish".into(),
        values: vec![Some(AttributeValue::String("polished".into()))],
    });
    let mut entities = vec![entity(116, 0, "0,0,0;".into())];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(entities.len(), 1);
    assert!(exported.preserved.is_empty());
    assert_eq!(exported.losses.len(), 1);
    assert!(exported.losses[0].message.contains(id.as_str()));
}

#[test]
fn short_current_carrier_name_is_consumed_by_directory_label_only() {
    let mut ir = CadIr::empty();
    let point = "test:model:point#owner"
        .try_into()
        .expect("valid authored test identity");
    let id: cadmpeg_ir::ids::AttributeId = "test:model:attribute#name"
        .try_into()
        .expect("valid authored test identity");
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: id.clone(),
        target: AttributeTarget::Point(point),
        name: "name".into(),
        values: vec![Some(AttributeValue::String("CURRENT".into()))],
    });
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push("test:model:point#owner".into());
    let mut entities = vec![owner];
    let result = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(result.losses.is_empty());
    assert!(result.preserved.contains(id.as_str()));
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].label, "CURRENT");
}

#[test]
fn long_current_carrier_name_uses_one_property_and_a_blank_label() {
    let mut ir = CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: "test:model:attribute#name"
            .try_into()
            .expect("valid authored test identity"),
        target: AttributeTarget::Point(
            "test:model:point#owner"
                .try_into()
                .expect("valid authored test identity"),
        ),
        name: "name".into(),
        values: vec![Some(AttributeValue::String("longer label".into()))],
    });
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push("test:model:point#owner".into());
    let mut entities = vec![owner];
    let result = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(result.losses.is_empty());
    assert_eq!(entities.len(), 2);
    assert!(entities[0].label.is_empty());
    assert_eq!(entities[1].form, 27);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body)
            .expect("long-name property parameter body is UTF-8"),
        "4,4Hname,1,3,12Hlonger label;"
    );
}

#[test]
fn marker_looking_text_in_hollerith_is_never_interpreted_as_a_reference() {
    let mut property = entity(406, 27, "4,4Hname,1,3,4H@R0@;".into());
    property
        .source_targets
        .push("test:model:attribute#literal".into());
    let mut entities = vec![property];
    super::super::resolve_entity_references(&mut entities)
        .expect("literal Hollerith text does not create unresolved references");
    assert_eq!(entities[0].parameter_body, b"4,4Hname,1,3,4H@R0@;");
}

mod linkage;

#[test]
fn unreadable_ordered_slots_drop_only_their_source_positions() {
    let mut ir = CadIr::empty();
    let point = cadmpeg_ir::ids::PointId::mint("test:model:point#holes").unwrap();
    let id = cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#holes").unwrap();
    ir.model.attributes.push(SourceAttribute {
        id: id.clone(),
        native_ref: None,
        target: AttributeTarget::Point(point.clone()),
        name: "holes".into(),
        values: vec![
            Some(AttributeValue::Integer(7)),
            None,
            Some(AttributeValue::String("A".into())),
            None,
            Some(AttributeValue::Boolean(true)),
        ],
    });
    let before = ir.clone();
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let output = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(ir, before);
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].parameter_body, b"8,5Hholes,3,1,7,3,1HA,6,1;");
    assert!(output.preserved.contains(id.as_str()));
    assert_eq!(output.losses.len(), 1);
    assert_eq!(
        output.losses[0].code,
        IgesLossCode::WriterAttributeOmitted.note("").code
    );
    assert!(output.losses[0].message.contains(id.as_str()));
    assert!(output.losses[0].message.contains("None positions [1, 3]"));

    ir.model.attributes[0].values = vec![None];
    let mut owner = entity(116, 0, "0,0,0;".into());
    owner.source_targets.push(point.to_string());
    let mut entities = vec![owner];
    let output = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(
        entities.len(),
        1,
        "an unreadable slot is not an authored no-value scalar"
    );
    assert!(!output.preserved.contains(id.as_str()));
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())
            && loss.message.contains("None positions [0]")));
}

#[test]
fn structured_and_empty_link_values_are_not_coerced_into_generic_scalars() {
    for value in [
        AttributeValue::EmptyReference,
        AttributeValue::Json(cadmpeg_ir::attributes::AttributeJson::Null),
        AttributeValue::Map(vec![cadmpeg_ir::attributes::AttributeMapEntry {
            key: "a".into(),
            value: "1".into(),
        }]),
    ] {
        let mut ir = CadIr::empty();
        let point = cadmpeg_ir::ids::PointId::mint("test:model:point#structured").unwrap();
        let id = cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#structured").unwrap();
        ir.model.attributes.push(SourceAttribute {
            id: id.clone(),
            native_ref: None,
            target: AttributeTarget::Point(point.clone()),
            name: "structured".into(),
            values: vec![Some(value)],
        });
        let before = ir.clone();
        let mut owner = entity(116, 0, "0,0,0;".into());
        owner.source_targets.push(point.to_string());
        let mut entities = vec![owner];
        let output = append(&ir, &mut entities, crate::IgesVersion::V5_3);
        assert_eq!(ir, before);
        assert_eq!(entities.len(), 1);
        assert!(entities[0].property_links.is_empty());
        assert!(!output.preserved.contains(id.as_str()));
        assert_eq!(output.losses.len(), 1);
        assert_eq!(
            output.losses[0].code,
            IgesLossCode::WriterAttributeOmitted.note("").code
        );
        assert!(output.losses[0].message.contains(id.as_str()));
    }
}
