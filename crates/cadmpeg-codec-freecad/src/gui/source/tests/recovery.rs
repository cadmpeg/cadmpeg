// SPDX-License-Identifier: Apache-2.0
//! GUI source recovery keeps independent typed owners and value siblings.

use super::*;
use cadmpeg_ir::source_fidelity::parts::{PartOutput, PartState};

fn check(bytes: &[u8], inspect: impl FnOnce(Graph, PartOutput)) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let entries = Default::default();
    let sources = GuiSources {
        entries: &entries,
        entry_owners: None,
        entry_interpretations: None,
        gui_entry_interpretations: None,
        declared_entries: None,
        declared_objects: None,
        admitted_objects: None,
        entry_parents: None,
        objects: &[],
        object_sources: None,
        properties: &[],
        payloads: &[],
        element_maps: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let graph = parse(&ctx, view)
        .unwrap()
        .decode(&ctx, &sources, &mut output)
        .unwrap();
    inspect(graph, output);
}

#[test]
fn malformed_gui_value_retains_property_header_provider_and_valid_peer() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='2'><Property name='Broken' type='App::PropertyFloat'><Float value='NaN'/></Property><Property name='Good' type='App::PropertyString'><String value='kept'/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=''/></Document>";
    check(bytes, |graph, output| {
        assert_eq!(graph.documents.len(), 1);
        assert_eq!(graph.providers.len(), 1);
        assert_eq!(graph.properties.len(), 2);
        assert!(graph.properties[0].values.is_empty());
        assert_eq!(graph.properties[1].values[0].attributes["value"], "kept");
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_property_value");
    });
}

#[test]
fn duplicate_gui_provider_gives_up_only_the_later_provider() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='3'><ViewProvider name='A'><Properties Count='0'/></ViewProvider><ViewProvider name='A'><Properties Count='0'/></ViewProvider><ViewProvider name='B'><Properties Count='0'/></ViewProvider></ViewProviderData></Document>";
    check(bytes, |graph, output| {
        assert_eq!(
            graph
                .providers
                .iter()
                .map(|provider| provider.name.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.view_provider");
    });
}

#[test]
fn broken_state_child_keeps_its_peer_and_later_document_state() {
    let bytes = b"<Document SchemaVersion='1'><State><Good value='one'/><Broken value='&missing;'/><Tail value='two'/></State><Camera settings=''/><ViewProviderData Count='0'/></Document>";
    check(bytes, |graph, output| {
        assert_eq!(graph.documents[0].states.len(), 2);
        assert_eq!(
            graph.documents[0].states[0]
                .values
                .iter()
                .map(|value| value.tag.as_str())
                .collect::<Vec<_>>(),
            ["Good", "Tail"]
        );
        assert_eq!(graph.documents[0].states[0].values[1].order, 2);
        let slots = serde_json::to_value(&graph.documents[0].states[0].values).unwrap();
        assert_eq!(slots.as_array().unwrap().len(), 3);
        assert!(slots[1].is_null());
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_state_element");
    });
}

#[test]
fn nonfinite_binary_float_gives_up_only_its_indexed_value() {
    let xml = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Values' type='App::PropertyFloatList'><FloatList file='values'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut payload = 3_u32.to_le_bytes().to_vec();
    for scalar in [2.0_f64, f64::NAN, 4.0] {
        payload.extend_from_slice(&scalar.to_le_bytes());
    }
    let result = binary(xml, "values", &payload);
    let entries = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = entries
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            crate::native::entry_contents::EntryContents::Float { values } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        values
            .iter()
            .enumerate()
            .filter_map(|(order, value)| value.map(|value| (order, value.get())))
            .collect::<Vec<_>>(),
        [(0, 2.0), (2, 4.0)]
    );
    assert_eq!(values.len(), 3);
    assert_eq!(
        result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
            .unwrap()
            .len(),
        1
    );
    let failed = result
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.float_list_value");
    assert_eq!(failed[0].offset(), 4 + 8);
    assert_eq!(failed[0].byte_len(), 8);
}

