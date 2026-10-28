// SPDX-License-Identifier: Apache-2.0

use crate::compact_matrix::CompactMatrix;
use crate::feature::motion::*;
use crate::feature::tests::{test_feature, test_header};
use crate::feature::{
    feature_property_parser, parse_face_item, FACE_COLLECTION_TYPE, FACE_ITEM_TYPE,
};
use crate::pmdc::{PmDcListMetadata, PmDcReferenceList, PmDcU32List};
use crate::record_identity::{Located, RecordTypeId};
use crate::sketch::PmDcTransformPayload;
use crate::test_support::test_fixtures::{content_at, parse};

fn reference(index: u32) -> PmDcReference {
    PmDcReference::new(index, true).expect("valid synthetic motion")
}

fn list(indices: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!indices.is_empty()).then_some(PmDcListMetadata::U32([
            u32::try_from(indices.len()).expect("valid synthetic motion"),
            0,
        ])),
        indices.iter().copied().map(reference).collect(),
    )
    .expect("valid synthetic motion")
}

fn property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
    let type_id = match &kind {
        PmDcFeaturePropertyKind::References { family, .. } => match family {
            PmDcFeatureReferenceFamily::ObjectCollection => crate::feature::OBJECT_COLLECTION_TYPE,
            PmDcFeatureReferenceFamily::TransformList => TRANSFORM_LIST_TYPE,
            PmDcFeatureReferenceFamily::BodySelection => BODY_SELECTION_TYPE,
            PmDcFeatureReferenceFamily::FaceCollection => FACE_COLLECTION_TYPE,
            PmDcFeatureReferenceFamily::FeatureDimensions => {
                crate::feature::FEATURE_DIMENSIONS_TYPE
            }
            _ => panic!("unused fixture family"),
        },
        PmDcFeaturePropertyKind::SurfaceBody { .. } => crate::feature::SURFACE_BODY_TYPE,
        PmDcFeaturePropertyKind::Boolean { .. } => crate::feature::BOOLEAN_TYPE,
        PmDcFeaturePropertyKind::FaceItem { .. } => FACE_ITEM_TYPE,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Motion,
            ..
        } => MOTION_MODE_TYPE,
        _ => panic!("unused fixture kind"),
    };
    Located::new(
        crate::feature::PmDcFeaturePropertyPayload {
            save_version_major: 19,
            header: test_header(),
            kind,
        },
        RecordTypeId::from_bytes(type_id),
        test_feature(0, 0, &[]).identity.segment_token.clone(),
        ordinal,
    )
}

fn matrix(rows: [[f64; 4]; 4]) -> PmDcTransform {
    Located::new(
        PmDcTransformPayload {
            save_version_major: 19,
            header: test_header(),
            prefix_present: false,
            matrix: CompactMatrix::try_from_rows(0, 0, rows).expect("valid synthetic motion"),
        },
        RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
        test_feature(0, 0, &[]).identity.segment_token.clone(),
        9,
    )
}

fn properties() -> Vec<PmDcFeatureProperty> {
    vec![
        property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: list(&[4, 5]),
            },
        ),
        property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::TransformList,
                items: list(&[10]),
            },
        ),
        property(
            3,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(101),
            },
        ),
        property(
            4,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(102),
            },
        ),
        property(
            8,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                items: list(&[]),
            },
        ),
    ]
}

fn decode_fixture<'a>(
    source: &PmDcFeature,
    properties: &'a [PmDcFeatureProperty],
    transforms: &[PmDcTransform],
) -> Option<Motion<'a>> {
    let properties = unique_index(properties.iter().map(|r| {
        (
            (r.identity.segment_token.as_str(), r.identity.record_ordinal),
            r,
        )
    }));
    let transforms = unique_index(transforms.iter().map(|r| {
        (
            (r.identity.segment_token.as_str(), r.identity.record_ordinal),
            r,
        )
    }));
    decode(source, &properties, &transforms)
}

