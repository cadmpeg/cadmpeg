// SPDX-License-Identifier: Apache-2.0
//! Quantity Float values use their original source child and existing native grammar.

use super::*;

#[test]
fn quantity_scalar_changes_flow_to_native_and_neutral_values_with_exact_original_reads() {
    for (family, unit, raw_unit) in [
        ("App::PropertyLength", "mm", "m&#109;"),
        ("App::PropertyAngle", "deg", "d&#101;g"),
        ("App::PropertyDistance", "mm", "m&#109;"),
        ("App::PropertyQuantity", "N", "&#78;"),
    ] {
        let mut previous = None;
        for (raw, text, expected) in [("-12.5", "-12.5", -12.5), ("4&#x2e;25", "4.25", 4.25)] {
            let child = format!("<Float value='{raw}' unit='{raw_unit}' extra='held'/>");
            let property_xml = format!("<Property name='Scaled' type='{family}' status='9' group='Custom' doc='A&amp;B' ro='true'>{child}</Property>");
            let xml = document(&format!("{property_xml}<Property name='Peer' type='App::PropertyString'><String value='independent'/></Property>"));
            let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
            let prefix = b"original prefix outside archive";
            let mut bytes = prefix.to_vec();
            bytes.extend(&archive);
            bytes.extend(b"original suffix outside archive");
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
            let source = root
                .child(prefix.len(), prefix.len() + archive.len())
                .unwrap();
            assert!(source.start() > 0);
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
                .find(|property| property.name == "Scaled")
                .unwrap();
            assert_eq!(held.family, crate::native::PropertyFamily::Quantity);
            assert_eq!(held.type_name, family);
            assert_eq!(held.status, Some(9));
            let dynamic = held.dynamic.as_ref().unwrap();
            assert_eq!(dynamic.group, "Custom");
            assert_eq!(dynamic.documentation.as_deref(), Some("A&B"));
            assert_eq!(dynamic.read_only, Some(true));
            assert_eq!(held.values().len(), 1);
            let fields = &held.values()[0].attributes;
            assert_eq!(fields["value"], text);
            assert_eq!(fields["unit"], unit);
            assert_eq!(fields["extra"], "held");
            if let Some(previous) = previous.replace(held.clone()) {
                assert_ne!(held, &previous);
            }
            let neutral = stage
                .model()
                .attributes
                .iter()
                .find(|attribute| attribute.name == "Scaled")
                .unwrap();
            assert_eq!(
                neutral.values,
                [Some(AttributeValue::float(expected).unwrap())]
            );
            assert_eq!(neutral.native_ref.as_deref(), Some(held.id.as_str()));
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
            assert_eq!(part.stream(), extent.stream());
            assert_eq!(part.state(), &PartState::Decoded {});
            assert_eq!(part.byte_len(), u64::try_from(child.len()).unwrap());
            let parent = &stage.fidelity().parts()[part.parent().unwrap()];
            assert_eq!(parent.level().as_str(), "freecad.property");
            assert_eq!(parent.state(), &PartState::Decoded {});
            let captures = ctx.take_record_captures().unwrap();
            let capture = captures
                .iter()
                .find(|capture| capture.id() == held.id)
                .unwrap();
            let mut expected_reads = Vec::new();
            let mut from = 0;
            for raw in [
                "Scaled", family, "9", "Custom", "A&amp;B", "true", "value", raw, "unit", raw_unit,
                "extra", "held",
            ] {
                let at = from + property_xml[from..].find(raw).unwrap();
                from = at + raw.len();
                let start =
                    extent.offset() + u64::try_from(xml.find(&property_xml).unwrap() + at).unwrap();
                expected_reads.push(start..start + u64::try_from(raw.len()).unwrap());
            }
            assert_eq!(capture.reads()[capture.stream()], expected_reads);
        }
    }
}

#[test]
fn malformed_quantity_values_keep_headers_and_peers_with_one_exact_child_home() {
    for family in [
        "App::PropertyLength",
        "App::PropertyAngle",
        "App::PropertyDistance",
        "App::PropertyQuantity",
    ] {
        for bad in [
            "<Float value='NaN' unit='kept-only-if-readable'/>",
            "<Float unit='kept-only-if-readable'/>",
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
            assert_eq!(property.type_name, family);
            assert_eq!(property.family, crate::native::PropertyFamily::Quantity);
            assert_eq!(property.status, Some(7));
            assert_eq!(property.dynamic.as_ref().unwrap().group, "Custom");
            assert_eq!(
                property.dynamic.as_ref().unwrap().documentation.as_deref(),
                Some("A&B")
            );
            assert!(property.body.is_none());
            let peer = decoded
                .ir()
                .model
                .attributes
                .iter()
                .find(|attribute| attribute.name == "Peer")
                .unwrap();
            assert_eq!(
                peer.values,
                [Some(AttributeValue::String("independent".into()))]
            );
            assert!(!decoded
                .ir()
                .model
                .attributes
                .iter()
                .any(|attribute| attribute.name == "Bad"));
            let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
            let [unknown] = unknowns.as_slice() else {
                panic!("one failed quantity value")
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
            let original =
                crate::source_archive::OriginalArchive::new(&ctx, root, fidelity).unwrap();
            assert_eq!(
                original.open(&ctx, "Document.xml").unwrap().window(),
                xml.as_bytes()
            );
            let mut shifted = serde_json::to_value(fidelity).unwrap();
            for field in ["parts", "record_layout", "retained_records"] {
                shifted[field][unknown.id.as_str()]["offset"] =
                    serde_json::json!(part.offset() + 1);
            }
            for range in shifted["parts"][unknown.id.as_str()]["framing_bytes"]
                .as_array_mut()
                .unwrap()
            {
                range["start"] = serde_json::json!(range["start"].as_u64().unwrap() + 1);
                range["end"] = serde_json::json!(range["end"].as_u64().unwrap() + 1);
            }
            let shifted: cadmpeg_ir::SourceFidelity = serde_json::from_value(shifted).unwrap();
            assert!(crate::source_archive::OriginalArchive::new(&ctx, root, &shifted).is_err());
        }
    }
}
