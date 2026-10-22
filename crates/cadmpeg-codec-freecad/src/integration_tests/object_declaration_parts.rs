// SPDX-License-Identifier: Apache-2.0
//! Application object births belong to their actual declaration and value scopes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn document(schema: u32, declarations: &str, data: &str) -> String {
    let (objects, values, _) = if schema == 2 {
        ("Features", "FeatureData", "Feature")
    } else {
        ("Objects", "ObjectData", "Object")
    };
    format!("<Document SchemaVersion='{schema}' FileVersion='1'><Properties Count='0'/><{objects} Count='2'>{declarations}</{objects}><{values} Count='2'>{data}</{values}></Document>")
}

#[test]
fn native_objects_and_owned_values_are_born_in_original_parts_and_reused_without_second_captures() {
    for schema in [2, 4] {
        let tag = if schema == 2 { "Feature" } else { "Object" };
        let declaration = format!("<{tag} name='A&amp;B' type='Vendor::Feature'/>");
        let peer_declaration = format!("<{tag} name='Peer' type='Vendor::Feature'/>");
        let child = "<Float value='3&#x2e;5' unit='mm' extra='held'/>";
        let property = format!(
            "<Property name='Length' type='App::PropertyLength' group='Custom'>{child}</Property>"
        );
        let data =
            format!("<{tag} name='A&#38;B'><Properties Count='1'>{property}</Properties></{tag}>");
        let peer_data = format!("<{tag} name='Peer'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='independent'/></Property></Properties></{tag}>");
        let xml = document(
            schema,
            &format!("{declaration}{peer_declaration}"),
            &format!("{data}{peer_data}"),
        );
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let prefix = b"outside original source archive";
        let mut bytes = prefix.to_vec();
        bytes.extend(&archive);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root
            .child(prefix.len(), prefix.len() + archive.len())
            .unwrap();
        let (scan, stage) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let member = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
        let staged = stage
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        assert_eq!(staged.len(), 2);
        let object = staged.iter().find(|object| object.name() == "A&B").unwrap();
        assert_eq!(object.type_name, "Vendor::Feature");
        let graph = crate::persistence::parse_document_source_with_properties(
            &ctx,
            scan.data["Document.xml"],
            &scan.document_xml,
            crate::persistence::Vocabulary::from_declaration(&scan.schema_version).unwrap(),
            scan.document_properties.as_ref(),
            scan.object_declarations.as_ref(),
        )
        .unwrap();
        let pure = crate::persistence::parse_document(
            &scan.document_xml,
            crate::persistence::Vocabulary::from_declaration(&scan.schema_version).unwrap(),
        )
        .unwrap();
        assert_eq!(graph.objects, pure.objects);
        assert_eq!(graph.properties, pure.properties);
        assert_eq!(graph.object_sources, pure.object_sources);
        assert_eq!(
            graph.property_sources.keys().collect::<Vec<_>>(),
            pure.property_sources.keys().collect::<Vec<_>>()
        );
        let held = graph
            .properties
            .iter()
            .find(|property| property.owner == object.id().as_str() && property.name == "Length")
            .unwrap();
        assert_eq!(held.values()[0].attributes["value"], "3.5");
        assert_eq!(held.values()[0].attributes["unit"], "mm");
        assert_eq!(held.values()[0].attributes["extra"], "held");
        assert!(stage.model().attributes.is_empty());
        let part = stage
            .fidelity()
            .parts()
            .values()
            .find(|part| {
                part.level().as_str() == "freecad.object_declaration"
                    && part.offset()
                        == member.offset() + u64::try_from(xml.find(&declaration).unwrap()).unwrap()
            })
            .unwrap();
        assert_eq!(part.state(), &PartState::Decoded {});
        assert_eq!(part.byte_len(), u64::try_from(declaration.len()).unwrap());
        let parent = &stage.fidelity().parts()[part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "freecad.objects");
        assert_eq!(
            stage.fidelity().parts()[parent.parent().unwrap()]
                .level()
                .as_str(),
            "freecad.document"
        );
        let value_part = stage
            .fidelity()
            .parts()
            .values()
            .find(|part| {
                part.level().as_str() == "freecad.property_value"
                    && part.offset()
                        == member.offset() + u64::try_from(xml.find(child).unwrap()).unwrap()
            })
            .unwrap();
        let property_part = &stage.fidelity().parts()[value_part.parent().unwrap()];
        assert_eq!(property_part.level().as_str(), "freecad.property");
        let properties_part = &stage.fidelity().parts()[property_part.parent().unwrap()];
        assert_eq!(properties_part.level().as_str(), "freecad.properties");
        assert_eq!(
            stage.fidelity().parts()[properties_part.parent().unwrap()]
                .level()
                .as_str(),
            "freecad.document"
        );
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 5);
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.id() == object.id())
                .count(),
            1
        );
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.id() == held.id)
                .count(),
            1
        );
        let capture = captures
            .iter()
            .find(|capture| capture.id() == object.id())
            .unwrap();
        assert_eq!(
            capture.offset(),
            member.offset() + u64::try_from(xml.find(&declaration).unwrap()).unwrap()
        );
        assert_eq!(
            capture.byte_len(),
            u64::try_from(declaration.len()).unwrap()
        );
        let mut expected = Vec::new();
        for (source, raw) in [
            (&declaration, "A&amp;B"),
            (&declaration, "Vendor::Feature"),
            (&data, "A&#38;B"),
        ] {
            let start = member.offset()
                + u64::try_from(xml.find(source).unwrap() + source.find(raw).unwrap()).unwrap();
            expected.push(start..start + u64::try_from(raw.len()).unwrap());
        }
        assert_eq!(capture.reads()[capture.stream()], expected);
        assert!(expected[2].start >= capture.offset() + capture.byte_len());
    }
}

