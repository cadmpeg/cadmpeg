use crate::design::decode::meta::decode_types;
use crate::design::decode::operands::decode_construction_operand_groups;
use crate::design::decode::scopes::parameter_scope::{
    decode_parameter_scopes, ParameterScopeSources,
};
use crate::design::decode::sketch::decode_related_record_headers;
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::move_face::{
    MoveFaceFixture, MoveFaceFrame, OTHER_GUID, TRANSFORM_GUID, TRANSLATION,
};
use cadmpeg_ir::features::{FaceMotion, FaceSelection, FeatureDefinition, FeatureOperation};

struct Projection {
    scope: DesignParameterScope,
    operation: FeatureOperation,
    group_id: Option<String>,
    losses: Vec<cadmpeg_ir::report::loss::LossKind>,
}

fn project(fixture: &MoveFaceFixture) -> Projection {
    crate::test_support::zip_test::with_scan(&fixture.archive(), |scan| {
        let types = decode_types(scan).unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut decoded = decode_parameter_scopes(
            &ctx,
            scan,
            &ParameterScopeSources {
                types: &types,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(decoded.scopes.len(), 1);
        assert!(decoded.scopes[0]
            .reference_members()
            .values()
            .copied()
            .eq(fixture.references.iter().copied()));
        let scope = &decoded.scopes[0];
        let stream = crate::ids::native_stream(&scope.id).unwrap();
        let indices = scope
            .reference_members()
            .values()
            .copied()
            .chain(
                scope
                    .move_operation()
                    .map(|motion| motion.transform_record_index),
            )
            .chain(
                scope
                    .move_operation()
                    .and_then(|motion| motion.face_group.as_ref().map(|group| group.value)),
            )
            .map(|index| (stream.to_owned(), index))
            .collect::<Vec<_>>();
        let headers = decode_related_record_headers(scan, &indices).unwrap();
        let groups =
            decode_construction_operand_groups(scan, &mut decoded.scopes, &headers).unwrap();
        let timelines = crate::design::test_support::synthetic_feature_timelines(&decoded.scopes);
        let projected = project_parameter_design_with_edge_identities(
            &ctx,
            &ProjectInputs {
                scopes: &decoded.scopes,
                timelines: &timelines,
                construction_groups: &groups,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(projected.features.len(), 1);
        decoded.losses.extend(projected.projection_losses);
        let FeatureDefinition::Operation(operation) = projected.features[0].evaluation.definition()
        else {
            panic!("expected operation");
        };
        Projection {
            scope: decoded.scopes.remove(0),
            operation: operation.clone(),
            group_id: groups.first().map(|group| group.id.clone()),
            losses: crate::test_support::loss_codes(&decoded.losses),
        }
    })
}

fn assert_complete_motion(motion: &FaceMotion, source: [[f64; 4]; 4]) {
    const EPS_RECONSTRUCTION: f64 = 1.0e-12;
    let actual = match motion {
        FaceMotion::Transform { transform } => transform.rows(),
        FaceMotion::Translate {
            direction,
            distance,
        } => {
            let direction = direction.get();
            [
                [1., 0., 0., direction.x * distance.get()],
                [0., 1., 0., direction.y * distance.get()],
                [0., 0., 1., direction.z * distance.get()],
                [0., 0., 0., 1.],
            ]
        }
        FaceMotion::Rotate {
            axis_origin,
            axis_dir,
            angle,
        } => {
            let axis = axis_dir.get();
            let axis = [axis.x, axis.y, axis.z];
            let point = axis_origin.get();
            let point = [point.x, point.y, point.z];
            let (sine, cosine) = angle.get().sin_cos();
            let cross = [
                [0., -axis[2], axis[1]],
                [axis[2], 0., -axis[0]],
                [-axis[1], axis[0], 0.],
            ];
            let basis = std::array::from_fn::<_, 3, _>(|row| {
                std::array::from_fn::<_, 3, _>(|column| {
                    (if row == column { cosine } else { 0. })
                        + (1. - cosine) * axis[row] * axis[column]
                        + sine * cross[row][column]
                })
            });
            std::array::from_fn(|row| {
                if row == 3 {
                    return [0., 0., 0., 1.];
                }
                [
                    basis[row][0],
                    basis[row][1],
                    basis[row][2],
                    point[row]
                        - (0..3)
                            .map(|column| basis[row][column] * point[column])
                            .sum::<f64>(),
                ]
            })
        }
        FaceMotion::Offset { .. } => panic!("matrix requires spatial motion"),
    };
    for (row, (source, actual)) in source.into_iter().zip(actual).enumerate() {
        for (column, (source, actual)) in source.into_iter().zip(actual).enumerate() {
            let expected = source * if row < 3 && column == 3 { 10. } else { 1. };
            if expected == 0. {
                assert!(
                    actual.abs() <= EPS_RECONSTRUCTION,
                    "{row},{column}: {actual}"
                );
            } else {
                assert!(
                    actual != 0.
                        && (expected - actual).abs() <= EPS_RECONSTRUCTION * expected.abs(),
                    "{row},{column}: {actual} != {expected}"
                );
            }
        }
    }
}

fn assert_projected(result: &Projection, source: [[f64; 4]; 4], resolved: bool) {
    let FeatureOperation::MoveFace { faces, motion } = &result.operation else {
        panic!("expected MoveFace");
    };
    let expected = if resolved {
        result.group_id.as_ref().unwrap()
    } else {
        &result.scope.id
    };
    assert_eq!(faces, &FaceSelection::Native(expected.clone()));
    assert_complete_motion(motion, source);
    assert!(result
        .losses
        .contains(&F3dLossCode::FeatureMoveFaceMatrixAssumed.kind()));
    assert_eq!(
        result
            .losses
            .contains(&F3dLossCode::FeatureMoveFaceSelectionUnresolved.kind()),
        !resolved
    );
    if !resolved {
        assert!(result
            .losses
            .contains(&F3dLossCode::FeatureMoveFaceScopeReferencesAssumed.kind()));
    }
}

#[test]
fn framed_move_face_decodes_middle_transform_trailing_references_and_source_forms() {
    for marker_one in [false, true] {
        for form in [1, 2, 5] {
            for table in [
                &[50, 60, 71, 80, 81][..],
                &[50, 60, 80, 71, 81],
                &[71, 50, 60, 80],
            ] {
                let fixture = MoveFaceFixture {
                    marker_one,
                    references: table.to_vec(),
                    frames: vec![MoveFaceFrame {
                        form,
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                let result = project(&fixture);
                assert_projected(&result, TRANSLATION, true);
                assert!(result
                    .losses
                    .contains(&F3dLossCode::FeatureMoveFaceScopeReferencesAssumed.kind()));
                let native = result.scope.move_operation().unwrap();
                assert_eq!(u32::from(native.form), form);
                assert_eq!(native.face_group.as_ref().unwrap().value, 50);
                let group_at = if marker_one {
                    crate::layout::move_face_marker_one_prologue::GROUP_REFERENCE
                } else {
                    crate::layout::move_face_marker_zero_prologue::GROUP_REFERENCE
                };
                assert_eq!(
                    native.face_group.as_ref().unwrap().offset,
                    result.scope.byte_offset() + u64::try_from(group_at + 1).unwrap()
                );
            }
        }
    }
}

#[test]
fn framed_move_face_retains_motion_through_inconsistent_selection_tables() {
    for table in [
        &[][..],
        &[50],
        &[50, 60],
        &[50, 60, 71, 71],
        &[60, 50, 71],
        &[50, 71, 60],
    ] {
        let result = project(&MoveFaceFixture {
            references: table.to_vec(),
            ..Default::default()
        });
        assert_projected(&result, TRANSLATION, false);
        assert!(result
            .losses
            .contains(&F3dLossCode::FeatureMoveFaceScopeReferencesAssumed.kind()));
    }
    for fixture in [
        MoveFaceFixture {
            group_role: None,
            ..Default::default()
        },
        MoveFaceFixture {
            group_role: Some(0x0000_0004_0000_0000),
            ..Default::default()
        },
        MoveFaceFixture {
            group_mark: None,
            ..Default::default()
        },
        MoveFaceFixture {
            group_mark: Some(61),
            ..Default::default()
        },
    ] {
        assert_projected(&project(&fixture), TRANSLATION, false);
    }
}

#[test]
fn framed_move_face_missing_metadata_and_unknown_prefix_preserve_motion() {
    for with_metadata in [false, true] {
        let mut fixture = MoveFaceFixture {
            with_metadata,
            prefix: 7,
            ..Default::default()
        };
        fixture.types.truncate(1);
        let result = project(&fixture);
        assert_projected(&result, TRANSLATION, true);
        for code in [
            F3dLossCode::FeatureMoveFaceTypeAssumed,
            F3dLossCode::FeatureMoveFacePrologueAssumed,
        ] {
            assert!(result.losses.contains(&code.kind()));
        }
        let native = serde_json::to_value(&result.scope).unwrap();
        assert_eq!(
            serde_json::from_value::<DesignParameterScope>(native).unwrap(),
            result.scope
        );
    }
}

#[test]
fn framed_move_face_filters_candidates_before_ambiguity_and_prefers_registration() {
    let mut different = TRANSLATION;
    different[2][3] = 3.;
    for sibling in [
        MoveFaceFrame {
            class: *b"258",
            matrix: different,
            ..Default::default()
        },
        MoveFaceFrame {
            class: *b"999",
            matrix: different,
            ..Default::default()
        },
        MoveFaceFrame {
            malformed: true,
            ..Default::default()
        },
        MoveFaceFrame {
            class: *b"2x8",
            ..Default::default()
        },
    ] {
        for first in [false, true] {
            let mut frames = vec![MoveFaceFrame::default(), sibling.clone()];
            if first {
                frames.reverse();
            }
            let result = project(&MoveFaceFixture {
                frames,
                ..Default::default()
            });
            assert_projected(&result, TRANSLATION, true);
            assert!(!result
                .losses
                .contains(&F3dLossCode::FeatureMoveFaceTypeAssumed.kind()));
        }
    }
}

#[test]
fn framed_move_face_refuses_conflicting_registration_and_actual_transform_ambiguity() {
    for registration in [(OTHER_GUID, 2), (TRANSFORM_GUID, 1)] {
        let mut fixture = MoveFaceFixture::default();
        fixture.types[1] = registration;
        let result = project(&fixture);
        assert!(result.scope.move_operation().is_none());
        assert!(result
            .losses
            .contains(&F3dLossCode::FeatureMoveFaceTypeConflict.kind()));
        assert!(matches!(result.operation, FeatureOperation::Native { .. }));
    }
    let mut distinct = MoveFaceFixture {
        frames: vec![
            MoveFaceFrame::default(),
            MoveFaceFrame {
                class: *b"258",
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    distinct.types[2] = (TRANSFORM_GUID, 2);
    assert!(project(&distinct).scope.move_operation().is_none());
    for registered in [false, true] {
        let mut fixture = MoveFaceFixture {
            frames: vec![MoveFaceFrame::default(); 2],
            ..Default::default()
        };
        if !registered {
            fixture.types.truncate(1);
        }
        let result = project(&fixture);
        assert!(result.scope.move_operation().is_none());
        assert!(matches!(result.operation, FeatureOperation::Native { .. }));
    }
}

#[test]
fn archive_move_face_keeps_the_selected_header_when_the_table_omits_its_transform() {
    let fixture = MoveFaceFixture {
        references: vec![50, 60],
        frames: vec![
            MoveFaceFrame {
                class: *b"258",
                ..Default::default()
            },
            MoveFaceFrame::default(),
        ],
        ..Default::default()
    };
    let decoded = super::decode(fixture.archive());
    let native = crate::test_support::native_test::f3d_native(decoded.ir());
    let scope = native.design_parameter_scopes.first().unwrap();
    let motion = scope.move_operation().unwrap();
    let header = native
        .design_record_headers
        .iter()
        .find(|header| header.record_index == motion.transform_record_index)
        .unwrap();
    assert!(
        crate::design::decode::scopes::direct_face::move_operation_links_hold(scope, Some(header))
    );
    let FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, motion }) =
        decoded.ir().model.features[0].evaluation.definition()
    else {
        panic!("expected complete face motion");
    };
    assert_eq!(faces, &FaceSelection::Native(scope.id.clone()));
    assert_complete_motion(motion, TRANSLATION);
}

#[test]
fn framed_move_face_conserves_small_screw_and_tiny_nonidentity_rotation() {
    let mut screw = [
        [0., -1., 0., 0.25],
        [1., 0., 0., 0.],
        [0., 0., 1., 2.0e-14],
        [0., 0., 0., 1.],
    ];
    let (sine, cosine) = 2.0e-14_f64.sin_cos();
    let tiny = [
        [cosine, -sine, 0., 0.25],
        [sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let mut tiny_only = tiny;
    tiny_only[0][3] = 0.;
    let mut disparate = TRANSLATION;
    disparate[0][3] = 1.0e200;
    disparate[1][3] = 1.0e-200;
    for matrix in [screw, tiny, tiny_only, disparate] {
        let result = project(&MoveFaceFixture {
            frames: vec![MoveFaceFrame {
                matrix,
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_projected(&result, matrix, true);
        assert!(matches!(
            result.operation,
            FeatureOperation::MoveFace {
                motion: FaceMotion::Transform { .. },
                ..
            }
        ));
    }
    let mut identity = TRANSLATION;
    identity[0][3] = 0.;
    identity[1][3] = 0.;
    let result = project(&MoveFaceFixture {
        frames: vec![MoveFaceFrame {
            matrix: identity,
            ..Default::default()
        }],
        ..Default::default()
    });
    assert_projected(&result, identity, true);
    assert!(matches!(
        result.operation,
        FeatureOperation::MoveFace {
            motion: FaceMotion::Transform { .. },
            ..
        }
    ));
    screw[2][3] = 0.;
    for matrix in [TRANSLATION, screw] {
        let result = project(&MoveFaceFixture {
            frames: vec![MoveFaceFrame {
                matrix,
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_projected(&result, matrix, true);
        assert!(matches!(
            result.operation,
            FeatureOperation::MoveFace {
                motion: FaceMotion::Translate { .. } | FaceMotion::Rotate { .. },
                ..
            }
        ));
    }
}

#[test]
fn move_face_native_links_validate_marked_offsets() {
    use crate::design::decode::scopes::direct_face::move_operation_links_hold;
    use crate::records::decal::DesignRecordHeader;
    use crate::records::feature::scope::DesignScopePayloadMut;

    for (marker_one, count, transform, group, header_start) in
        [(false, 127, 22, 38, 276), (true, 131, 26, 42, 280)]
    {
        let scope = project(&MoveFaceFixture {
            marker_one,
            ..Default::default()
        })
        .scope;
        assert_eq!(scope.reference_count_offset(), count);
        let motion = scope.move_operation().unwrap();
        assert_eq!(motion.transform_reference_offset, Some(transform));
        assert_eq!(motion.face_group.as_ref().unwrap().offset, group);
        let header = DesignRecordHeader {
            id: "synthetic:header:transform#71".into(),
            record_index: 71,
            class_tag: "257".to_owned().try_into().unwrap(),
            byte_offset: header_start,
        };
        assert!(move_operation_links_hold(&scope, Some(&header)));
        for (is_transform, offset) in [
            (true, 0),
            (true, if marker_one { 22 } else { 26 }),
            (false, 0),
            (false, if marker_one { 38 } else { 42 }),
        ] {
            let mut changed = scope.clone();
            let DesignScopePayloadMut::MoveFace(Some(operation)) = changed.payload_mut() else {
                panic!("MoveFace payload");
            };
            if is_transform {
                operation.transform_reference_offset = Some(offset);
            } else {
                operation.face_group.as_mut().unwrap().offset = offset;
            }
            assert!(!move_operation_links_hold(&changed, Some(&header)));
        }
    }
}

#[test]
fn move_face_native_links_keep_marked_motion_without_table_provenance() {
    use crate::design::decode::scopes::direct_face::move_operation_links_hold;
    use crate::records::decal::DesignRecordHeader;
    use crate::records::feature::scope::DesignScopePayloadMut;

    for (marker_one, header_start) in [(false, 243), (true, 247)] {
        let mut scope = project(&MoveFaceFixture {
            marker_one,
            references: Vec::new(),
            group_mark: None,
            group_role: None,
            ..Default::default()
        })
        .scope;
        let header = DesignRecordHeader {
            id: "synthetic:header:transform#71".into(),
            record_index: 71,
            class_tag: "257".to_owned().try_into().unwrap(),
            byte_offset: header_start,
        };
        assert!(move_operation_links_hold(&scope, Some(&header)));
        let DesignScopePayloadMut::MoveFace(Some(operation)) = scope.payload_mut() else {
            panic!("MoveFace payload");
        };
        operation.transform_reference_offset = None;
        assert!(!move_operation_links_hold(&scope, Some(&header)));
    }
}
