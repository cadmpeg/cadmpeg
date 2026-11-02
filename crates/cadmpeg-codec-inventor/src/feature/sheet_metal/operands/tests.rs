// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::sheet_metal::Fixture;

type Edit = (u32, fn(&mut PmDcFeaturePropertyKind), PlateRefusal);

fn projected(fixture: &Fixture) -> Result<PlateOperands, PlateRefusal> {
    plate(
        &fixture.plate,
        Some(&fixture.style),
        |reference| fixture.property(reference),
        Fixture::length,
        Fixture::direction,
        cadmpeg_ir::units::UnitVector3::Z_AXIS,
        cadmpeg_ir::units::UnitVector3::X_AXIS,
    )
}

#[test]
fn plate_dimensions_use_their_exact_operands_with_style_default_forms() {
    let fixture = Fixture::new();
    let value = projected(&fixture).expect("operands");
    assert_eq!(value.thickness.get(), 7.0);
    assert_eq!(value.inside_radius.get(), 11.0);
    assert_eq!(value.side, SheetMetalThicknessSide::Forward);
    assert_eq!(value.relief.form, SheetMetalReliefForm::StyleDefault);
    assert_eq!(value.relief.width.get(), 13.0);
    assert_eq!(value.relief.depth.get(), 17.0);
    assert_eq!(value.relief.minimum_remnant.get(), 0.0);
    assert_eq!(value.relief.seam_gap.get(), 19.0);
    assert_eq!(
        value.transition.form,
        SheetMetalTransitionForm::StyleDefault
    );
    assert_eq!(value.transition.radius.get(), 23.0);
}

#[test]
fn native_control_values_outside_the_admitted_branch_block_projection() {
    for index in [20, 21, 22, 23, 24, 12] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| match kind {
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = true,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::Enumeration { value, .. },
            } => *value = 0,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::PlateControl { value },
            } => *value = 1,
            _ => panic!("control"),
        });
        if index == 12 {
            fixture.edit(index, |kind| {
                if let PmDcFeaturePropertyKind::SheetMetal {
                    definition: SheetMetalProperty::PlateControl { value },
                } = kind
                {
                    *value = 1;
                }
            });
        }
        assert!(projected(&fixture).is_err());
    }
    let mut fixture = Fixture::new();
    fixture.edit(5, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Attachment { states, .. },
        } = kind
        {
            *states = cadmpeg_ir::native::bytes::NativeBytes::new([1, 0, 1]);
        }
    });
    assert!(projected(&fixture).is_err());
}

#[test]
fn equal_numeric_forms_in_different_domains_do_not_select_style_default() {
    let mut fixture = Fixture::new();
    fixture.edit(31, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration { type_value, .. },
        } = kind
        {
            *type_value = 304;
        }
    });
    assert!(projected(&fixture).is_err());
    let mut fixture = Fixture::new();
    fixture.edit(31, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration { value, .. },
        } = kind
        {
            *value = 0;
        }
    });
    assert!(projected(&fixture).is_err());
}

#[test]
fn relief_and_transition_booleans_require_the_pinned_true_value() {
    assert!(projected(&Fixture::new()).is_ok());
    for index in [29, 35] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| {
            if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                *value = false;
            }
        });
        assert!(projected(&fixture).is_err());
    }
}

#[test]
fn bend_requires_the_shared_plate_operands_and_its_pinned_controls() {
    let fixture = Fixture::new();
    assert!(bend(&fixture.plate, &fixture.bend, |reference| fixture
        .property(reference)));
    for index in [44, 45] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| match kind {
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::BendControl { value, .. },
            } => *value = 0,
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = false,
            _ => panic!("bend control"),
        });
        assert!(!bend(&fixture.plate, &fixture.bend, |reference| fixture
            .property(reference)));
    }
}

#[test]
fn attachment_extension_requires_the_repeated_operands_and_pinned_state() {
    use crate::test_support::sheet_metal::{property, reference};
    let mut fixture = Fixture::new();
    fixture.edit(5, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Attachment { extension, .. },
        } = kind
        {
            *extension = Some(reference(47));
        }
    });
    fixture.properties.push(property(
        47,
        &PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState {
                state: 10,
                thickness: reference(4),
                radius: reference(25),
                attachment: reference(5),
                relief_width: reference(26),
                minimum_remnant: reference(28),
                options: [reference(23), reference(24)],
            },
        },
    ));
    assert!(projected(&fixture).is_ok());
    fixture.edit(47, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState { state, .. },
        } = kind
        {
            *state = 11;
        }
    });
    assert!(projected(&fixture).is_err());
    fixture.edit(47, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState { state, radius, .. },
        } = kind
        {
            *state = 10;
            *radius = reference(26);
        }
    });
    assert!(projected(&fixture).is_err());
}

