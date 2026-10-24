// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{
    make_block, native_envelope_fixture, outer_header, sldprt_with_body,
};
use crate::test_support::history::resolved_features_payload;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
use crate::SldprtCodec;

const MEMBER: &str = "Contents/Config-0-ResolvedFeatures";

fn shared_member() -> (EditableDecodeResult, Vec<u8>) {
    shared_member_with_payload(resolved_features_payload(&[0]))
}

fn shared_member_with_payload(payload: Vec<u8>) -> (EditableDecodeResult, Vec<u8>) {
    shared_member_with_sections(payload, &[])
}

fn shared_member_with_sections(
    payload: Vec<u8>,
    extra: &[(&str, &[u8])],
) -> (EditableDecodeResult, Vec<u8>) {
    shared_member_with_partition(payload, extra, "Contents/Config-0-Partition")
}

fn shared_member_with_partition(
    mut payload: Vec<u8>,
    extra: &[(&str, &[u8])],
    partition: &str,
) -> (EditableDecodeResult, Vec<u8>) {
    payload.extend(b"moBBoxCenterData_c");
    payload.extend(1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend(value.to_le_bytes());
    }
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        partition,
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(0x45, MEMBER, &payload));
    for (index, (name, bytes)) in extra.iter().enumerate() {
        source.extend(make_block(
            0x90 + u32::try_from(index).unwrap(),
            name,
            bytes,
        ));
    }
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

/// A feature-input lane keeps no layout positions through CADIR, so a write
/// that re-emits its member declines the patch and synthesizes (#237 item 1).
/// An unrepresented source class selects synthesis before the patch (#209 §1.4).
fn synthesized_after_declined_patch(decoded: &EditableDecodeResult) -> Vec<u8> {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, SynthesisCause};
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    if decoded.source_fidelity().unrepresented().is_empty() {
        assert!(matches!(plan.report().synthesis_cause(),
            Some(SynthesisCause::PatchDeclined { reason }) if !reason.is_empty()));
    } else {
        assert!(matches!(
            plan.report().synthesis_cause(),
            Some(SynthesisCause::SourceUnrepresented { .. })
        ));
    }
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    written
}

#[test]
fn metadata_edit_in_a_shared_feature_member_is_written_once() {
    let (mut decoded, _) = shared_member();
    edit_radius(&mut decoded.ir_mut());
    let written = synthesized_after_declined_patch(&decoded);
    let scan = crate::test_support::container::scan(&written);
    let members: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(MEMBER))
        .collect();
    assert_eq!(members.len(), 1);
    let readback = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(readback.ir().model.vertices.len(), 3);
    let native = sldprt_native(readback.ir());
    assert_eq!(native.feature_input_lanes.len(), 1);
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
    let error = crate::writer::resolved_feature_payload(
        lane,
        &native.feature_histories,
        &renames,
        crate::writer::ClassLayouts::Source(decoded.source_fidelity()),
        &crate::resolved_features::class_sources::ClassSources::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, CodecError::UnsupportedContent(ref detail) if detail.contains(&lane.id)),
        "{error}"
    );
    let originals = vec![(MEMBER.to_owned(), original.clone())];

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
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity());
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
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity());
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
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity());
    let mut payload = b"moConfigurationMgr_c".to_vec();
    let mut body = [0u8; 125];
    body[66..70].copy_from_slice(&17u32.to_le_bytes());
    body[107] = 3;
    body[117..125].copy_from_slice(&132_537_600_000_000_000u64.to_le_bytes());
    payload.extend(body);
    let record_id = cadmpeg_ir::ids::UnknownId::mint("sldprt:file:block#999_999").unwrap();
    let attribute_id =
        cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:configuration_manager#999_999:0")
            .unwrap();
    for (id, offset) in [(record_id.as_str(), 999_999), (attribute_id.as_str(), 0)] {
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
        native_ref: None,
        id: attribute_id,
        target: AttributeTarget::Document,
        name: "configuration_manager".into(),
        values: vec![
            AttributeValue::Integer(17),
            AttributeValue::Integer(3),
            AttributeValue::Integer(132_537_600_000_000_000),
        ],
    });
    records.push(crate::SourceRecord {
        id: record_id,
        data: Some(&payload),
        metadata: &decoded.source_fidelity().metadata,
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
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity());
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
        id: cadmpeg_ir::ids::UnknownId::mint("sldprt:file:block#999_999").unwrap(),
        data: record.data,
        metadata: record.metadata,
    };
    crate::annotations::note(
        &mut annotations,
        duplicate.id.as_str(),
        &cadmpeg_ir::stream_name!("Contents/Config-0-ResolvedFeatures"),
        999_999,
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
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity());
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
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity());
    let id =
        cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:source_linear_unit_code#999_999:0")
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
        native_ref: None,
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

