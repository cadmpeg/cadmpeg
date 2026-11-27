// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::Severity;

use crate::layout::{
    kernel_entity_link as link_layout, meta_external_reference as external,
    meta_external_revision as revision, meta_segment_pair as pair, pm_dc_content_header as header,
};
use crate::test_support::base_envelope::{decode, fixtures};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream_with_body_key, base_envelope_fixture, push_u16, push_u32,
};
use crate::InventorCodec;

mod derived;
mod history;

fn base_inputs<'a>(
    document: &'a crate::rse::RseInventory<'a>,
    features: &'a crate::feature::FeatureInventory,
    design: &'a crate::design::DesignInventory,
    keys: &'a [cadmpeg_asm::brep::records::BodyNativeKey],
) -> crate::feature::base::Inputs<'a> {
    crate::feature::base::Inputs {
        document,
        inventory: features,
        design,
        transforms: &[],
        keys,
        states: None,
    }
}

#[test]
fn stored_base_keeps_history_order_suppression_and_native_result_identity() {
    let (design, base) = fixtures();
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        &design,
        &base,
    );
    let (ir, report, fidelity) = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic Base decode")
        .into_parts();
    let neutral =
        cadmpeg_ir::validate::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
            .expect("neutral validation");
    assert!(
        !neutral
            .findings
            .iter()
            .any(|finding| finding.severity >= Severity::Error),
        "{:?}",
        neutral.findings
    );
    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.feature_result_topologies.len(), 2);
    for feature in &ir.model.features {
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        assert!(feature.source_properties.contains_key("result_body_link"));
    }
    let mut history = ir.model.features.iter().collect::<Vec<_>>();
    history.sort_unstable_by_key(|feature| feature.ordinal);
    assert!(history[0]
        .native_ref
        .as_ref()
        .expect("source")
        .ends_with("-6"));
    assert_eq!(history[0].ordinal, 1);
    assert_eq!(history[0].suppressed, Some(true));
    assert!(history[1]
        .native_ref
        .as_ref()
        .expect("source")
        .ends_with("-1"));
    assert_eq!(history[1].ordinal, 2);
    assert_eq!(history[1].suppressed, Some(false));
    for result in &ir.model.feature_result_topologies {
        assert_eq!(result.bodies().len(), 1);
        assert_eq!(
            result.native_ref.as_deref(),
            Some(result.bodies()[0].as_str())
        );
        assert!(result.bodies()[0].as_str().ends_with("-3"));
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("native validation");
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity >= Severity::Error),
        "{findings:?}"
    );
    let round_trip: cadmpeg_ir::CadIr =
        json::from_slice(&json::to_vec(&ir).expect("serialize")).expect("deserialize");
    let findings = InventorCodec
        .validate_native(&ctx, &round_trip, None)
        .expect("round-trip native validation");
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity >= Severity::Error),
        "{findings:?}"
    );
    for change in 0..2 {
        let mut changed = ir.clone();
        if change == 0 {
            changed.model.features[0].source_properties.insert(
                cadmpeg_core::nonblank_literal!("result_body_link"),
                "inventor:pmdc:kernel-entity-link#seg-0".into(),
            );
        } else {
            changed.model.features[0]
                .source_properties
                .remove("result_body_link");
        }
        let findings = InventorCodec
            .validate_native(&ctx, &changed, None)
            .expect("changed native validation");
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains("Base result")));
    }
}

