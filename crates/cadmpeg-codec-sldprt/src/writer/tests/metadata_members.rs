// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{
    make_block, native_envelope_fixture, outer_header, sldprt_with_body,
};
use crate::test_support::history::resolved_features_payload;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
use crate::writer::metadata_owners::{metadata_owners, MetadataOwners};
use crate::writer::{MetadataMembers, Unwritten, UnwrittenAttributes};
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

fn members(originals: &[(String, Vec<u8>)]) -> MetadataMembers {
    MetadataMembers {
        originals: originals.iter().cloned().collect(),
        written: BTreeMap::new(),
    }
}

fn bounding_envelope_id(ir: &cadmpeg_ir::CadIr) -> String {
    ir.model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "bounding_envelope")
        .unwrap()
        .id
        .as_str()
        .to_owned()
}

fn edit_radius(ir: &mut cadmpeg_ir::CadIr) {
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "bounding_envelope")
        .unwrap();
    let Some(AttributeValue::Vector(values)) = &mut attribute.values[0] else {
        panic!("bbox vector")
    };
    values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
}

#[test]
fn metadata_edit_without_the_source_image_patches_the_retained_member() {
    let (mut decoded, original) = shared_member();
    edit_radius(&mut decoded.ir_mut());
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
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
    assert_eq!(readback.ir().geometry.vertices.len(), 3);
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
    let Some(AttributeValue::Vector(values)) = &bbox.values[0] else {
        panic!("bbox vector")
    };
    assert_eq!(values[3].get(), 41.0);
}

#[test]
fn a_real_feature_rename_replaces_a_patched_member_and_charges_its_metadata() {
    let (decoded, original) = shared_member();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let renames = [crate::history::write::features::FeatureInputRename {
        lane: lane.id.clone(),
        name_index: 0,
        value: cadmpeg_core::nonblank_literal!("RenamedSketchLong1"),
    }];
    let generated = crate::writer::resolved_feature_payload(
        lane,
        &native.feature_histories,
        &renames,
        cadmpeg_ir::units::Tolerances::default().linear,
    )
    .unwrap();
    assert_ne!(generated, original);
    let names = crate::resolved_features::names::object_names(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &generated,
        &lane.id,
    )
    .unwrap();
    assert_eq!(names[0].value, "RenamedSketchLong1");
    let originals = vec![(MEMBER.to_owned(), original.clone())];
    let mut members = members(&originals);
    let mut unwritten = UnwrittenAttributes::default();
    let mut sections = originals.clone();
    members.push(
        &mut sections,
        &mut unwritten,
        MEMBER.to_owned(),
        generated.clone(),
    );
    assert_eq!(sections, vec![(MEMBER.to_owned(), generated.clone())]);
    assert_eq!(
        &generated[generated.len() - 32..],
        &original[original.len() - 32..]
    );

    let mut ir = decoded.ir().clone();
    edit_radius(&mut ir);
    let owners = MetadataOwners {
        patchable: originals.clone(),
        ..MetadataOwners::default()
    };
    let mut patched = originals.clone();
    members.written = crate::writer::patch_retained_swobjects_metadata(
        &ir,
        &decoded.source_fidelity().annotations,
        &owners,
        &mut patched,
        0.001,
        &mut unwritten,
    );
    assert_ne!(patched[0].1, original);
    let bbox = bounding_envelope_id(&ir);
    assert_eq!(members.written[MEMBER], std::slice::from_ref(&bbox));
    members.push(
        &mut patched,
        &mut unwritten,
        MEMBER.to_owned(),
        generated.clone(),
    );
    assert_eq!(patched, vec![(MEMBER.to_owned(), generated)]);
    assert_eq!(unwritten.reason(&bbox), Some(Unwritten::OwnerRegenerated));
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
        let members = members(&originals);
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
            let mut unwritten = UnwrittenAttributes::default();
            members.push(
                &mut sections,
                &mut unwritten,
                name.to_owned(),
                first.to_vec(),
            );
            members.push(
                &mut sections,
                &mut unwritten,
                name.to_owned(),
                second.to_vec(),
            );
            assert_eq!(sections, vec![(name.to_owned(), expected.to_vec())]);
        }
    }
}

