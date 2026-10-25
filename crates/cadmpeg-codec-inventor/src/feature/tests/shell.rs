// SPDX-License-Identifier: Apache-2.0
//! Shell operand admission and resource limits.

use super::*;
use crate::feature::{
    feature_property_parser, native_kind, project_shell, FeatureFamily, FACE_COLLECTION_TYPE,
    SELECTION_LIST_TYPE, SHELL_CLASS_ID,
};

#[test]
fn shell_selection_lists_require_complete_versioned_frames() {
    for (type_id, family) in [
        (
            FACE_COLLECTION_TYPE,
            PmDcFeatureReferenceFamily::FaceCollection,
        ),
        (
            SELECTION_LIST_TYPE,
            PmDcFeatureReferenceFamily::SelectionList,
        ),
    ] {
        for (version, stamps) in [(16, Vec::new()), (23, vec![-1]), (28, vec![-1, 2800])] {
            for (values, expected) in [
                (&[][..], Vec::new()),
                (
                    &[2, 0x8000_0003][..],
                    vec![
                        crate::pmdc::PmDcReference::new(2, false).expect("plain reference"),
                        reference(3),
                    ],
                ),
            ] {
                let mut bytes = content_at(1, version, &stamps);
                bytes.extend_from_slice(&references(values));
                let parser = feature_property_parser(type_id).expect("selection parser");
                let parsed = parse(&bytes, |ctx, view| {
                    parser(ctx, view, version).expect("selection property")
                });
                assert_eq!(parsed.header.stamps, stamps);
                assert!(matches!(
                    parsed.kind,
                    PmDcFeaturePropertyKind::References { family: actual, items }
                    if actual == family && items.references() == expected.as_slice()
                ));
                parse(&bytes[..bytes.len() - 1], |ctx, view| {
                    assert!(parser(ctx, view, version).is_err());
                });
                bytes.push(0);
                parse(&bytes, |ctx, view| {
                    assert!(parser(ctx, view, version).is_err());
                });
            }
        }
    }
}

fn generated_shell(
    faces: &[u32],
    additional: &[u32],
    thickness: f64,
    slot_count: usize,
    policy: DecodePolicy,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    generated_shell_with(
        faces,
        additional,
        thickness,
        slot_count,
        policy,
        |_, _, _| {},
    )
}

fn generated_shell_with(
    faces: &[u32],
    additional: &[u32],
    thickness: f64,
    slot_count: usize,
    policy: DecodePolicy,
    configure: impl FnOnce(&mut PmDcFeature, &mut [PmDcFeatureProperty], &mut [DesignParameter]),
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let mut properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(faces),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SelectionList,
                items: reference_list(additional),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[5]),
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let raw = [raw_parameter(10)];
    let mut neutral = [neutral_parameter(
        &raw[0],
        ParameterValue::Length(Length::new(thickness).expect("finite test thickness")),
    )];
    let mut source = test_feature(100, slot_count, &[(1, 1), (2, 2), (3, 10), (9, 3)]);
    configure(&mut source, &mut properties, &mut neutral);
    let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
    let label = test_label(100, 7, SHELL_CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test decode context");
    project_shell(&ctx, &source, &label, &index)
}

#[test]
fn shell_projects_thickness_native_faces_and_result_without_direction_defaults() {
    for faces in [&[20][..], &[][..]] {
        let (feature, result) = generated_shell(faces, &[], 2.0, 10, DecodePolicy::service())
            .expect("shell candidate")
            .expect("shell projection");
        let FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness,
            outward,
            mode,
            join,
            resolve_intersections,
            allow_self_intersections,
        }) = feature.evaluation.definition()
        else {
            panic!("shell definition")
        };
        assert_eq!(
            thickness.map(cadmpeg_ir::scalar::PositiveLength::get),
            Some(2.0)
        );
        assert!(bodies.is_none() && outward.is_none() && mode.is_none() && join.is_none());
        assert!(resolve_intersections.is_none() && allow_self_intersections.is_none());
        if faces.is_empty() {
            assert_eq!(
                *removed_faces,
                cadmpeg_ir::features::FaceSelection::Faces(Vec::new())
            );
        } else {
            assert_eq!(
                *removed_faces,
                cadmpeg_ir::features::FaceSelection::Native(format!(
                    "inventor:pmdc:feature-property#{}-1",
                    segment()
                ))
            );
        }
        assert_eq!(result.output_of, feature.id);
        assert_eq!(
            result.bodies()[0].as_str(),
            format!("inventor:pmdc:feature-property#{}-4", segment())
        );
        assert_eq!(
            result.native_ref,
            Some(format!("inventor:pmdc:feature-property#{}-3", segment()))
        );
        let family = FeatureFamily::from_definition(feature.evaluation.definition())
            .expect("Shell feature family");
        assert_eq!(family.class_ids(), &[SHELL_CLASS_ID]);
        assert!(FeatureFamily::from_class_id(SHELL_CLASS_ID)
            .is_some_and(|actual| actual.class_ids() == family.class_ids()));
        assert_eq!(
            native_kind(Some(SHELL_CLASS_ID)),
            cadmpeg_ir::features::NativeFeatureKind::Other("Shell".into())
        );
        assert_eq!(family.output_slot(&super::test_feature(100, 18, &[])), 9);
    }
}