#[test]
fn all_body_motion_classes_select_the_same_body_motion_family() {
    for (class, expected) in [
        (MOVE_CLASS_ID, "Move"),
        (MOVE_BODY_CLASS_ID, "MoveBody"),
        (ROTATE_CLASS_ID, "Rotate"),
    ] {
        assert!(class_admits(class, 3));
        assert_eq!(class_admits(class, 13), class == MOVE_CLASS_ID);
        assert!(matches!(
            crate::feature::FeatureFamily::from_class_id(class),
            Some(crate::feature::FeatureFamily::Motion)
        ));
        assert_eq!(
            crate::feature::native_kind(Some(class)),
            cadmpeg_ir::features::NativeFeatureKind::Other(expected.into())
        );
    }
}

#[test]
fn one_transform_moves_every_selected_persistent_body_in_millimetres() {
    let source = test_feature(0, 3, &[(0, 1), (1, 2), (2, 8)]);
    let props = properties();
    let transforms = [matrix([
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ])];
    let motion = decode_fixture(&source, &props, &transforms).expect("valid synthetic motion");
    let FeatureOperation::MoveBody {
        bodies: BodySelection::Local { bodies, native },
        translation,
        rotation,
        copies,
    } = motion.operation
    else {
        panic!("body move")
    };
    assert_eq!(
        bodies.iter().map(String::as_str).collect::<Vec<_>>(),
        [props[2].id(), props[3].id()]
    );
    assert_eq!(native.as_str(), props[0].id());
    assert_eq!(translation.get(), Vector3::new(10.0, -20.0, 30.0));
    assert!(rotation.is_none());
    assert_eq!(copies, 0);
}

fn apply_motion(point: Vector3, translation: FiniteVector3, rotation: &AxisAngle) -> Vector3 {
    let a = rotation.direction.get();
    let angle = rotation.angle.get();
    let c = angle.cos();
    let s = angle.sin();
    let dot = a.x * point.x + a.y * point.y + a.z * point.z;
    Vector3::new(
        c * point.x
            + s * (a.y * point.z - a.z * point.y)
            + (1.0 - c) * dot * a.x
            + translation.get().x,
        c * point.y
            + s * (a.z * point.x - a.x * point.z)
            + (1.0 - c) * dot * a.y
            + translation.get().y,
        c * point.z
            + s * (a.x * point.y - a.y * point.x)
            + (1.0 - c) * dot * a.z
            + translation.get().z,
    )
}

