// SPDX-License-Identifier: Apache-2.0
//! Quantity constraints keep bounds and units as authored native facts.

use super::*;

#[test]
fn quantity_constraint_source_values_and_bounds_reach_their_actual_homes_with_exact_reads() {
    for (raw, text, expected, raw_unit, unit, min, max, step) in [
        (
            "-2&#x2e;5",
            "-2.5",
            -2.5,
            "m&#109;",
            "mm",
            "-10",
            "10",
            "0.25",
        ),
        ("3&#x2e;5", "3.5", 3.5, "c&#109;", "cm", "-20", "20", "0.5"),
    ] {
        let child = format!("<Float value='{raw}' unit='{raw_unit}' min='{min}' max='{max}' step='{step}' extra='held'/>");
        let property_xml = format!("<Property name='Constrained' type='App::PropertyQuantityConstraint' status='9' group='Custom' doc='A&amp;B' ro='true'>{child}</Property>");
        let xml = document(&format!("{property_xml}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let prefix = b"outside actual archive";
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
        let (scan, stage) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let member = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
        let properties = stage
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let original = crate::persistence::parse_document(
            &crate::container::admit_document(&ctx, scan.data["Document.xml"].window())
                .expect("valid source XML"),
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        assert_eq!(properties, original.properties);
        let held = properties
            .iter()
            .find(|property| property.name == "Constrained")
            .unwrap();
        assert_eq!(held.family, crate::native::PropertyFamily::Quantity);
        assert_eq!(held.status, Some(9));
        let dynamic = held.dynamic.as_ref().unwrap();
        assert_eq!(dynamic.group, "Custom");
        assert_eq!(dynamic.documentation.as_deref(), Some("A&B"));
        assert_eq!(dynamic.read_only, Some(true));
        let [value] = held.values() else {
            panic!("one actual Float body")
        };
        assert_eq!(value.tag, "Float");
        for (key, expected) in [
            ("value", text),
            ("unit", unit),
            ("min", min),
            ("max", max),
            ("step", step),
            ("extra", "held"),
        ] {
            assert_eq!(value.attributes[key], expected);
        }
        let neutral = stage
            .model()
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Constrained")
            .unwrap();
        // Neither unit nor constraint bounds manufacture a different selected value.
        assert_eq!(neutral.values, [AttributeValue::float(expected).unwrap()]);
        assert_eq!(neutral.native_ref.as_deref(), Some(held.id.as_str()));
        let start = member.offset() + u64::try_from(xml.find(&child).unwrap()).unwrap();
        let part = stage
            .fidelity()
            .parts()
            .values()
            .find(|part| {
                part.level().as_str() == "freecad.property_value" && part.offset() == start
            })
            .unwrap();
        assert_eq!(part.state(), &PartState::Decoded {});
        assert_eq!(part.stream(), member.stream());
        assert_eq!(part.byte_len(), u64::try_from(child.len()).unwrap());
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == held.id)
            .unwrap();
        let mut expected_reads = Vec::new();
        let mut from = 0;
        for raw in [
            "name",
            "Constrained",
            "type",
            "App::PropertyQuantityConstraint",
            "status",
            "9",
            "group",
            "Custom",
            "doc",
            "A&amp;B",
            "ro",
            "true",
            "Float",
            "value",
            raw,
            "unit",
            raw_unit,
            "min",
            min,
            "max",
            max,
            "step",
            step,
            "extra",
            "held",
        ] {
            let at = from + property_xml[from..].find(raw).unwrap();
            from = at + raw.len();
            let offset =
                member.offset() + u64::try_from(xml.find(&property_xml).unwrap() + at).unwrap();
            expected_reads.push(offset..offset + u64::try_from(raw.len()).unwrap());
        }
        assert_eq!(capture.reads()[capture.stream()], expected_reads);
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.id() == held.id)
                .count(),
            1
        );
    }
}

#[test]
fn invalid_quantity_constraint_value_keeps_header_and_peer_with_one_original_child_home() {
    for bad in [
        "<Float value='NaN' min='-10' max='10' step='0.25' unit='mm'/>",
        "<Float min='-10' max='10' step='0.25' unit='mm'/>",
    ] {
        let property_xml = format!("<Property name='Bad' type='App::PropertyQuantityConstraint' status='7' group='Custom' doc='A&amp;B'>{bad}</Property>");
        let xml = document(&format!("{property_xml}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
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
        let property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert_eq!(property.type_name, "App::PropertyQuantityConstraint");
        assert_eq!(property.family, crate::native::PropertyFamily::Quantity);
        assert_eq!(property.status, Some(7));
        assert_eq!(property.dynamic.as_ref().unwrap().group, "Custom");
        assert_eq!(
            property.dynamic.as_ref().unwrap().documentation.as_deref(),
            Some("A&B")
        );
        assert!(property.body.is_none());
        assert_eq!(properties.len(), 2);
        let peer = decoded
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Peer")
            .unwrap();
        assert_eq!(peer.values, [AttributeValue::String("independent".into())]);
        assert!(!decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Bad"));
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else {
            panic!("one failed QuantityConstraint child")
        };
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), property.id);
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property_value");
        assert_eq!(part.state(), &PartState::GivenUp {});
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
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        let member = archive.open(&ctx, "Document.xml").unwrap();
        let extent = ctx.capture_part_extent(member).unwrap();
        let start = extent.offset() + u64::try_from(xml.find(bad).unwrap()).unwrap();
        let end = start + u64::try_from(bad.len()).unwrap();
        assert_eq!(part.stream(), extent.stream());
        assert_eq!(part.offset(), start);
        assert_eq!(part.byte_len(), end - start);
        assert!(
            fidelity.record_layout()[property.id.as_str()].reads()[part.stream()]
                .iter()
                .all(|range| range.end <= start || end <= range.start)
        );
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
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
}