#[test]
fn dimensions_require_resolved_lengths_and_their_ir_domains() {
    let fixture = Fixture::new();
    for index in [4, 25, 26, 27] {
        assert!(plate(
            &fixture.plate,
            Some(&fixture.style),
            |r| fixture.property(r),
            |r| if r.index() == index {
                Length::new(0.0)
            } else {
                Fixture::length(r)
            },
            Fixture::direction,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        )
        .is_err());
    }
    for index in [4, 25, 26, 27, 28, 30, 36] {
        assert!(plate(
            &fixture.plate,
            Some(&fixture.style),
            |r| fixture.property(r),
            |r| if r.index() == index {
                None
            } else {
                Fixture::length(r)
            },
            Fixture::direction,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        )
        .is_err());
        assert!(plate(
            &fixture.plate,
            Some(&fixture.style),
            |r| fixture.property(r),
            |r| if r.index() == index {
                Length::new(-1.0)
            } else {
                Fixture::length(r)
            },
            Fixture::direction,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        )
        .is_err());
    }
}

#[test]
fn bend_state_and_repeated_operand_mismatches_stay_native() {
    let mut fixture = Fixture::new();
    fixture.edit(44, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::BendControl { state, .. },
        } = kind
        {
            *state = None;
        }
    });
    assert!(!bend(&fixture.plate, &fixture.bend, |r| fixture.property(r)));
    let fixture = Fixture::new();
    for slot in [0, 1, 2, 3, 4, 7, 9] {
        let mut slots = fixture
            .bend
            .properties
            .references()
            .iter()
            .map(|r| r.index())
            .collect::<Vec<_>>();
        slots[slot] = 999;
        let source = crate::test_support::sheet_metal::feature(200, &slots);
        assert!(!bend(&fixture.plate, &source, |r| fixture.property(r)));
    }
}

#[test]
fn slot_one_is_antiparallel_to_the_first_attachment_direction() {
    use cadmpeg_ir::units::UnitVector3;
    let fixture = Fixture::new();
    assert!(projected(&fixture).is_ok());
    for stored in [UnitVector3::Z_AXIS, UnitVector3::X_AXIS] {
        assert!(plate(
            &fixture.plate,
            Some(&fixture.style),
            |r| fixture.property(r),
            Fixture::length,
            |r| if r.index() == 2 {
                Some(stored)
            } else {
                Fixture::direction(r)
            },
            UnitVector3::Z_AXIS,
            UnitVector3::X_AXIS,
        )
        .is_err());
    }
}

#[test]
fn attachment_directions_admit_either_u_sign_without_a_side_flip() {
    use cadmpeg_ir::units::UnitVector3;
    let fixture = Fixture::new();
    assert!(projected(&fixture).is_ok());
    let values = plate(
        &fixture.plate,
        Some(&fixture.style),
        |r| fixture.property(r),
        Fixture::length,
        |r| match r.index() {
            2 => Some(UnitVector3::X_AXIS),
            33 => Some(UnitVector3::X_AXIS.reversed()),
            _ => Fixture::direction(r),
        },
        UnitVector3::Z_AXIS,
        UnitVector3::X_AXIS,
    )
    .expect("negative attachment U");
    assert_eq!(values.side, SheetMetalThicknessSide::Forward);
    for (index, stored) in [
        (34, UnitVector3::Z_AXIS),
        (34, UnitVector3::Y_AXIS.reversed()),
        (33, UnitVector3::Z_AXIS),
    ] {
        assert!(plate(
            &fixture.plate,
            Some(&fixture.style),
            |r| fixture.property(r),
            Fixture::length,
            |r| if r.index() == index {
                Some(stored)
            } else {
                Fixture::direction(r)
            },
            UnitVector3::Z_AXIS,
            UnitVector3::X_AXIS,
        )
        .is_err());
    }
}

#[test]
fn construction_wire_edge_collection_and_corner_controls_are_gated() {
    use crate::test_support::sheet_metal::list;
    assert!(projected(&Fixture::new()).is_ok());
    for index in [1, 6, 8] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| match kind {
            PmDcFeaturePropertyKind::ProfileSelection { .. } => {
                *kind = PmDcFeaturePropertyKind::SurfaceBody {
                    body: crate::test_support::sheet_metal::reference(0),
                }
            }
            PmDcFeaturePropertyKind::References { items, .. } => *items = list(&[99]),
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = false,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::Corner { value, .. },
            } => *value = 1,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::Enumeration { value, .. },
            } => *value = 0,
            _ => panic!("control"),
        });
        assert!(projected(&fixture).is_err());
    }
}

