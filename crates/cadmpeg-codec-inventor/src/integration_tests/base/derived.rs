// SPDX-License-Identifier: Apache-2.0
use super::{fixtures, header, link_layout, list, SyntheticRecords};
use crate::loss::InventorLossCode;
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream_with_body_key, base_envelope_fixture, content, push_u16, push_u32,
    stored_kernel_body_fixture, BaseEnvelopeRecords,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::Severity;

fn kind(hex: &str) -> [u8; 16] {
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex pair"), 16).expect("hex byte")
        })
        .collect();
    bytes.try_into().expect("sixteen-byte identifier")
}

fn derived(mirror: bool) -> (SyntheticRecords, BaseEnvelopeRecords) {
    let (mut design, mut base) = fixtures();
    base.design_version = 19;
    let mut feature = content(100);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    list(&mut feature, &[0x8000_0003, 0x8000_0004, 0x8000_0009]);
    push_u32(&mut feature, 0);
    design[1].1 = feature;
    let label = &mut design[5].1;
    let len = label.len();
    label[len - 16..].copy_from_slice(&kind("eecb298e2c45650820104eb55d062c80"));
    let mut source = content(110);
    push_u32(&mut source, 0x8000_000a);
    design.push((kind("3cc048210042420ba62103b20b7f4891"), source));
    let mut component = content(111);
    for value in [
        u32::MAX,
        0x8000_000b,
        0x8000_000c,
        0x8000_000d,
        0x8000_000e,
        0x3000_0004,
        0,
        0x8000_000f,
    ] {
        push_u32(&mut component, value);
    }
    design.push((kind("a034363fd31191b5c0000385e89c6b4f"), component));
    design.push((kind("9f34363fd31191b5c0000385e89c6b4f"), content(114)));
    design.push((kind("604d8790d011f8d10008cabc0663dc09"), content(115)));
    let mut transform = content(112);
    push_u16(&mut transform, 0x8421);
    push_u16(&mut transform, if mirror { 0x7bdf } else { 0x7bde });
    design.push((kind("184d8790d011f8d10008cabc0663dc09"), transform));
    design.push((kind("d77e5d25d311f2b5c0000385e89c6b4f"), content(116)));
    design.push((kind("a7fbcb1db64f16cfe979019eae94a4df"), content(117)));
    design[2].1[header::LEN..header::LEN + 4].copy_from_slice(&0x8000_0010_u32.to_le_bytes());
    let mut link = base.brep[0].1.clone();
    link[link_layout::HEADER_ID..link_layout::HEADER_ID + 2].copy_from_slice(&88_u16.to_le_bytes());
    link[link_layout::SELECTION_SOURCE..link_layout::SELECTION_SOURCE + 4]
        .copy_from_slice(&101_u32.to_le_bytes());
    link[link_layout::BODY_REFERENCE..link_layout::BODY_REFERENCE + 4]
        .copy_from_slice(&0x8000_0011_u32.to_le_bytes());
    link[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4].copy_from_slice(&9_u32.to_le_bytes());
    design.push((base.brep[0].0, link));
    design.push((
        crate::kernel::KERNEL_RECORD_TYPE_ID,
        stored_kernel_body_fixture(
            &acis_sphere_kernel_stream_with_body_key(21_800, 9),
            0x8000_0011,
        ),
    ));
    let mut tagging = content(120);
    push_u32(&mut tagging, 0);
    push_u32(&mut tagging, 0);
    list(&mut tagging, &[0x8000_0012]);
    push_u32(&mut tagging, 0);
    design[6].1 = tagging;
    let label = &mut design[7].1;
    let len = label.len();
    label[len - 16..].copy_from_slice(&kind("e899dbdfd31165afc0000185e89c6b4f"));
    let mut collection = content(121);
    list(&mut collection, &[0x8000_0004]);
    design.push((kind("ae70680ed14a1e86d76248b03c2a96e1"), collection));
    (design, base)
}

fn derived_at_version(mirror: bool, version: u8) -> (SyntheticRecords, BaseEnvelopeRecords) {
    let (mut design, mut base) = derived(mirror);
    base.design_version = version;
    if version == 23 {
        for ordinal in [2, 3, 8, 12, 17] {
            design[ordinal]
                .1
                .splice(header::LEN..header::LEN, (-1_i32).to_le_bytes());
        }
        push_u32(
            &mut design[9].1,
            crate::layout::pm_dc_derived_component_suffix::STATE_VALUE,
        );
        let footer = design[16].1.len() - 2 * std::mem::size_of::<u32>();
        design[16].1.insert(footer, 0);
    }
    (design, base)
}

