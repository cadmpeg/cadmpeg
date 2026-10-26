// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::sheet_metal::Fixture;

fn projected(fixture: &Fixture) -> Option<PlateOperands> {
    plate(
        &fixture.plate,
        &fixture.style,
        |reference| fixture.property(reference),
        Fixture::length,
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
fn retained_relief_and_transition_booleans_do_not_change_explicit_lengths() {
    for relief in [false, true] {
        for transition in [false, true] {
            let mut fixture = Fixture::new();
            for (index, expected) in [(29, relief), (35, transition)] {
                fixture.edit(index, |kind| {
                    if let PmDcFeaturePropertyKind::Boolean { value, .. } = kind {
                        *value = expected;
                    }
                });
            }
            let value = projected(&fixture).expect("retained controls");
            assert_eq!(value.relief.width.get(), 13.0);
            assert_eq!(value.relief.depth.get(), 17.0);
            assert_eq!(value.transition.radius.get(), 23.0);
        }
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
            }
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
            }
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
            }
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
