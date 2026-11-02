// SPDX-License-Identifier: Apache-2.0
//! Planar termination control admission and validation.

use super::*;

fn changed_surface(case: usize) -> PmDcFeatureProperty {
    let mut surface = plane_surface(21);
    surface.edit(|payload| {
        if case == 12 {
            payload.save_version_major = 22;
        }
        let PmDcFeaturePropertyKind::PlaneSurface {
            references,
            extension,
            state,
            surface_form,
            value,
            ..
        } = &mut payload.kind
        else {
            panic!("plane surface fixture");
        };
        match case {
            0..=2 => references[case] = crate::pmdc::PmDcReference::from_packed(1),
            3..=5 => references[case - 3] = crate::pmdc::PmDcReference::from_packed(0x8000_0000),
            6 => *state = 0,
            7 => *state = 2,
            8 => *surface_form = 29,
            9 => *value = 1,
            10 => *extension = Some(1),
            11 | 12 => {}
            13 => *extension = Some(0),
            _ => panic!("control case"),
        }
    });
    surface
}

#[test]
fn face_surface_controls_keep_unresolved_forms_native_with_a_loss() {
    for case in 0..=10 {
        let feature = extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                changed_surface(case),
            ],
        );
        assert!(feature.is_none(), "control case {case}");
    }
}

#[test]
fn face_surface_validator_checks_every_control() {
    configured_extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
        |_| {},
        |source, label, index, projected| {
            let definition = projected.expect("planar target").evaluation.definition();
            assert!(crate::feature::extrusion::matches(
                index, source, label, definition
            ));
            for case in 0..=13 {
                let surface = changed_surface(case);
                let mut properties = index.properties.clone();
                properties.insert(("generated", 21), Some(&surface));
                let changed = crate::feature::ProjectionIndex {
                    properties,
                    directions: index.directions.clone(),
                    parameters: index.parameters.clone(),
                    parameter_values: index.parameter_values.clone(),
                    sketches: index.sketches.clone(),
                    transforms: index.transforms.clone(),
                    work_planes: index.work_planes.clone(),
                    ..crate::feature::ProjectionIndex::default()
                };
                assert_eq!(
                    crate::feature::extrusion::matches(&changed, source, label, definition),
                    case == 11
                );
            }
        },
    )
    .expect("plane surface target");
}

#[test]
fn plane_target_sources_distinguish_face_work_plane_and_work_point() {
    use cadmpeg_ir::features::PlaneTargetSource;
    for (slots, extra, expected) in [
        (
            vec![(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface(21),
            ],
            PlaneTargetSource::FaceSurface,
        ),
        (vec![(11, 40)], vec![], PlaneTargetSource::WorkPlane),
        (
            POINT_SLOTS.to_vec(),
            vec![work_point(24, [0.0, 0.0, 2.5])],
            PlaneTargetSource::WorkPoint,
        ),
    ] {
        configured_extent_extrusion(
            7,
            false,
            &slots,
            extra,
            |_| {},
            |source, label, index, projected| {
                let definition = projected.expect("planar target").evaluation.definition();
                let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    extent: ExtrudeExtent::OneSided { side },
                    ..
                }) = definition
                else {
                    panic!("one-sided extrusion");
                };
                let LinearTermination::ToPlane { source: actual, .. } = side.termination else {
                    panic!("planar target");
                };
                assert_eq!(actual, Some(expected));
                assert_eq!(
                    crate::feature::extrusion::native_face_targets(definition),
                    usize::from(expected == PlaneTargetSource::FaceSurface)
                );
                for alternative in [
                    None,
                    Some(PlaneTargetSource::WorkPoint),
                    Some(PlaneTargetSource::WorkPlane),
                    Some(PlaneTargetSource::FaceSurface),
                ] {
                    let mut changed = definition.clone();
                    let FeatureDefinition::Operation(FeatureOperation::Extrude {
                        extent: ExtrudeExtent::OneSided { side },
                        ..
                    }) = &mut changed
                    else {
                        panic!("one-sided extrusion");
                    };
                    let LinearTermination::ToPlane { source: actual, .. } = &mut side.termination
                    else {
                        panic!("planar target");
                    };
                    *actual = alternative;
                    assert_eq!(
                        crate::feature::extrusion::matches(index, source, label, &changed),
                        alternative == Some(expected)
                    );
                }
            },
        )
        .expect("resolved plane");
    }
}

