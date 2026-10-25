// SPDX-License-Identifier: Apache-2.0
//! Every selected property family uses the same genuine child-value producer.

use super::*;

#[test]
fn selected_families_keep_exact_native_neutral_and_read_values_in_real_child_scopes() {
    let cases = [
        (
            "App::PropertyString",
            "<String value='A&amp;B' extra='native-only'/>",
            AttributeValue::String("A&B".into()),
            vec!["value", "A&amp;B", "extra", "native-only"],
        ),
        (
            "App::PropertyInteger",
            "<Integer value='-&#55;' extra='native-only'/>",
            AttributeValue::Integer(-7),
            vec!["value", "-&#55;", "extra", "native-only"],
        ),
        (
            "App::PropertyIntegerConstraint",
            "<Integer value='8' extra='native-only'/>",
            AttributeValue::Integer(8),
            vec!["value", "8", "extra", "native-only"],
        ),
        (
            "App::PropertyBool",
            "<Bool value='TrUe' extra='native-only'/>",
            AttributeValue::Boolean(true),
            vec!["value", "TrUe", "extra", "native-only"],
        ),
        (
            "App::PropertyFloat",
            "<Float value='1&#x2e;25' extra='native-only'/>",
            AttributeValue::float(1.25).unwrap(),
            vec!["value", "1&#x2e;25", "extra", "native-only"],
        ),
        (
            "App::PropertyVector",
            "<PropertyVector valueX='1&#x2e;5' valueY='-2' valueZ='3' extra='native-only'/>",
            AttributeValue::vector([1.5, -2.0, 3.0]).unwrap(),
            vec![
                "valueX",
                "1&#x2e;5",
                "valueY",
                "-2",
                "valueZ",
                "3",
                "extra",
                "native-only",
            ],
        ),
    ];
    for (family, child, expected, value_reads) in cases {
        let property = format!("<Property name='Input' type='{family}' status='9' group='Custom' doc='kept' Vendor='unheld'>{child}</Property>");
        let xml = document(&format!("{property}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let prefix = b"outside original archive";
        let mut bytes = prefix.to_vec();
        bytes.extend(&archive);
        bytes.extend(b"outside suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root
            .child(prefix.len(), prefix.len() + archive.len())
            .unwrap();
        assert!(source.start() > 0);
        let (scan, output) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let member = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
        let properties = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let pure = crate::persistence::parse_document(
            &crate::container::admit_document(&ctx, scan.data["Document.xml"].window())
                .expect("valid source XML"),
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        assert_eq!(properties, pure.properties);
        let held = properties
            .iter()
            .find(|property| property.name == "Input")
            .unwrap();
        assert_eq!(held.type_name, family);
        assert_eq!(held.status, Some(9));
        assert_eq!(held.dynamic.as_ref().unwrap().group, "Custom");
        assert_eq!(
            held.dynamic.as_ref().unwrap().documentation.as_deref(),
            Some("kept")
        );
        assert_eq!(held.values()[0].attributes["extra"], "native-only");
        let attribute = output
            .model()
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Input")
            .unwrap();
        assert_eq!(attribute.values, [expected]);
        assert_eq!(attribute.native_ref.as_deref(), Some(held.id.as_str()));
        let value_parts = output
            .fidelity()
            .parts()
            .iter()
            .filter(|(_, part)| part.level().as_str() == "freecad.property_value")
            .collect::<Vec<_>>();
        assert_eq!(value_parts.len(), 2);
        assert!(value_parts
            .iter()
            .all(|(_, part)| part.state() == &PartState::Decoded {}));
        let child_offset = member.offset() + u64::try_from(xml.find(child).unwrap()).unwrap();
        let child_part = value_parts
            .iter()
            .map(|(_, part)| *part)
            .find(|part| part.offset() == child_offset)
            .unwrap();
        assert_eq!(child_part.stream(), member.stream());
        assert_eq!(child_part.byte_len(), u64::try_from(child.len()).unwrap());
        let parent = &output.fidelity().parts()[child_part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "freecad.property");
        assert_eq!(
            parent.offset(),
            member.offset() + u64::try_from(xml.find(&property).unwrap()).unwrap()
        );
        assert_eq!(parent.byte_len(), u64::try_from(property.len()).unwrap());
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == held.id)
            .unwrap();
        let mut expected_reads = Vec::new();
        let mut from = 0;
        let tag = child
            .strip_prefix('<')
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        for raw in [
            "name", "Input", "type", family, "status", "9", "group", "Custom", "doc", "kept",
        ]
        .into_iter()
        .chain(std::iter::once(tag))
        .chain(value_reads)
        {
            let at = from + property[from..].find(raw).unwrap();
            from = at + raw.len();
            let start = member.offset() + u64::try_from(xml.find(&property).unwrap() + at).unwrap();
            expected_reads.push(start..start + u64::try_from(raw.len()).unwrap());
        }
        assert_eq!(capture.reads()[capture.stream()], expected_reads);
        let unheld = member.offset() + u64::try_from(xml.find("unheld").unwrap()).unwrap();
        assert!(capture.reads()[capture.stream()]
            .iter()
            .all(|range| range.end <= unheld || unheld + 6 <= range.start));
    }
}

#[test]
fn failed_selected_scalar_children_keep_headers_and_have_one_exact_source_home() {
    for (family, bad) in [
        (
            "App::PropertyInteger",
            "<Integer value='9223372036854775808' extra='not-held'/>",
        ),
        (
            "App::PropertyBool",
            "<Bool value='unknown' extra='not-held'/>",
        ),
        (
            "App::PropertyFloat",
            "<Float value='NaN' extra='not-held'/>",
        ),
    ] {
        let header = format!("<Property name='Bad' type='{family}' status='7' group='Custom' doc='A&amp;B'>{bad}</Property>");
        let xml = document(&format!("{header}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
        let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        assert_eq!(properties.len(), 2);
        let property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert!(property.body.is_none());
        assert_eq!(property.type_name, family);
        assert_eq!(property.status, Some(7));
        assert_eq!(property.dynamic.as_ref().unwrap().group, "Custom");
        assert_eq!(
            property.dynamic.as_ref().unwrap().documentation.as_deref(),
            Some("A&B")
        );
        assert!(!decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Bad"));
        let peer = decoded
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Peer")
            .unwrap();
        assert_eq!(peer.values, [AttributeValue::String("independent".into())]);
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else {
            panic!("one failed selected value")
        };
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), property.id);
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property_value");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.byte_len(), u64::try_from(bad.len()).unwrap());
        assert_eq!(
            fidelity.parts()[part.parent().unwrap()].state(),
            &PartState::Decoded {}
        );
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
        let reads = &fidelity.record_layout()[property.id.as_str()].reads()[part.stream()];
        let at = u64::try_from(xml.find(bad).unwrap()).unwrap();
        assert!(reads
            .iter()
            .all(|range| range.end <= at || at + u64::try_from(bad.len()).unwrap() <= range.start));
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        let member = archive.open(&ctx, "Document.xml").unwrap();
        let extent = ctx.capture_part_extent(member).unwrap();
        assert_eq!(part.stream(), extent.stream());
        assert_eq!(part.offset(), extent.offset() + at);
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
                    return bytes.get(start..end);
                }
                if stream == extent.stream() {
                    return member.child(start, end).map(|view| view.window());
                }
                None
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
}