#[test]
fn bad_owned_value_keeps_the_real_object_header_and_linked_property_with_an_exact_peer() {
    let bad = "<Float value='NaN'/>";
    let xml = document(4, "<Object name='Bad' type='Vendor::Feature'/><Object name='Peer' type='Vendor::Feature'/>", &format!("<Object name='Bad'><Properties Count='1'><Property name='Length' type='App::PropertyLength' status='7' group='Custom'>{bad}</Property></Properties></Object><Object name='Peer'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='independent'/></Property></Properties></Object>"));
    let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert_eq!(objects.len(), 2);
    let object = objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.owner == object.id().as_str() && property.name == "Length")
        .unwrap();
    assert!(property.body.is_none());
    assert_eq!(property.status, Some(7));
    assert_eq!(property.dynamic.as_ref().unwrap().group, "Custom");
    let peer = properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("object", "Peer") && property.name == "Label"
        })
        .unwrap();
    assert_eq!(peer.values()[0].attributes["value"], "independent");
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(object.id().as_str()))
        .unwrap();
    let cadmpeg_ir::features::FeatureOperation::Native { kind, parameters } =
        feature.evaluation.definition().operation()
    else {
        panic!("vendor object retains its native operation")
    };
    assert_eq!(kind.as_str(), "Vendor::Feature");
    assert!(!parameters.keys().any(|name| name.as_str() == "Length"));
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed owned value")
    };
    assert_eq!(unknown.links.len(), 1);
    assert_eq!(unknown.links[0].as_str(), property.id);
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "freecad.property_value");
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(bad.as_bytes())
    );
    assert!(fidelity.record_layout()[property.id.as_str()]
        .payloads()
        .is_empty());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::source_archive::OriginalArchive::new(&ctx, root, fidelity)
            .unwrap()
            .open(&ctx, "Document.xml")
            .unwrap()
            .window(),
        xml.as_bytes()
    );
}