#[test]
fn legacy_marker_metadata_edits_preserve_the_complete_marker_bytes() {
    for prefix in [
        [0xff, 0xff, 0x07, 0x00, 0x01],
        [0xff, 0xff, 0x1f, 0x00, 0x01],
    ] {
        let mut payload = resolved_features_payload(&[0]);
        let offset = payload
            .windows(5)
            .position(|bytes| bytes == [0xff, 0xff, 0x1f, 0x00, 0x03])
            .unwrap();
        payload[offset..offset + 5].copy_from_slice(&prefix);
        let (mut decoded, _) = shared_member_with_payload(payload);
        let native = sldprt_native(decoded.ir());
        let lane = &native.feature_input_lanes[0];
        assert_eq!(lane.sketch_entities.len(), 1);
        let error = crate::writer::resolved_feature_payload(
            lane,
            &native.feature_histories,
            &[],
            crate::writer::ClassLayouts::Source(decoded.source_fidelity()),
            &crate::resolved_features::class_sources::ClassSources::default(),
        )
        .unwrap_err();
        assert!(
            matches!(error, CodecError::UnsupportedContent(ref detail) if detail.contains(&lane.id)),
            "{error}"
        );
        edit_radius(&mut decoded.ir_mut());
        let written = synthesized_after_declined_patch(&decoded);
        let scan = crate::test_support::container::scan(&written);
        let members: Vec<_> = scan
            .blocks
            .iter()
            .filter(|block| block.section.name() == Some(MEMBER))
            .collect();
        assert_eq!(members.len(), 1);
        let readback = SldprtCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        assert_eq!(readback.ir().model.vertices.len(), 3);
    }
}

