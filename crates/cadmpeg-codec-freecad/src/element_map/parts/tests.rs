// SPDX-License-Identifier: Apache-2.0
//! Actual naming callbacks preserve independently framed fields and their reads.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartState};
use std::collections::BTreeMap;

#[test]
fn damaged_inline_string_record_keeps_positional_peers_and_rolls_back_its_reads() {
    let xml = b"<Document><StringHasher count='3'>1.c first\n\xff.c damaged\n3.c last\n</StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "Document.xml").unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let index = source.children(0).next().unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    ctx.decode_part(
        &mut output,
        source.extent(index).unwrap(),
        source.spec(index, "freecad.string_hasher").unwrap(),
        |stage| {
            crate::element_map::adopted::decode(&ctx, &source, index, None, None, &context, stage)
        },
    )
    .unwrap();
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    let entries = tables[0].entries().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].as_ref().unwrap().payload, "first");
    assert!(entries[1].is_none());
    assert_eq!(entries[2].as_ref().unwrap().payload, "last");
    assert_eq!(
        serde_json::to_value(&tables[0]).unwrap()["entries"][1],
        serde_json::Value::Null
    );
    let failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.string_hasher_record");
    assert_eq!(output.losses().len(), 1);
    assert_eq!(output.losses()[0].code.local_code(), "record.unreadable");
    let bad =
        cadmpeg_core::decode::u64_from_index(xml.iter().position(|byte| *byte == 0xff).unwrap());
    let captures = ctx.take_record_captures().unwrap();
    let table = captures
        .iter()
        .find(|capture| capture.id() == tables[0].id())
        .unwrap();
    assert!(table.reads()["Document.xml"]
        .iter()
        .all(|range| !range.contains(&bad)));
}

#[test]
fn damaged_map_children_and_chains_keep_their_distinct_null_slots() {
    let xml = b"<Property><Part ElementMap='1'/><ElementMap new='1'/><ElementMap2>7 PostfixCount 1 tag MapCount 1\nElementMap 1 7 1 Face ChildCount 2\nx 0 1 0 0 suffix 0\n1 0 1 0 0 suffix 0\nNameCount 3\n0\n\xffbad.0 0\n;stable.0 0\nEndMap\n</ElementMap2></Property>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let property = crate::native::PropertyRecord {
        dynamic: None,
        id: "fcstd:native:property#Shape".into(),
        owner: "fcstd:native:object#Shape".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: Some(crate::native::PropertyBody::Persisted {
            values: Vec::new().into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
        order: 0,
    };
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut previous = Vec::new();
    for index in source.children(0) {
        let value = ctx
            .decode_part(
                &mut output,
                source.extent(index).unwrap(),
                source.spec(index, "freecad.property_value").unwrap(),
                |stage| {
                    crate::element_map::tests::adapters::decode_value(
                        &ctx,
                        &source,
                        index,
                        Some(&property),
                        &previous,
                        &context,
                        stage,
                    )
                },
            )
            .unwrap()
            .flatten();
        if let Some(value) = value {
            previous.push(value);
        }
    }
    let maps = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap();
    assert_eq!(maps.len(), 1, "{:?}", output.losses());
    let group = maps[0].maps.root().groups[0].as_ref().unwrap();
    assert_eq!(group.children.len(), 2);
    assert!(group.children[0].is_none());
    assert_eq!(group.children[1].as_deref(), Some("1 0 1 0 0 suffix 0"));
    assert_eq!(group.names.len(), 3);
    assert!(group.names[0].as_ref().unwrap().is_empty());
    assert!(group.names[1].is_none());
    assert_eq!(
        group.names[2].as_ref().unwrap()[0].resolved.as_deref(),
        Some("stable")
    );
    let failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .map(|part| part.level().as_str())
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert!(failed.contains(&"freecad.inline_map_child"));
    assert!(failed.contains(&"freecad.inline_map_name_chain"));
    assert_eq!(output.losses().len(), 2);
    let wire = serde_json::to_value(&maps[0]).unwrap();
    assert_eq!(
        wire["maps"][0]["groups"][0]["children"][0],
        serde_json::Value::Null
    );
    assert_eq!(
        wire["maps"][0]["groups"][0]["names"][1],
        serde_json::Value::Null
    );
    let restored =
        serde_json::from_value::<crate::native::element_map::ElementMapRecord>(wire).unwrap();
    assert_eq!(restored, maps[0]);
}

#[test]
fn naming_navigation_whitespace_excludes_spaces_inside_a_held_text_payload() {
    let xml = b"<Document><StringHasher count='1'>\n  1.0 0:  a b  \n  </StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let index = source.children(0).next().unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    ctx.decode_part(
        &mut output,
        source.extent(index).unwrap(),
        source.spec(index, "freecad.string_hasher").unwrap(),
        |stage| {
            crate::element_map::adopted::decode(&ctx, &source, index, None, None, &context, stage)
        },
    )
    .unwrap();
    assert!(output.losses().is_empty());
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(
        tables[0].entries().unwrap()[0].as_ref().unwrap().payload,
        "  a b  "
    );
    let payload_start = xml.windows(2).position(|bytes| bytes == b"0:").unwrap() + 2;
    let payload_end = payload_start + b"  a b  ".len();
    let whitespace = context.take_source_whitespace();
    assert!(!whitespace.is_empty());
    assert!(whitespace
        .iter()
        .all(|range| range.end() <= payload_start || range.start() >= payload_end));
    assert!(whitespace
        .iter()
        .all(|view| view.window().iter().all(u8::is_ascii_whitespace)));
}

