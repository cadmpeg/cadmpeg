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
        let properties = stage
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let original = crate::persistence::parse_document(
            &scan.document_xml,
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
        assert_eq!(
            neutral.values,
            [Some(AttributeValue::float(expected).unwrap())]
        );
        assert_eq!(neutral.native_ref.as_deref(), Some(held.id.as_str()));
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
            panic!("one failed QuantityConstraint child")
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
        assert_eq!(
            cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
                .unwrap()
                .open(&ctx, "Document.xml")
                .unwrap()
                .window(),
            xml.as_bytes()
        );
    }
}
