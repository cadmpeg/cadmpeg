// SPDX-License-Identifier: Apache-2.0
//! Optional naming metadata must not gate independently framed shape payloads.

use super::*;
use crate::test_support::test_archive::rewrite_entry;

#[test]
fn malformed_naming_metadata_keeps_geometry_and_valid_native_state() {
    let original = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        std::str::from_utf8(data)
            .unwrap()
            .replacen(
                "<Part ElementMap=",
                "<Part HasherIndex=\"0\" ElementMap=",
                1,
            )
            .into_bytes()
    });
    let expected = FcstdCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    let expected_native = expected.ir().native.namespace("fcstd").unwrap();
    let expected_properties = expected_native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let affected = expected_properties
        .iter()
        .filter(|property| {
            property.values().iter().any(|value| {
                value.tag == "Part"
                    && value.attributes.get("HasherIndex").map(String::as_str) == Some("0")
            })
        })
        .map(|property| property.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(affected.len(), 4);
    let expected_payloads = expected_native
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    assert!(
        expected_payloads.len() > 1,
        "the fixture has independent peers"
    );
    for (before, after) in [
        ("threshold=\"0\"", "threshold=\"invalid\""),
        ("saveall=\"0\"", "saveall=\"invalid\""),
        ("HasherIndex=\"0\"", "HasherIndex=\"invalid\""),
        (
            "HasherIndex=\"0\"",
            "HasherIndex=\"9999999999999999999999999\"",
        ),
        ("HasherIndex=\"0\"", "HasherIndex=\"9999\""),
    ] {
        let source = rewrite_entry(&original, "Document.xml", |data| {
            let text = std::str::from_utf8(data).unwrap();
            assert!(text.contains(before), "missing {before}");
            text.replace(before, after).into_bytes()
        });
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        if before.starts_with("HasherIndex") {
            // #215 §4.5: HasherIndex is read by the actual Part value, so a
            // malformed selector gives up that value, including its file ref.
            let native = recovered.ir().native.namespace("fcstd").unwrap();
            let properties = native
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap();
            for id in &affected {
                let property = properties.iter().find(|p| &p.id == id).unwrap();
                assert!(!property.values().iter().any(|value| value.tag == "Part"));
            }
            let payloads = native
                .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
                .unwrap();
            assert!(!payloads
                .iter()
                .any(|payload| affected.contains(&payload.property)));
            assert_eq!(payloads.len() + affected.len(), expected_payloads.len());
            for peer in expected_payloads
                .iter()
                .filter(|p| !affected.contains(&p.property))
            {
                let retained = payloads
                    .iter()
                    .find(|p| p.property == peer.property)
                    .unwrap();
                assert_eq!(
                    retained.payload, peer.payload,
                    "independent {}",
                    peer.property
                );
            }
            assert!(recovered.ir().model.faces.len() < expected.ir().model.faces.len());
        } else {
            assert_eq!(recovered.ir().model, expected.ir().model, "{after}");
        }
        let level = if before.starts_with("HasherIndex") {
            "freecad.property_value"
        } else {
            "freecad.string_hasher"
        };
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"
                && loss.message.contains(level)));
        assert!(recovered
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == level
                && matches!(
                    part.state(),
                    cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                )));
        if before.starts_with("HasherIndex") {
            let mut zip = zip::ZipArchive::new(Cursor::new(&source)).unwrap();
            let mut document = String::new();
            zip.by_name("Document.xml")
                .unwrap()
                .read_to_string(&mut document)
                .unwrap();
            let xml = roxmltree::Document::parse(&document).unwrap();
            let mut expected_parts = xml
                .descendants()
                .filter(|node| node.has_tag_name("Part") && node.attribute("HasherIndex").is_some())
                .map(|node| node.range())
                .collect::<Vec<_>>();
            expected_parts.sort_by_key(|range| range.start);
            let mut given_up = recovered
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| {
                    part.level().as_str() == "freecad.property_value"
                        && matches!(
                            part.state(),
                            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                        )
                })
                .collect::<Vec<_>>();
            given_up.sort_by_key(|part| part.offset());
            assert_eq!(given_up.len(), affected.len());
            for (part, extent) in given_up.iter().zip(expected_parts) {
                assert_eq!(part.offset(), u64::try_from(extent.start).unwrap());
                assert_eq!(part.byte_len(), u64::try_from(extent.len()).unwrap());
            }
        }
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
        let wire = serde_json::to_value(recovered.ir()).unwrap();
        let roundtrip = serde_json::from_value::<cadmpeg_ir::CadIr>(wire).unwrap();
        assert!(crate::test_support::validate_native(&roundtrip).is_empty());
    }
}

#[test]
fn string_table_count_above_the_bound_becomes_a_naming_loss() {
    let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        std::str::from_utf8(data)
            .unwrap()
            .replacen(
                "threshold=\"0\" count=\"0\"",
                "threshold=\"0\" count=\"10000001\"",
                1,
            )
            .into_bytes()
    });
    let result = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("an oversized naming table stays source-only");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.string_hasher")
            && loss.message.contains("count exceeds limit")
    }));
}