fn owners_of(
    decoded: &EditableDecodeResult,
    records: &[crate::SourceRecord<'_>],
    annotations: &cadmpeg_ir::Annotations,
) -> MetadataOwners {
    metadata_owners(decoded.ir(), records, annotations, None, &HashSet::new())
}

#[test]
fn the_actual_owned_stream_is_unavailable_even_if_a_lookalike_member_exists() {
    let (decoded, _) = shared_member();
    let mut records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let annotations = &decoded.source_fidelity().annotations;
    assert!(owners_of(&decoded, &records, annotations)
        .patchable
        .iter()
        .any(|(name, _)| name == MEMBER));
    records.retain(|record| {
        annotations
            .provenance
            .get(record.id.as_str())
            .is_none_or(|provenance| provenance.stream() != MEMBER)
    });
    let owners = owners_of(&decoded, &records, annotations);
    assert!(owners.unavailable.contains(MEMBER));
    assert!(owners.patchable.is_empty());
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
    let owners = owners_of(&decoded, &records, &lookalike_annotations);
    assert!(owners.unavailable.contains(MEMBER));
    assert!(owners.patchable.is_empty());
    assert_eq!(
        owners.unpatched(MEMBER),
        Some((Unwritten::OwnerUnavailable, crate::writer::Held::Nothing))
    );
}

#[test]
fn unsupported_manager_fields_keep_a_retained_member() {
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
        id: attribute_id.clone(),
        target: AttributeTarget::Document.into(),
        name: "configuration_manager".into(),
        values: vec![
            Some(AttributeValue::Integer(17)),
            Some(AttributeValue::Integer(4)),
            Some(AttributeValue::Integer(132_537_600_000_000_000)),
        ],
        native_ref: None,
    });
    records.push(crate::SourceRecord {
        id: record_id,
        data: Some(&payload),
    });
    let owners = metadata_owners(&ir, &records, &annotations, None, &HashSet::new());
    let mut sections = owners.patchable.clone();
    assert_eq!(
        sections
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Contents/CMgr", MEMBER]
    );
    let before = sections.clone();
    let mut unwritten = crate::writer::UnwrittenAttributes::default();
    crate::writer::patch_retained_swobjects_metadata(
        &ir,
        &annotations,
        &owners,
        &mut sections,
        0.001,
        &mut unwritten,
    );
    assert_eq!(
        unwritten.reason(attribute_id.as_str()),
        Some(crate::writer::Unwritten::NoSerializer)
    );
    assert_eq!(sections, before);
}

/// An attribute no record can hold, stored in a member the write does not
/// hold, is named as not written, not as kept in its retained bytes.
#[test]
fn a_null_value_in_an_unavailable_owner_is_named_as_not_written() {
    let (decoded, _) = shared_member();
    let mut ir = decoded.ir().clone();
    ir.model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "bounding_envelope")
        .unwrap()
        .values[0] = None;
    let owners = MetadataOwners {
        unavailable: std::collections::BTreeSet::from([MEMBER.to_owned()]),
        ..MetadataOwners::default()
    };
    let mut unwritten = UnwrittenAttributes::default();
    crate::writer::patch_retained_swobjects_metadata(
        &ir,
        &decoded.source_fidelity().annotations,
        &owners,
        &mut [],
        0.001,
        &mut unwritten,
    );
    crate::test_support::UnwrittenLosses::of(&unwritten.notes()).assert_charged(
        crate::writer::Held::Nothing,
        "state a null value",
        &bounding_envelope_id(&ir),
    );
}

#[test]
fn duplicate_physical_metadata_owners_are_carried_and_their_excess_named() {
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
        id: cadmpeg_ir::ids::UnknownId::mint("sldprt:file:block#999_999").unwrap(),
        data: record.data,
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
    let owners = owners_of(&decoded, &records, &annotations);
    assert!(owners.carried.contains(MEMBER));
    assert!(owners.patchable.is_empty());
    assert_eq!(
        owners.unpatched(MEMBER),
        Some((Unwritten::OwnerCarried, crate::writer::Held::Source))
    );
    let opaque = crate::writer::opaque_blocks(
        decoded.ir(),
        &records,
        &annotations,
        &HashSet::new(),
        &HashMap::from([(MEMBER.to_owned(), 1)]),
        true,
        crate::writer::RetainedBlockPatches {
            active_configuration: false,
            edited_parameters: &HashSet::new(),
            native: None,
            stale: &std::collections::BTreeSet::new(),
        },
    )
    .unwrap();
    assert!(opaque.blocks.iter().all(|(name, _)| name != MEMBER));
    assert_eq!(opaque.replaced, ["sldprt:file:block#999_999"]);
}

#[test]
fn digest_only_metadata_owner_is_unavailable() {
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
    let owners = owners_of(&decoded, &records, annotations);
    assert!(owners.unavailable.contains(MEMBER));
    assert!(!owners.replay());
}