#[test]
fn axis_angle_and_translation_preserve_rotation_about_an_offset_axis() {
    let (translation, rotation) = rigid_motion([
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("valid synthetic motion");
    let rotation = rotation.expect("valid synthetic motion");
    assert_eq!(rotation.origin, FinitePoint3::ZERO);
    assert!((rotation.angle.get() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    let reached = apply_motion(Vector3::new(10.0, 20.0, 30.0), translation, &rotation);
    assert!((reached.x - 0.0).abs() < 1e-12);
    assert!((reached.y + 10.0).abs() < 1e-12);
    assert!((reached.z - 40.0).abs() < 1e-12);
}

#[test]
fn half_turns_and_small_rotations_do_not_lose_the_axis() {
    for axis in 0..3 {
        let mut rows = Transform::identity().rows();
        for (row, values) in rows.iter_mut().enumerate().take(3) {
            if row != axis {
                values[row] = -1.0;
            }
        }
        let (_, rotation) = rigid_motion(rows).expect("valid synthetic motion");
        let rotation = rotation.expect("valid synthetic motion");
        assert!((rotation.angle.get() - std::f64::consts::PI).abs() < 1e-12);
        let expected = [0, 1, 2].map(|i| if i == axis { 1.0 } else { 0.0 });
        let actual = rotation.direction.get();
        assert_eq!([actual.x, actual.y, actual.z], expected);
    }
    let angle = 1e-12_f64;
    let (c, s) = (angle.cos(), angle.sin());
    let (_, rotation) = rigid_motion([
        [c, -s, 0.0, 0.0],
        [s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("valid synthetic motion");
    assert!((rotation.expect("valid synthetic motion").angle.get() - angle).abs() < 1e-24);
}

#[test]
fn non_rigid_non_affine_and_overflowing_motions_stay_native() {
    for rows in [
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        assert!(rigid_motion(rows).is_none());
    }
}

#[test]
fn duplicate_missing_foreign_and_multiple_motion_operands_stay_native() {
    let source = test_feature(0, 3, &[(0, 1), (1, 2), (2, 8)]);
    let transforms = [matrix(Transform::identity().rows())];
    for indices in [vec![], vec![4, 4], vec![999]] {
        let mut props = properties();
        props[0] = property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: list(&indices),
            },
        );
        assert!(decode_fixture(&source, &props, &transforms).is_none());
    }
    for indices in [vec![], vec![10, 10], vec![999]] {
        let mut props = properties();
        props[1] = property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::TransformList,
                items: list(&indices),
            },
        );
        assert!(decode_fixture(&source, &props, &transforms).is_none());
    }
    let mut foreign = transforms[0].clone();
    foreign.identity.segment_token =
        cadmpeg_ir::ids::IdentityKey::try_new("other").expect("valid synthetic motion");
    assert!(decode_fixture(&source, &properties(), &[foreign]).is_none());
    let mut wrong_type = transforms[0].clone();
    wrong_type.identity.type_id = RecordTypeId::from_bytes(FACE_ITEM_TYPE);
    assert!(decode_fixture(&source, &properties(), &[wrong_type]).is_none());
    let mut props = properties();
    props.push(props[0].clone());
    assert!(decode_fixture(&source, &props, &transforms).is_none());
}

fn list_bytes(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(indices.len())
            .expect("valid synthetic motion")
            .to_le_bytes(),
    );
    if !indices.is_empty() {
        bytes.extend_from_slice(
            &u32::try_from(indices.len())
                .expect("valid synthetic motion")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

#[test]
fn motion_lists_use_the_versioned_header_and_end_at_the_record_boundary() {
    for (version, stamps) in [
        (19, vec![]),
        (23, vec![-1]),
        (25, vec![-1, 7]),
        (28, vec![-1, 7]),
    ] {
        for type_id in [
            TRANSFORM_LIST_TYPE,
            BODY_SELECTION_TYPE,
            FACE_COLLECTION_TYPE,
        ] {
            let mut bytes = content_at(5, version, &stamps);
            bytes.extend(list_bytes(&[0x8000_0007]));
            let parser = feature_property_parser(type_id).expect("valid synthetic motion");
            let parsed = parse(&bytes, |ctx, view| {
                parser(ctx, view, version).expect("valid synthetic motion")
            });
            assert_eq!(parsed.header.stamps, stamps);
            let PmDcFeaturePropertyKind::References { items, .. } = parsed.kind else {
                panic!("reference list")
            };
            assert_eq!(items.references(), [reference(7)]);
            for end in [bytes.len() - 1, bytes.len() + 1] {
                let mut invalid = bytes.clone();
                invalid.resize(end, 0);
                assert!(parse(&invalid, |ctx, view| parser(ctx, view, version)).is_err());
            }
        }
    }
}

#[test]
fn face_items_keep_native_indices_out_of_the_record_reference_space() {
    for indices in [vec![], vec![71]] {
        let mut bytes = content_at(5, 25, &[-1, 7]);
        bytes.extend(list_bytes(&indices));
        if !indices.is_empty() {
            bytes.extend_from_slice(&(-1i32).to_le_bytes());
        }
        let parsed = parse(&bytes, |ctx, view| {
            parse_face_item(ctx, view, 25).expect("valid synthetic motion")
        });
        let PmDcFeaturePropertyKind::FaceItem { index_references } = parsed.kind else {
            panic!("face item")
        };
        assert_eq!(index_references.values(), indices);
    }
}

#[test]
fn face_items_require_minus_one_only_after_nonempty_lists() {
    for indices in [vec![], vec![71]] {
        let mut bytes = content_at(5, 25, &[-1, 7]);
        bytes.extend(list_bytes(&indices));
        assert_eq!(
            parse(&bytes, |ctx, view| parse_face_item(ctx, view, 25)).is_ok(),
            indices.is_empty()
        );
        for terminal in [i32::MIN, -2, 0, 1, i32::MAX, -1] {
            let mut with_terminal = bytes.clone();
            with_terminal.extend(terminal.to_le_bytes());
            assert_eq!(
                parse(&with_terminal, |ctx, view| parse_face_item(ctx, view, 25)).is_ok(),
                !indices.is_empty() && terminal == -1
            );
        }
    }
}

#[test]
fn a_face_translation_keeps_the_face_selection_and_affected_body_distinct() {
    let source = test_feature(0, 13, &[(0, 1), (1, 2), (8, 5), (9, 8), (11, 6), (12, 9)]);
    let mut props = properties();
    props[0] = property(
        1,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            items: list(&[8]),
        },
    );
    props[1] = property(
        2,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Motion,
            type_value: 2,
            value: 2,
        },
    );
    props.extend([
        property(
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: list(&[4]),
            },
        ),
        property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        property(
            7,
            PmDcFeaturePropertyKind::FaceItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(PmDcListMetadata::U32([1, 0x1000_0000])),
                    vec![71],
                )
                .expect("valid synthetic motion"),
            },
        ),
    ]);
    let transforms = [matrix([
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])];
    let motion = decode_fixture(&source, &props, &transforms).expect("valid synthetic motion");
    assert_eq!(motion.bodies, [props[2].id()]);
    let FeatureOperation::MoveFace {
        faces: FaceSelection::Native(native),
        motion: FaceMotion::Translate {
            distance,
            direction,
        },
    } = motion.operation
    else {
        panic!("face translation")
    };
    assert_eq!(native, props[0].id());
    assert_eq!(distance.get(), 20.0);
    assert_eq!(direction.get(), Vector3::new(1.0, 0.0, 0.0));
    let face_item = props.pop().expect("synthetic face item");
    props.push(property(
        7,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(2, None, vec![])
                .expect("valid empty native selection"),
        },
    ));
    assert!(decode_fixture(&source, &props, &transforms).is_none());
    props.pop();
    props.push(face_item);
    props[1] = property(
        2,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Motion,
            type_value: 2,
            value: 3,
        },
    );
    assert!(decode_fixture(&source, &props, &transforms).is_none());
}