#[test]
fn a_failed_declaration_has_its_original_source_home_and_keeps_an_independent_native_object() {
    let bad = "<Object name='Bad'/>";
    let xml = document(4, &format!("{bad}<Object name='Peer' type='Vendor::Feature'/>"), "<Object name='Bad'><Properties Count='0'/></Object><Object name='Peer'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='independent'/></Property></Properties></Object>");
    let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let objects = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name(), "Peer");
    assert_eq!(objects[0].type_name, "Vendor::Feature");
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    assert_eq!(unknowns.len(), 2);
    let fidelity = decoded.source_fidelity();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let member = archive.open(&ctx, "Document.xml").unwrap();
    let extent = ctx.capture_part_extent(member).unwrap();
    for (level, raw, parent) in [
        ("freecad.object_declaration", bad, "freecad.objects"),
        (
            "freecad.object_data",
            "<Object name='Bad'><Properties Count='0'/></Object>",
            "freecad.object_data_section",
        ),
    ] {
        let expected_offset = extent.offset() + u64::try_from(xml.find(raw).unwrap()).unwrap();
        let part = fidelity
            .parts()
            .values()
            .find(|part| part.level().as_str() == level && part.offset() == expected_offset)
            .unwrap();
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.stream(), extent.stream());
        assert_eq!(
            part.offset(),
            extent.offset() + u64::try_from(xml.find(raw).unwrap()).unwrap()
        );
        assert_eq!(part.byte_len(), u64::try_from(raw.len()).unwrap());
        assert_eq!(
            fidelity.parts()[part.parent().unwrap()].level().as_str(),
            parent
        );
        assert_eq!(
            fidelity.parts()[part.parent().unwrap()].state(),
            &PartState::Decoded {}
        );
        let unknown = unknowns
            .iter()
            .find(|unknown| fidelity.parts()[&unknown.id].level().as_str() == level)
            .unwrap();
        assert_eq!(
            fidelity
                .retained_record(unknown.id.as_str())
                .unwrap()
                .data(),
            Some(raw.as_bytes())
        );
    }
    cadmpeg_test_support::contract::parts::check_one_home(
        cadmpeg_ir::CodecFormat::Fcstd,
        &decoded.ir().native,
        fidelity,
        &decoded.report().losses,
        "file",
        |stream, range| {
            let start = usize::try_from(range.start).ok()?;
            let end = usize::try_from(range.end).ok()?;
            if stream == "file" {
                bytes.get(start..end)
            } else if stream == extent.stream() {
                member.child(start, end).map(|view| view.window())
            } else {
                None
            }
        },
    )
    .unwrap();
    assert_eq!(
        crate::source_archive::OriginalArchive::new(&ctx, root, fidelity)
            .unwrap()
            .open(&ctx, "Document.xml")
            .unwrap()
            .window(),
        xml.as_bytes()
    );
}

#[test]
fn unsupported_object_header_metadata_keeps_the_original_generic_property_grammar() {
    let xml = document(4, "<Object name='A' type='Vendor::Feature' id='7'/><Object name='Peer' type='Vendor::Feature'/>", "<Object name='A'><Properties Count='1'><Property name='Length' type='App::PropertyLength'><Float value='NaN'/></Property></Properties></Object><Object name='Peer'><Properties Count='0'/></Object>");
    let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (scan, stage) = crate::container::scan_for_decode(&ctx, root).unwrap();
    assert!(scan.object_declarations.is_none());
    assert!(!stage.fidelity().parts().values().any(|part| matches!(
        part.level().as_str(),
        "freecad.objects" | "freecad.object_declaration"
    )));
    let graph = crate::persistence::parse_document_source_with_properties(
        &ctx,
        scan.data["Document.xml"],
        &scan.document_xml,
        crate::persistence::Vocabulary::Objects,
        scan.document_properties.as_ref(),
        scan.object_declarations.as_ref(),
    )
    .unwrap();
    let pure = crate::persistence::parse_document(
        &scan.document_xml,
        crate::persistence::Vocabulary::Objects,
    )
    .unwrap();
    assert_eq!(graph.objects, pure.objects);
    assert_eq!(graph.properties, pure.properties);
    assert_eq!(graph.objects[0].persistent_id, Some(7));
    assert!(matches!(
        graph.properties[0].body,
        Some(crate::native::PropertyBody::Unreadable(_))
    ));
}

