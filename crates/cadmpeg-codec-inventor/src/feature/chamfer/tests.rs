// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{reference_list, test_feature, test_property};
use crate::pmdc::PmDcU32List;
use cadmpeg_ir::features::{edge_treatments::ChamferGroup, EdgeSelection};

pub(crate) fn fixture(form: u16) -> (PmDcFeature, Vec<PmDcFeatureProperty>) {
    (
        test_feature(
            100,
            12,
            &[(0, 1), (1, 5), (2, 49), (3, 50), (4, 3), (5, 4), (10, 51)],
        ),
        vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::EdgeCollection,
                    items: reference_list(&[3]),
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::EdgeItem {
                    index_references: indexes(),
                    index_reference_value: -1,
                    value: 0,
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Chamfer,
                    type_value: 2,
                    value: form,
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FaceCollection,
                    items: reference_list(&[7]),
                },
            ),
            test_property(
                6,
                PmDcFeaturePropertyKind::FaceItem {
                    index_references: indexes(),
                },
            ),
        ],
    )
}

fn indexes() -> PmDcU32List {
    PmDcU32List::new(2, Some(crate::pmdc::PmDcListMetadata::U32([1, 0])), vec![9])
        .expect("index list")
}

fn read<'a>(
    source: &PmDcFeature,
    properties: &'a [PmDcFeatureProperty],
    first: f64,
    second: f64,
    angle: f64,
) -> Result<Operands<'a>, NativeReason> {
    operands(
        source,
        |r| {
            properties
                .iter()
                .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
        },
        |slot| Length::new(if slot == 2 { first } else { second }),
        |_| Angle::new(angle),
    )
}

#[test]
fn chamfer_validator_checks_each_existing_form_and_direction() {
    for (form, first, second) in [(0, 2.0, 3.0), (1, 2.0, 3.0), (2, 2.0, 3.0), (2, 2.0, 2.0)] {
        let (source, properties) = fixture(form);
        let expected = read(&source, &properties, first, second, 0.5).expect("operands");
        let mut groups = vec![ChamferGroup {
            edges: EdgeSelection::Native(if form == 0 {
                properties[0].id()
            } else {
                source.id()
            }),
            spec: expected.spec,
        }];
        assert!(matches(&expected, &source, &groups, true));
        assert!(!matches(&expected, &source, &groups, false));
        groups[0].edges = EdgeSelection::Native(properties[1].id());
        assert!(!matches(&expected, &source, &groups, true));
        assert!(!matches(&expected, &source, &[], true));
    }
}

#[test]
fn chamfer_first_refusal_and_loss_cover_each_operand_check() {
    for reason in [
        NativeReason::Slots,
        NativeReason::Form,
        NativeReason::Edges,
        NativeReason::EmptyItems,
        NativeReason::Distance,
        NativeReason::Support,
        NativeReason::Angle,
        NativeReason::SecondDistance,
        NativeReason::Direction,
    ] {
        let form = if reason == NativeReason::SecondDistance {
            2
        } else {
            1
        };
        let (mut source, mut properties) = fixture(form);
        let (mut first, mut second, mut angle) = (2.0, 3.0, 0.5);
        match reason {
            NativeReason::Slots => source = test_feature(100, 0, &[]),
            NativeReason::Form => {
                change(&mut properties[2], |kind| {
                    if let PmDcFeaturePropertyKind::Enumeration { value, .. } = kind {
                        *value = 3;
                    }
                });
            }
            NativeReason::Edges => {
                properties.remove(0);
            }
            NativeReason::EmptyItems => {
                change(&mut properties[1], |kind| {
                    if let PmDcFeaturePropertyKind::EdgeItem {
                        index_references, ..
                    } = kind
                    {
                        *index_references = PmDcU32List::new(2, None, vec![]).expect("empty list");
                    }
                });
            }
            NativeReason::Distance => first = 0.0,
            NativeReason::Support => {
                properties.pop();
            }
            NativeReason::Angle => angle = std::f64::consts::PI,
            NativeReason::SecondDistance => second = 0.0,
            NativeReason::Direction => {
                properties.remove(3);
            }
            NativeReason::Result | NativeReason::AngleReference | NativeReason::SideOwnership => {
                unreachable!()
            }
        }
        assert_eq!(
            read(&source, &properties, first, second, angle).err(),
            Some(reason)
        );
        assert_eq!(
            reason.note().code.local_code(),
            "feature.chamfer-operands-native"
        );
        assert!(reason
            .note()
            .message
            .starts_with("Retained Inventor chamfer operands natively:"));
    }
    assert!(NativeReason::Result.note().message.contains("result-body"));
}

fn change(property: &mut PmDcFeatureProperty, edit: impl FnOnce(&mut PmDcFeaturePropertyKind)) {
    let mut kind = property.kind.clone();
    edit(&mut kind);
    *property = test_property(property.identity.record_ordinal, kind);
}

#[test]
fn chamfer_partial_orientations_pin_the_first_unresolved_construction_operand() {
    for (form, detail) in [
        (ChamferForm::DistanceAngle, "distance-angle reference"),
        (ChamferForm::TwoDistances, "support-side ownership"),
    ] {
        let note = orientation_loss(ChamferSpec::Unresolved { form: Some(form) })
            .expect("native orientation");
        assert_eq!(note.code.local_code(), "feature.chamfer-operands-native");
        assert!(note.message.contains(detail));
    }
    assert!(orientation_loss(ChamferSpec::Distance {
        distance: PositiveLength::new(2.0).expect("distance")
    })
    .is_none());
}

#[test]
fn chamfer_projection_emits_the_first_refusal_loss() {
    let (source, properties) = fixture(3);
    let projected = crate::feature::fillet::tests::project_native_fixture(source, properties, true);
    assert_eq!(projected.features.len(), 1);
    let [loss] = projected.edge_treatment_losses.as_slice() else {
        panic!("one chamfer refusal loss")
    };
    assert_eq!(
        loss.code,
        crate::loss::InventorLossCode::FeatureChamferOperandsNative
            .note("refusal")
            .code
    );
    assert_eq!(loss.code.local_code(), "feature.chamfer-operands-native");
    assert!(loss.message.contains("slot 4"));
}

#[test]
fn chamfer_false_direction_remains_native() {
    let (source, mut properties) = fixture(0);
    change(&mut properties[3], |kind| {
        if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
            *value = false;
        }
    });
    assert_eq!(
        read(&source, &properties, 2.0, 3.0, 0.5).err(),
        Some(NativeReason::Direction)
    );
}
