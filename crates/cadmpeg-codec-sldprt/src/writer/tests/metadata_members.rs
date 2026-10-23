// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::history::resolved_features_payload;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

const MEMBER: &str = "Contents/Config-0-ResolvedFeatures";

fn shared_member() -> (EditableDecodeResult, Vec<u8>) {
    let mut payload = resolved_features_payload(&[0]);
    payload.extend(b"moBBoxCenterData_c");
    payload.extend(1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend(value.to_le_bytes());
    }
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x45, MEMBER, &payload));
    let source = native_envelope_fixture(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sldprt_native(decoded.ir()).feature_input_lanes.len(), 1);
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "bounding_envelope"));
    (EditableDecodeResult::from(decoded), payload)
}

fn edit_radius(ir: &mut cadmpeg_ir::CadIr) {
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "bounding_envelope")
        .unwrap();
    let AttributeValue::Vector(values) = &mut attribute.values[0] else {
        panic!("bbox vector")
    };
    values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
}

#[test]
fn metadata_edit_in_a_shared_feature_member_is_written_once() {
    let (mut decoded, original) = shared_member();
    edit_radius(&mut decoded.ir_mut());
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let members: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(MEMBER))
        .collect();
    assert_eq!(members.len(), 1);
    let mut expected = original;
    let start = expected.len() - 8;
    expected[start..].copy_from_slice(&0.041f64.to_le_bytes());
    assert_eq!(members[0].payload, expected);
    let readback = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(readback.ir().model.vertices.len(), 3);
    let native = sldprt_native(readback.ir());
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(
        native.feature_input_lanes[0]
            .names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        vec!["Sketch1", "Boss-Extrude1", "D1"]
    );
    let bbox = readback
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "bounding_envelope")
        .unwrap();
    let AttributeValue::Vector(values) = &bbox.values[0] else {
        panic!("bbox vector")
    };
    assert_eq!(values[3].get(), 41.0);
}

#[test]
fn a_real_feature_rename_survives_unchanged_metadata_in_the_same_member() {
    let (decoded, original) = shared_member();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let renames = [crate::history::write::features::FeatureInputRename {
        lane: lane.id.clone(),
        name_index: 0,
        value: cadmpeg_core::nonblank_literal!("RenamedSketchLong1"),
    }];
    let generated =
        crate::writer::resolved_feature_payload(lane, &native.feature_histories, &renames).unwrap();
    assert_ne!(generated, original);
    let names = crate::resolved_features::names::object_names(
        &cadmpeg_test_support::service_decode_context(),
        &generated,
        &lane.id,
    )
    .unwrap();
    assert_eq!(names[0].value, "RenamedSketchLong1");
    let originals = vec![(MEMBER.to_owned(), original.clone())];
    let mut sections = originals.clone();
    crate::writer::push_generated_metadata_member(
        &mut sections,
        &originals,
        MEMBER.to_owned(),
        generated.clone(),
    )
    .unwrap();
    assert_eq!(sections, vec![(MEMBER.to_owned(), generated.clone())]);
    assert_eq!(
        &generated[generated.len() - 32..],
        &original[original.len() - 32..]
    );

    let mut ir = decoded.ir().clone();
    edit_radius(&mut ir);
    let mut patched = originals.clone();
    crate::writer::patch_retained_swobjects_metadata(
        &ir,
        &decoded.source_fidelity().annotations,
        &mut patched,
        0.001,
    )
    .unwrap();
    assert_ne!(patched[0].1, original);
    let error = crate::writer::push_generated_metadata_member(
        &mut patched,
        &originals,
        MEMBER.to_owned(),
        generated,
    )
    .unwrap_err();
    assert!(
        matches!(error, CodecError::NotImplemented(detail) if detail.contains("conflicting metadata and generated payload edits"))
    );
}