#[test]
fn source_unit_code_uses_its_separate_units_route() {
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
    let (decoded, _) = shared_member();
    let mut ir = decoded.ir().clone();
    let mut annotations = decoded.source_fidelity().annotations.clone();
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
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
        id,
        target: AttributeTarget::Document.into(),
        name: "source_linear_unit_code".into(),
        values: vec![Some(AttributeValue::Integer(1))],
        native_ref: None,
    });
    let retained = metadata_owners(&ir, &records, &annotations, None, &HashSet::new()).patchable;
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
        let (mut decoded, original) = shared_member_with_payload(payload);
        let native = sldprt_native(decoded.ir());
        let lane = &native.feature_input_lanes[0];
        assert_eq!(lane.sketch_entities.len(), 1);
        assert_eq!(
            crate::writer::resolved_feature_payload(
                lane,
                &native.feature_histories,
                &[],
                cadmpeg_ir::units::Tolerances::default().linear
            )
            .unwrap(),
            original
        );
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
        assert_eq!(readback.ir().geometry.vertices.len(), 3);
        assert_eq!(
            sldprt_native(readback.ir()).feature_input_lanes[0]
                .sketch_entities
                .len(),
            1
        );
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
                cadmpeg_ir::units::Tolerances::default().linear
            )
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
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let units_blocks: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some("Contents/User Units Table"))
        .collect();
    assert_eq!(units_blocks.len(), 1);
    assert_eq!(units_blocks[0].payload, units);
    let opaque_blocks: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(lookalike))
        .collect();
    assert_eq!(opaque_blocks.len(), 2);
    assert!(opaque_blocks.iter().all(|block| block.payload == opaque));
    assert_eq!(
        opaque_blocks
            .iter()
            .map(|block| block.type_id)
            .collect::<Vec<_>>(),
        vec![0x91, 0x92]
    );
    let readback = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(readback.ir().geometry.vertices.len(), 3);
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
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let blocks: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some("Contents/Keywords"))
        .collect();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].payload, history);
    let mut changed = native.feature_histories[0].clone();
    changed.features[0].name = "EditedTree".into();
    let mut losses = Vec::new();
    let regenerated = crate::history::write::keywords::history_member(
        &changed,
        "Contents/Keywords",
        &[(
            &native.feature_histories[0],
            "Contents/Keywords".into(),
            history.to_vec(),
        )],
        &[],
        &mut losses,
    )
    .unwrap()
    .unwrap();
    assert!(losses.is_empty());
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
        let Some(AttributeValue::Vector(values)) = &mut attribute.values[0] else {
            panic!("bbox vector");
        };
        values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
    }
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    for (name, expected) in [
        ("Contents/ExtraMetadata", &metadata),
        ("Contents/DisplayLists", &display),
    ] {
        let blocks: Vec<_> = scan
            .blocks
            .iter()
            .filter(|block| block.section.name() == Some(name))
            .collect();
        assert_eq!(blocks.len(), 1);
        assert_eq!(&blocks[0].payload, expected);
    }
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
        assert_eq!(original.of.faces(), actual.of.faces());
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
    let scan = crate::container::scan(&ctx, root).unwrap();
    let retained =
        crate::writer::retained_display_sections(decoded.ir(), Some(&baseline), Some(&scan))
            .unwrap();
    assert_eq!(retained, vec![("Contents/DisplayLists".into(), display)]);
    assert!(
        crate::writer::retained_display_sections(&changed, Some(&baseline), Some(&scan)).is_none()
    );
}

