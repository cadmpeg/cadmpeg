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
        ),
        (
            "App::PropertyInteger",
            "<Integer value='-&#55;' extra='native-only'/>",
            AttributeValue::Integer(-7),
        ),
        (
            "App::PropertyIntegerConstraint",
            "<Integer value='8' extra='native-only'/>",
            AttributeValue::Integer(8),
        ),
        (
            "App::PropertyBool",
            "<Bool value='TrUe' extra='native-only'/>",
            AttributeValue::Boolean(true),
        ),
        (
            "App::PropertyFloat",
            "<Float value='1&#x2e;25' extra='native-only'/>",
            AttributeValue::float(1.25).unwrap(),
        ),
        (
            "App::PropertyVector",
            "<PropertyVector valueX='1&#x2e;5' valueY='-2' valueZ='3' extra='native-only'/>",
            AttributeValue::vector([1.5, -2.0, 3.0]).unwrap(),
        ),
    ];
    for (family, child, expected) in cases {
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
        let properties = output
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
        assert_eq!(attribute.values, [Some(expected)]);
        assert_eq!(attribute.native_ref.as_deref(), Some(held.id.as_str()));
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
        assert_eq!(
            peer.values,
            [Some(AttributeValue::String("independent".into()))]
        );
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::source_fidelity::given_up(fidelity);
        let [(_, retained)] = parts.as_slice() else {
            panic!("one failed selected value")
        };
        assert_eq!(retained.byte_len(), u64::try_from(bad.len()).unwrap());
        assert_eq!(retained.data(), Some(bad.as_bytes()));
        assert_eq!(
            retained.offset(),
            u64::try_from(xml.find(bad).unwrap()).unwrap()
        );
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
