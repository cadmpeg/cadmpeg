// SPDX-License-Identifier: Apache-2.0
//! Exact runtime dispatch precedes registered side-entry grammar validation.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::gui::{validate_gui_list_payloads, GuiSources};
use crate::native::GuiPropertyRecord;

fn property(type_name: &str, side_entries: &[&str]) -> GuiPropertyRecord {
    GuiPropertyRecord {
        id: "fcstd:native:gui-property#payload".into(),
        owner: "fcstd:native:gui-provider#provider".into(),
        name: "Payload".into(),
        type_name: type_name.into(),
        status: None,
        order: 0,
        values: Vec::new(),
        side_entries: side_entries.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn unknown_runtime_keeps_multiple_opaque_members_without_list_dispatch() {
    let xml = "<GuiDocument SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='GUI only' expanded='0'><Properties Count='1'><Property name='Opaque' type='Vendor::Payload'><Vendor><Leaf file='first.bin'/><Leaf file='second.bin'/></Vendor></Property></Properties></ViewProvider></ViewProviderData></GuiDocument>";
    let mut bytes = xml.as_bytes().to_vec();
    bytes.extend_from_slice(&[0xff; 4]);
    bytes.extend_from_slice(&[0x7f; 4]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (context, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let end = u64::try_from(xml.len()).unwrap();
    let gui = context
        .register_named_slice(root, ByteRange { start: 0, end }, "GuiDocument.xml")
        .unwrap();
    let first = context
        .register_named_slice(
            root,
            ByteRange {
                start: end,
                end: end + 4,
            },
            "first.bin",
        )
        .unwrap();
    let second = context
        .register_named_slice(
            root,
            ByteRange {
                start: end + 4,
                end: end + 8,
            },
            "second.bin",
        )
        .unwrap();
    let entries = BTreeMap::from([
        ("GuiDocument.xml".into(), gui),
        ("first.bin".into(), first),
        ("second.bin".into(), second),
    ]);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let graph = crate::gui::transfer(
        &context,
        &mut ir,
        gui.window(),
        &GuiSources {
            entries: &entries,
            objects: &[],
            properties: &[],
            payloads: &[],
            topology_occurrences: &[],
            requires_alpha_conversion: false,
        },
    )
    .unwrap();
    assert!(!graph.untyped);
    assert_eq!(graph.properties.len(), 1);
    let retained = &graph.properties[0];
    assert_eq!(retained.type_name, "Vendor::Payload");
    assert_eq!(retained.side_entries, ["first.bin", "second.bin"]);
    assert_eq!(retained.values.len(), 3);
    assert_eq!(retained.values[0].tag, "Vendor");
    assert_eq!(retained.values[1].attributes["file"], "first.bin");
    assert_eq!(retained.values[2].attributes["file"], "second.bin");
    assert_eq!(first.window(), &[0xff; 4]);
    assert_eq!(second.window(), &[0x7f; 4]);
    assert_eq!(context.resource_refusal(), None);
    context.finish_session().unwrap();
}

#[test]
fn registered_list_grammars_still_refuse_their_missing_members() {
    for runtime in [
        "App::PropertyColorList",
        "App::PropertyFloatList",
        "Part::PropertyFilletEdges",
        "App::PropertyMaterialList",
        "App::PropertyPlacementList",
        "App::PropertyVectorList",
        "Part::PropertyTopoShapeList",
    ] {
        let property = property(runtime, &["missing.bin"]);
        let Err(CodecError::Malformed(message)) =
            validate_gui_list_payloads(std::slice::from_ref(&property), &BTreeMap::new(), false)
        else {
            panic!("a registered list must resolve its declared member")
        };
        assert!(message.contains(&property.id));
        assert!(message.contains("missing.bin"));
    }
}

#[test]
fn single_member_grammars_still_reject_multiple_valid_empty_payloads() {
    let bytes = [0_u8; 8];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (context, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = context
        .register_named_slice(root, ByteRange { start: 0, end: 4 }, "first.bin")
        .unwrap();
    let second = context
        .register_named_slice(root, ByteRange { start: 4, end: 8 }, "second.bin")
        .unwrap();
    let entries = BTreeMap::from([("first.bin".into(), first), ("second.bin".into(), second)]);
    for runtime in [
        "App::PropertyColorList",
        "App::PropertyFloatList",
        "Part::PropertyFilletEdges",
        "App::PropertyMaterialList",
        "App::PropertyPlacementList",
        "App::PropertyVectorList",
    ] {
        let property = property(runtime, &["first.bin", "second.bin"]);
        assert!(matches!(
            validate_gui_list_payloads(&[property], &entries, false),
            Err(CodecError::Malformed(_))
        ));
    }
    assert_eq!(context.resource_refusal(), None);
    context.finish_session().unwrap();
}

#[test]
fn topo_shape_lists_still_resolve_every_plural_member() {
    let bytes = [0_u8; 8];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (context, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = context
        .register_named_slice(root, ByteRange { start: 0, end: 4 }, "first.bin")
        .unwrap();
    let second = context
        .register_named_slice(root, ByteRange { start: 4, end: 8 }, "second.bin")
        .unwrap();
    let entries = BTreeMap::from([("first.bin".into(), first), ("second.bin".into(), second)]);
    let property = property("Part::PropertyTopoShapeList", &["first.bin", "second.bin"]);
    assert!(
        validate_gui_list_payloads(std::slice::from_ref(&property), &entries, false)
            .unwrap()
            .is_empty()
    );
    let missing_second = BTreeMap::from([("first.bin".into(), first)]);
    let Err(CodecError::Malformed(message)) =
        validate_gui_list_payloads(&[property], &missing_second, false)
    else {
        panic!("the second declared shape member is required")
    };
    assert!(message.contains("second.bin"));
    assert_eq!(context.resource_refusal(), None);
    context.finish_session().unwrap();
}

#[test]
fn registered_non_list_members_keep_presence_and_cardinality_validation() {
    let missing = property("App::PropertyFileIncluded", &["missing.bin"]);
    let Err(CodecError::Malformed(message)) =
        validate_gui_list_payloads(&[missing], &BTreeMap::new(), false)
    else {
        panic!("a registered non-list property must resolve its member")
    };
    assert!(message.contains("missing.bin"));
    let multiple = property("App::PropertyFileIncluded", &["first.bin", "second.bin"]);
    let Err(CodecError::Malformed(message)) =
        validate_gui_list_payloads(&[multiple], &BTreeMap::new(), false)
    else {
        panic!("a registered non-list property retains its original cardinality check")
    };
    assert!(message.contains("references more than one side entry"));
}
