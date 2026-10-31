// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{reference, reference_list, test_feature, test_property};
use crate::pmdc::PmDcU32List;

pub(crate) fn fixture() -> (PmDcFeature, Vec<PmDcFeatureProperty>) {
    (
        test_feature(100, 16, &[(0, 2), (11, 1)]),
        vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Fillet,
                    type_value: 2,
                    value: 0,
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                    items: reference_list(&[4]),
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::FilletEdgeSet {
                    edges: reference(5),
                    radius: reference(50),
                    selection: reference(6),
                    continuity: reference(7),
                    option: None,
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::EdgeCollection,
                    items: reference_list(&[8]),
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::WideEnumeration {
                    type_value: 4,
                    value: 0,
                },
            ),
            test_property(
                6,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            test_property(
                7,
                PmDcFeaturePropertyKind::EdgeItem {
                    index_references: PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        vec![9],
                    )
                    .expect("index list"),
                    index_reference_value: -1,
                    value: 0,
                },
            ),
        ],
    )
}

fn read<'a>(
    source: &PmDcFeature,
    properties: &'a [PmDcFeatureProperty],
    length: f64,
    corner: bool,
) -> Result<Vec<Operand<'a>>, NativeReason> {
    operands(
        source,
        corner,
        |r| {
            properties
                .iter()
                .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
        },
        |_| Length::new(length),
    )
}

#[test]
fn fillet_operands_and_validator_keep_each_constant_radius_edge_set() {
    let (source, mut properties) = fixture();
    change(&mut properties[1], |kind| {
        if let PmDcFeaturePropertyKind::References { items, .. } = kind {
            *items = reference_list(&[4, 9]);
        }
    });
    let mut second = properties[2].clone();
    second.identity.record_ordinal = 8;
    change(&mut second, |kind| {
        if let PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } = kind {
            *radius = reference(51);
        }
    });
    properties.push(second);
    let expected = operands(
        &source,
        false,
        |r| {
            properties
                .iter()
                .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
        },
        |r| Length::new(if r.index() == 50 { 2.0 } else { 3.0 }),
    )
    .expect("two sets");
    assert_eq!(
        expected
            .iter()
            .map(|set| set.radius.get())
            .collect::<Vec<_>>(),
        vec![2.0, 3.0]
    );
    assert_eq!(expected.len(), 2);
    let mut groups = expected
        .iter()
        .map(|operand| FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(operand.collection.id()),
            radius: RadiusSpec::Constant {
                radius: operand.radius,
            },
            tangency_weight: None,
        })
        .collect::<Vec<_>>();
    assert!(matches(&expected, &groups));
    groups[1].radius = RadiusSpec::Constant {
        radius: PositiveLength::new(4.0).expect("radius"),
    };
    assert!(!matches(&expected, &groups));
    groups.pop();
    assert!(!matches(&expected, &groups));
}

#[test]
fn fillet_first_refusal_and_loss_cover_each_operand_check() {
    for reason in [
        NativeReason::Slots,
        NativeReason::Law,
        NativeReason::Slot(1),
        NativeReason::Slot(10),
        NativeReason::Sets,
        NativeReason::Set,
        NativeReason::Selection,
        NativeReason::Continuity,
        NativeReason::Option,
        NativeReason::Edges,
        NativeReason::EmptyItems,
        NativeReason::Radius,
    ] {
        let (mut source, mut properties) = fixture();
        let mut radius = 2.0;
        match reason {
            NativeReason::Slots => source = test_feature(100, 0, &[]),
            NativeReason::Law => {
                change(&mut properties[0], |kind| {
                    if let PmDcFeaturePropertyKind::Enumeration { value, .. } = kind {
                        *value = 1;
                    }
                });
            }
            NativeReason::Slot(slot) => {
                source = test_feature(100, 16, &[(0, 2), (11, 1), (slot, 6)]);
            }
            NativeReason::Sets => {
                change(&mut properties[1], |kind| {
                    if let PmDcFeaturePropertyKind::References { items, .. } = kind {
                        *items = reference_list(&[]);
                    }
                });
            }
            NativeReason::Set => {
                properties.remove(2);
            }
            NativeReason::Selection => {
                change(&mut properties[4], |kind| {
                    if let PmDcFeaturePropertyKind::WideEnumeration { value, .. } = kind {
                        *value = 1;
                    }
                });
            }
            NativeReason::Continuity => {
                change(&mut properties[2], |kind| {
                    if let PmDcFeaturePropertyKind::FilletEdgeSet { continuity, .. } = kind {
                        *continuity = reference(0);
                    }
                });
            }
            NativeReason::Option => {
                change(&mut properties[2], |kind| {
                    if let PmDcFeaturePropertyKind::FilletEdgeSet { option, .. } = kind {
                        *option = Some(reference(6));
                    }
                });
            }
            NativeReason::Edges => {
                properties.remove(3);
            }
            NativeReason::EmptyItems => {
                change(&mut properties[6], |kind| {
                    if let PmDcFeaturePropertyKind::EdgeItem {
                        index_references, ..
                    } = kind
                    {
                        *index_references = PmDcU32List::new(2, None, vec![]).expect("empty list");
                    }
                });
            }
            NativeReason::Radius => radius = 0.0,
            NativeReason::Result => unreachable!(),
        }
        assert_eq!(
            read(&source, &properties, radius, false).err(),
            Some(reason)
        );
        assert_eq!(
            reason.note().code.local_code(),
            "feature.fillet-operands-native"
        );
        assert!(reason
            .note()
            .message
            .starts_with("Retained Inventor fillet natively:"));
    }
    assert!(NativeReason::Result.note().message.contains("result-body"));
}