#[test]
fn identical_outputs_and_both_insertion_orders_reconcile_one_member() {
    for name in [
        MEMBER,
        "Contents/Keywords",
        "Contents/DisplayLists",
        "Contents/Config-0-Partition",
    ] {
        let originals = vec![(name.to_owned(), b"original".to_vec())];
        for (first, second, expected) in [
            (
                b"original".as_slice(),
                b"patched".as_slice(),
                b"patched".as_slice(),
            ),
            (
                b"patched".as_slice(),
                b"original".as_slice(),
                b"patched".as_slice(),
            ),
            (
                b"patched".as_slice(),
                b"patched".as_slice(),
                b"patched".as_slice(),
            ),
        ] {
            let mut sections = Vec::new();
            crate::writer::push_generated_metadata_member(
                &mut sections,
                &originals,
                name.to_owned(),
                first.to_vec(),
            )
            .unwrap();
            crate::writer::push_generated_metadata_member(
                &mut sections,
                &originals,
                name.to_owned(),
                second.to_vec(),
            )
            .unwrap();
            assert_eq!(sections, vec![(name.to_owned(), expected.to_vec())]);
        }
    }
}

#[test]
fn the_actual_owned_stream_is_required_even_if_a_lookalike_member_exists() {
    let (decoded, _) = shared_member();
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let annotations = &decoded.source_fidelity().annotations;
    assert!(
        crate::writer::retained_swobjects_sections(decoded.ir(), &records, annotations)
            .unwrap()
            .iter()
            .any(|(name, _)| name == MEMBER)
    );
    records.retain(|record| {
        annotations
            .provenance
            .get(record.id.as_str())
            .is_none_or(|provenance| provenance.stream() != MEMBER)
    });
    let error = crate::writer::retained_swobjects_sections(decoded.ir(), &records, annotations)
        .unwrap_err();
    assert!(
        matches!(error, CodecError::Malformed(detail) if detail.contains(MEMBER) && detail.contains("no retained source member"))
    );
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let owner = records
        .iter()
        .find(|record| {
            annotations
                .provenance
                .get(record.id.as_str())
                .is_some_and(|provenance| provenance.stream() == MEMBER)
        })
        .unwrap();
    let mut lookalike_annotations = annotations.clone();
    lookalike_annotations.provenance.remove(owner.id.as_str());
    crate::annotations::note(
        &mut lookalike_annotations,
        owner.id.as_str(),
        &cadmpeg_ir::stream_name!("Other/Config-0-ResolvedFeatures"),
        0,
        "unknown",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    let error =
        crate::writer::retained_swobjects_sections(decoded.ir(), &records, &lookalike_annotations)
            .unwrap_err();
    assert!(
        matches!(error, CodecError::Malformed(detail) if detail.contains(MEMBER) && detail.contains("no retained source member"))
    );
}

#[test]
fn configuration_manager_and_feature_metadata_keep_their_separate_owners() {
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
    let (decoded, _) = shared_member();
    let mut ir = decoded.ir().clone();
    let mut annotations = decoded.source_fidelity().annotations.clone();
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let mut payload = b"moConfigurationMgr_c".to_vec();
    let mut body = [0u8; 125];
    body[66..70].copy_from_slice(&17u32.to_le_bytes());
    body[107] = 3;
    body[117..125].copy_from_slice(&132_537_600_000_000_000u64.to_le_bytes());
    payload.extend(body);
    let record_id = cadmpeg_ir::ids::UnknownId::mint("sldprt:file:block#999999").unwrap();
    let attribute_id =
        cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:configuration_manager#999999:0")
            .unwrap();
    for (id, offset) in [(record_id.as_str(), 999999), (attribute_id.as_str(), 0)] {
        crate::annotations::note(
            &mut annotations,
            id,
            &cadmpeg_ir::stream_name!("Contents/CMgr"),
            offset,
            "moConfigurationMgr_c",
            cadmpeg_ir::Exactness::ByteExact,
        )
        .unwrap();
    }
    ir.model.attributes.push(SourceAttribute {
        id: attribute_id,
        target: AttributeTarget::Document,
        name: "configuration_manager".into(),
        values: vec![
            AttributeValue::Integer(17),
            AttributeValue::Integer(3),
            AttributeValue::Integer(132_537_600_000_000_000),
        ],
    });
    let retained = cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
        "Contents/CMgr",
        999999,
        cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
            data: payload.clone(),
        },
    )
    .unwrap();
    records.push(crate::SourceRecord {
        id: record_id,
        sha256: retained.sha256(),
        data: Some(&payload),
    });
    let mut sections =
        crate::writer::retained_swobjects_sections(&ir, &records, &annotations).unwrap();
    assert_eq!(
        sections
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Contents/CMgr", MEMBER]
    );
    edit_radius(&mut ir);
    crate::writer::patch_retained_swobjects_metadata(&ir, &annotations, &mut sections, 0.001)
        .unwrap();
    assert_eq!(sections[0].1, payload);
    assert_eq!(
        &sections[1].1[sections[1].1.len() - 8..],
        &0.041f64.to_le_bytes()
    );
}