#[test]
fn validation_detects_changed_translation_rotation_selection_and_copy_count() {
    let source = test_feature(0, 3, &[(0, 1), (1, 2), (2, 8)]);
    let props = properties();
    let transforms = [matrix(Transform::identity().rows())];
    let original = decode_fixture(&source, &props, &transforms)
        .expect("valid synthetic motion")
        .operation;
    let index = ValidationIndex::new(&props, &transforms);
    assert!(index.matches(&source, &FeatureDefinition::Operation(original.clone())));
    let mut changed_source = serde_json::to_value(&source).expect("synthetic source");
    changed_source["properties"]["marker"] = serde_json::json!(3);
    let changed_source = serde_json::from_value(changed_source).expect("synthetic source");
    assert!(!index.matches(
        &changed_source,
        &FeatureDefinition::Operation(original.clone())
    ));
    let FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation,
        copies,
    } = original
    else {
        panic!("body move")
    };
    for changed in [
        FeatureOperation::MoveBody {
            bodies: bodies.clone(),
            translation: FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0))
                .expect("valid synthetic motion"),
            rotation: rotation.clone(),
            copies,
        },
        FeatureOperation::MoveBody {
            bodies: BodySelection::Native("other".into()),
            translation,
            rotation: rotation.clone(),
            copies,
        },
        FeatureOperation::MoveBody {
            bodies: bodies.clone(),
            translation,
            rotation: Some(AxisAngle {
                origin: FinitePoint3::ZERO,
                direction: FeatureDirection3::from(
                    UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))
                        .expect("valid synthetic motion"),
                ),
                angle: Angle::new(1.0).expect("valid synthetic motion"),
            }),
            copies,
        },
        FeatureOperation::MoveBody {
            bodies,
            translation,
            rotation,
            copies: 1,
        },
    ] {
        assert!(!index.matches(&source, &FeatureDefinition::Operation(changed)));
    }
}