#[test]
fn corner_values_belong_to_the_corner_feature_and_only_their_types_are_gated() {
    for index in [70, 73, 74, 75, 77] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| match kind {
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = false,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::Enumeration { value, .. },
            } => *value = 0,
            _ => panic!("corner value"),
        });
        assert!(projected(&fixture).is_ok());
        fixture.edit(index, |kind| {
            *kind = PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: crate::test_support::sheet_metal::reference(0),
                value: 0,
            }
        });
        assert!(projected(&fixture).is_err());
    }
}

#[test]
fn flange_thickness_uses_the_positive_normal_for_zero_and_false_sides() {
    let mut fixture = Fixture::new();
    assert_eq!(
        projected(&fixture).expect("false side").side,
        SheetMetalThicknessSide::Forward
    );
    fixture.edit(3, |kind| {
        *kind = PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PlateSide,
            type_value: 2,
            value: 0,
        };
    });
    assert_eq!(
        projected(&fixture).expect("zero side").side,
        SheetMetalThicknessSide::Forward
    );
    fixture.edit(3, |kind| {
        *kind = PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        };
    });
    assert!(projected(&fixture).is_err());
}

fn first_refusal(fixture: &Fixture) -> PlateRefusal {
    projected(fixture).expect_err("refused")
}

#[test]
fn body_form_reads_the_same_dimensions_as_the_collection_form() {
    let body = Fixture::body();
    let collection = Fixture::new();
    assert_eq!(PlateForm::of(&body.plate), Some(PlateForm::Body));
    assert_eq!(
        PlateForm::of(&collection.plate),
        Some(PlateForm::Collection)
    );
    assert_eq!(
        projected(&body).expect("body form"),
        projected(&collection).expect("collection form")
    );
    assert!(bend(&body.plate, &body.bend, |r| body.property(r)));
    assert!(bend(&collection.plate, &collection.bend, |r| collection
        .property(r)));
    assert_eq!(PlateForm::Body.result_slot(), 6);
    assert_eq!(PlateForm::Body.bend_result_slot(), 5);
    assert_eq!(PlateForm::Collection.result_slot(), 15);
    assert_eq!(PlateForm::Collection.bend_result_slot(), 9);
}

#[test]
fn body_form_requires_a_surface_body_and_null_reserved_slots() {
    let fixture = Fixture::body();
    for (slots, refusal) in [
        // Slot 6 null.
        (
            [1, 2, 3, 4, 5, 0, 0, 6, 7, 8, 9, 10, 11, 12, 0],
            PlateRefusal::ResultBody,
        ),
        // Slot 6 names a Boolean instead of a surface body.
        (
            [1, 2, 3, 4, 5, 0, 20, 6, 7, 8, 9, 10, 11, 12, 0],
            PlateRefusal::ResultBody,
        ),
        // Slot 5 and slot 14 are reserved.
        (
            [1, 2, 3, 4, 5, 20, 46, 6, 7, 8, 9, 10, 11, 12, 0],
            PlateRefusal::ReservedSlot,
        ),
        (
            [1, 2, 3, 4, 5, 0, 46, 6, 7, 8, 9, 10, 11, 12, 20],
            PlateRefusal::ReservedSlot,
        ),
    ] {
        let mut changed = Fixture::body();
        changed.plate = crate::test_support::sheet_metal::feature(100, &slots);
        assert_eq!(first_refusal(&changed), refusal, "{slots:?}");
    }
    assert!(projected(&fixture).is_ok());
}

#[test]
fn each_other_property_count_is_refused() {
    for count in [14_usize, 16, 18] {
        let mut slots = vec![0_u32; count];
        slots[..5].copy_from_slice(&[1, 2, 3, 4, 5]);
        let mut fixture = Fixture::new();
        fixture.plate = crate::test_support::sheet_metal::feature(100, &slots);
        assert_eq!(first_refusal(&fixture), PlateRefusal::PropertyCount);
        assert!(PlateForm::of_count(count).is_none());
    }
}

#[test]
fn the_nine_property_bend_pins_the_shared_body_and_scalar() {
    let fixture = Fixture::body();
    let with = |slots: &[u32]| {
        let source = crate::test_support::sheet_metal::feature(200, slots);
        bend(&fixture.plate, &source, |r| fixture.property(r))
    };
    assert!(with(&[6, 25, 4, 5, 9, 46, 7, 10, 47]));
    // The result body differs from the plate slot 6.
    assert!(!with(&[6, 25, 4, 5, 9, 0, 7, 10, 47]));
    assert!(!with(&[6, 25, 4, 5, 9, 20, 7, 10, 47]));
    // The scalar is the 17-property form's value, or a missing property.
    assert!(!with(&[6, 25, 4, 5, 9, 46, 7, 10, 44]));
    assert!(!with(&[6, 25, 4, 5, 9, 46, 7, 10, 0]));
    // Another property count belongs to the other form.
    assert!(!with(&[6, 25, 4, 5, 9, 0, 7, 10, 44, 13, 45]));
    for slot in [0, 2, 3, 4, 7] {
        let mut slots = [6, 25, 4, 5, 9, 46, 7, 10, 47];
        slots[slot] = 999;
        assert!(!with(&slots), "slot {slot}");
    }
    // The 17-property plate takes only the 11-property bend.
    let collection = Fixture::new();
    let nine = crate::test_support::sheet_metal::feature(200, &[6, 25, 4, 5, 9, 46, 7, 10, 47]);
    assert!(!bend(&collection.plate, &nine, |r| collection.property(r)));
    let mut changed = Fixture::body();
    changed.edit(47, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::BendControl { value, .. },
        } = kind
        {
            *value = 6;
        }
    });
    assert!(!bend(&changed.plate, &changed.bend, |r| changed.property(r)));
}