#[test]
fn duplicate_scalar_value_root_preserves_first_value_and_next_property() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='2'><Property name='Repeated' type='App::PropertyString'><String value='first'/><String value='extra'/></Property><Property name='Next' type='App::PropertyBool'><Bool value='1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    check(bytes, |graph, output| {
        assert_eq!(graph.properties.len(), 2);
        assert_eq!(graph.properties[0].values.len(), 1);
        assert_eq!(graph.properties[0].values[0].attributes["value"], "first");
        assert_eq!(graph.properties[1].values[0].attributes["value"], "1");
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_property_value");
        let at = std::str::from_utf8(bytes)
            .unwrap()
            .find("<String value='extra'/>")
            .unwrap();
        assert_eq!(failed[0].offset(), u64::try_from(at).unwrap());
        assert_eq!(
            failed[0].byte_len(),
            u64::try_from("<String value='extra'/>".len()).unwrap()
        );
    });
}

#[test]
fn malformed_techdraw_scalar_gives_up_only_the_field() {
    let bytes = br##"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Formats' type='TechDraw::PropertyGeomFormatList'><GeomFormatList count='1'><GeomFormat type='TechDraw::GeomFormat'><GeomIndex value='0'/><Style value='2'/><Weight value='NaN'/><Color value='#FF0000'/><Visible value='1'/></GeomFormat></GeomFormatList></Property></Properties></ViewProvider></ViewProviderData></Document>"##;
    check(bytes, |graph, output| {
        assert_eq!(graph.properties.len(), 1);
        let held = &graph.properties[0].values;
        assert!(held.iter().any(|value| value.tag == "GeomFormat"));
        assert!(held.iter().any(|value| value.tag == "Color"));
        assert!(!held.iter().any(|value| value.tag == "Weight"));
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_value_element");
        assert_eq!(
            failed[0].byte_len(),
            u64::try_from(b"<Weight value='NaN'/>".len()).unwrap()
        );
    });
}

#[test]
fn gui_link_list_retains_typed_local_and_external_target_slots() {
    let bytes = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='2'><Property name='Links' type='App::PropertyLinkList'><LinkList count='2'><Link value='A'/><Link value='B'/></LinkList></Property><Property name='External' type='App::PropertyXLink'><XLink name='Remote' file='other.FCStd' sub='Face1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    use cadmpeg_ir::Codec;
    let document=b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object type='App::Feature' name='A'/><Object type='App::Feature' name='B'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", bytes),
    ]);
    let result = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(archive),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .source_fidelity()
        .parts()
        .values()
        .all(|part| !matches!(part.state(), PartState::GivenUp { .. })));
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let local = properties
        .iter()
        .find(|property| property.name == "Links")
        .unwrap();
    let external = properties
        .iter()
        .find(|property| property.name == "External")
        .unwrap();
    let links = &local.links;
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[0].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "A").as_str())
    );
    assert_eq!(
        links[1].as_ref().unwrap().object(),
        Some(crate::native::native_id("object", "B").as_str())
    );
    assert_eq!(local.values[1].attributes["value"], "A");
    assert_eq!(external.links[0].as_ref().unwrap().object(), Some("Remote"));
    assert!(external.side_entries.is_empty());
}

#[test]
fn invalid_gui_utf8_is_bounded_by_its_xml_member() {
    check(b"\xff", |graph, output| {
        assert!(graph.documents.is_empty());
        let parts = output.fidelity().parts().values().collect::<Vec<_>>();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].level().as_str(), "freecad.gui_document_xml");
        assert!(matches!(parts[0].state(), PartState::GivenUp { .. }));
    });
}

#[test]
fn gui_extension_owns_its_nested_properties_and_duplicate_type_is_local() {
    let bytes=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Extensions Count='2'><Extension name='First' type='Gui::One'><Properties Count='1'><Property name='Nested' type='App::PropertyInteger'><Integer value='7'/></Property></Properties></Extension><Extension name='Second' type='Gui::One'/></Extensions><Properties Count='1'><Property name='Direct' type='App::PropertyBool'><Bool value='1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    check(bytes, |graph, output| {
        assert_eq!(graph.providers.len(), 1);
        assert_eq!(graph.properties.len(), 2);
        let extension =
            crate::native::native_child_id("extension", &graph.providers[0].id, "First");
        let nested = graph
            .properties
            .iter()
            .find(|property| property.name == "Nested")
            .unwrap();
        assert_eq!(nested.owner, extension);
        assert_eq!(nested.values[0].attributes["value"], "7");
        let failed = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_extension");
    });
}

fn binary(xml: &[u8], name: &str, payload: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::Codec;
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='A' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", xml),
        (name, payload),
    ]);
    crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap()
}