#[test]
fn from_to_plane_sources_have_independent_polarity() {
    use cadmpeg_ir::features::{ExtrudeStart, PlaneTargetSource};
    for (slots, expected) in [
        (
            vec![(8, 41), (13, 20), (18, 21)],
            PlaneTargetSource::WorkPlane,
        ),
        (
            vec![(10, 20), (17, 21), (11, 40)],
            PlaneTargetSource::FaceSurface,
        ),
    ] {
        configured_extent_extrusion(
            6,
            false,
            &slots,
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface(21),
            ],
            |_| {},
            |source, label, index, projected| {
                let definition = projected.expect("from-to planes").evaluation.definition();
                let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    start: ExtrudeStart::FromPlane { source: actual, .. },
                    ..
                }) = definition
                else {
                    panic!("from plane");
                };
                assert_eq!(*actual, Some(expected));
                assert_eq!(
                    crate::feature::extrusion::native_face_targets(definition),
                    1
                );
                let mut changed = definition.clone();
                let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    start: ExtrudeStart::FromPlane { source: actual, .. },
                    ..
                }) = &mut changed
                else {
                    panic!("from plane");
                };
                *actual = Some(if expected == PlaneTargetSource::FaceSurface {
                    PlaneTargetSource::WorkPlane
                } else {
                    PlaneTargetSource::FaceSurface
                });
                assert!(!crate::feature::extrusion::matches(
                    index, source, label, &changed
                ));
            },
        )
        .expect("from-to planes");
    }
}

#[test]
fn face_surface_projection_and_validator_require_the_surface_class() {
    let mut surface = plane_surface(21);
    surface.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::WORK_POINT_TYPE);
    assert!(extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            surface.clone()
        ]
    )
    .is_none());
    configured_extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
        |_| {},
        |source, label, index, projected| {
            let mut properties = index.properties.clone();
            properties.insert(("generated", 21), Some(&surface));
            let changed = crate::feature::ProjectionIndex {
                properties,
                directions: index.directions.clone(),
                parameters: index.parameters.clone(),
                parameter_values: index.parameter_values.clone(),
                ..crate::feature::ProjectionIndex::default()
            };
            assert!(!crate::feature::extrusion::matches(
                &changed,
                source,
                label,
                projected
                    .expect("surface class positive control")
                    .evaluation
                    .definition()
            ));
        },
    )
    .expect("surface class positive control");
}

#[test]
fn face_surface_decoder_and_validator_share_the_versioned_extension_form() {
    for (version, extension, admitted) in [
        (19, None, true),
        (21, None, true),
        (22, Some(0), true),
        (23, Some(0), true),
        (19, Some(0), false),
        (22, None, false),
    ] {
        let mut surface = plane_surface_with(21, extension);
        surface.edit(|payload| payload.save_version_major = version);
        assert_eq!(
            extent_extrusion(
                7,
                false,
                &[(13, 20), (18, 21)],
                vec![
                    test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                    surface,
                ]
            )
            .is_some(),
            admitted
        );
    }
}

#[test]
fn face_target_requires_the_termination_flag_class_in_projection_and_validation() {
    let mut flag = test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 });
    flag.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::WORK_POINT_TYPE);
    assert!(extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![flag.clone(), plane_surface(21)]
    )
    .is_none());
    configured_extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
        |_| {},
        |source, label, index, projected| {
            let mut properties = index.properties.clone();
            properties.insert(("generated", 20), Some(&flag));
            let changed = crate::feature::ProjectionIndex {
                properties,
                directions: index.directions.clone(),
                parameters: index.parameters.clone(),
                parameter_values: index.parameter_values.clone(),
                ..crate::feature::ProjectionIndex::default()
            };
            assert!(!crate::feature::extrusion::matches(
                &changed,
                source,
                label,
                projected
                    .expect("flag class positive control")
                    .evaluation
                    .definition()
            ));
        },
    )
    .expect("flag class positive control");
}

#[test]
fn two_face_surface_targets_have_two_identity_losses() {
    let mut first = plane_surface(21);
    let mut second = plane_surface(22);
    for (surface, height) in [(&mut first, 1.0), (&mut second, 4.0)] {
        surface.edit(|payload| {
            let PmDcFeaturePropertyKind::PlaneSurface {
                origin,
                u_axis,
                v_axis,
                ..
            } = &mut payload.kind
            else {
                panic!("plane surface");
            };
            *origin = reals([0.0, 0.0, height]);
            *u_axis = reals([1.0, 0.0, 0.0]);
            *v_axis = reals([0.0, 1.0, 0.0]);
        });
    }
    let feature = extent_extrusion(
        6,
        false,
        &[(10, 20), (17, 21), (13, 20), (18, 22)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            first,
            second,
        ],
    )
    .expect("two face targets");
    assert_eq!(
        crate::feature::extrusion::native_face_targets(feature.evaluation.definition()),
        2
    );
}
