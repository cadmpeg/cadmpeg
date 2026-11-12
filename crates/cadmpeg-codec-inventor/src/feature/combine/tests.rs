// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    reference, test_feature, test_label, test_projection_index, test_property,
};
use crate::test_support::test_fixtures::parse;

fn packed_reference_list(values: &[u32]) -> crate::pmdc::PmDcReferenceList {
    crate::pmdc::PmDcReferenceList::new(
        2,
        (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
        values
            .iter()
            .copied()
            .map(crate::pmdc::PmDcReference::from_packed)
            .collect(),
    )
    .expect("synthetic packed reference list")
}

fn properties(
    targets: &[u32],
    tools: &[u32],
    operation: u16,
    keep: bool,
) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: packed_reference_list(targets),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: packed_reference_list(tools),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: operation,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: keep,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ]
}

#[test]
fn combine_projects_boolean_modes_tools_and_retained_target() {
    let source = test_feature(0, 4, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    let label = test_label(0, 2, CLASS_ID, &[]);
    for (value, expected) in [
        (2, BooleanKind::Cut),
        (3, BooleanKind::Join),
        (4, BooleanKind::Intersect),
    ] {
        for keep in [false, true] {
            for tools in [vec![7], vec![8, 7]] {
                let properties = properties(&[6], &tools, value, keep);
                let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
                parse(&[], |ctx, _| {
                    let (feature, result) = project(ctx, &source, &label, &index)
                        .expect("supported Combine")
                        .expect("project Combine");
                    let FeatureDefinition::Operation(FeatureOperation::Combine {
                        operands,
                        op,
                        keep_tools,
                    }) = feature.evaluation.definition()
                    else {
                        panic!("neutral Combine");
                    };
                    let expected_tools = tools
                        .iter()
                        .map(|index| {
                            properties
                                [usize::try_from(index - 2).expect("synthetic property index")]
                            .id()
                        })
                        .collect::<Vec<_>>();
                    let (target, actual_tools) = operands.clone().into_parts();
                    assert_eq!(target, BodySelection::Native(properties[4].id()));
                    assert_eq!(
                        actual_tools,
                        if expected_tools.len() == 1 {
                            BodySelection::Native(expected_tools[0].clone())
                        } else {
                            BodySelection::NativeSet(
                                NativeSelections::try_from(expected_tools)
                                    .expect("distinct synthetic tools"),
                            )
                        }
                    );
                    assert_eq!(*op, expected);
                    assert_eq!(*keep_tools, keep);
                    assert_eq!(result.output_of, feature.id);
                    assert_eq!(result.native_ref, Some(properties[0].id()));
                    assert_eq!(
                        result
                            .bodies()
                            .iter()
                            .map(cadmpeg_core::text::NonBlankString::as_str)
                            .collect::<Vec<_>>(),
                        vec![properties[4].id()]
                    );
                });
            }
        }
    }
}

#[test]
fn combine_refuses_missing_ambiguous_or_overlapping_operands() {
    let source = test_feature(0, 4, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    for (targets, tools, operation) in [
        (vec![], vec![7], 3),
        (vec![6, 8], vec![7], 3),
        (vec![6], vec![], 3),
        (vec![6], vec![6], 3),
        (vec![6], vec![7, 6], 3),
        (vec![6], vec![7, 7], 3),
        (vec![6], vec![7, 0x8000_0007], 3),
        (vec![6], vec![0x8000_0006], 3),
        (vec![6], vec![99], 3),
        (vec![0], vec![7], 3),
        (vec![6], vec![7], 1),
    ] {
        let properties = properties(&targets, &tools, operation, false);
        let projected = definition(&source, |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        });
        assert!(projected.is_none());
    }
    for corrupt in 0..7 {
        let mut properties = properties(&[6], &[7], 3, false);
        properties[corrupt] = test_property(
            u32::try_from(corrupt + 1).expect("synthetic property ordinal"),
            PmDcFeaturePropertyKind::TerminationFlag { value: 1 },
        );
        let projected = definition(&source, |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        });
        assert_eq!(projected.is_none(), corrupt != 6);
    }
    let properties = properties(&[6], &[7], 3, false);
    for count in [3, 5] {
        let source = test_feature(0, count, &[(0, 1), (1, 2), (2, 3)]);
        let projected = definition(&source, |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        });
        assert!(projected.is_none());
    }
}

#[test]
fn combine_propagates_result_and_feature_entity_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let source = test_feature(0, 4, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    let label = test_label(0, 2, CLASS_ID, &[]);
    let properties = properties(&[6], &[7], 3, false);
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    for (budget, operation) in [
        (0, "project Inventor feature result topology"),
        (1, "project Inventor feature"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = budget;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("decode context");
        assert!(matches!(
            project(&ctx, &source, &label, &index),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities && limit.operation == operation
        ));
    }
}