fn auxiliary(
    kind: &str,
    payload: &[u8],
    inspect: impl FnOnce(
        &[crate::native::entry_contents::EntryContents],
        &cadmpeg_ir::codec::DecodeResult,
    ),
) {
    let xml=format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Values' type='{kind}'><{} file='values'/></Property></Properties></ViewProvider></ViewProviderData></Document>",if kind=="Mesh::PropertyMaterial" {"Material"}else{"VectorList"});
    let result = binary(xml.as_bytes(), "values", payload);
    let entries = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    inspect(
        entries
            .iter()
            .find(|entry| entry.name() == "values")
            .unwrap()
            .contents(),
        &result,
    );
}

#[test]
fn nonfinite_f32_record_preserves_peer_records_and_source_ordinals() {
    let mut payload = 3_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, 4.0, f32::NAN, 6.0, 7.0, 8.0, 9.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    auxiliary("Mesh::PropertyNormalList", &payload, |contents, result| {
        let values = contents
            .iter()
            .find_map(|content| match content {
                crate::native::entry_contents::EntryContents::F32 {
                    values: crate::native::entry_contents::F32Values::Vector(values),
                } => Some(values),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            values
                .iter()
                .enumerate()
                .filter_map(|(order, value)| value
                    .map(|value| (order, value.map(|value| value.get()).to_vec())))
                .collect::<Vec<_>>(),
            [(0, vec![1., 2., 3.]), (2, vec![7., 8., 9.])]
        );
        assert_eq!(values.len(), 3);
        let failed = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.f32_record");
        assert_eq!(failed[0].byte_len(), 12);
        assert!(result
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == "freecad.f32_record_list_entry"));
    });
}