#[test]
fn shell_rejects_local_selections_bad_thickness_and_extended_slots_before_entity_charge() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    for (faces, additional, thickness, slots) in [
        (&[20][..], &[21][..], 2.0, 10),
        (&[20, 20][..], &[][..], 2.0, 10),
        (&[0][..], &[][..], 2.0, 10),
        (&[20][..], &[][..], 0.0, 10),
        (&[20][..], &[][..], -1.0, 10),
        (&[20][..], &[][..], 2.0, 11),
    ] {
        assert!(generated_shell(faces, additional, thickness, slots, policy).is_none());
    }
    assert!(generated_shell(&[20], &[], 2.0, 10, policy)
        .expect("Shell candidate")
        .is_err());
}

#[test]
fn shell_rejects_non_null_slot_four_and_incompatible_operand_slots() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    for (slot, ordinal) in [(1, 2), (2, 1), (3, 1), (4, 1), (9, 1)] {
        assert!(
            generated_shell_with(&[20], &[], 2.0, 10, policy, |source, _, _| {
                let mut slots = vec![(1, 1), (2, 2), (3, 10), (9, 3)];
                slots.retain(|(actual, _)| *actual != slot);
                slots.push((slot, ordinal));
                *source = test_feature(100, 10, &slots);
            },)
            .is_none()
        );
    }
    for absent in [1, 2, 3, 9] {
        assert!(
            generated_shell_with(&[20], &[], 2.0, 10, policy, |source, _, _| {
                let slots = [(1, 1), (2, 2), (3, 10), (9, 3)]
                    .into_iter()
                    .filter(|(slot, _)| *slot != absent)
                    .collect::<Vec<_>>();
                *source = test_feature(100, 10, &slots);
            },)
            .is_none()
        );
    }
    assert!(
        generated_shell_with(&[20], &[], 2.0, 10, policy, |source, _, _| *source =
            test_feature(100, 9, &[(1, 1), (2, 2), (3, 10)]),)
        .is_none()
    );
}

#[test]
fn shell_requires_a_resolved_length_and_a_nonempty_distinct_body_result() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    for value in [None, Some(ParameterValue::Angle(Angle::ZERO))] {
        assert!(
            generated_shell_with(&[20], &[], 2.0, 10, policy, |_, _, parameters| parameters
                [0]
            .value =
                value,)
            .is_none()
        );
    }
    for bodies in [&[][..], &[0][..], &[5, 5][..], &[6][..]] {
        assert!(
            generated_shell_with(&[20], &[], 2.0, 10, policy, |_, properties, _| {
                properties[2] = test_property(
                    3,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: reference_list(bodies),
                    },
                );
            },)
            .is_none()
        );
    }
}

#[test]
fn shell_rejects_face_duplicates_with_different_reference_qualifiers() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(
        generated_shell_with(&[20], &[], 2.0, 10, policy, |_, properties, _| {
            properties[0] = test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FaceCollection,
                    items: PmDcReferenceList::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([2, 0])),
                        vec![
                            crate::pmdc::PmDcReference::new(20, false)
                                .expect("plain face reference"),
                            crate::pmdc::PmDcReference::new(20, true)
                                .expect("qualified face reference"),
                        ],
                    )
                    .expect("type-2 reference list"),
                },
            );
        },)
        .is_none()
    );
}

#[test]
fn shell_charges_both_the_feature_and_its_result() {
    for (max_entities, operation) in [
        (0, "project Inventor feature result topology"),
        (1, "project Inventor feature"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = max_entities;
        assert!(matches!(
            generated_shell(&[20], &[], 2.0, 10, policy),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities && limit.operation == operation
        ));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    assert!(matches!(
        generated_shell(&[20], &[], 2.0, 10, policy),
        Some(Ok(_))
    ));
}