#[test]
fn base_keeps_native_operation_when_result_join_does_not_close() {
    for change in 0..10 {
        let (mut design, mut base) = fixtures();
        match change {
            0 => base.references[external::REVISION_INDEX..external::REVISION_INDEX + 4]
                .copy_from_slice(&1_u32.to_le_bytes()),
            1 => base.references[external::KEY_EXTENSION..external::KEY_EXTENSION + 2]
                .copy_from_slice(&1_u16.to_le_bytes()),
            2 => base.revisions[revision::REFERENCE_COUNT..revision::REFERENCE_COUNT + 2]
                .copy_from_slice(&2_u16.to_le_bytes()),
            3 => base.pairs[pair::SEGMENT_ID] ^= 1,
            4 => base.brep[0].1[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4]
                .copy_from_slice(&8_u32.to_le_bytes()),
            5 => {
                let duplicate = base.brep[0].clone();
                base.brep.push(duplicate);
            }
            6 => design[2].1[header::LEN..header::LEN + 4]
                .copy_from_slice(&0x8000_0004_u32.to_le_bytes()),
            7 => design[3].1[header::LEN..header::LEN + 4]
                .copy_from_slice(&0x8000_0001_u32.to_le_bytes()),
            8 => design[2].1[header::LEN..header::LEN + 4]
                .copy_from_slice(&0x8000_0000_u32.to_le_bytes()),
            _ => {
                let mut unused_revision = vec![0x43; 16];
                push_u16(&mut unused_revision, 0);
                push_u16(&mut unused_revision, 1);
                base.revisions.extend(unused_revision);
            }
        }
        let ir = decode(&design, &base);
        assert_eq!(ir.model.features.len(), 2);
        assert!(ir.model.features.iter().all(|feature| matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        )));
        assert!(ir.model.feature_result_topologies.is_empty());
    }
}

#[test]
fn base_source_body_uses_its_local_carrier_and_independent_key() {
    let (mut design, base) = fixtures();
    design[2].1[header::LEN..header::LEN + 4].copy_from_slice(&0x8000_0009_u32.to_le_bytes());
    let mut link = base.brep[0].1.clone();
    link[link_layout::HEADER_ID..link_layout::HEADER_ID + 2].copy_from_slice(&88_u16.to_le_bytes());
    link[link_layout::SELECTION_SOURCE..link_layout::SELECTION_SOURCE + 4]
        .copy_from_slice(&101_u32.to_le_bytes());
    link[link_layout::BODY_REFERENCE..link_layout::BODY_REFERENCE + 4]
        .copy_from_slice(&0x8000_000a_u32.to_le_bytes());
    link[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4].copy_from_slice(&9_u32.to_le_bytes());
    design.push((base.brep[0].0, link));
    design.push((
        crate::kernel::KERNEL_RECORD_TYPE_ID,
        crate::test_support::test_fixtures::stored_kernel_body_fixture(
            &acis_sphere_kernel_stream_with_body_key(21_800, 9),
            0x8000_000a,
        ),
    ));
    let ir = decode(&design, &base);
    assert_eq!(ir.model.feature_result_topologies.len(), 2);
    assert!(ir.model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    )));
    design[8].1[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4]
        .copy_from_slice(&10_u32.to_le_bytes());
    let ir = decode(&design, &base);
    assert!(ir.model.feature_result_topologies.is_empty());
    assert!(ir.model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    )));
}

#[test]
fn base_result_follows_nonzero_external_revision_and_segment_indices() {
    let (mut design, mut base) = fixtures();
    base.pairs.splice(0..0, [0x19; 32]);
    base.revisions[revision::SEGMENT_PAIR_INDEX..revision::SEGMENT_PAIR_INDEX + 2]
        .copy_from_slice(&1_u16.to_le_bytes());
    let mut other_revision = vec![0x18; 16];
    push_u16(&mut other_revision, 0);
    push_u16(&mut other_revision, 1);
    base.revisions.splice(0..0, other_revision);
    base.references[external::REVISION_INDEX..external::REVISION_INDEX + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    let mut other_reference = Vec::new();
    push_u32(&mut other_reference, 0);
    push_u16(&mut other_reference, 0);
    push_u16(&mut other_reference, 66);
    base.references.splice(0..0, other_reference);
    design[3].1[header::LEN..header::LEN + 4].copy_from_slice(&2_u32.to_le_bytes());
    let ir = decode(&design, &base);
    assert_eq!(ir.model.feature_result_topologies.len(), 2);
    assert!(ir.model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    )));
}