#[test]
fn native_motion_properties_validate_the_member_type_and_empty_item_state() {
    let bytes = content_at(5, 19, &[])
        .into_iter()
        .chain(list_bytes(&[0x8000_0007]))
        .collect::<Vec<_>>();
    for (type_id, expected) in [
        (BODY_SELECTION_TYPE, "474d8790d011f8d10008cabc0663dc09"),
        (TRANSFORM_LIST_TYPE, "184d8790d011f8d10008cabc0663dc09"),
        (FACE_COLLECTION_TYPE, "ccb59f50d1114f6b0008a1ba32a3dc09"),
    ] {
        let parsed = parse(&bytes, |ctx, view| {
            feature_property_parser(type_id).expect("valid synthetic motion")(ctx, view, 19)
                .expect("valid synthetic motion")
        });
        let record = Located::new(
            parsed,
            RecordTypeId::from_bytes(type_id),
            test_feature(0, 0, &[]).identity.segment_token.clone(),
            1,
        );
        assert!(property_is_valid(&record, |r| (r.index() == 7).then_some(expected)));
        assert!(!property_is_valid(&record, |_| Some(
            "00000000000000000000000000000000"
        )));
        assert!(!property_is_valid(&record, |_| None));
    }
    let mut item = property(
        1,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(2, None, vec![]).expect("valid synthetic motion"),
        },
    );
    item.identity.type_id = RecordTypeId::from_bytes(FACE_ITEM_TYPE);
    assert!(property_is_valid(&item, |_| None));
    let item = property(
        1,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(2, Some(PmDcListMetadata::U32([1, 0])), vec![71])
                .expect("synthetic face indices"),
        },
    );
    assert!(property_is_valid(&item, |_| None));
}

#[test]
fn body_selections_admit_null_members_and_booleans_admit_both_boolean_types() {
    let bytes = content_at(5, 19, &[])
        .into_iter()
        .chain(list_bytes(&[0, 0x8000_0007]))
        .collect::<Vec<_>>();
    for (type_id, expected, valid) in [
        (
            BODY_SELECTION_TYPE,
            "474d8790d011f8d10008cabc0663dc09",
            true,
        ),
        (
            TRANSFORM_LIST_TYPE,
            "184d8790d011f8d10008cabc0663dc09",
            false,
        ),
    ] {
        let parsed = parse(&bytes, |ctx, view| {
            feature_property_parser(type_id).expect("valid synthetic motion")(ctx, view, 19)
                .expect("valid synthetic motion")
        });
        let record = Located::new(
            parsed,
            RecordTypeId::from_bytes(type_id),
            test_feature(0, 0, &[]).identity.segment_token.clone(),
            1,
        );
        assert_eq!(
            property_is_valid(&record, |r| (r.index() == 7).then_some(expected)),
            valid
        );
    }
    for (type_id, valid) in [
        ("284d8790d011f8d10008cabc0663dc09", true),
        ("9f8f2a40d21120e66000aaabc8433cb0", true),
        ("474d8790d011f8d10008cabc0663dc09", false),
    ] {
        let mut record = property(
            1,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        );
        let mut bytes = [0_u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte =
                u8::from_str_radix(&type_id[index * 2..index * 2 + 2], 16).expect("synthetic type");
        }
        record.identity.type_id = RecordTypeId::from_bytes(bytes);
        assert_eq!(property_is_valid(&record, |_| None), valid);
    }
}

#[test]
fn native_motion_lists_require_type_2_markers_after_deserialization() {
    for family in [
        PmDcFeatureReferenceFamily::ObjectCollection,
        PmDcFeatureReferenceFamily::FeatureDimensions,
        PmDcFeatureReferenceFamily::TransformList,
        PmDcFeatureReferenceFamily::BodySelection,
        PmDcFeatureReferenceFamily::FaceCollection,
    ] {
        let valid = property(
            1,
            PmDcFeaturePropertyKind::References {
                family,
                items: list(&[]),
            },
        );
        assert!(property_is_valid(&valid, |_| None));
        let mut changed = serde_json::to_value(&valid).expect("synthetic property");
        changed["kind"]["items"]["marker"] = serde_json::json!(3);
        let changed = serde_json::from_value(changed).expect("synthetic property");
        assert!(!property_is_valid(&changed, |_| None));
    }
    let valid = property(
        1,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(2, None, vec![]).expect("empty index list"),
        },
    );
    assert!(property_is_valid(&valid, |_| None));
    let mut changed = serde_json::to_value(&valid).expect("synthetic face item");
    changed["kind"]["index_references"]["marker"] = serde_json::json!(3);
    let changed = serde_json::from_value(changed).expect("synthetic face item");
    assert!(!property_is_valid(&changed, |_| None));
}

