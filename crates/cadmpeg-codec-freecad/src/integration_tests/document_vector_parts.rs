// SPDX-License-Identifier: Apache-2.0
//! Direct document vectors are produced from actual owned property source fields.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn document(properties: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='{count}'>{properties}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

fn archive(xml: &str) -> Vec<u8> {
    zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())])
}

#[test]
fn vector_native_and_neutral_values_come_from_exact_fields_in_a_nonzero_archive_window() {
    for (raw_x, x) in [("1&#x2e;5", 1.5), ("2.5", 2.5)] {
        let raw_name = "Vector&#x3a9;";
        let property = format!("<Property name='{raw_name}' type='App::PropertyVector' Vendor='unheld'><PropertyVector valueX='{raw_x}' valueY='-2' valueZ='3' extra='native-only'/></Property>");
        let xml = document(&property, 1);
        let zip = archive(&xml);
        let prefix = b"outside archive";
        let mut bytes = prefix.to_vec();
        bytes.extend(&zip);
        bytes.extend(b"outside suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(prefix.len(), prefix.len() + zip.len()).unwrap();
        assert!(source.start() > 0);
        let (scan, output) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let properties = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let [kept] = properties.as_slice() else {
            panic!("one native vector")
        };
        assert_eq!(kept.name, "VectorΩ");
        assert_eq!(kept.type_name, "App::PropertyVector");
        assert_eq!(kept.values()[0].attributes["valueX"], x.to_string());
        assert_eq!(kept.values()[0].attributes["valueY"], "-2");
        assert_eq!(kept.values()[0].attributes["valueZ"], "3");
        assert_eq!(kept.values()[0].attributes["extra"], "native-only");
        let pure = crate::persistence::parse_document(
            &scan.document_xml,
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        assert_eq!(properties, pure.properties);
        let [attribute] = output.model().attributes.as_slice() else {
            panic!("one live vector attribute")
        };
        assert_eq!(
            attribute.values,
            [Some(AttributeValue::vector([x, -2.0, 3.0]).unwrap())]
        );
        assert_eq!(attribute.native_ref.as_deref(), Some(kept.id.as_str()));
    }
}

#[test]
fn nameless_vector_is_given_up_without_changing_any_readable_vector_or_scalar_peer() {
    let peers = "<Property name='Vector' type='App::PropertyVector'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property><Property name='Integer' type='App::PropertyInteger'><Integer value='17'/></Property><Property name='Note' type='App::PropertyString'><String value='peer'/></Property>";
    let bad = "<Property type='App::PropertyVector'><PropertyVector valueX='4' valueY='5' valueZ='6'/></Property>";
    let baseline = crate::FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(peers, 3))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let xml = document(&format!("{peers}{bad}"), 4);
    let bytes = archive(&xml);
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
    assert_eq!(
        properties,
        baseline
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap()
    );
    assert_eq!(
        decoded.ir().model.attributes,
        baseline.ir().model.attributes
    );
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one nameless property")
    };
    assert_eq!(
        retained.offset(),
        u64::try_from(xml.find(bad).unwrap()).unwrap()
    );
    assert_eq!(retained.byte_len(), u64::try_from(bad.len()).unwrap());
    assert_eq!(retained.data(), Some(bad.as_bytes()));
    let errors = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(errors[0].message.contains("name"));
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(
            fidelity,
            &crate::native::native_id("entry", "Document.xml")
        ),
        xml.as_bytes()
    );
}

#[test]
fn invalid_vectors_give_up_the_value_and_valid_fallback_vectors_stay_native() {
    for (value, dynamic, projected) in [
        ("<PropertyVector valueX='1' valueY='2'/>", "", false),
        (
            "<PropertyVector valueX='NaN' valueY='2' valueZ='3'/>",
            "",
            false,
        ),
        (
            "<PropertyVector valueX='1' valueY='1e999' valueZ='3'/>",
            "",
            false,
        ),
        (
            "<PropertyVector valueX='1' valueY='2' valueZ='3'>retained text</PropertyVector>",
            "",
            true,
        ),
        (
            "<PropertyVector valueX='1' valueY='2' valueZ='3'/>",
            " group='Custom' ro='TrUe' hide='unknown'",
            true,
        ),
    ] {
        let xml = document(
            &format!(
                "<Property name='Vector' type='App::PropertyVector'{dynamic}>{value}</Property>"
            ),
            1,
        );
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure =
            crate::persistence::parse_document(&dom, crate::persistence::Vocabulary::Objects);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
            .unwrap();
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        if projected {
            assert_eq!(properties, pure.unwrap().properties);
            assert!(matches!(
                properties[0].body,
                Some(crate::native::PropertyBody::Persisted { .. })
            ));
            assert!(
                crate::test_support::source_fidelity::given_up(decoded.source_fidelity())
                    .is_empty()
            );
            assert_eq!(
                decoded
                    .ir()
                    .model
                    .attributes
                    .iter()
                    .find(|attribute| attribute.name == "Vector")
                    .unwrap()
                    .values,
                [Some(AttributeValue::vector([1.0, 2.0, 3.0]).unwrap())]
            );
        } else {
            assert!(pure.is_err());
            assert!(properties[0].body.is_none());
            assert!(!decoded
                .ir()
                .model
                .attributes
                .iter()
                .any(|attribute| attribute.name == "Vector"));
            let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].1.data(), Some(value.as_bytes()));
            assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "record.unreadable"));
        }
    }
}

