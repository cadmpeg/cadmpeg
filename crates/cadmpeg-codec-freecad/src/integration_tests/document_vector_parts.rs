// SPDX-License-Identifier: Apache-2.0
//! Direct document vectors are produced from actual owned property source fields.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn document(properties: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='{count}'>{properties}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

fn archive(xml: &str) -> Vec<u8> {
    zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())])
}

fn literal_field_reads(
    source: &str,
    fields: &[(&str, &str)],
    offset: u64,
) -> Vec<std::ops::Range<u64>> {
    let mut ranges: Vec<_> = fields
        .iter()
        .flat_map(|(name, raw)| {
            let token = format!("{name}='{raw}'");
            let start = source.find(&token).unwrap();
            let mut fields = vec![offset + start as u64..offset + (start + name.len()) as u64];
            if !raw.is_empty() {
                let value_start = start + name.len() + "='".len();
                fields.push(offset + value_start as u64..offset + (value_start + raw.len()) as u64);
            }
            fields
        })
        .collect();
    let start = source.find("<PropertyVector").unwrap() + 1;
    ranges.push(offset + start as u64..offset + (start + "PropertyVector".len()) as u64);
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<u64>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged
            .last_mut()
            .filter(|previous| previous.end == range.start)
        {
            previous.end = range.end;
        } else {
            merged.push(range);
        }
    }
    merged
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
        let origin = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
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
            &crate::container::admit_document(&ctx, scan.data["Document.xml"].window())
                .expect("valid source XML"),
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        assert_eq!(properties, pure.properties);
        let [attribute] = output.model().attributes.as_slice() else {
            panic!("one live vector attribute")
        };
        assert_eq!(
            attribute.values,
            [AttributeValue::vector([x, -2.0, 3.0]).unwrap()]
        );
        assert_eq!(attribute.native_ref.as_deref(), Some(kept.id.as_str()));
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == kept.id)
            .unwrap();
        assert_eq!(capture.stream(), origin.stream());
        assert_eq!(
            capture.offset(),
            origin.offset() + u64::try_from(xml.find(&property).unwrap()).unwrap()
        );
        assert_eq!(capture.byte_len(), u64::try_from(property.len()).unwrap());
        let expected = literal_field_reads(
            &property,
            &[
                ("name", raw_name),
                ("type", "App::PropertyVector"),
                ("valueX", raw_x),
                ("valueY", "-2"),
                ("valueZ", "3"),
                ("extra", "native-only"),
            ],
            origin.offset() + xml.find(&property).unwrap() as u64,
        );
        assert_eq!(capture.reads()[capture.stream()], expected);
        let start = origin.offset() + u64::try_from(xml.find("unheld").unwrap()).unwrap();
        assert!(capture.reads()[capture.stream()]
            .iter()
            .all(|read| read.end <= start || start + 6 <= read.start));
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
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one nameless property")
    };
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "freecad.property");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(
        part.offset(),
        u64::try_from(xml.find(bad).unwrap()).unwrap()
    );
    assert_eq!(part.byte_len(), u64::try_from(bad.len()).unwrap());
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(bad.as_bytes())
    );
    assert_eq!(
        fidelity.parts()[part.parent().unwrap()].state(),
        &PartState::Decoded {}
    );
    assert_eq!(
        fidelity
            .record_layout()
            .iter()
            .filter(
                |(_, layout)| layout.stream() == part.stream() && layout.offset() == part.offset()
            )
            .count(),
        1
    );
    assert!(fidelity.record_layout()[unknown.id.as_str()]
        .reads()
        .is_empty());
    let errors = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(errors[0].message.contains("name"));
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
fn incomplete_nonfinite_text_and_dynamic_fallback_vectors_keep_the_generic_native_record() {
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
            false,
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
            crate::persistence::parse_document(&dom, crate::persistence::Vocabulary::Objects)
                .unwrap();
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
        assert_eq!(properties, pure.properties);
        // Registered numeric value failures belong to the actual property_value leaf (#215).
        assert_eq!(properties[0].body.is_some(), projected);
        assert_eq!(properties[0].values().len(), usize::from(projected));
        assert!(decoded
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == "freecad.property"
                && part.state() == &PartState::Decoded {}));
        let failed: Vec<_> = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect();
        assert_eq!(failed.len(), usize::from(!projected));
        if !projected {
            assert_eq!(failed[0].level().as_str(), "freecad.property_value");
        }
        if projected {
            assert_eq!(
                decoded
                    .ir()
                    .model
                    .attributes
                    .iter()
                    .find(|attribute| attribute.name == "Vector")
                    .unwrap()
                    .values,
                [AttributeValue::vector([1.0, 2.0, 3.0]).unwrap()]
            );
        } else {
            assert!(!decoded
                .ir()
                .model
                .attributes
                .iter()
                .any(|attribute| attribute.name == "Vector"));
        }
        let errors: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(errors.len(), usize::from(!projected));
        if !projected {
            assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
        }
    }
}

#[test]
fn a_failed_archive_parent_discards_already_born_vector_fields_and_capture() {
    let xml = document("<Property name='Vector' type='App::PropertyVector'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property>", 1);
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
    assert!(decoded.ir().model.attributes.is_empty());
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one archive failure")
    };
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity.parts()[&unknown.id].level().as_str(),
        "zip.archive"
    );
    // All actual entry headers are admitted before any XML producer runs. This
    // malformed later header gives up the archive before a property is reached.
    assert!(fidelity
        .parts()
        .values()
        .all(|part| part.level().as_str() != "freecad.property"));
    let id = crate::native::native_child_id(
        "property",
        &crate::native::native_id("document", "0"),
        "Vector",
    );
    assert!(!fidelity.record_layout().contains_key(&id));
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
}