#[test]
fn base_native_validation_rejects_ambiguous_or_invalid_result_identity() {
    let (design, base) = fixtures();
    let ir = decode(&design, &base);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for change in 0..5 {
        let mut changed = ir.clone();
        if change == 0 {
            changed
                .model
                .feature_result_topologies
                .push(ir.model.feature_result_topologies[0].clone());
        } else {
            let namespace = changed
                .native
                .namespace_mut(cadmpeg_ir::codec_id!("inventor"));
            let arena_name = match change {
                3 => "segment_meta",
                4 => "pm_dc_feature_properties",
                _ => "pm_dc_kernel_entity_links",
            };
            let mut records: Vec<serde_json::Value> =
                namespace.arena_as(arena_name).expect("native records");
            if change == 1 {
                records[0]["header_value"] = 1.into();
            } else if change == 2 {
                let mut duplicate = records[0].clone();
                let token = duplicate["segment_token"].as_str().expect("segment token");
                duplicate["id"] = format!("inventor:pmdc:kernel-entity-link#{token}-2").into();
                duplicate["record_ordinal"] = 2.into();
                records.push(duplicate);
            } else if change == 3 {
                let target_id = records
                    .iter()
                    .find(|record| record["kind"] == "pm_brep")
                    .expect("current carrier metadata")["segment_id"]
                    .clone();
                for record in &mut records {
                    record["segment_id"] = target_id.clone();
                }
            } else {
                let output = records
                    .iter_mut()
                    .find(|record| record["record_ordinal"] == 3)
                    .expect("result wrapper");
                output["kind"]["body"]["index"] = 2.into();
            }
            namespace
                .set_arena(&ctx, arena_name, &records)
                .expect("changed native records");
        }
        let findings = InventorCodec
            .validate_native(&ctx, &changed, None)
            .expect("changed native validation");
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("Base result")),
            "mutation {change}: {findings:?}"
        );
    }
}

#[test]
fn base_projection_rejects_duplicate_segment_identifiers() {
    use cadmpeg_container::compound::CompoundSnapshot;

    use crate::rse::{RseInventory, SegmentKind, SegmentMetaState};

    let (design, base) = fixtures();
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        &design,
        &base,
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("compound");
    let mut document = RseInventory::build(&ctx, &snapshot).expect("RSe inventory");
    let features = crate::feature::inventory(&ctx, &document).expect("feature inventory");
    let mut ir = decode(&design, &base);
    let design = crate::design::inventory(&ctx, &document).expect("design inventory");
    let keys = ir
        .native
        .namespace("inventor")
        .expect("native namespace")
        .arena_as("body_native_keys")
        .expect("body keys");
    ir.model.feature_result_topologies.clear();
    for feature in &mut ir.model.features {
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".into()),
                parameters: BTreeMap::new(),
            }));
    }
    let mut valid_ir = ir.clone();
    assert_eq!(
        crate::feature::base::project(
            &ctx,
            &base_inputs(&document, &features, &design, &keys),
            &mut valid_ir
        )
        .expect("valid Base projection")
        .transferred,
        2
    );
    let target_id = document
        .segments
        .iter()
        .find_map(|segment| match &segment.meta {
            SegmentMetaState::Parsed(meta) if segment.kind == SegmentKind::PmBRep => {
                Some(meta.segment_id)
            }
            _ => None,
        })
        .expect("current carrier metadata");
    for segment in &mut document.segments {
        if let SegmentMetaState::Parsed(meta) = &mut segment.meta {
            meta.segment_id = target_id;
        }
    }
    assert_eq!(
        crate::feature::base::project(
            &ctx,
            &base_inputs(&document, &features, &design, &keys),
            &mut ir
        )
        .expect("ambiguous Base projection")
        .transferred,
        0
    );
    assert!(ir.model.feature_result_topologies.is_empty());
}
