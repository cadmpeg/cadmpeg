// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::sheet_metal::Fixture;

fn projected(fixture: &Fixture) -> Option<PlateOperands> {
    plate(
        &fixture.plate,
        &fixture.style,
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
        assert!(projected(&fixture).is_none());
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
    assert!(projected(&fixture).is_none());
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
    assert!(projected(&fixture).is_none());
    let mut fixture = Fixture::new();
    fixture.edit(31, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration { value, .. },
        } = kind
        {
            *value = 0;
        }
    });
    assert!(projected(&fixture).is_none());
}

#[test]
fn relief_and_transition_booleans_require_the_pinned_true_value() {
    assert!(projected(&Fixture::new()).is_some());
    for index in [29, 35] {
        let mut fixture = Fixture::new();
        fixture.edit(index, |kind| {
            if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                *value = false;
            }
        });
        assert!(projected(&fixture).is_none());
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
    assert!(projected(&fixture).is_some());
    fixture.edit(47, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState { state, .. },
        } = kind
        {
            *state = 11;
        }
    });
    assert!(projected(&fixture).is_none());
    fixture.edit(47, |kind| {
        if let PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState { state, radius, .. },
        } = kind
        {
            *state = 10;
            *radius = reference(26);
        }
    });
    assert!(projected(&fixture).is_none());
}

#[test]
fn dimensions_require_resolved_lengths_and_their_ir_domains() {
    let fixture = Fixture::new();
    for index in [4, 25, 26, 27] {
        assert!(plate(
            &fixture.plate,
            &fixture.style,
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
        .is_none());
    }
    for index in [4, 25, 26, 27, 28, 30, 36] {
        assert!(plate(
            &fixture.plate,
            &fixture.style,
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
        .is_none());
        assert!(plate(
            &fixture.plate,
            &fixture.style,
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
        .is_none());
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
    assert!(projected(&fixture).is_some());
    for stored in [UnitVector3::Z_AXIS, UnitVector3::X_AXIS] {
        assert!(plate(
            &fixture.plate,
            &fixture.style,
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
        .is_none());
    }
}

#[test]
fn attachment_directions_admit_either_u_sign_without_a_side_flip() {
    use cadmpeg_ir::units::UnitVector3;
    let fixture = Fixture::new();
    assert!(projected(&fixture).is_some());
    let values = plate(
        &fixture.plate,
        &fixture.style,
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
            &fixture.style,
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
        .is_none());
    }
}

#[test]
fn construction_wire_edge_collection_and_corner_controls_are_gated() {
    use crate::test_support::sheet_metal::list;
    assert!(projected(&Fixture::new()).is_some());
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
        assert!(projected(&fixture).is_none());
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
        assert!(projected(&fixture).is_some());
        fixture.edit(index, |kind| {
            *kind = PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: crate::test_support::sheet_metal::reference(0),
                value: 0,
            }
        });
        assert!(projected(&fixture).is_none());
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
    assert!(projected(&fixture).is_none());
}