#[test]
fn the_first_refused_check_follows_the_evaluation_order() {
    let fixture = Fixture::new();
    let run = |fixture: &Fixture, style: Option<&Style>| {
        plate(
            &fixture.plate,
            style,
            |r| fixture.property(r),
            Fixture::length,
            Fixture::direction,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        )
        .expect_err("refused")
    };
    assert_eq!(run(&fixture, None), PlateRefusal::Style);
    let mut other_version = Fixture::new();
    other_version.plate = {
        let mut payload = (*fixture.plate).clone();
        payload.save_version_major = 19;
        crate::record_identity::Located::new(
            payload,
            fixture.plate.identity.type_id.clone(),
            fixture.plate.identity.segment_token.clone(),
            fixture.plate.identity.record_ordinal,
        )
    };
    assert_eq!(
        run(&other_version, Some(&fixture.style)),
        PlateRefusal::StyleVersion
    );
    let edits: [Edit; 6] = [
        (
            14,
            |kind| {
                if let PmDcFeaturePropertyKind::Enumeration { value, .. } = kind {
                    *value = 1;
                }
            },
            PlateRefusal::PartOperation,
        ),
        (
            20,
            |kind| {
                if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                    *value = true;
                }
            },
            PlateRefusal::AttachmentBoolean,
        ),
        (
            21,
            |kind| {
                if let PmDcFeaturePropertyKind::SheetMetal {
                    definition: SheetMetalProperty::Enumeration { value, .. },
                } = kind
                {
                    *value = 0;
                }
            },
            PlateRefusal::AttachmentMode,
        ),
        (
            29,
            |kind| {
                if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                    *value = false;
                }
            },
            PlateRefusal::Relief,
        ),
        (
            35,
            |kind| {
                if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                    *value = false;
                }
            },
            PlateRefusal::Transition,
        ),
        (
            12,
            |kind| {
                if let PmDcFeaturePropertyKind::SheetMetal {
                    definition: SheetMetalProperty::PlateControl { value },
                } = kind
                {
                    *value = 1;
                }
            },
            PlateRefusal::PlateControl,
        ),
    ];
    for (index, edit, refusal) in edits {
        let mut changed = Fixture::new();
        changed.edit(index, edit);
        assert_eq!(first_refusal(&changed), refusal, "property {index}");
    }
}

#[test]
fn every_refusal_has_a_distinct_kebab_detail() {
    let all = [
        PlateRefusal::Style,
        PlateRefusal::PropertyCount,
        PlateRefusal::StyleVersion,
        PlateRefusal::ReservedSlot,
        PlateRefusal::PartOperation,
        PlateRefusal::ResultBody,
        PlateRefusal::ProfileSelection,
        PlateRefusal::EdgeCollection,
        PlateRefusal::CornerControl,
        PlateRefusal::FeatureDimensions,
        PlateRefusal::PlateControl,
        PlateRefusal::AttachmentControl,
        PlateRefusal::Direction,
        PlateRefusal::PlateDirection,
        PlateRefusal::FirstDirection,
        PlateRefusal::SecondDirection,
        PlateRefusal::ThicknessReference,
        PlateRefusal::AttachmentBoolean,
        PlateRefusal::AttachmentMode,
        PlateRefusal::AttachmentStates,
        PlateRefusal::AttachmentExtension,
        PlateRefusal::Relief,
        PlateRefusal::Transition,
        PlateRefusal::Length,
        PlateRefusal::Side,
        PlateRefusal::SelectionList,
        PlateRefusal::EdgeName,
        PlateRefusal::InputGeometry,
        PlateRefusal::ProfileSketch,
        PlateRefusal::Profile,
        PlateRefusal::Result,
        PlateRefusal::InputProducer,
        PlateRefusal::BendPair,
    ];
    let details: std::collections::BTreeSet<_> =
        all.iter().map(|refusal| refusal.detail()).collect();
    assert_eq!(details.len(), all.len());
    assert!(details.iter().all(|detail| {
        !detail.is_empty()
            && detail.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            && !detail.starts_with('-')
            && !detail.ends_with('-')
    }));
}