#[test]
fn side_naming_members_use_each_declared_table_and_map_part() {
    let xml = b"<Document><StringHasher count='1' file='table.txt'/><Property><Part HasherIndex='0' ElementMap='1'/><ElementMap file='map.txt'/></Property></Document>";
    let table = b"StringTableStart v1 1\n1.c admitted\n";
    let map = b"BeginElementMap v1\n7 PostfixCount 1 tag\nMapCount 1\nElementMap 1 7 1\nFace ChildCount 1\n1 0 1 0 0 suffix 0\nNameCount 2\n0\n;stable.0 0\nEndMap\n";
    let bytes = [xml.as_slice(), table.as_slice(), map.as_slice()].concat();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let named = |start, end, name| {
        ctx.register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange {
                start: cadmpeg_core::decode::u64_from_index(start),
                end: cadmpeg_core::decode::u64_from_index(end),
            },
            name,
        )
        .unwrap()
    };
    let document = named(0, xml.len(), "Document.xml");
    let table_view = named(xml.len(), xml.len() + table.len(), "table.txt");
    let map_view = named(xml.len() + table.len(), bytes.len(), "map.txt");
    let source = crate::xml_framing::XmlSource::new(&ctx, document).unwrap();
    let entries = BTreeMap::from([
        ("table.txt".into(), table_view),
        ("map.txt".into(), map_view),
    ]);
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let property = crate::native::PropertyRecord {
        dynamic: None,
        id: "fcstd:native:property#Shape".into(),
        owner: "fcstd:native:object#Shape".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: Some(crate::native::PropertyBody::Persisted {
            values: Vec::new().into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
        order: 0,
    };
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut previous = Vec::new();
    for (index, _) in source.elements().iter().enumerate().filter(|(index, _)| {
        source
            .name(*index)
            .is_ok_and(|name| matches!(name, "StringHasher" | "Part" | "ElementMap"))
    }) {
        let property = (source.name(index).unwrap() != "StringHasher").then_some(&property);
        let value = ctx
            .decode_part(
                &mut output,
                source.extent(index).unwrap(),
                source
                    .spec(
                        index,
                        if property.is_some() {
                            "freecad.property_value"
                        } else {
                            "freecad.string_hasher"
                        },
                    )
                    .unwrap(),
                |stage| {
                    crate::element_map::tests::adapters::decode_value(
                        &ctx, &source, index, property, &previous, &context, stage,
                    )
                },
            )
            .unwrap()
            .flatten();
        if let Some(value) = value {
            previous.push(value);
        }
    }
    assert!(output.losses().is_empty(), "{:?}", output.losses());
    for level in [
        "string_table_entry",
        "string_table_header",
        "string_table_record",
        "element_map_entry",
        "element_map_header",
        "element_map_postfix_dictionary",
        "element_map_postfix",
        "element_map_node",
        "element_map_group",
        "element_map_child",
        "element_map_name_chain",
    ] {
        let level = format!("freecad.{level}");
        assert!(
            output.fidelity().parts().values().any(
                |part| part.level().as_str() == level && part.state() == &PartState::Decoded {}
            ),
            "{level}"
        );
    }
    let namespace = output.native().namespace("fcstd").unwrap();
    assert_eq!(
        namespace
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        namespace
            .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rolled_back_hasher_owner_prunes_a_later_map_despite_transient_lookup_state() {
    use cadmpeg_core::CodecError;
    let xml = b"<Document><Property><Part/><StringHasher count='1'>1.c admitted\n</StringHasher></Property><Property><Part HasherIndex='0' ElementMap='1'/><ElementMap new='1'/><ElementMap2>7 PostfixCount 0 MapCount 1 ElementMap 1 7 0 EndMap</ElementMap2></Property></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let property = |name| crate::native::PropertyRecord {
        dynamic: None,
        id: crate::native::native_id("property", name),
        owner: crate::native::native_id("object", name),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: None,
        order: 0,
    };
    let owners = source.children(0).collect::<Vec<_>>();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    for (position, owner) in owners.into_iter().enumerate() {
        let property = property(if position == 0 { "first" } else { "second" });
        let parsed = ctx
            .decode_part(
                &mut output,
                source.extent(owner).unwrap(),
                source.spec(owner, "freecad.property").unwrap(),
                |stage| {
                    let mut previous = Vec::new();
                    for index in source.children(owner) {
                        let value = ctx
                            .decode_part(
                                stage,
                                source.extent(index)?,
                                source.spec(index, "freecad.property_value")?,
                                |value_stage| {
                                    crate::element_map::tests::adapters::decode_value(
                                        &ctx,
                                        &source,
                                        index,
                                        Some(&property),
                                        &previous,
                                        &context,
                                        value_stage,
                                    )
                                },
                            )?
                            .flatten();
                        if let Some(value) = value {
                            previous.push(value);
                        }
                    }
                    if position == 0 {
                        Err(CodecError::malformed(
                            "the enclosing property transaction cannot commit",
                        ))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();
        assert_eq!(parsed.is_some(), position != 0);
    }
    let namespace = output.native().namespace("fcstd").unwrap();
    assert!(
        !namespace
            .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            .unwrap()
            .is_empty(),
        "{:?}",
        output.losses()
    );
    assert_eq!(
        namespace
            .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            .unwrap()[0]
            .string_table
            .as_deref(),
        Some("fcstd:native:string-table#0")
    );
    output.prune_dependencies(&ctx).unwrap();
    let namespace = output.native().namespace("fcstd").unwrap();
    assert!(namespace
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap()
        .is_empty());
    assert_eq!(output.losses().len(), 1);
    assert!(output.losses()[0]
        .message
        .contains("fcstd:native:string-table#0"));
    assert!(output.losses()[0]
        .message
        .contains("fcstd:native:element-map#"));
    assert_eq!(
        output
            .fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .count(),
        1
    );
}

#[test]
fn invalid_new_layout_markers_give_up_the_marker_without_a_second_successor_loss() {
    for (xml, level, marker_text) in [
        (b"<Document><StringHasher new='1' saveall='bad'/><StringHasher2 count='1'>1.c admitted\n</StringHasher2></Document>".as_slice(), "freecad.string_hasher", b"<StringHasher new='1' saveall='bad'/>".as_slice()),
        (b"<Property><Part ElementMap='1'/><ElementMap new='bad'/><ElementMap2>7 PostfixCount 0 MapCount 1 ElementMap 1 7 0 EndMap</ElementMap2></Property>".as_slice(), "freecad.property_value", b"<ElementMap new='bad'/>".as_slice()),
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
        let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
        let entries = BTreeMap::new();
        let context = crate::element_map::adopted::Context::new(&entries, Some(2));
        let property = crate::native::PropertyRecord {
            dynamic: None, id: "fcstd:native:property#Shape".into(), owner: "fcstd:native:object#Shape".into(), name: "Shape".into(), type_name: "Part::PropertyPartShape".into(), family: crate::native::PropertyFamily::Geometry, status: None, body: None, order: 0,
        };
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        let mut previous = Vec::new();
        for index in source.children(0) {
            let value = ctx.decode_part(&mut output, source.extent(index).unwrap(), source.spec(index, level).unwrap(), |stage| {
                crate::element_map::tests::adapters::decode_value(&ctx, &source, index, (level == "freecad.property_value").then_some(&property), &previous, &context, stage)
            }).unwrap().flatten();
            if let Some(value) = value { previous.push(value); }
        }
        let failed = output.fidelity().parts().values().filter(|part| part.state() == &PartState::GivenUp {}).collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        let start = xml.windows(marker_text.len()).position(|bytes| bytes == marker_text).unwrap();
        assert_eq!(failed[0].offset(), cadmpeg_core::decode::u64_from_index(start));
        assert_eq!(failed[0].byte_len(), cadmpeg_core::decode::u64_from_index(marker_text.len()));
        let namespace = output.native().namespace("fcstd").unwrap();
        let tables = namespace.arena_as::<crate::native::StringTableRecord>("string_tables").unwrap();
        if level == "freecad.string_hasher" {
            // #215: invalid policy metadata cannot erase the independently framed successor.
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].save_all, None);
            assert_eq!(tables[0].threshold, None);
            assert_eq!(tables[0].entries().unwrap()[0].as_ref().unwrap().payload, "admitted");
            let successor = source.children(0).nth(1).unwrap();
            let extent = ctx.capture_part_extent(source.extent(successor).unwrap()).unwrap();
            let captures = ctx.take_record_captures().unwrap();
            let actual = captures.iter().find(|capture| capture.id() == tables[0].id()).unwrap();
            assert_eq!(actual.offset(), extent.offset());
            assert_eq!(actual.byte_len(), extent.byte_len());
            assert!(actual.reads().values().flatten().all(|range| extent.offset() <= range.start && range.end <= extent.offset() + extent.byte_len()));
        } else {
            assert!(tables.is_empty());
        }
        assert!(namespace.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps").unwrap().is_empty());
    }
}

#[test]
fn known_text_header_locates_a_cut_short_record_and_preserves_the_admitted_prefix() {
    // #215 cut-short/end-extent interpretation: an intact flag word and decimal
    // line count locate the unfinished final record up to the carrier text end.
    let xml = b"<Document><StringHasher count='3'>1.c first\n2.c middle\n3.0 0:cut</StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let index = source.children(0).next().unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    ctx.decode_part(
        &mut output,
        source.extent(index).unwrap(),
        source.spec(index, "freecad.string_hasher").unwrap(),
        |stage| {
            crate::element_map::adopted::decode(&ctx, &source, index, None, None, &context, stage)
        },
    )
    .unwrap();
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    let slots = tables[0].entries().unwrap();
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0].as_ref().unwrap().payload, "first");
    assert_eq!(slots[1].as_ref().unwrap().payload, "middle");
    assert!(slots[2].is_none());
    let given_up = output
        .fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1, "{:?}", output.losses());
    assert_eq!(
        given_up[0].1.level().as_str(),
        "freecad.string_hasher_record"
    );
    assert_eq!(
        output
            .fidelity()
            .retained_record(given_up[0].0.as_str())
            .unwrap()
            .data(),
        Some(b"3.0 0:cut".as_slice())
    );
    assert_eq!(output.losses().len(), 1);
}

