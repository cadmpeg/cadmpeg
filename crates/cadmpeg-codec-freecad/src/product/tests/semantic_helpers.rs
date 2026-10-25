// SPDX-License-Identifier: Apache-2.0
//! Source-based fixture helpers for product validation and typed binary rows.

use super::*;

pub(super) fn assert_product_semantic_finding(
    ir: &cadmpeg_ir::CadIr,
    expected: &str,
) -> native::ProductNodeRecord {
    // #215: validation reports source-readable contradictions without refusing decode.
    let id = native::native_id("product", "Occurrence");
    let findings = crate::test_support::validate_native(ir);
    assert!(
        findings.iter().any(|finding| finding.check
            == cadmpeg_ir::report::check::Check::PayloadIntegrity
            && finding.entity.as_deref() == Some(id.as_str())
            && finding.message.contains(expected)),
        "{findings:?}"
    );
    let namespace = ir.native.namespace("fcstd").unwrap();
    let records = namespace
        .arena_as::<native::ProductNodeRecord>("product_nodes")
        .unwrap();
    let record = records
        .into_iter()
        .find(|record| record.id == id)
        .expect("native product survives validation");
    assert!(namespace
        .arena_as::<native::ObjectRecord>("objects")
        .unwrap()
        .iter()
        .any(|object| object.name() == "Prototype"));
    assert!(!ir
        .model
        .occurrences
        .iter()
        .any(|occurrence| occurrence.native_ref.as_deref() == Some(record.object.as_str())));
    record
}

pub(super) fn assert_source_failure(
    result: &cadmpeg_ir::codec::DecodeResult,
    expected_levels: &[&str],
    owner: &str,
) {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let failures = result
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .map(|part| part.level().as_str())
        .collect::<Vec<_>>();
    assert_eq!(failures, expected_levels);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        expected_levels.len()
    );
    let native = result.ir().native.namespace("fcstd").unwrap();
    let object = native::native_id("object", owner);
    assert!(native
        .arena_as::<native::ObjectRecord>("objects")
        .unwrap()
        .iter()
        .any(|record| record.id() == &object));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

pub(super) fn decode_real_list(bytes: &[u8], start: usize, end: usize) -> (Vec<Vec<f64>>, bool) {
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Link' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Link'><Properties Count='1'><Property name='ScaleList' type='App::PropertyVectorList'><VectorList file='ScaleList'/></Property></Properties></Object></ObjectData></Document>";
    let source = archive_entries(&[
        ("Document.xml", document.as_slice()),
        ("ScaleList", &bytes[start..end]),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let failed = decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable");
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<native::EntryRecord>("entries")
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name() == "ScaleList")
        .unwrap();
    let values = entry
        .contents()
        .iter()
        .find_map(|contents| match contents {
            native::entry_contents::EntryContents::Vector { values, .. } => Some(
                values
                    .iter()
                    .flatten()
                    .map(|value| value.map(cadmpeg_ir::scalar::FiniteReal::get).to_vec())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    (values, failed)
}

pub(super) fn assert_multiple_prototype_carriers(result: &cadmpeg_ir::codec::DecodeResult) {
    let findings = crate::test_support::validate_native(result.ir());
    assert!(
        findings.iter().any(|finding| finding.entity.as_deref()
            == Some(native::native_id("product", "Occurrence").as_str())
            && finding.message.contains("expected App::PropertyXLink")),
        "{findings:?}"
    );
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "LinkedObject")
        .unwrap();
    assert_eq!(property.type_name, "App::PropertyLinkList");
    assert_eq!(property.links().len(), 2);
    assert!(result.ir().model.occurrences.is_empty());
}

pub(super) fn assert_overlapping_members(result: &cadmpeg_ir::codec::DecodeResult) {
    let findings = crate::test_support::validate_native(result.ir());
    for owner in ["First", "Second"] {
        assert!(
            findings.iter().any(|finding| finding.entity.as_deref()
                == Some(native::native_id("product", owner).as_str())
                && finding.message.contains("multiple parent containers")),
            "{findings:?}"
        );
    }
    let records = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<native::ProductNodeRecord>("product_nodes")
        .unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.members() == [native::native_id("object", "Member")]));
    assert!(result.ir().model.occurrences.is_empty());
}

pub(super) fn assert_description_failure(
    result: &cadmpeg_ir::codec::DecodeResult,
    document: &str,
    properties: &[native::PropertyRecord],
) {
    let description = properties
        .iter()
        .find(|property| property.name == "Description")
        .unwrap();
    assert!(description.body.is_none());
    let expected = br#"<Wrapper><String value="nested description"/></Wrapper>"#;
    let fidelity = result.source_fidelity();
    let offset = u64::try_from(
        document
            .find(std::str::from_utf8(expected).unwrap())
            .unwrap(),
    )
    .unwrap();
    let (id, part) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| {
            part.level().as_str() == "freecad.property_value" && part.offset() == offset
        })
        .unwrap();
    assert_eq!(
        part.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    );
    assert_eq!(
        fidelity.retained_record(id.as_str()).unwrap().data(),
        Some(expected.as_slice())
    );
}

pub(super) fn assert_invalid_placement(result: &cadmpeg_ir::codec::DecodeResult) {
    let property = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<native::PropertyRecord>("properties")
        .unwrap()
        .into_iter()
        .find(|property| property.name == "Placement")
        .unwrap();
    assert!(property.body.is_none());
    if property.type_name == "App::PropertyPlacement" {
        assert_source_failure(result, &["freecad.property_value"], "Occurrence");
    } else {
        assert_eq!(property.type_name, "App::PropertyString");
        let failures = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count();
        assert_eq!(failures, 1);
        assert_product_semantic_finding(result.ir(), "expected App::PropertyPlacement");
    }
}
