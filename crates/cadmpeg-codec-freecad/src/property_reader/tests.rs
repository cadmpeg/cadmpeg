// SPDX-License-Identifier: Apache-2.0
//! Property interpretation source and typed-edit controls.

use super::PropertyReader;
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, ValueRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

fn property() -> PropertyRecord {
    PropertyRecord {
        dynamic: None,
        id: "property".into(),
        owner: "owner".into(),
        name: "Label".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: vec![ValueRecord {
                item_index: None,
                link_index: None,
                tag: "String".into(),
                order: 0,
                parent: None,
                attributes: BTreeMap::from([("value".into(), "old".into())]),
                decoded_json: None,
                text: None,
            }]
            .into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    }
}

#[test]
fn source_reader_keeps_direct_mixed_text_and_never_credits_lookup_as_a_read() {
    let xml = "<Property><String>direct Ω<Sub>middle &amp; content</Sub>tail</String></Property>";
    let fixture = crate::test_support::source_fidelity::PropertyFixture::new(property(), xml);
    crate::test_support::source_fidelity::with_property_reader(
        &[],
        &[&fixture],
        &DecodePolicy::service(),
        |reader| {
            let source = reader.property_xml(&fixture).unwrap();
            assert_eq!(source.as_ref(), xml);
            let tree = reader
                .parse_xml(source.as_ref(), "source reader control")
                .unwrap();
            let text = tree.root_element().first_element_child().unwrap();
            assert_eq!(text.text(), Some("direct Ω"));
            assert_eq!(text.last_child().unwrap().text(), Some("tail"));
            assert!(reader.take_record_captures().unwrap().is_empty());
        },
    );
}

#[test]
fn typed_reader_uses_current_values_after_an_edit_and_deletion() {
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let reader = PropertyReader::typed(&context);
    let mut property = property();
    assert!(reader.property_xml(&property).unwrap().contains("old"));
    let Some(PropertyBody::Persisted { values, .. }) = &mut property.body else {
        panic!("persisted fixture")
    };
    values[0].attributes.insert("value".into(), "new Ω".into());
    let edited = reader.property_xml(&property).unwrap();
    assert!(edited.contains("new Ω"));
    assert!(!edited.contains("old"));
    let Some(PropertyBody::Persisted { values, .. }) = &mut property.body else {
        panic!("persisted fixture")
    };
    values.clear();
    let deleted = reader.property_xml(&property).unwrap();
    assert!(!deleted.contains("<String"));
    assert!(!deleted.contains("new Ω"));
}

#[test]
fn source_reader_rejects_a_view_from_another_session() {
    let xml = b"<Property><String value='old'/></Property>";
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let other_arena = DecodeArena::new();
    let (_foreign_context, foreign) =
        DecodeContext::from_root_bytes(xml, &other_arena, &DecodePolicy::service()).unwrap();
    let sources = BTreeMap::from([(property().id, foreign)]);
    assert!(PropertyReader::fixtures(&context, sources, foreign).is_err());
}

#[test]
fn entry_lists_keep_sparse_ordinals_and_use_the_current_referenced_entry() {
    use crate::native::entry_contents::EntryContents;
    use cadmpeg_ir::scalar::FiniteReal;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut property = property();
    property.id = "authored:native:property#list".into();
    property.owner = "authored:native:object#owner".into();
    property.type_name = "App::PropertyFloatList".into();
    property.family = crate::persistence::property_family(&property.type_name);
    let Some(PropertyBody::Persisted {
        values,
        side_entries,
        ..
    }) = &mut property.body
    else {
        panic!("persisted property");
    };
    values[0].tag = "FloatList".into();
    values[0].attributes = BTreeMap::from([("file".into(), "selected.bin".into())]);
    *side_entries = vec!["selected.bin".into()];
    let mut selected = crate::native::EntryRecord::new(
        "authored:native:entry#selected".into(),
        "selected.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        vec![property.id.clone()],
    )
    .unwrap();
    selected.contents.push(EntryContents::Float {
        values: vec![
            Some(FiniteReal::new(1.5).unwrap()),
            None,
            Some(FiniteReal::new(3.5).unwrap()),
        ],
    });
    let mut unrelated = crate::native::EntryRecord::new(
        "authored:native:entry#other".into(),
        "other.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
    )
    .unwrap();
    unrelated.contents.push(EntryContents::Float {
        values: vec![Some(FiniteReal::new(99.0).unwrap())],
    });
    let mut entries = vec![unrelated, selected];
    {
        let reader = PropertyReader::typed(&ctx).with_entries(entries.as_slice());
        let values = reader.float_slots(&property).unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0].unwrap().get(), 1.5);
        assert_eq!(values[1], None);
        assert_eq!(
            reader.payload_ordinals(&property).collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(
            reader.float_slots(&property).unwrap()[2].unwrap().get(),
            3.5
        );
    }
    let EntryContents::Float { values } = &mut entries[1].contents[0] else {
        panic!("float interpretation");
    };
    values[2] = Some(FiniteReal::new(4.5).unwrap());
    let reader = PropertyReader::typed(&ctx).with_entries(entries.as_slice());
    assert_eq!(
        reader.float_slots(&property).unwrap()[2].unwrap().get(),
        4.5
    );
    assert!(
        ctx.take_record_captures().unwrap().is_empty(),
        "source-less typed entry access invents no source reads"
    );
}

#[test]
fn projection_lookup_working_storage_does_not_charge_retained_output() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let reader = PropertyReader::typed(&context);
    reader
        .begin_projection("custom:native:object#current")
        .unwrap();
    assert!(!reader.unavailable_named_exact("ExpressionEngine").unwrap());
    assert!(reader.take_unavailable().is_empty());
    context.finish_session().unwrap();
}
