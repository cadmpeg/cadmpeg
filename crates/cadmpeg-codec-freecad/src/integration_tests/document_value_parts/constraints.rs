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
            assert_eq!(attribute.values, [Some(expected.clone())]);
            assert_eq!(attribute.native_ref.as_deref(), Some(held.id.as_str()));
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
            panic!("one failed numeric constraint")
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
    }
}
