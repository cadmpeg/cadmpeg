// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native::{PropertyFamily, PropertyRecord};
use cadmpeg_ir::attributes::SourceAttribute;

#[test]
fn native_ordered_links_keep_repeated_peers_and_their_emitted_names() {
    for type_name in [
        "App::PropertyLinkList",
        "App::PropertyLinkListChild",
        "App::PropertyLinkListGlobal",
        "App::PropertyLinkListHidden",
    ] {
        let original = PropertyRecord {
            id: "test:native:property#links".into(),
            owner: "test:native:document#0".into(),
            name: "Links".into(),
            type_name: type_name.into(),
            family: PropertyFamily::Link,
            status: Some(8),
            dynamic: Some(crate::native::DynamicPropertyMeta {
                group: "Authored".into(),
                documentation: Some("Link documentation".into()),
                attributes: Some(4),
                read_only: Some(false),
                hidden: Some(true),
            }),
            body: None,
            order: 0,
        };
        let attribute = SourceAttribute {
            id: crate::native::model_id("attribute", &original.id, "property")
                .try_into()
                .unwrap(),
            native_ref: Some(original.id.clone()),
            target: AttributeTarget::Document,
            name: "Links".into(),
            values: vec![
                Some(AttributeValue::Reference("body-b".into())),
                None,
                Some(AttributeValue::Reference("body-a".into())),
                Some(AttributeValue::Reference("body-b".into())),
                None,
            ],
        };
        let id = attribute.id.to_string();
        let mut ir = CadIr::empty();
        ir.model.attributes.push(attribute);
        let names = HashMap::from([
            ("body-a".into(), "Alpha".into()),
            ("body-b".into(), "Beta".into()),
        ]);
        let mut document = BTreeMap::new();
        let mut losses = Vec::new();
        write(
            &ir,
            &names,
            &mut [],
            &mut document,
            &mut losses,
            &[original],
        )
        .unwrap();
        assert_eq!(document["Links"], format!("<Property name=\"Links\" type=\"{type_name}\" status=\"8\" group=\"Authored\" doc=\"Link documentation\" attr=\"4\" ro=\"0\" hide=\"1\"><LinkList count=\"3\"><Link value=\"Beta\"/><Link value=\"Alpha\"/><Link value=\"Beta\"/></LinkList></Property>"));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            FreecadLossCode::ExportAttributeNotTransferred.note("").code
        );
        assert!(losses[0].message.contains(&id));
        assert!(losses[0]
            .message
            .contains("unavailable value positions [1, 4]"));
    }
}

#[test]
fn native_template_join_uses_only_unique_forward_identity() {
    let original = PropertyRecord {
        id: "test:native:property#scalar".into(),
        owner: "test:native:document#0".into(),
        name: "Meta".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: None,
        dynamic: None,
        body: None,
        order: 0,
    };
    let coincident_id = crate::native::model_id("attribute", &original.id, "property");
    let mut attribute = SourceAttribute {
        id: coincident_id.try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "Meta".into(),
        values: vec![Some(AttributeValue::String("current".into())), None],
    };
    let mut ir = CadIr::empty();
    ir.model.attributes.push(attribute.clone());
    let mut document = BTreeMap::new();
    let mut losses = Vec::new();
    write(
        &ir,
        &HashMap::new(),
        &mut [],
        &mut document,
        &mut losses,
        std::slice::from_ref(&original),
    )
    .unwrap();
    assert!(document["Meta"].contains("type=\"App::PropertyStringList\""));
    assert!(document["Meta"]
        .contains("<StringList count=\"1\"><String value=\"current\"/></StringList>"));
    assert_eq!(losses.len(), 1);

    attribute.id = "test:model:attribute#reminted".try_into().unwrap();
    attribute.native_ref = Some(original.id.clone());
    ir.model.attributes[0] = attribute;
    ir.model.attributes[0].native_ref = Some("test:native:property#missing".into());
    document.clear();
    losses.clear();
    write(
        &ir,
        &HashMap::new(),
        &mut [],
        &mut document,
        &mut losses,
        std::slice::from_ref(&original),
    )
    .unwrap();
    assert!(document.is_empty());
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("test:native:property#missing"));
    assert!(losses[0]
        .message
        .contains("unavailable value positions [1]"));
    ir.model.attributes[0].native_ref = Some(original.id.clone());
    document.clear();
    losses.clear();
    write(
        &ir,
        &HashMap::new(),
        &mut [],
        &mut document,
        &mut losses,
        &[original.clone(), original],
    )
    .unwrap();
    assert!(document.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        FreecadLossCode::ExportAttributeNotTransferred.note("").code
    );
    assert!(losses[0].message.contains("test:model:attribute#reminted"));
    assert!(losses[0]
        .message
        .contains("ambiguous native property owner"));
    assert!(losses[0]
        .message
        .contains("unavailable value positions [1]"));
}

#[test]
fn reminted_native_string_list_writes_all_holes_as_zero_entries() {
    let original = PropertyRecord {
        id: "test:native:property#list".into(),
        owner: "test:native:document#0".into(),
        name: "Meta".into(),
        type_name: "App::PropertyStringList".into(),
        family: PropertyFamily::List,
        status: None,
        dynamic: None,
        body: None,
        order: 0,
    };
    let mut ir = CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#reminted-list".try_into().unwrap(),
        native_ref: Some(original.id.clone()),
        target: AttributeTarget::Document,
        name: "Meta".into(),
        values: vec![None, None],
    });
    let mut document = BTreeMap::new();
    let mut losses = Vec::new();
    write(
        &ir,
        &HashMap::new(),
        &mut [],
        &mut document,
        &mut losses,
        &[original],
    )
    .unwrap();
    assert!(document["Meta"].contains("type=\"App::PropertyStringList\""));
    assert!(document["Meta"].contains("<StringList count=\"0\"></StringList>"));
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("test:model:attribute#reminted-list"));
    assert!(losses[0]
        .message
        .contains("unavailable value positions [0, 1]"));
}