#[test]
fn nonzero_marker_flags_share_the_actual_marker_metadata_with_the_successor() {
    for flag in ["2", "-1"] {
        let xml = format!("<Document><StringHasher new='{flag}' saveall='1' threshold='7'/><StringHasher2 count='1'>1.c stable\n</StringHasher2></Document>");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .unwrap();
        let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
        let entries = BTreeMap::new();
        let context = crate::element_map::adopted::Context::new(&entries, Some(2));
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        for index in source.children(0) {
            ctx.decode_part(
                &mut output,
                source.extent(index).unwrap(),
                source.spec(index, "freecad.string_hasher").unwrap(),
                |stage| {
                    crate::element_map::adopted::decode(
                        &ctx, &source, index, None, None, &context, stage,
                    )
                },
            )
            .unwrap();
        }
        assert!(output.losses().is_empty(), "{:?}", output.losses());
        let tables = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .unwrap();
        assert_eq!(tables[0].save_all, Some(true));
        assert_eq!(tables[0].threshold, Some(7));
        assert_eq!(
            tables[0].entries().unwrap()[0].as_ref().unwrap().payload,
            "stable"
        );
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == tables[0].id())
            .unwrap();
        let marker = source.children(0).next().unwrap();
        assert_eq!(
            capture.byte_len(),
            u64::try_from(source.extent(marker).unwrap().window().len()).unwrap()
        );
        assert!(!capture.reads().is_empty());
    }
}