fn decoded(
    design: &SyntheticRecords,
    base: &BaseEnvelopeRecords,
) -> cadmpeg_ir::codec::DecodeResult {
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        design,
        base,
    );
    crate::InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("derived decode")
}

#[test]
fn derived_identity_and_reflection_retain_exact_native_source_and_stored_result() {
    for (version, mirror) in [(19, false), (19, true), (23, false), (23, true)] {
        let (design, base) = derived_at_version(mirror, version);
        let (ir, report, fidelity) = decoded(&design, &base).into_parts();
        assert!(!report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedResultNative
                .note("resolved result")
                .code));
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedSourceNative
                .note("source retention")
                .code));
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedTaggingNative
                .note("tagging retention")
                .code));
        let tagging = ir
            .model
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some("inventor:pmdc:feature#dc-6"))
            .expect("native tagging");
        assert!(matches!(
            tagging.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        let feature = ir
            .model
            .features
            .iter()
            .find(|f| f.source_properties.contains_key("derived_source"))
            .expect("projected derived body");
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        assert_eq!(
            feature.source_properties["derived_source"],
            "inventor:pmdc:feature-property#dc-9"
        );
        assert_eq!(
            feature.source_properties["derived_transform"],
            "inventor:pmdc:transform#dc-12"
        );
        let transforms: Vec<serde_json::Value> = ir
            .native
            .namespace("inventor")
            .expect("namespace")
            .arena_as("pm_dc_transforms")
            .expect("transforms");
        assert_eq!(
            transforms[0]["matrix"][0][0],
            if mirror { -1.0 } else { 1.0 }
        );
        let neutral = cadmpeg_ir::validate::validate_neutral_with_source_fidelity(
            &ir,
            &fidelity,
            report.losses,
        )
        .expect("neutral validation");
        assert!(
            !neutral
                .findings
                .iter()
                .any(|f| f.severity >= Severity::Error),
            "{:?}",
            neutral.findings
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let findings = crate::InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation");
        assert!(
            !findings.iter().any(|f| f.severity >= Severity::Error),
            "{findings:?}"
        );
    }
}

#[test]
fn derived_refuses_each_changed_operand_and_pins_native_source_loss() {
    use crate::layout::pm_dc_derived_component_fields as fields;
    for change in 0..10 {
        let (mut design, base) = derived(false);
        match change {
            0 => design[8].1[header::LEN..header::LEN + 4].copy_from_slice(&0_u32.to_le_bytes()),
            1 => design[9].1[header::LEN + fields::STATE..header::LEN + fields::STATE + 4]
                .copy_from_slice(&0_u32.to_le_bytes()),
            2..=6 => design[10 + change - 2].0 = [0x77; 16],
            7 => design[9].1
                [header::LEN + fields::REFERENCES..header::LEN + fields::REFERENCES + 4]
                .copy_from_slice(&11_u32.to_le_bytes()),
            8 => design[8].1.push(0),
            _ => design[9].1[header::LEN + fields::COUNT..header::LEN + fields::COUNT + 4]
                .copy_from_slice(&1_u32.to_le_bytes()),
        }
        let (ir, report, _) = decoded(&design, &base).into_parts();
        let feature = ir
            .model
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some("inventor:pmdc:feature#dc-1"))
            .expect("derived feature");
        assert!(
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ),
            "mutation {change}"
        );
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedSourceNative
                .note("source retention")
                .code));
    }
}