#[test]
fn mesh_material_float_failure_keeps_other_arrays_and_later_values() {
    let mut payload = 1_u32.to_le_bytes().to_vec();
    for color in [1_u32, 2, 3, 4] {
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&color.to_le_bytes());
    }
    payload.extend_from_slice(&2_u32.to_le_bytes());
    payload.extend_from_slice(&0.5_f32.to_le_bytes());
    payload.extend_from_slice(&f32::NAN.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&0.25_f32.to_le_bytes());
    auxiliary("Mesh::PropertyMaterial", &payload, |contents, result| {
        let (ambient, diffuse, specular, emissive, shininess, transparency) = contents
            .iter()
            .find_map(|content| match content {
                crate::native::entry_contents::EntryContents::MeshMaterial {
                    ambient,
                    diffuse,
                    specular,
                    emissive,
                    shininess,
                    transparency,
                    ..
                } => Some((
                    ambient,
                    diffuse,
                    specular,
                    emissive,
                    shininess,
                    transparency,
                )),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            [
                ambient.is_some(),
                diffuse.is_some(),
                specular.is_some(),
                emissive.is_some(),
                shininess.is_some(),
                transparency.is_some()
            ]
            .into_iter()
            .filter(|value| *value)
            .count(),
            6
        );
        assert_eq!(
            [ambient, diffuse, specular, emissive]
                .iter()
                .map(|array| array.as_ref().unwrap().iter().flatten().count())
                .sum::<usize>(),
            4
        );
        assert_eq!(transparency.as_ref().unwrap()[0].unwrap().get(), 0.25);
        let failed = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.mesh_material_value");
        assert_eq!(failed[0].byte_len(), 4);
        let levels = result
            .source_fidelity()
            .parts()
            .values()
            .map(|part| part.level().as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for level in [
            "freecad.mesh_material_entry",
            "freecad.mesh_material_binding",
            "freecad.mesh_material_array",
        ] {
            assert!(levels.contains(level));
        }
    });
}

#[test]
fn gui_link_to_failed_declaration_keeps_header_without_reading_target_value() {
    use cadmpeg_ir::Codec;
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object type='App::Feature' name='A'/><Object name='B'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='2'><Property name='Failed' type='App::PropertyLink'><Link value='B'/></Property><Property name='Absent' type='App::PropertyLink'><Link value='C'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(archive),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    assert_eq!(properties.len(), 2);
    for property in &properties {
        assert!(property.values.is_empty());
        assert!(property.links.is_empty());
        assert_eq!(
            serde_json::to_value(&property.values).unwrap(),
            serde_json::json!([null])
        );
    }
    let failed = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert_eq!(
        failed
            .iter()
            .filter(|part| part.level().as_str() == "freecad.object_declaration")
            .count(),
        1
    );
    let value = failed
        .iter()
        .find(|part| part.level().as_str() == "freecad.gui_property_value")
        .unwrap();
    assert_eq!(
        value.byte_len(),
        u64::try_from(b"<Link value='C'/>".len()).unwrap()
    );
}

#[test]
fn material_v3_strings_have_their_actual_material_parent_and_count_framing() {
    let xml = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Materials' type='App::PropertyMaterialList'><MaterialList file='materials' version='3'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut payload = 2_u32.to_le_bytes().to_vec();
    for _ in 0..2 {
        for color in [1_u32, 2, 3, 4] {
            payload.extend_from_slice(&color.to_le_bytes());
        }
        for scalar in [0.5_f32, 0.25] {
            payload.extend_from_slice(&scalar.to_le_bytes());
        }
    }
    for text in [
        &b"\xff"[..],
        &b"p"[..],
        &b"u"[..],
        &b""[..],
        &b"pp"[..],
        &b"uu"[..],
    ] {
        payload.extend_from_slice(&u32::try_from(text.len()).unwrap().to_le_bytes());
        payload.extend_from_slice(text);
    }
    let result = binary(xml, "materials", &payload);
    let entries = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = entries
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            crate::native::entry_contents::EntryContents::Material { values, .. } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(values.iter().filter(|slot| slot.fixed.is_some()).count(), 2);
    assert_eq!(
        values
            .iter()
            .flat_map(|slot| [&slot.image, &slot.image_path, &slot.uuid])
            .flatten()
            .count(),
        5
    );
    assert_eq!(values[1].uuid.as_deref(), Some("uu"));
    let parts = result.source_fidelity().parts();
    let failed = parts
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.material_list_string");
    assert_eq!(failed[0].byte_len(), 5);
    for string in parts
        .values()
        .filter(|part| part.level().as_str() == "freecad.material_list_string")
    {
        let parent = parts.get(string.parent().unwrap()).unwrap();
        assert_eq!(parent.level().as_str(), "freecad.material_list_entry");
    }
}

#[test]
fn malformed_nested_extension_value_gives_up_its_nearest_listed_extension() {
    let xml=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Extensions Count='2'><Extension name='Broken' type='Gui::One'><Properties Count='1'><Property name='Value' type='App::PropertyFloat'><Float value='NaN'/></Property></Properties></Extension><Extension name='Good' type='Gui::Two'><Properties Count='1'><Property name='Value' type='App::PropertyInteger'><Integer value='7'/></Property></Properties></Extension></Extensions><Properties Count='0'/></ViewProvider></ViewProviderData></Document>";
    check(xml, |graph, output| {
        assert_eq!(graph.providers.len(), 1);
        assert_eq!(graph.properties.len(), 1);
        assert_eq!(graph.properties[0].values[0].attributes["value"], "7");
        let parts = output.fidelity().parts();
        let failed = parts
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.gui_extension");
        assert!(!parts.values().any(|part| matches!(
            part.level().as_str(),
            "freecad.gui_property" | "freecad.gui_property_value" | "freecad.gui_value_element"
        )));
    });
}

#[test]
fn recursive_gui_state_refusal_is_local_and_keeps_the_later_peer() {
    let mut bytes = String::from("<Document SchemaVersion='1'><State>");
    for _ in 0..14 {
        bytes.push_str("<One>");
    }
    bytes.push_str("<TooDeep/>");
    for _ in 0..14 {
        bytes.push_str("</One>");
    }
    bytes.push_str("<Peer/></State></Document>");

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 32;
    let (ctx, view) = DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
    let entries = Default::default();
    let sources = GuiSources {
        entries: &entries,
        entry_owners: None,
        entry_interpretations: None,
        gui_entry_interpretations: None,
        declared_entries: None,
        declared_objects: None,
        admitted_objects: None,
        entry_parents: None,
        objects: &[],
        object_sources: None,
        properties: &[],
        payloads: &[],
        element_maps: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let graph = parse(&ctx, view)
        .unwrap()
        .decode(&ctx, &sources, &mut output)
        .unwrap();
    let values = &graph.documents[0].states[0].values;
    assert_eq!(values.len(), 15);
    assert!(values.iter().take(14).all(|value| value.tag == "One"));
    assert_eq!(values.last().unwrap().tag, "Peer");
    let failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.gui_state_element");
    assert_eq!(
        failed[0].byte_len(),
        u64::try_from(b"<TooDeep/>".len()).unwrap()
    );
    assert!(ctx.finish_session().is_ok());
}
