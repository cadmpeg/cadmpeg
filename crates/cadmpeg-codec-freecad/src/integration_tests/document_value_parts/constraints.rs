// SPDX-License-Identifier: Apache-2.0
//! Numeric constraint values are held by their actual child transaction.

use super::*;

#[test]
fn constraint_bounds_and_scalars_keep_exact_native_facts_and_child_source_reads() {
    for (family, tag, value, minimum, changed, max, step, expected) in [
        (
            "App::PropertyFloatConstraint",
            "Float",
            "1.5",
            "3&#x2e;5",
            "-9.25",
            "-2.25",
            "-0.125",
            AttributeValue::float(1.5).unwrap(),
        ),
        (
            "App::PropertyPercent",
            "Integer",
            "-7",
            "&#50;0",
            "41",
            "-2",
            "-1",
            AttributeValue::Integer(-7),
        ),
    ] {
        let mut previous = None;
        for min in [minimum, changed] {
            let child = format!(
                "<{tag} value='{value}' min='{min}' max='{max}' step='{step}' Vendor='held'/>"
            );
            let property = format!("<Property name='Constrained' type='{family}' status='9' group='Custom'>{child}</Property>");
            let xml = document(&format!("{property}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
            let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
            let prefix = b"actual outside archive prefix";
            let mut bytes = prefix.to_vec();
            bytes.extend(&archive);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
            let source = root
                .child(prefix.len(), prefix.len() + archive.len())
                .unwrap();
            let (scan, stage) = crate::container::scan_for_decode(&ctx, source).unwrap();
            let extent = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
            let properties = stage
                .native()
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap();
            let pure = crate::persistence::parse_document(
                &scan.document_xml,
                crate::persistence::Vocabulary::Objects,
            )
            .unwrap();
            assert_eq!(properties, pure.properties);
            let held = properties
                .iter()
                .find(|property| property.name == "Constrained")
                .unwrap();
            let fields = &held.values()[0].attributes;
            assert_eq!(fields["value"], value);
            assert_eq!(
                fields["min"],
                if min == "3&#x2e;5" {
                    "3.5"
                } else if min == "&#50;0" {
                    "20"
                } else {
                    min
                }
            );
            assert_eq!(fields["max"], max);
            assert_eq!(fields["step"], step);
            assert_eq!(fields["Vendor"], "held");
            if let Some(previous) = previous.replace(held.clone()) {
                assert_ne!(held, &previous);
            }
            let attribute = stage
                .model()
                .attributes
                .iter()
                .find(|attribute| attribute.name == "Constrained")
                .unwrap();
            assert_eq!(attribute.values, [expected.clone()]);
            assert_eq!(attribute.native_ref.as_deref(), Some(held.id.as_str()));
            let part = stage
                .fidelity()
                .parts()
                .values()
                .find(|part| {
                    part.level().as_str() == "freecad.property_value"
                        && part.offset()
                            == extent.offset() + u64::try_from(xml.find(&child).unwrap()).unwrap()
                })
                .unwrap();
            assert_eq!(part.state(), &PartState::Decoded {});
            assert_eq!(part.stream(), extent.stream());
            assert_eq!(part.byte_len(), u64::try_from(child.len()).unwrap());
            let parent = &stage.fidelity().parts()[part.parent().unwrap()];
            assert_eq!(parent.level().as_str(), "freecad.property");
            assert_eq!(parent.state(), &PartState::Decoded {});
            let captures = ctx.take_record_captures().unwrap();
            let capture = captures
                .iter()
                .find(|capture| capture.id() == held.id)
                .unwrap();
            let mut from = 0;
            let mut expected_reads = Vec::new();
            for raw in [
                "Constrained",
                family,
                "9",
                "Custom",
                "value",
                value,
                "min",
                min,
                "max",
                max,
                "step",
                step,
                "Vendor",
                "held",
            ] {
                let at = from + property[from..].find(raw).unwrap();
                from = at + raw.len();
                let start =
                    extent.offset() + u64::try_from(xml.find(&property).unwrap() + at).unwrap();
                expected_reads.push(start..start + u64::try_from(raw.len()).unwrap());
            }
            assert_eq!(capture.reads()[capture.stream()], expected_reads);
        }
    }
}

#[test]
fn malformed_constraint_values_keep_header_and_peer_with_exact_one_home_and_no_failed_reads() {
    for (family, bad) in [
        (
            "App::PropertyFloatConstraint",
            "<Float value='NaN' min='1' max='9' step='0.5'/>",
        ),
        (
            "App::PropertyFloatConstraint",
            "<Float min='1' max='9' step='0.5'/>",
        ),
        (
            "App::PropertyPercent",
            "<Integer value='9223372036854775808' min='0' max='100' step='1'/>",
        ),
        (
            "App::PropertyPercent",
            "<Integer min='0' max='100' step='1'/>",
        ),
    ] {
        let property_xml = format!("<Property name='Bad' type='{family}' status='7' group='Custom' doc='kept'>{bad}</Property>");
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
        assert_eq!(properties.len(), 2);
        let property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert_eq!(property.type_name, family);
        assert_eq!(property.status, Some(7));
        assert_eq!(property.dynamic.as_ref().unwrap().group, "Custom");
        assert_eq!(
            property.dynamic.as_ref().unwrap().documentation.as_deref(),
            Some("kept")
        );
        assert!(property.body.is_none());
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
            panic!("one failed numeric constraint")
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
    }
}