#[test]
fn cut_short_required_map_node_keeps_its_exact_home_and_retires_the_unformed_map() {
    let xml = b"<Property><Part ElementMap='1'/><ElementMap new='1'/><ElementMap2>7 PostfixCount 0\nMapCount 3\nElementMap 1 7 0\nEndMap\nElementMap 2 7 0\n</ElementMap2></Property>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service()).unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let property = crate::native::PropertyRecord {
        dynamic: None,
        id: crate::native::native_id("property", "Shape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: None,
        order: 0,
    };
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(2));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut previous = Vec::new();
    for index in source.children(0) {
        let value = ctx
            .decode_part(
                &mut output,
                source.extent(index).unwrap(),
                source.spec(index, "freecad.property_value").unwrap(),
                |stage| {
                    crate::element_map::tests::adapters::decode_value(
                        &ctx,
                        &source,
                        index,
                        Some(&property),
                        &previous,
                        &context,
                        stage,
                    )
                },
            )
            .unwrap()
            .flatten();
        if let Some(value) = value {
            previous.push(value);
        }
    }
    output.prune_dependencies(&ctx).unwrap();
    let parts = output.fidelity().parts();
    let nodes = parts
        .values()
        .filter(|part| part.level().as_str() == "freecad.inline_map_node")
        .collect::<Vec<_>>();
    assert_eq!(
        nodes.len(),
        2,
        "the unvisited third node has no fabricated part"
    );
    assert_eq!(
        nodes
            .iter()
            .filter(|part| part.state() == &PartState::Decoded {})
            .count(),
        1
    );
    let failures = parts
        .iter()
        .filter(|(_, part)| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:?}", output.losses());
    assert_eq!(failures[0].1.level().as_str(), "freecad.inline_map_node");
    assert_eq!(
        output
            .fidelity()
            .retained_record(failures[0].0.as_str())
            .unwrap()
            .data(),
        Some(b"ElementMap 2 7 0\n".as_slice())
    );
    let native = output.native().namespace("fcstd").unwrap();
    assert!(native
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap()
        .is_empty());
    let unknowns = native
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    assert!(unknowns[0].links.is_empty());
    let id = crate::native::native_child_id("element-map", &property.id, "map");
    assert!(output.losses()[0].message.contains(&id));
    assert!(!ctx
        .take_record_captures()
        .unwrap()
        .iter()
        .any(|capture| capture.id() == id));
}