#[test]
fn archive_failure_rolls_back_the_real_object_and_owned_property_births_together() {
    let xml = document(4, "<Object name='A' type='Vendor::Feature'/><Object name='Peer' type='Vendor::Feature'/>", "<Object name='A'><Properties Count='1'><Property name='Length' type='App::PropertyLength'><Float value='3.5'/></Property></Properties></Object><Object name='Peer'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='independent'/></Property></Properties></Object>");
    let mut bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", xml.as_bytes()),
        RawEntry::stored("broken.bin", b"x"),
    ]);
    let offset = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        usize::try_from(
            cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
                .unwrap()
                .entry("broken.bin")
                .unwrap()
                .header_start,
        )
        .unwrap()
    };
    cadmpeg_test_support::bytes::put_u16(&mut bytes, offset + 26, u16::MAX);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap()
        .is_empty());
    assert!(namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    assert!(decoded.ir().model.features.is_empty());
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed archive parent")
    };
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity.parts()[&unknown.id].level().as_str(),
        "zip.archive"
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "freecad.object_declaration"
                && matches!(part.state(), PartState::Covered { by } if by == &unknown.id))
            .count(),
        2
    );
    for name in ["A", "Peer"] {
        assert!(!fidelity
            .record_layout()
            .contains_key(&crate::native::native_id("object", name)));
    }
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
}

#[test]
fn prepared_object_properties_preserve_exact_original_population_diagnostics_once() {
    for (counts, expected) in [
        ("Count='1'", Vec::<&str>::new()),
        (
            "Count='9'",
            vec!["Properties Count=Some(\"9\"); reading 1 framed elements"],
        ),
        (
            "Count='1' TransientCount='7'",
            vec!["Properties TransientCount=Some(\"7\"); reading 0 framed elements"],
        ),
        (
            "Count='9' TransientCount='7'",
            vec![
                "Properties Count=Some(\"9\"); reading 1 framed elements",
                "Properties TransientCount=Some(\"7\"); reading 0 framed elements",
            ],
        ),
    ] {
        let property =
            "<Property name='Label' type='App::PropertyString'><String value='held'/></Property>";
        let xml = document(
            4,
            "<Object name='Body' type='Vendor::Feature'/><Object name='Peer' type='Vendor::Feature'/>",
            &format!("<Object name='Body'><Properties {counts}>{property}</Properties></Object><Object name='Peer'><Properties Count='0'/></Object>"),
        );
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::service()).unwrap();
        let (scan, stage) = crate::container::scan_for_decode(&ctx, root).unwrap();
        assert!(scan.object_declarations.is_some());
        let schema =
            crate::persistence::Vocabulary::from_declaration(&scan.schema_version).unwrap();
        let graph = crate::persistence::parse_document_source_with_properties(
            &ctx,
            scan.data["Document.xml"],
            &scan.document_xml,
            schema,
            scan.document_properties.as_ref(),
            scan.object_declarations.as_ref(),
        )
        .unwrap();
        let original = crate::persistence::parse_document(&scan.document_xml, schema).unwrap();
        assert_eq!(graph.objects, original.objects);
        assert_eq!(graph.properties, original.properties);
        let warnings = |losses: &[cadmpeg_ir::report::loss::LossNote]| {
            losses
                .iter()
                .filter(|loss| {
                    loss.code
                        == crate::loss::FreecadLossCode::PersistenceCountNoncanonical
                            .note("")
                            .code
                })
                .map(|loss| loss.message.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(warnings(&original.losses), expected);
        assert_eq!(warnings(&graph.losses), expected);
        assert!(stage.model().attributes.is_empty());
        let label = graph
            .properties
            .iter()
            .find(|property| property.name == "Label")
            .unwrap();
        assert_eq!(label.values()[0].attributes["value"], "held");
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.id() == label.id)
                .count(),
            1
        );
    }
}
