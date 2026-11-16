// SPDX-License-Identifier: Apache-2.0
//! Work-point extrusion targets and their validator.

use super::*;

#[test]
fn work_point_plane_follows_the_placed_profile_and_reversed_travel() {
    for reverse in [false, true] {
        let z = if reverse { -2.0 } else { 3.0 };
        let feature = placed_extent_extrusion(
            7,
            false,
            &[(25, 24), (3, 20)],
            vec![
                work_point(24, [4.0, 5.0, z]),
                test_property(
                    20,
                    PmDcFeaturePropertyKind::Boolean {
                        name: String::new(),
                        name_value: 0,
                        value: reverse,
                    },
                ),
            ],
            [
                [0.0, -1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            |_| {},
            |source, label, index, projected| {
                let mut definition = projected
                    .expect("projected")
                    .evaluation
                    .definition()
                    .clone();
                let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    extent: ExtrudeExtent::OneSided { side },
                    ..
                }) = &mut definition
                else {
                    panic!("extrusion");
                };
                let LinearTermination::ToPlane { plane, native, .. } = &mut side.termination else {
                    panic!("plane");
                };
                assert_eq!(plane.origin().get(), Point3::new(40.0, 50.0, z * 10.0));
                assert_eq!(
                    plane.normal().get(),
                    Vector3::new(0.0, 0.0, if reverse { -1.0 } else { 1.0 })
                );
                assert_eq!(plane.u_axis().get(), Vector3::new(0.0, 1.0, 0.0));
                *native = cadmpeg_core::text::NonBlankString::new("wrong-point").expect("native");
                assert!(!crate::feature::extrusion::matches(
                    index,
                    source,
                    label,
                    &definition
                ));
            },
        );
        assert!(feature.is_some());
    }
}

#[test]
fn work_point_validator_refuses_changed_operands() {
    for slot in [2, 11, 13, 18, 25] {
        configured_extent_extrusion(
            7,
            false,
            &POINT_SLOTS,
            vec![work_point(24, [0.0, 0.0, 2.5])],
            |_| {},
            |source, label, index, projected| {
                let mut source = source.clone();
                let mut slots = source.properties.references().to_vec();
                slots[slot] = reference(if slot == 2 { 62 } else { 40 });
                let mut payload = (*source).clone();
                payload.properties = PmDcReferenceList::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([30, 0])),
                    slots,
                )
                .expect("slots");
                source = Located::new(
                    payload,
                    source.identity.type_id.clone(),
                    source.identity.segment_token.clone(),
                    source.identity.record_ordinal,
                );
                assert!(!crate::feature::extrusion::matches(
                    index,
                    &source,
                    label,
                    projected.expect("projected").evaluation.definition()
                ));
            },
        );
    }
}

#[test]
fn work_point_extent_and_validator_accept_each_value_count() {
    for version in [19, 21, 22, 23, 25, 29] {
        let record = work_point_with(
            24,
            0,
            if version > 21 {
                vec![0, 0, 0]
            } else {
                vec![0, 0]
            },
            [0.0, 0.0, 2.5],
            [&[], &[]],
        );
        let mut payload = (*record).clone();
        payload.save_version_major = version;
        let point = Located::new(payload, record.identity.type_id.clone(), segment(), 24);
        let feature = extent_extrusion(7, false, &POINT_SLOTS, vec![point]).expect("point extent");
        let ExtrudeExtent::OneSided { side } = extrude_definition(&feature).0 else {
            panic!("one-sided");
        };
        assert_to_plane(
            &side.termination,
            [0.0, 0.0, 25.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            &work_point(24, [0.0, 0.0, 2.5]).id(),
        );
    }
    for values in [vec![1, 0], vec![], vec![0], vec![0, 0, 0]] {
        assert!(point_extrusion(
            work_point_with(24, 0, values, [0.0, 0.0, 2.5], [&[], &[]]),
            &[]
        )
        .is_none());
    }
}

#[test]
fn work_point_validator_checks_direction_controls_and_plane_frame() {
    configured_extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [0.0, 0.0, 2.5])],
        |_| {},
        |source, label, index, projected| {
            let definition = projected.expect("projected").evaluation.definition();
            for (flags, parameter, extension) in [(1, 0.0, None), (0, 1.0, None), (0, 0.0, Some(1))]
            {
                let record = index.directions[&("generated", 60)].expect("direction");
                let mut payload = (**record).clone();
                payload.entity_flags = flags;
                payload.parameter = cadmpeg_ir::scalar::FiniteReal::new(parameter).expect("finite");
                payload.extension = extension;
                let direction =
                    Located::new(payload, record.identity.type_id.clone(), segment(), 60);
                let mut changed = crate::feature::ProjectionIndex {
                    properties: index.properties.clone(),
                    sketches: index.sketches.clone(),
                    transforms: index.transforms.clone(),
                    directions: index.directions.clone(),
                    ..crate::feature::ProjectionIndex::default()
                };
                changed
                    .directions
                    .insert(("generated", 60), Some(&direction));
                assert!(crate::feature::extrusion::work_point_plane(
                    source,
                    label,
                    &changed,
                    &cadmpeg_ir::units::UnitVector3::Z_AXIS
                )
                .is_none());
                assert!(!crate::feature::extrusion::matches(
                    &changed, source, label, definition
                ));
            }
            for (origin, normal, u_axis) in [
                (
                    Point3::new(0.0, 0.0, 26.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ),
                (
                    Point3::new(0.0, 0.0, 25.0),
                    Vector3::new(0.0, 0.0, -1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ),
                (
                    Point3::new(0.0, 0.0, 25.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 1.0, 0.0),
                ),
            ] {
                let mut changed = definition.clone();
                let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    extent: ExtrudeExtent::OneSided { side },
                    ..
                }) = &mut changed
                else {
                    panic!("extrusion");
                };
                let LinearTermination::ToPlane { plane, .. } = &mut side.termination else {
                    panic!("plane");
                };
                *plane =
                    cadmpeg_ir::features::FeatureSupportPlaneFrame::new(origin, normal, u_axis)
                        .expect("plane");
                assert!(!crate::feature::extrusion::matches(
                    index, source, label, &changed
                ));
            }
        },
    );
}

