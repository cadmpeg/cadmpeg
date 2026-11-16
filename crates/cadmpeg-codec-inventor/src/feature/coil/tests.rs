// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{admission, definition_matches, native_losses, NativeReason, CLASS_ID};
use crate::feature::tests::{test_feature, test_label, test_property};
use crate::feature::{
    native_feature, FeatureInventory, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
};
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};

pub(crate) fn source() -> PmDcFeature {
    test_feature(0, 20, &[(0, 1), (1, 2), (2, 3)])
}

pub(crate) fn properties() -> Vec<PmDcFeatureProperty> {
    let patch = PmDcReferenceList::new(
        2,
        Some(PmDcListMetadata::U32([1, 0])),
        vec![PmDcReference::from_packed(0x8000_0005)],
    )
    .expect("patch");
    [
        (
            1,
            super::super::PART_OPERATION_TYPE,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            },
        ),
        (
            2,
            super::super::BOUNDARY_PATCH_TYPE,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: patch,
            },
        ),
        (
            3,
            super::super::WORK_AXIS_TYPE,
            PmDcFeaturePropertyKind::WorkAxis {
                entity_flags: 0,
                values: vec![0, 0],
                references: PmDcReferenceList::new(2, None, vec![]).expect("axis list"),
                marker: false,
                origin: [cadmpeg_ir::scalar::FiniteReal::ZERO; 3],
                span: [
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("span"),
                ],
                state: 1,
            },
        ),
    ]
    .into_iter()
    .map(|(ordinal, type_id, kind)| {
        let mut record = test_property(ordinal, kind);
        record.identity.type_id = RecordTypeId::from_bytes(type_id);
        record
    })
    .collect()
}

fn edit_property(
    record: &mut PmDcFeatureProperty,
    edit: impl FnOnce(&mut crate::feature::PmDcFeaturePropertyPayload),
) {
    let mut payload = (**record).clone();
    edit(&mut payload);
    *record = Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    );
}

fn reason(source: &PmDcFeature, properties: &[PmDcFeatureProperty]) -> NativeReason {
    admission(source, |ordinal| {
        properties
            .iter()
            .find(|p| p.identity.record_ordinal == ordinal)
    })
    .expect_err("Coil remains native")
}

#[test]
fn coil_refusals_pin_the_loss_code_and_first_unresolved_operand() {
    let source = source();
    let mut payload = (*source).clone();
    payload.properties = PmDcReferenceList::new(
        8,
        Some(PmDcListMetadata::U16([20, 0])),
        source.properties.references().to_vec(),
    )
    .expect("synthetic list");
    let marker = Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        0,
    );
    let mut operation = properties();
    edit_property(&mut operation[0], |payload| {
        payload.kind = PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value: 2,
        };
    });
    for (source, properties, expected) in [
        (marker, properties(), NativeReason::Marker),
        (test_feature(0, 19, &[]), properties(), NativeReason::Slots),
        (test_feature(0, 21, &[]), properties(), NativeReason::Slots),
        (source.clone(), operation, NativeReason::Operation),
        (
            source.clone(),
            properties()[..1].to_vec(),
            NativeReason::Profile,
        ),
        (
            source.clone(),
            properties()[..2].to_vec(),
            NativeReason::Axis,
        ),
        (source, properties(), NativeReason::Polarity),
    ] {
        assert_eq!(reason(&source, &properties), expected);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let label = test_label(0, 1, CLASS_ID, &[]);
        let feature = native_feature(&ctx, &source.identity, Some(&label), source.id())
            .expect("native feature");
        assert!(definition_matches(
            &feature,
            &source,
            false,
            std::slice::from_ref(&label),
            &properties
        ));
        let inventory = FeatureInventory {
            features: vec![source],
            properties,
            labels: vec![label],
            ..Default::default()
        };
        assert!(native_losses(&inventory, &[]).is_empty());
        let losses = native_losses(&inventory, &[feature]);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.namespace(), "inventor");
        assert_eq!(losses[0].code.local_code(), "feature.coil-operands-native");
        assert_eq!(
            losses[0].message,
            format!("Retained Inventor Coil natively: {}.", expected.detail())
        );
    }
    for (reason, detail) in [
        (
            NativeReason::Operation,
            "slot 0 requires its Coil operation and input/result body joins",
        ),
        (
            NativeReason::Profile,
            "slot 1 requires its Coil profile selection",
        ),
        (
            NativeReason::Axis,
            "slot 2 requires its Coil axis controls and orientation",
        ),
        (
            NativeReason::Polarity,
            "slot 3 Boolean construction role and polarity are unresolved",
        ),
    ] {
        assert_eq!(reason.detail(), detail);
    }
}