#[test]
fn derived_validator_rejects_changed_source_transform_state_types_and_qualifiers() {
    for (version, mirror) in [(19, false), (19, true), (23, false), (23, true)] {
        let (design, base) = derived_at_version(mirror, version);
        let ir = decoded(&design, &base).into_parts().0;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        for change in 0..15 {
            let mut changed = ir.clone();
            if change < 2 {
                let feature = changed
                    .model
                    .features
                    .iter_mut()
                    .find(|f| f.source_properties.contains_key("derived_source"))
                    .expect("derived feature");
                feature.source_properties.remove(if change == 0 {
                    "derived_source"
                } else {
                    "derived_transform"
                });
            } else {
                let namespace = changed.native.namespace_mut("inventor");
                let arena_name = if (6..=8).contains(&change) {
                    "rse_records"
                } else {
                    "pm_dc_feature_properties"
                };
                let mut records: Vec<serde_json::Value> =
                    namespace.arena_as(arena_name).expect("native records");
                let ordinal = if matches!(change, 2 | 10 | 12 | 13) {
                    8
                } else if change == 6 {
                    10
                } else if change == 7 {
                    8
                } else {
                    9
                };
                let record = records
                    .iter_mut()
                    .find(|r| {
                        r[if (6..=8).contains(&change) {
                            "ordinal"
                        } else {
                            "record_ordinal"
                        }] == ordinal
                            && r[if (6..=8).contains(&change) {
                                "token"
                            } else {
                                "segment_token"
                            }] == "dc"
                    })
                    .expect("source record");
                match change {
                    2 => record["kind"]["definition"]["component"]["index"] = 0.into(),
                    3 => record["kind"]["definition"]["state"] = 0.into(),
                    4 => record["kind"]["definition"]["references"][2]["qualified"] = false.into(),
                    5 => record["kind"]["definition"]["terminal"]["index"] = 0.into(),
                    6 => record["type_id"] = "77777777777777777777777777777777".into(),
                    7 => {
                        record["payload_len"] = 25.into();
                        record["trailing_payload_len"] = 25.into();
                    }
                    8 => {
                        record["payload_len"] = 55.into();
                        record["trailing_payload_len"] = 55.into();
                    }
                    9 => record["save_version_major"] = 18.into(),
                    10 => record["header"]["stamps"] = serde_json::json!([0, 0]),
                    11 => record["header"]["stamps"] = serde_json::json!([0]),
                    12 => record["header"]["stamps"] = serde_json::json!([-2]),
                    _ => record["header"]["flags_extension"] = 0.into(),
                }
                namespace
                    .set_arena(&ctx, arena_name, &records)
                    .expect("changed records");
            }
            let findings = crate::InventorCodec
                .validate_native(&ctx, &changed, None)
                .expect("validation");
            assert!(
                findings.iter().any(|f| f.message.contains("Base result")),
                "mutation {change}: {findings:?}"
            );
        }
    }
}

#[test]
fn derived_keeps_translated_source_placement_without_changing_stored_result_geometry() {
    for version in [19, 23] {
        let (mut design, base) = derived_at_version(false, version);
        let mut transform = content(112);
        if version == 23 {
            push_u32(&mut transform, u32::MAX);
        }
        push_u16(&mut transform, 0x8421);
        push_u16(&mut transform, 0x7356);
        for value in [2.0_f64, 3.0, 4.0] {
            transform.extend_from_slice(&value.to_le_bytes());
        }
        design[12].1 = transform;
        let (ir, _, _) = decoded(&design, &base).into_parts();
        let transforms: Vec<serde_json::Value> = ir
            .native
            .namespace("inventor")
            .expect("namespace")
            .arena_as("pm_dc_transforms")
            .expect("source transforms");
        for (row, expected) in [2.0, 3.0, 4.0].into_iter().enumerate() {
            assert_eq!(transforms[0]["matrix"][row][3], expected);
        }
        let feature = ir
            .model
            .features
            .iter()
            .find(|f| f.source_properties.contains_key("derived_source"))
            .expect("derived stored body");
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        let ordinary = decoded(&derived_at_version(false, version).0, &base)
            .into_parts()
            .0;
        assert_eq!(ir.model.bodies, ordinary.model.bodies);
        assert_eq!(ir.model.points, ordinary.model.points);
        assert_eq!(ir.model.surfaces, ordinary.model.surfaces);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let findings = crate::InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation");
        assert!(
            !findings.iter().any(|f| f.severity >= Severity::Error),
            "{findings:?}"
        );
    }
}