#[test]
fn unreadable_naming_carriers_keep_geometry_and_contiguous_table_indices() {
    let expected = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for after in [
        "<StringHasher saveall=\"0\" threshold=\"0\" count=\"1\">malformed</StringHasher>",
        "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\" file=\"missing-table.txt\"/>",
        "<StringHasher new=\"1\" saveall=\"0\" threshold=\"0\"/>",
    ] {
        let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
            let text = std::str::from_utf8(data).unwrap();
            assert!(text.contains(
                "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>"
            ));
            text.replace(
                "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>",
                after,
            )
            .into_bytes()
        });
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model);
        let tables = recovered
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .unwrap();
        let unavailable = after.contains("missing-table.txt");
        if unavailable {
            // #215: readable XML metadata survives an unavailable side member;
            // no synthetic row count or table payload may be claimed.
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].source_entry.as_deref(), Some("missing-table.txt"));
            assert!(tables[0].entries().is_none());
        } else {
            assert!(tables.is_empty());
        }
        assert_eq!(
            recovered
                .source_fidelity()
                .parts()
                .values()
                .any(|part| part.level().as_str() == "freecad.string_hasher"
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )),
            !unavailable
        );
        assert_eq!(
            recovered
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "record.unreadable"),
            !unavailable
        );
        assert_eq!(
            recovered.source_fidelity().unrepresented(),
            [
                cadmpeg_ir::ArenaClass::Model,
                cadmpeg_ir::ArenaClass::Design
            ]
            .into_iter()
            .collect(),
            "unread ZIP fields and unreadable naming content retain their actual source classes"
        );
        crate::fidelity::b1::assert_classified_spans(&recovered);
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
    }
}

#[test]
fn damaged_table_framing_does_not_erase_a_readable_peer() {
    let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        let text = std::str::from_utf8(data).unwrap();
        let xml = roxmltree::Document::parse(text).unwrap();
        let part = xml
            .descendants()
            .find(|node| node.has_tag_name("Part"))
            .unwrap();
        let mut text = text.to_owned();
        text.insert_str(
            part.range().end,
            "<StringHasher saveall=\"1\" threshold=\"7\" count=\"1\">1.c stable</StringHasher>",
        );
        text.replace(
            "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>",
            "<StringHasher new=\"1\" saveall=\"0\" threshold=\"0\"/>",
        )
        .into_bytes()
    });
    let recovered = FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let tables = recovered
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].index, 1);
    assert_eq!(
        tables[0].entries().unwrap()[0].as_ref().unwrap().payload,
        "stable"
    );
    assert!(tables[0].owner_property.is_some());
    assert!(recovered
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.string_hasher"
            && matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )));
    assert_eq!(recovered.ir().model.faces.len(), 48);
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}

#[test]
fn failed_hasher_policy_keeps_independent_successor_rows_and_map_dependencies() {
    for policy in ["saveall='bad' threshold='7'", "saveall='1' threshold='bad'"] {
        for side in [false, true] {
            for property_owned in [false, true] {
                let successor = if side {
                    "<StringHasher2 file='naming.txt'/>"
                } else {
                    "<StringHasher2 count='1'>1.c stable\n</StringHasher2>"
                };
                let donor = format!("<StringHasher new='1' {policy}/>{successor}");
                let (root_donor, shape_donor, index) = if property_owned {
                    ("", donor.as_str(), 0)
                } else {
                    (donor.as_str(), "", 0)
                };
                let document = format!("<Document SchemaVersion='4' FileVersion='2'>{root_donor}<Objects Count='1'><Object name='A' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='' ElementMap='legacy' HasherIndex='{index}'/>{shape_donor}<ElementMap count='1'>Face1 stable 1 1</ElementMap></Property></Properties></Object></ObjectData></Document>");
                let bytes = archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("naming.txt", b"StringTableStart v1 1\n1.c stable\n"),
                ]);
                let result = FcstdCodec
                    .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                    .unwrap();
                let native = result.ir().native.namespace("fcstd").unwrap();
                let tables = native
                    .arena_as::<crate::native::StringTableRecord>("string_tables")
                    .unwrap();
                assert_eq!(
                    tables.len(),
                    1,
                    "{policy} side={side} property_owned={property_owned}: {:?}",
                    result.report().losses
                );
                assert_eq!(tables[0].index, 0);
                assert_eq!(tables[0].save_all, None);
                assert_eq!(tables[0].threshold, None);
                assert_eq!(
                    tables[0].entries().unwrap()[0].as_ref().unwrap().payload,
                    "stable"
                );
                let maps = native
                    .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
                    .unwrap();
                assert_eq!(maps.len(), 1);
                assert_eq!(
                    maps[0].string_table.as_deref(),
                    Some(tables[0].id().as_str())
                );
                let failed = result
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| {
                        part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    })
                    .collect::<Vec<_>>();
                assert_eq!(failed.len(), 1, "{:?}", result.report().losses);
                assert_eq!(
                    failed[0].level().as_str(),
                    if property_owned {
                        "freecad.property_value"
                    } else {
                        "freecad.string_hasher"
                    }
                );
                assert_eq!(
                    result
                        .report()
                        .losses
                        .iter()
                        .filter(|loss| loss.code.local_code() == "record.unreadable")
                        .count(),
                    1
                );
                assert!(crate::test_support::validate_native(result.ir()).is_empty());
            }
        }
    }
}