#[test]
fn marker_replay_requires_reparsed_membership_identity_and_values() {
    let mut payload = resolved_features_payload(&[0]);
    let offset = payload
        .windows(5)
        .position(|bytes| bytes == [0xff, 0xff, 0x1f, 0x00, 0x03])
        .unwrap();
    payload[offset + 4] = 1;
    let (decoded, _) = shared_member_with_payload(payload);
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.sketch_entities.len(), 1);
    for mutation in 0..4 {
        let mut changed = lane.clone();
        match mutation {
            0 => {
                changed.sketch_entities.clear();
            }
            1 => changed.sketch_entities[0].set_test_id("forged-marker"),
            2 => changed.sketch_entities[0].set_test_parent("forged-lane"),
            _ => {
                changed.sketch_entities[0].state_value =
                    Some(cadmpeg_ir::scalar::FiniteReal::new(97.0).unwrap());
            }
        }
        assert!(
            crate::writer::resolved_feature_payload(
                &changed,
                &native.feature_histories,
                &[],
                crate::writer::ClassLayouts::Source(decoded.source_fidelity()),
                &crate::resolved_features::class_sources::ClassSources::default(),
            )
            .map(|(payload, _)| payload)
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn metadata_edits_preserve_unemitted_named_members_and_physical_duplicates() {
    let units: &[u8] = b"authored user units table\0\x01\xff";
    let opaque: &[u8] = b"authored opaque member\0\x02\xfe";
    let lookalike = "ThirdPty/UnitsKeywordsDisplayListsNote";
    let extra = [
        ("Contents/User Units Table", units),
        (lookalike, opaque),
        (lookalike, opaque),
    ];
    let (mut decoded, _) = shared_member_with_sections(resolved_features_payload(&[0]), &extra);
    edit_radius(&mut decoded.ir_mut());
    let written = synthesized_after_declined_patch(&decoded);
    let readback = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(readback.ir().model.vertices.len(), 3);
}

#[test]
fn unchanged_history_replays_full_source_xml_and_an_edited_history_regenerates() {
    let history: &[u8] = br#"<?xml version="1.0"?><Keywords Name="Authored">
<!-- retain source XML bytes -->
<Feature Name="NativeTree" Type="Origin" id="11"/>
</Keywords>"#;
    let (mut decoded, _) = shared_member_with_sections(
        resolved_features_payload(&[0]),
        &[("Contents/Keywords", history)],
    );
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_histories.len(), 1);
    assert_eq!(native.feature_histories[0].features.len(), 1);
    edit_radius(&mut decoded.ir_mut());
    synthesized_after_declined_patch(&decoded);
    let mut changed = native.feature_histories[0].clone();
    changed.features[0].name = "EditedTree".into();
    let regenerated = crate::writer::retained_history_payload(
        &changed,
        "Contents/Keywords",
        &[(
            native.feature_histories[0].clone(),
            "Contents/Keywords".into(),
            history.to_vec(),
        )],
    )
    .unwrap();
    assert_ne!(regenerated, history);
    assert!(String::from_utf8(regenerated)
        .unwrap()
        .contains("EditedTree"));
}

#[test]
fn metadata_edits_preserve_unedited_f64_bits_and_complete_display_member() {
    let raw_length = 9.0f64 / 10000.0;
    assert_ne!(
        raw_length.to_bits(),
        (raw_length * 1000.0 * 0.001).to_bits()
    );
    let mut metadata = b"moBBoxCenterData_c".to_vec();
    metadata.extend(1u32.to_le_bytes());
    for value in [raw_length, 0.0, -raw_length, raw_length] {
        metadata.extend(value.to_le_bytes());
    }
    metadata.extend(b"moDefaultRefPlnData_c");
    for value in [raw_length, -raw_length, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        metadata.extend(value.to_le_bytes());
    }
    let mut display = crate::test_support::tessellation::display_list_payload();
    display.extend(b"authored opaque display suffix");
    let (mut decoded, _) = shared_member_with_sections(
        resolved_features_payload(&[0]),
        &[
            ("Contents/ExtraMetadata", metadata.as_slice()),
            ("Contents/DisplayLists", display.as_slice()),
        ],
    );
    assert!(!decoded.ir().model.tessellations.is_empty());
    let original_meshes = decoded.ir().model.tessellations.clone();
    let original_appearances = decoded.ir().model.appearances.len();
    let target = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.name == "bounding_envelope"
                && decoded
                    .source_fidelity()
                    .annotations
                    .provenance
                    .get(attribute.id.as_str())
                    .is_some_and(|source| source.stream() == MEMBER)
        })
        .unwrap()
        .id
        .clone();
    {
        let mut edit = decoded.ir_mut();
        let attribute = edit
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.id == target)
            .unwrap();
        let AttributeValue::Vector(values) = &mut attribute.values[0] else {
            panic!("bbox vector");
        };
        values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
    }
    let written = synthesized_after_declined_patch(&decoded);
    let readback = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        readback.ir().model.tessellations.len(),
        original_meshes.len()
    );
    assert_eq!(readback.ir().model.appearances.len(), original_appearances);
    for (original, actual) in original_meshes
        .iter()
        .zip(&readback.ir().model.tessellations)
    {
        assert_eq!(original.mesh(), actual.mesh());
        assert_eq!(original.channels(), actual.channels());
        assert_eq!(original.faces, actual.faces);
    }
}