#[test]
fn fillet_fifth_reference_and_continuity_require_the_exact_controls() {
    let (source, mut properties) = fixture();
    for option in [
        None,
        Some(reference(0)),
        Some(reference(7)),
        Some(reference(6)),
    ] {
        change(&mut properties[2], |kind| {
            if let PmDcFeaturePropertyKind::FilletEdgeSet { option: actual, .. } = kind {
                *actual = option;
            }
        });
        assert_eq!(
            read(&source, &properties, 2.0, false).is_ok(),
            option != Some(reference(6))
        );
    }
    properties.push(test_property(
        9,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    change(&mut properties[2], |kind| {
        if let PmDcFeaturePropertyKind::FilletEdgeSet { option, .. } = kind {
            *option = Some(reference(10));
        }
    });
    assert_eq!(
        read(&source, &properties, 2.0, false).err(),
        Some(NativeReason::Option)
    );
    change(&mut properties[2], |kind| {
        if let PmDcFeaturePropertyKind::FilletEdgeSet {
            option, continuity, ..
        } = kind
        {
            *option = None;
            *continuity = reference(0);
        }
    });
    assert_eq!(
        read(&source, &properties, 2.0, false).err(),
        Some(NativeReason::Continuity)
    );
    assert!(read(&source, &properties, 2.0, true).is_ok());
}

fn change(property: &mut PmDcFeatureProperty, edit: impl FnOnce(&mut PmDcFeaturePropertyKind)) {
    let mut kind = property.kind.clone();
    edit(&mut kind);
    *property = test_property(property.identity.record_ordinal, kind);
}

pub(crate) fn parameter(
    ordinal: u32,
    value: cadmpeg_ir::features::ParameterValue,
) -> (
    crate::design::PmDcParameter,
    cadmpeg_ir::features::DesignParameter,
) {
    let raw = crate::feature::tests::raw_parameter(ordinal);
    let neutral = crate::feature::tests::neutral_parameter(&raw, value);
    (raw, neutral)
}

pub(crate) fn label(chamfer: bool) -> crate::feature::PmDcFeatureLabel {
    crate::feature::tests::test_label(
        100,
        1,
        if chamfer {
            crate::feature::CHAMFER_CLASS_ID
        } else {
            crate::feature::FILLET_CLASS_ID
        },
        &[],
    )
}

#[test]
fn fillet_first_refusal_diagnostics_keep_the_existing_empty_item_charge() {
    assert!(native_loss(NativeReason::EmptyItems, false).is_none());
    let corner = native_loss(NativeReason::EmptyItems, true).expect("corner first refusal");
    assert_eq!(corner.code.local_code(), "feature.fillet-operands-native");
    assert!(corner.message.contains("index-reference list is empty"));
    assert!(native_loss(NativeReason::Result, false)
        .expect("result refusal")
        .message
        .contains("result-body"));
}

pub(crate) fn project_native_fixture(
    source: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
    chamfer: bool,
) -> crate::feature::FeatureProjection {
    let root = crate::record_identity::Located::new(
        crate::design::scope::PmDcModelRootPayload {
            save_version_major: 19,
            header_value: 0,
            header_id: 0,
            next: reference(0),
            flags: 0x40,
            flags_extension: None,
            identifier: "0".repeat(32),
            name: String::new(),
            members: reference_list(&[]),
            values: [0; 15],
            parameter_table: reference(0),
            brep_segment: "PmBRepSegment".into(),
            scale: cadmpeg_ir::scalar::FiniteReal::ZERO,
            terminal_value: 0,
        },
        crate::record_identity::RecordTypeId::from_bytes(crate::design::scope::MODEL_ROOT_TYPE),
        source.identity.segment_token.clone(),
        0,
    );
    let inventory = crate::feature::FeatureInventory {
        features: vec![source],
        properties,
        labels: vec![
            label(chamfer),
            crate::feature::tests::test_label(
                0,
                0,
                crate::feature::ClassId::try_from("0".repeat(32)).expect("root class"),
                &[101],
            ),
        ],
        ..Default::default()
    };
    let design = crate::design::DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: vec![root],
        brep_segments: vec![crate::design::scope::BrepSegmentName {
            display_name: "PmBRepSegment".into(),
            role: crate::design::scope::BrepSegmentRole::Document,
        }],
        issues: Vec::new(),
    };
    let sketch = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("projection context");
    crate::feature::project(
        &ctx,
        &inventory,
        &design,
        &sketch,
        &[],
        &crate::feature::ProjectedSketches {
            sketches: &[],
            entities: &[],
            profile_bodies: &[],
            result_model: None,
        },
    )
    .expect("native feature projection")
}

#[test]
fn fillet_projection_emits_the_first_refusal_loss() {
    let (source, mut properties) = fixture();
    change(&mut properties[0], |kind| {
        if let PmDcFeaturePropertyKind::Enumeration { value, .. } = kind {
            *value = 1;
        }
    });
    let projected = project_native_fixture(source, properties, false);
    assert_eq!(projected.features.len(), 1);
    let [loss] = projected.edge_treatment_losses.as_slice() else {
        panic!("one fillet refusal loss")
    };
    assert_eq!(
        loss.code,
        crate::loss::InventorLossCode::FeatureFilletOperandsNative
            .note("refusal")
            .code
    );
    assert_eq!(loss.code.local_code(), "feature.fillet-operands-native");
    assert!(loss.message.contains("slot 11"));
}