#[test]
fn shared_motion_operand_kinds_require_their_record_types() {
    let mut records = properties();
    records.push(property(
        11,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    for mut record in records {
        record.identity.type_id = RecordTypeId::from_bytes([0; 16]);
        assert!(!property_is_valid(&record, |_| None));
    }
    let selection = property(
        1,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items: list(&[4]),
        },
    );
    assert!(property_is_valid(&selection, |_| Some(
        "474d8790d011f8d10008cabc0663dc09"
    )));
    assert!(!property_is_valid(&selection, |_| Some(
        "ccb59f50d1114f6b0008a1ba32a3dc09"
    )));
}

#[test]
fn native_motion_selections_reject_repeated_record_identities() {
    for (family, member_type) in [
        (
            PmDcFeatureReferenceFamily::BodySelection,
            "474d8790d011f8d10008cabc0663dc09",
        ),
        (
            PmDcFeatureReferenceFamily::FaceCollection,
            "ccb59f50d1114f6b0008a1ba32a3dc09",
        ),
    ] {
        for (references, expected) in [(&[4, 5][..], true), (&[4, 4][..], false)] {
            let selection = property(
                1,
                PmDcFeaturePropertyKind::References {
                    family,
                    items: list(references),
                },
            );
            assert_eq!(
                property_is_valid(&selection, |_| Some(member_type)),
                expected
            );
        }
    }
}

#[test]
fn motion_mode_uses_the_shared_enumeration_layout_after_the_stamps() {
    for (version, stamps) in [
        (19, vec![]),
        (23, vec![-1]),
        (25, vec![-1, 7]),
        (28, vec![-1, 7]),
    ] {
        let mut bytes = content_at(5, version, &stamps);
        bytes.extend(2i16.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        let parsed = parse(&bytes, |ctx, view| {
            parse_mode(ctx, view, version).expect("valid synthetic motion")
        });
        assert_eq!(parsed.header.stamps, stamps);
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Motion,
                type_value: 2,
                value: 2
            }
        ));
    }
}

#[test]
fn quaternion_branches_preserve_a_non_coordinate_rotation_axis() {
    let a = Vector3::from(
        UnitVector3::normalized(Vector3::new(1.0, 2.0, 3.0)).expect("valid synthetic motion"),
    );
    for angle in [
        1e-12,
        0.3,
        std::f64::consts::PI - 1e-8,
        std::f64::consts::PI,
        std::f64::consts::PI + 1e-8,
        4.0,
        -0.7,
    ] {
        let (s, c) = angle.sin_cos();
        let k = 1.0 - c;
        let rows = [
            [
                c + a.x * a.x * k,
                a.x * a.y * k - a.z * s,
                a.x * a.z * k + a.y * s,
                1.0,
            ],
            [
                a.y * a.x * k + a.z * s,
                c + a.y * a.y * k,
                a.y * a.z * k - a.x * s,
                -2.0,
            ],
            [
                a.z * a.x * k - a.y * s,
                a.z * a.y * k + a.x * s,
                c + a.z * a.z * k,
                3.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let (translation, rotation) = rigid_motion(rows).expect("valid synthetic motion");
        let rotation = rotation.expect("valid synthetic motion");
        let reached = apply_motion(Vector3::new(2.0, -3.0, 5.0), translation, &rotation);
        let expected = rows.map(|r| r[0] * 2.0 - r[1] * 3.0 + r[2] * 5.0 + r[3] * 10.0);
        for (actual, expected) in [reached.x, reached.y, reached.z].into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-10);
        }
    }
}