#[test]
fn unsupported_derived_forms_keep_native_features_and_valid_native_links() {
    use crate::layout::pm_dc_derived_component_fields as fields;
    for change in 0..4 {
        let (mut design, mut base) = derived(false);
        match change {
            0 => base.design_version = 18,
            1 => design[8].1.push(0),
            2 => design[9].1.push(0),
            _ => design[9].1[header::LEN + fields::COUNT..header::LEN + fields::COUNT + 4]
                .copy_from_slice(&1_u32.to_le_bytes()),
        }
        let (ir, report, _) = decoded(&design, &base).into_parts();
        let feature = ir
            .model
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some("inventor:pmdc:feature#dc-1"))
            .expect("native derived feature");
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedSourceNative
                .note("source retention")
                .code));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let findings = crate::InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation");
        assert!(
            !findings.iter().any(|f| f.severity >= Severity::Error),
            "mutation {change}: {findings:?}"
        );
    }
}

#[test]
fn derived_validator_checks_all_three_feature_slot_qualifiers_and_nulls() {
    let (design, base) = derived(false);
    let ir = decoded(&design, &base).into_parts().0;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for slot in 0..3 {
        for null in [false, true] {
            let mut changed = ir.clone();
            let namespace = changed.native.namespace_mut("inventor");
            let mut records: Vec<serde_json::Value> =
                namespace.arena_as("pm_dc_features").expect("features");
            let source = records
                .iter_mut()
                .find(|record| record["segment_token"] == "dc" && record["record_ordinal"] == 1)
                .expect("derived feature");
            let reference = &mut source["properties"]["references"][slot];
            if null {
                reference["index"] = 0.into();
            } else {
                reference["qualified"] = false.into();
            }
            namespace
                .set_arena(&ctx, "pm_dc_features", &records)
                .expect("changed feature");
            let findings = crate::InventorCodec
                .validate_native(&ctx, &changed, None)
                .expect("validation");
            assert!(
                findings
                    .iter()
                    .any(|finding| finding.message.contains("Base result")),
                "slot {slot}, null {null}: {findings:?}"
            );
        }
    }
}

#[test]
fn derived_without_a_current_body_key_stays_native_with_source_loss() {
    for version in [19, 23] {
        let (design, mut base) = derived_at_version(false, version);
        base.brep[0].1[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4]
            .copy_from_slice(&99_u32.to_le_bytes());
        let (ir, report, _) = decoded(&design, &base).into_parts();
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some("inventor:pmdc:feature#dc-1"))
            .expect("derived feature");
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert!(!feature.source_properties.contains_key("derived_source"));
        let result_losses: Vec<_> = report
            .losses
            .iter()
            .filter(|loss| {
                loss.code
                    == InventorLossCode::FeatureDerivedResultNative
                        .note("result operand")
                        .code
            })
            .collect();
        assert_eq!(result_losses.len(), 1);
        assert_eq!(result_losses[0].message,
        "Derived result-body operand (slot 1) has an unresolved current-model body join; the feature remains native.");
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedSourceNative
                .note("unresolved derived source")
                .code));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let findings = crate::InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation");
        assert!(
            !findings
                .iter()
                .any(|finding| finding.severity >= Severity::Error),
            "{findings:?}"
        );
    }
}

#[test]
fn derived_version_23_refuses_suffix_polarity_missing_suffix_and_extra_stamps() {
    for change in 0..5 {
        let (mut design, base) = derived_at_version(false, 23);
        match change {
            0 => {
                let terminal =
                    design[9].1.len() - crate::layout::pm_dc_derived_component_suffix::LEN;
                design[9].1[terminal..].copy_from_slice(&1_u32.to_le_bytes());
            }
            1 => {
                design[9]
                    .1
                    .truncate(header::LEN + crate::layout::pm_dc_derived_component_fields::LEN);
            }
            2 => {
                push_u32(&mut design[9].1, 0);
            }
            3 => {
                design[8].1.drain(
                    header::LEN..header::LEN + crate::layout::pm_dc_derived_source_stamp::LEN,
                );
            }
            _ => {
                design[9]
                    .1
                    .splice(header::LEN..header::LEN, (-1_i32).to_le_bytes());
            }
        }
        let (ir, report, _) = decoded(&design, &base).into_parts();
        let feature = ir
            .model
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some("inventor:pmdc:feature#dc-1"))
            .expect("derived feature");
        assert!(
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ),
            "mutation {change}"
        );
        assert!(report.losses.iter().any(|loss| loss.code
            == InventorLossCode::FeatureDerivedSourceNative
                .note("source retention")
                .code));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let findings = crate::InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation");
        assert!(
            !findings.iter().any(|f| f.severity >= Severity::Error),
            "mutation {change}: {findings:?}"
        );
    }
}