#[test]
fn a_failed_sibling_entry_keeps_already_born_vector_fields_and_capture() {
    let xml = document("<Property name='Vector' type='App::PropertyVector'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property>", 1);
    let intact = crate::FcstdCodec
        .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
        .unwrap();
    let mut bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", xml.as_bytes()),
        RawEntry::stored("broken.bin", b"x"),
    ]);
    let header = {
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
    cadmpeg_test_support::bytes::put_u16(&mut bytes, header + 26, u16::MAX);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    // #215 §4.5: the unframeable sibling entry gives up only its own
    // part. The admitted vector matches the intact archive.
    assert_eq!(decoded.ir().model.attributes, intact.ir().model.attributes);
    assert!(!decoded.ir().model.attributes.is_empty());
    let (properties, intact_properties) = (
        decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap(),
        intact
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap(),
    );
    assert_eq!(properties, intact_properties);
    assert!(!properties.is_empty());
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one sibling entry failure")
    };
    assert_eq!(retained.offset(), u64::try_from(header).unwrap());
    assert_eq!(
        retained.byte_len(),
        u64::try_from(bytes.len() - header).unwrap()
    );
    assert_eq!(retained.data(), Some(&bytes[header..]));
}

#[test]
fn embedded_vector_list_projects_its_ordered_numeric_values() {
    let xml = document("<Property name='Vectors' type='App::PropertyVectorList'><VectorList file='vectors.bin'/></Property>", 1);
    let mut vectors = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f64, 2.0, 3.0, -4.0, 5.5, 6.0] {
        vectors.extend_from_slice(&value.to_le_bytes());
    }
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", xml.as_bytes()),
        RawEntry::stored("vectors.bin", &vectors),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let attribute = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Vectors")
        .expect("vector list attribute");
    assert_eq!(
        attribute.values,
        [
            Some(AttributeValue::vector([1.0, 2.0, 3.0]).unwrap()),
            Some(AttributeValue::vector([-4.0, 5.5, 6.0]).unwrap()),
        ]
    );
    assert!(decoded.source_fidelity().unrepresented().is_empty());
}

#[test]
fn unreadable_vector_list_gives_up_its_side_member_and_keeps_the_header() {
    let xml = document("<Property name='Vectors' type='App::PropertyVectorList'><VectorList file='vectors.bin'/></Property><Property name='Peer' type='App::PropertyString'><String value='held'/></Property>", 2);
    let bad = [2_u8, 0, 0, 0, 1, 2, 3];
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", xml.as_bytes()),
        RawEntry::stored("vectors.bin", &bad),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
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
        .find(|property| property.name == "Vectors")
        .unwrap();
    assert_eq!(property.type_name, "App::PropertyVectorList");
    assert!(property.body.is_none());
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Peer"));
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(stub, bytes)] = parts.as_slice() else {
        panic!("one unreadable side member")
    };
    assert_eq!(bytes.data(), Some(bad.as_slice()));
    let unknowns = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    let unknown = unknowns
        .iter()
        .find(|unknown| unknown.id.as_str() == *stub)
        .unwrap();
    assert!(unknown
        .links
        .iter()
        .any(|link| link.as_str() == property.id));
}

#[test]
fn missing_template_member_reports_the_derived_drawing_fields() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='0'/><Objects Count='1'><Object name='Template' type='TechDraw::DrawSVGTemplate'/></Objects><ObjectData Count='1'><Object name='Template'><Properties Count='2'><Property name='PageResult' type='App::PropertyFileIncluded'><FileIncluded file='missing.svg'/></Property><Property name='Label' type='App::PropertyString'><String value='A template'/></Property></Properties></Object></ObjectData></Document>";
    let source = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let drawing = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == cadmpeg_ir::drawings::DrawingKind::Template)
        .unwrap();
    assert!(drawing.sheet.is_none());
    assert!(drawing.template_geometry.is_none());
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .find(|property| property.name == "PageResult")
        .unwrap()
        .body
        .is_none());
    let fields = decoded
        .report()
        .losses
        .iter()
        .flat_map(|loss| &loss.dropped_fields)
        .filter(|field| field.record == drawing.id.as_str())
        .map(|field| field.field.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        fields,
        std::collections::BTreeSet::from(["sheet", "template_geometry"])
    );
}