#[test]
fn work_point_extent_pins_direction_controls_and_oblique_travel() {
    for ordinal in [63, 64, 65, 67] {
        assert!(point_extrusion(work_point(24, [0.0, 0.0, 2.5]), &[(2, ordinal)]).is_none());
    }
    assert!(point_extrusion(work_point(24, [0.0, 0.0, 2.5]), &[(2, 66)]).is_some());
}

#[test]
fn work_point_extent_and_validator_require_type_two_lists() {
    for list in 0..2 {
        let record = work_point(24, [0.0, 0.0, 2.5]);
        let mut payload = (*record).clone();
        let PmDcFeaturePropertyKind::WorkPoint { references, .. } = &mut payload.kind else {
            panic!("point");
        };
        references[list] = PmDcReferenceList::new(8, None, Vec::new()).expect("empty list");
        let point = Located::new(payload, record.identity.type_id.clone(), segment(), 24);
        assert!(point_extrusion(point, &[]).is_none());
    }
}

#[test]
fn work_point_extent_and_validator_keep_the_opposite_side() {
    let feature = extent_extrusion(
        7,
        false,
        &[(25, 24), (27, 72), (28, 71), (29, 22)],
        vec![
            work_point(24, [0.0, 0.0, 2.5]),
            enum_property(22, PmDcFeatureEnumFamily::Extent, 1),
        ],
    )
    .expect("two-sided point extent");
    let ExtrudeExtent::TwoSided { first, second } = extrude_definition(&feature).0 else {
        panic!("two-sided");
    };
    assert_to_plane(
        &first.termination,
        [0.0, 0.0, 25.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        &work_point(24, [0.0, 0.0, 2.5]).id(),
    );
    assert!(
        matches!(second.termination, LinearTermination::Blind { length } if length.get() == 3.0)
    );
}

#[test]
fn work_point_extent_and_validator_keep_a_native_profile_selection() {
    configured_extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [0.0, 0.0, 2.5])],
        |index| index.sketch_ids.clear(),
        assert_native_profile_validator,
    )
    .expect("native profile point extent");
}

#[test]
fn work_point_extent_and_validator_require_the_work_point_record_type() {
    let mut point = work_point(24, [0.0, 0.0, 2.5]);
    point.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::BOOLEAN_TYPE);
    assert!(point_extrusion(point, &[]).is_none());
}

#[test]
fn work_point_extent_and_validator_refuse_an_indeterminate_travel_distance() {
    let expected = extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [0.0, 0.0, 2.5])],
    )
    .expect("canonical extent");
    assert!(placed_extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [1.0e307, 0.0, -1.0])],
        [
            [1.0, 0.0, 0.0, -1.0e307],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        |_| {},
        |source, label, index, projected| {
            assert!(projected.is_none());
            assert!(!crate::feature::extrusion::matches(
                index,
                source,
                label,
                expected.evaluation.definition()
            ));
        }
    )
    .is_none());
}