#[test]
fn metadata_edits_preserve_configuration_xml_and_explicit_renames_update_it() {
    let history: &[u8] = br#"<Keywords Name="Authored"><Configuration Name="Saved"/><Feature Name="NativeTree" Type="Origin" id="11"/></Keywords>"#;
    let ordinary: &[u8] = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swConfigurationName = 'Saved'/></swSolidWorks>"#;
    let stale: &[u8] = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel id="stale" swConfigurationName="Stale" swConfigurationId="1"/><swModel id="saved" swConfigurationName="Saved" swConfigurationId="0"/><swConfiguration swID="0" swName="Saved" swModelRef="saved" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#;
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
        let mut unchanged = Vec::new();
        crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut unchanged,
        )
        .unwrap();
        let scan = crate::test_support::container::scan(&unchanged);
        let original_xml = scan
            .blocks
            .iter()
            .find(|block| block.section.name() == Some("swXmlContents/Features"))
            .unwrap();
        assert_eq!(original_xml.payload, xml);
        // Source XML deliberately has no SourceIndex. An indexed active
        // partition enriches the decoded native configuration to Some(0),
        // while the untouched complete Keywords bytes must still replay.
        let source_native = sldprt_native(&source);
        assert_eq!(source_native.feature_histories.len(), 1);
        assert_eq!(
            source_native.feature_histories[0].configurations[0].source_index,
            selected.source_index
        );
        let original_history = scan
            .blocks
            .iter()
            .find(|block| block.section.name() == Some("Contents/Keywords"))
            .unwrap();
        assert_eq!(original_history.payload, history);
        if rename {
            decoded
                .ir_mut()
                .model
                .configurations
                .iter_mut()
                .find(|configuration| configuration.active)
                .unwrap()
                .name = Some("RenamedSaved".into());
            let mut renamed = Vec::new();
            crate::test_support::plan_inherited_write(
                decoded.ir(),
                decoded.source_fidelity(),
                &mut renamed,
            )
            .unwrap();
            let scan = crate::test_support::container::scan(&renamed);
            let changed_xml = scan
                .blocks
                .iter()
                .find(|block| block.section.name() == Some("swXmlContents/Features"))
                .unwrap();
            let envelope =
                crate::container::first_solidworks_envelope([changed_xml.payload.as_slice()])
                    .unwrap();
            assert_eq!(envelope.configuration_name.as_deref(), Some("RenamedSaved"));
            let changed_history = scan
                .blocks
                .iter()
                .find(|block| block.section.name() == Some("Contents/Keywords"))
                .unwrap();
            assert_ne!(changed_history.payload, history);
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
fn duplicate_feature_owned_names_write_each_retained_member() {
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
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let owned: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(member))
        .collect();
    assert_eq!(owned.len(), 2);
    assert!(owned.iter().all(|block| block.payload == opaque));
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sldprt_native(rewritten.ir()).feature_input_lanes.len(), 3);
}

#[test]
fn digest_only_owner_beside_a_patchable_owner_is_named_without_an_edit() {
    use cadmpeg_ir::codec::write::Encoder as _;
    const EXTRA: &str = "Contents/Extra";
    let mut extra = b"moBBoxCenterData_c".to_vec();
    extra.extend(1u32.to_le_bytes());
    for value in [0.05f64, 0.06, -0.07, 0.08] {
        extra.extend(value.to_le_bytes());
    }
    let (mut decoded, _) =
        shared_member_with_sections(resolved_features_payload(&[0]), &[(EXTRA, &extra)]);
    let extra_attributes: Vec<String> = decoded
        .source_fidelity()
        .annotations
        .provenance
        .iter()
        .filter(|(id, provenance)| {
            id.starts_with("sldprt:metadata:") && provenance.stream() == EXTRA
        })
        .map(|(id, _)| id.clone())
        .collect();
    assert!(
        !extra_attributes.is_empty(),
        "EXTRA owns no metadata attribute"
    );
    let block = decoded
        .source_fidelity()
        .annotations
        .provenance
        .iter()
        .find(|(id, provenance)| {
            id.starts_with("sldprt:file:block#") && provenance.stream() == EXTRA
        })
        .map(|(id, _)| id.clone())
        .unwrap();
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    crate::test_support::make_retained_record_unavailable(decoded.source_fidelity_mut(), &block);
    let plan = SldprtCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput::new(
                decoded.ir(),
                Some(decoded.source_fidelity()),
            ),
            cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
        )
        .unwrap();
    let losses: Vec<String> = plan
        .report()
        .losses
        .iter()
        .map(|loss| {
            format!(
                "{}:{} {}",
                loss.code.namespace(),
                loss.code.local_code(),
                loss.message
            )
        })
        .collect();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let scan = crate::test_support::container::scan(&written);
    let members: Vec<String> = scan
        .blocks
        .iter()
        .filter_map(|block| block.section.name().map(str::to_owned))
        .collect();
    assert!(members.iter().any(|member| member == MEMBER));
    assert!(
        members.iter().all(|member| member != EXTRA),
        "EXTRA was written"
    );
    let omitted = format!("{EXTRA} ({block})");
    assert!(
        losses.iter().any(
            |loss| loss.starts_with("export:native.records-omitted") && loss.contains(&omitted)
        ),
        "no loss names the dropped member: {losses:?}"
    );
    for id in &extra_attributes {
        assert!(
            losses
                .iter()
                .any(|loss| loss.starts_with("sldprt:writer.attributes-omitted")
                    && loss.contains(id.as_str())),
            "no loss names {id}: {losses:?}"
        );
    }
}
