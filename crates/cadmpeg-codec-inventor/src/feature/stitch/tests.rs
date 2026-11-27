// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    neutral_parameter, raw_parameter, reference_list, test_feature, test_label,
    test_projection_index, test_property,
};
use crate::feature::PmDcFeatureProperty;
use crate::test_support::test_fixtures::parse;
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};

fn boolean(ordinal: u32, value: bool) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value,
        },
    )
}

fn body(ordinal: u32) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::feature::tests::reference(9),
        },
    )
}

/// Ordinal 0 is the operand collection, 1 the result, 2 and 3 operands, and
/// 4 and 5 the Booleans.
fn properties(members: &[u32]) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(
            0,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(members),
            },
        ),
        body(1),
        body(2),
        body(3),
        boolean(4, true),
        boolean(5, false),
    ]
}

const SLOTS: [(usize, u32); 5] = [(0, 0), (2, 1), (3, 4), (4, 20), (5, 5)];

fn length(value: ParameterValue) -> (Vec<crate::design::PmDcParameter>, Vec<DesignParameter>) {
    let raw = raw_parameter(20);
    let neutral = vec![neutral_parameter(&raw, value)];
    (vec![raw], neutral)
}

fn positive() -> ParameterValue {
    ParameterValue::Length(cadmpeg_ir::scalar::Length::new(2.5).expect("synthetic length"))
}

#[test]
fn stitch_projects_operand_wrappers_and_the_result_wrapper() {
    for members in [&[3, 4][..], &[3][..]] {
        let properties = properties(members);
        let source = test_feature(0, 6, &SLOTS);
        let label = test_label(0, 2, CLASS_ID, &[]);
        let (parameters, neutral) = length(positive());
        let index =
            test_projection_index(&properties, &parameters, &neutral, &[], &[], &[], &[], &[]);
        parse(&[], |ctx, _| {
            let (feature, result) = project(ctx, &source, &label, &index)
                .expect("supported Stitch Surface")
                .expect("project Stitch Surface");
            let ids = members
                .iter()
                .map(|member| properties[usize::try_from(member - 1).expect("ordinal")].id())
                .collect::<Vec<_>>();
            let expected = match ids.as_slice() {
                [one] => BodySelection::Native(one.clone()),
                _ => BodySelection::NativeSet(
                    NativeSelections::try_from(ids.clone()).expect("distinct operands"),
                ),
            };
            assert_eq!(
                feature.evaluation.definition(),
                &FeatureDefinition::Operation(FeatureOperation::SewBodies {
                    bodies: SewBodySelection::try_from(expected).expect("sew operands"),
                    gap_tolerance: PositiveLength::new(2.5),
                })
            );
            assert_eq!(feature.source_tag.as_deref(), Some(TAG));
            assert_eq!(
                feature
                    .source_properties
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("property_3_boolean", "true"),
                    ("property_5_boolean", "false")
                ]
            );
            assert_eq!(
                result
                    .bodies()
                    .iter()
                    .map(cadmpeg_core::text::NonBlankString::as_str)
                    .collect::<Vec<_>>(),
                [properties[1].id().as_str()]
            );
        });
    }
}

#[test]
fn stitch_refuses_other_slot_forms() {
    let label = test_label(0, 2, CLASS_ID, &[]);
    parse(&[], |ctx, _| {
        let run = |properties: &[PmDcFeatureProperty],
                   source: &PmDcFeature,
                   value: ParameterValue| {
            let (parameters, neutral) = length(value);
            let index =
                test_projection_index(properties, &parameters, &neutral, &[], &[], &[], &[], &[]);
            project(ctx, source, &label, &index).is_none()
        };
        let good = properties(&[3, 4]);
        let source = test_feature(0, 6, &SLOTS);
        assert!(!run(&good, &source, positive()));
        // A seventh slot, a nonnull slot 1, and a null slot 4 each refuse.
        assert!(run(&good, &test_feature(0, 7, &SLOTS), positive()));
        assert!(run(
            &good,
            &test_feature(0, 6, &[(0, 0), (1, 1), (2, 1), (3, 4), (4, 20), (5, 5)]),
            positive()
        ));
        assert!(run(
            &good,
            &test_feature(0, 6, &[(0, 0), (2, 1), (3, 4), (5, 5)]),
            positive()
        ));
        // A zero length, a parameter that is not a length, and an angle refuse.
        let zero = ParameterValue::Length(cadmpeg_ir::scalar::Length::new(0.0).expect("zero"));
        assert!(run(&good, &source, zero));
        let real = ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO);
        assert!(run(&good, &source, real));
        // An empty collection, a repeated member, and a Boolean member refuse.
        for members in [&[][..], &[3, 3][..], &[3, 5][..]] {
            assert!(run(&properties(members), &source, positive()));
        }
        // A control that is not a Boolean, a result that is not a surface
        // body, and a missing parameter record refuse.
        let mut other = properties(&[3, 4]);
        other[4] = body(4);
        assert!(run(&other, &source, positive()));
        let mut other = properties(&[3, 4]);
        other[5] = body(5);
        assert!(run(&other, &source, positive()));
        let mut other = properties(&[3, 4]);
        other[1] = boolean(1, true);
        assert!(run(&other, &source, positive()));
    });
}

#[test]
fn stitch_operation_is_rederived_from_the_slots() {
    let properties = properties(&[3, 4]);
    let source = test_feature(0, 6, &SLOTS);
    let (parameters, neutral) = length(positive());
    let index = test_projection_index(&properties, &parameters, &neutral, &[], &[], &[], &[], &[]);
    let token = source.identity.segment_token.as_str();
    let (operation, wrapper) = operation(
        &source,
        |reference| crate::feature::resolve_property(token, reference, &index),
        |reference| crate::feature::length_reference(token, reference, &index),
    )
    .expect("stitch slots");
    assert_eq!(wrapper.id(), properties[1].id());
    let FeatureOperation::SewBodies {
        bodies,
        gap_tolerance,
    } = operation
    else {
        panic!("sew operation");
    };
    assert_eq!(gap_tolerance, PositiveLength::new(2.5));
    assert!(matches!(&*bodies, BodySelection::NativeSet(names) if names.len() == 2));
}

#[test]
fn stitch_loss_counts_projected_features() {
    let notes = losses(3);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("Typed 3 Stitch Surface"));
    assert!(losses(0).is_empty());
}