#[test]
fn coil_prefix_checks_record_identity_type_version_and_controls() {
    let source = source();
    for slot in 0..3 {
        for change in 0..4 {
            let mut properties = properties();
            match change {
                0 => properties[slot].identity.segment_token = cadmpeg_ir::identity_key!("other"),
                1 => properties[slot].identity.record_ordinal += 10,
                2 => properties[slot].identity.type_id = RecordTypeId::from_bytes([0x42; 16]),
                _ => edit_property(&mut properties[slot], |payload| {
                    payload.save_version_major += 1;
                }),
            }
            assert_eq!(
                reason(&source, &properties),
                [
                    NativeReason::Operation,
                    NativeReason::Profile,
                    NativeReason::Axis
                ][slot]
            );
        }
    }
    for state in [0, 1] {
        let mut properties = properties();
        edit_property(&mut properties[2], |payload| {
            let PmDcFeaturePropertyKind::WorkAxis { state: actual, .. } = &mut payload.kind else {
                panic!("axis")
            };
            *actual = state;
        });
        assert_eq!(reason(&source, &properties), NativeReason::Polarity);
    }
    for change in 0..8 {
        let mut properties = properties();
        edit_property(&mut properties[2], |payload| {
            let PmDcFeaturePropertyKind::WorkAxis {
                entity_flags,
                values,
                marker,
                state,
                references,
                span,
                ..
            } = &mut payload.kind
            else {
                panic!("axis")
            };
            match change {
                0 => *entity_flags = 1,
                1 => values[0] = 1,
                2 => *marker = true,
                3 => *state = 2,
                4 => *span = [cadmpeg_ir::scalar::FiniteReal::ZERO; 3],
                5 => {
                    values.pop();
                }
                6 => *references = PmDcReferenceList::new(8, None, vec![]).expect("axis marker"),
                _ => {
                    *references = PmDcReferenceList::new(
                        2,
                        Some(PmDcListMetadata::U32([1, 0])),
                        vec![PmDcReference::from_packed(0x8000_0005)],
                    )
                    .expect("axis list");
                }
            }
        });
        assert_eq!(reason(&source, &properties), NativeReason::Axis);
    }
}

#[test]
fn coil_loss_uses_class_owner_segment_and_placed_native_record() {
    let source = source();
    let label = test_label(0, 1, CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let feature =
        native_feature(&ctx, &source.identity, Some(&label), source.id()).expect("native feature");
    let mut inventory = FeatureInventory {
        features: vec![source],
        properties: properties(),
        labels: vec![label],
        ..Default::default()
    };
    for change in 0..3 {
        let original = inventory.labels[0].clone();
        inventory.labels[0] = match change {
            0 => test_label(1, 1, CLASS_ID, &[]),
            1 => test_label(0, 1, super::super::THREAD_CLASS_ID, &[]),
            _ => {
                let mut label = original.clone();
                label.identity.segment_token = cadmpeg_ir::identity_key!("other");
                label
            }
        };
        assert!(native_losses(&inventory, std::slice::from_ref(&feature)).is_empty());
        inventory.labels[0] = original;
    }
    inventory.labels.push(inventory.labels[0].clone());
    assert_eq!(native_losses(&inventory, &[feature]).len(), 1);
}

#[test]
fn coil_prefix_checks_reference_qualifiers_and_parameter_forms() {
    let source = source();
    for slot in 0..3 {
        for packed in [0, 0x8000_0000, 2] {
            let mut payload = (*source).clone();
            let mut references = source.properties.references().to_vec();
            references[slot] = PmDcReference::from_packed(packed);
            payload.properties =
                PmDcReferenceList::new(2, Some(PmDcListMetadata::U32([20, 0])), references)
                    .expect("vector");
            let source = Located::new(
                payload,
                source.identity.type_id.clone(),
                source.identity.segment_token.clone(),
                source.identity.record_ordinal,
            );
            assert_eq!(
                reason(&source, &properties()),
                [
                    NativeReason::Operation,
                    NativeReason::Profile,
                    NativeReason::Axis
                ][slot]
            );
        }
        let mut properties = properties();
        edit_property(&mut properties[slot], |payload| {
            payload.kind = PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            }
        });
        assert_eq!(
            reason(&source, &properties),
            [
                NativeReason::Operation,
                NativeReason::Profile,
                NativeReason::Axis
            ][slot]
        );
    }
    for (family, type_value, value) in [
        (PmDcFeatureEnumFamily::Hole, 5, 1),
        (PmDcFeatureEnumFamily::PartOperation, 4, 1),
        (PmDcFeatureEnumFamily::PartOperation, 5, 0),
        (PmDcFeatureEnumFamily::PartOperation, 5, 2),
        (PmDcFeatureEnumFamily::PartOperation, 5, 3),
        (PmDcFeatureEnumFamily::PartOperation, 5, 4),
    ] {
        let mut properties = properties();
        edit_property(&mut properties[0], |payload| {
            payload.kind = PmDcFeaturePropertyKind::Enumeration {
                family,
                type_value,
                value,
            }
        });
        assert_eq!(reason(&source, &properties), NativeReason::Operation);
    }
}

#[test]
fn coil_prefix_checks_patch_marker_count_and_member_references() {
    let source = source();
    for (marker, packed) in [
        (8, vec![0x8000_0005]),
        (2, vec![]),
        (2, vec![0x8000_0005, 0x8000_0006]),
        (2, vec![5]),
        (2, vec![0]),
        (2, vec![0x8000_0000]),
    ] {
        let mut properties = properties();
        let metadata = if packed.is_empty() {
            None
        } else if marker == 8 {
            Some(PmDcListMetadata::U16([
                u16::try_from(packed.len()).expect("count"),
                0,
            ]))
        } else {
            Some(PmDcListMetadata::U32([
                u32::try_from(packed.len()).expect("count"),
                0,
            ]))
        };
        let items = PmDcReferenceList::new(
            marker,
            metadata,
            packed.into_iter().map(PmDcReference::from_packed).collect(),
        )
        .expect("patch");
        edit_property(&mut properties[1], |payload| {
            payload.kind = PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items,
            }
        });
        assert_eq!(reason(&source, &properties), NativeReason::Profile);
    }
}