#[test]
fn duplicate_physical_metadata_owners_are_ambiguous_even_with_identical_bytes() {
    let (decoded, _) = shared_member();
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let mut annotations = decoded.source_fidelity().annotations.clone();
    let record = records
        .iter()
        .find(|record| {
            annotations
                .provenance
                .get(record.id.as_str())
                .is_some_and(|provenance| provenance.stream() == MEMBER)
        })
        .unwrap();
    let duplicate = crate::SourceRecord {
        id: cadmpeg_ir::ids::UnknownId::mint("sldprt:file:block#999999").unwrap(),
        sha256: record.sha256.clone(),
        data: record.data,
    };
    crate::annotations::note(
        &mut annotations,
        duplicate.id.as_str(),
        &cadmpeg_ir::stream_name!("Contents/Config-0-ResolvedFeatures"),
        999999,
        "unknown",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    records.push(duplicate);
    let error = crate::writer::retained_swobjects_sections(decoded.ir(), &records, &annotations)
        .unwrap_err();
    assert!(
        matches!(error, CodecError::NotImplemented(detail) if detail.contains("ambiguous retained members"))
    );
}

#[test]
fn digest_only_metadata_owner_cannot_supply_a_patch_payload() {
    let (decoded, _) = shared_member();
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let annotations = &decoded.source_fidelity().annotations;
    let record = records
        .iter_mut()
        .find(|record| {
            annotations
                .provenance
                .get(record.id.as_str())
                .is_some_and(|provenance| provenance.stream() == MEMBER)
        })
        .unwrap();
    record.data = None;
    let error = crate::writer::retained_swobjects_sections(decoded.ir(), &records, annotations)
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(detail) if detail.contains("has no bytes")));
}

#[test]
fn generated_output_cannot_select_one_of_two_owned_members() {
    let originals = vec![(MEMBER.to_owned(), b"original".to_vec())];
    let mut sections = vec![
        (MEMBER.to_owned(), b"original".to_vec()),
        (MEMBER.to_owned(), b"other".to_vec()),
    ];
    let error = crate::writer::push_generated_metadata_member(
        &mut sections,
        &originals,
        MEMBER.to_owned(),
        b"patched".to_vec(),
    )
    .unwrap_err();
    assert!(
        matches!(error, CodecError::NotImplemented(detail) if detail.contains("ambiguous output members"))
    );
}

#[test]
fn source_unit_code_uses_its_separate_units_route() {
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
    let (decoded, _) = shared_member();
    let mut ir = decoded.ir().clone();
    let mut annotations = decoded.source_fidelity().annotations.clone();
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let id = cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:source_linear_unit_code#999999:0")
        .unwrap();
    crate::annotations::note(
        &mut annotations,
        id.as_str(),
        &cadmpeg_ir::stream_name!("Units"),
        0,
        "SW_UnitsLinear",
        cadmpeg_ir::Exactness::ByteExact,
    )
    .unwrap();
    ir.model.attributes.push(SourceAttribute {
        id,
        target: AttributeTarget::Document,
        name: "source_linear_unit_code".into(),
        values: vec![AttributeValue::Integer(1)],
    });
    let retained = crate::writer::retained_swobjects_sections(&ir, &records, &annotations).unwrap();
    assert_eq!(
        retained
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![MEMBER]
    );
}
