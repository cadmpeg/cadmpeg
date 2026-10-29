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
            let fidelity = decoded.source_fidelity();
            let parts = crate::test_support::source_fidelity::given_up(fidelity);
            let [(_, retained)] = parts.as_slice() else {
                panic!("one failed quantity value")
            };
            assert_eq!(retained.data(), Some(bad.as_bytes()));
            assert_eq!(
                retained.offset(),
                u64::try_from(xml.find(bad).unwrap()).unwrap()
            );
            assert_eq!(retained.byte_len(), u64::try_from(bad.len()).unwrap());
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
            let original = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
            assert_eq!(
                original.open(&ctx, "Document.xml").unwrap().window(),
                xml.as_bytes()
            );
        }
    }
}