#[test]
fn changed_display_semantics_do_not_replay_an_unchanged_source_member() {
    let display = crate::test_support::tessellation::display_list_payload();
    let (decoded, _) = shared_member_with_sections(
        resolved_features_payload(&[0]),
        &[("Contents/DisplayLists", &display)],
    );
    let mut changed = decoded.ir().clone();
    assert!(!changed.model.tessellations.is_empty());
    changed.model.tessellations[0] = changed.model.tessellations[0]
        .clone()
        .with_chordal_deflection(Some(0.25))
        .unwrap();
    let source = decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        source,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )
    .unwrap();
    let baseline = crate::decode::decode(&ctx, root).unwrap();
    let scan_arena = cadmpeg_core::decode::DecodeArena::new();
    let (scan_ctx, scan_root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        source,
        &scan_arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )
    .unwrap();
    let scan = crate::container::scan(&scan_ctx, scan_root).unwrap();
    let retained =
        crate::writer::retained_display_sections(decoded.ir(), Some(&baseline), Some(&scan))
            .unwrap()
            .unwrap();
    assert_eq!(retained, vec![("Contents/DisplayLists".into(), display)]);
    assert!(
        crate::writer::retained_display_sections(&changed, Some(&baseline), Some(&scan))
            .unwrap()
            .is_none()
    );
}

#[test]
fn metadata_edits_preserve_configuration_xml_and_explicit_renames_update_it() {
    let history: &[u8] = br#"<Keywords Name="Authored"><Configuration Name="Saved"/><Feature Name="NativeTree" Type="Origin" id="11"/></Keywords>"#;
    let ordinary: &[u8] = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swConfigurationName = 'Saved'/></swSolidWorks>"#;
    let stale: &[u8] = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swConfigurationName="Stale"/><swConfiguration swID="0" swName="Saved" swMostRecentConfiguration="YES"/></swSolidWorks>"#;
    for (xml, indexed, rename) in [
        (ordinary, false, false),
        (stale, true, false),
        (ordinary, true, true),
    ] {
        let partition = if indexed {
            "Contents/Config-0-Partition"
        } else {
            "Geometry/Partition"
        };
        let (mut decoded, _) = shared_member_with_partition(
            resolved_features_payload(&[0]),
            &[
                ("Contents/Keywords", history),
                ("swXmlContents/Features", xml),
            ],
            partition,
        );
        let source = decoded.ir().clone();
        let selected = source
            .model
            .configurations
            .iter()
            .find(|configuration| configuration.active)
            .unwrap();
        assert_eq!(selected.name.as_deref(), Some("Saved"));
        assert_eq!(selected.source_index, indexed.then_some(0));
        let source_intent = crate::writer::active_configuration_intent(&source);
        let mut indexed = source.clone();
        crate::writer::assign_configuration_indices(&mut indexed.model.configurations).unwrap();
        if selected.source_index.is_none() {
            assert!(selected.bodies.is_none());
        }
        assert_eq!(
            crate::writer::active_configuration_intent(&indexed),
            source_intent
        );
        edit_radius(&mut decoded.ir_mut());
        synthesized_after_declined_patch(&decoded);
        let source_native = sldprt_native(&source);
        assert_eq!(source_native.feature_histories.len(), 1);
        assert_eq!(
            source_native.feature_histories[0].configurations[0].source_index,
            selected.source_index
        );
        if rename {
            decoded
                .ir_mut()
                .model
                .configurations
                .iter_mut()
                .find(|configuration| configuration.active)
                .unwrap()
                .name = Some("RenamedSaved".into());
            let renamed = synthesized_after_declined_patch(&decoded);
            let readback = SldprtCodec
                .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
                .unwrap();
            assert_eq!(
                readback
                    .ir()
                    .model
                    .configurations
                    .iter()
                    .find(|configuration| configuration.active)
                    .unwrap()
                    .name
                    .as_deref(),
                Some("RenamedSaved")
            );
        }
    }
}

#[test]
fn duplicate_feature_owned_names_refuse_ambiguous_source_member_identity() {
    let mut payload = resolved_features_payload(&[0]);
    payload.extend(b"moBBoxCenterData_c");
    payload.extend(1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend(value.to_le_bytes());
    }
    let member = "ThirdPty/ResolvedFeaturesNote";
    let opaque = b"authored feature-owned opaque payload";
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x45, MEMBER, &payload));
    source.extend(make_block(0x91, member, opaque));
    source.extend(make_block(0x92, member, opaque));
    let source = native_envelope_fixture(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sldprt_native(decoded.ir()).feature_input_lanes.len(), 3);
    let mut decoded = EditableDecodeResult::from(decoded);
    edit_radius(&mut decoded.ir_mut());
    synthesized_after_declined_patch(&decoded);
}
