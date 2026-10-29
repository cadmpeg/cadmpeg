// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{reference, segment};
use crate::feature::{parse_pattern_feature, MIRROR_FEATURE_TYPE, WORK_PLANE_TYPE};
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::mirror::{pattern, property, seed, Fixture};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

#[test]
fn mirrors_keep_ordered_feature_seeds_and_reflect_about_the_offset_plane() {
    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        let fixture = Fixture::new(false).at_version(version);
        let source = pattern(version, false, &[5, 3], &[]);
        let resolved = fixture
            .index()
            .resolve(&source, &fixture.label, seed)
            .expect("feature mirror");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            resolved.definition
        else {
            panic!("pattern")
        };
        assert_eq!(
            seeds,
            vec![
                PatternSeed::Feature(seed(4).expect("seed")),
                PatternSeed::Feature(seed(2).expect("seed"))
            ]
        );
        let PatternTransform::Mirror {
            plane_origin,
            plane_normal,
        } = pattern.definition()
        else {
            panic!("reflection")
        };
        assert_eq!(plane_origin.get(), Point3::new(20.0, 0.0, 0.0));
        assert_eq!(plane_normal.get(), Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(
            resolved.members,
            FeatureResultMembers::new(
                vec![NonBlankString::new(fixture.properties[3].id()).expect("body id")],
                Vec::new(),
                Vec::new(),
                Vec::new()
            )
            .expect("members")
        );
    }
}

#[test]
fn body_mirrors_select_the_native_input_collection() {
    let fixture = Fixture::new(true);
    let source = pattern(19, true, &[], &[]);
    let resolved = fixture
        .index()
        .resolve(&source, &fixture.label, |_| None)
        .expect("body mirror");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = resolved.definition
    else {
        panic!("pattern")
    };
    assert_eq!(
        seeds,
        vec![PatternSeed::Bodies(BodySelection::Native(
            fixture.properties[2].id()
        ))]
    );
    assert_eq!(resolved.result_native, fixture.properties[2].id());
}

#[test]
fn mirrors_refuse_missing_duplicate_and_unordered_seed_bindings() {
    let fixture = Fixture::new(false);
    for seeds in [vec![], vec![0], vec![7], vec![3, 3]] {
        assert!(fixture
            .index()
            .resolve(&pattern(19, false, &seeds, &[]), &fixture.label, seed)
            .is_none());
    }
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, |_| None)
        .is_none());
    assert!(Fixture::new(true)
        .index()
        .resolve(&pattern(19, true, &[3], &[]), &fixture.label, seed)
        .is_none());
}

#[test]
fn mirror_seeds_precede_the_mirror_under_the_same_model_root() {
    use crate::feature::history::{History, HistoryNodeKind, HistoryPlacement};

    let source = crate::feature::tests::test_feature(2, 0, &[]);
    let mut history = History::default();
    let before = HistoryPlacement {
        ordinal: 1,
        kind: HistoryNodeKind::Feature,
        below_end_of_part: false,
        model_history: false,
        model_root: 30,
    };
    history.placements.insert(2, before);
    history.placements.insert(
        20,
        HistoryPlacement {
            ordinal: 2,
            kind: HistoryNodeKind::Pattern,
            ..before
        },
    );
    assert_eq!(seed_feature_id(&history, 20, &source.identity), seed(2));
    for invalid in [
        HistoryPlacement {
            ordinal: 2,
            ..before
        },
        HistoryPlacement {
            ordinal: 3,
            ..before
        },
        HistoryPlacement {
            model_root: 31,
            ..before
        },
        HistoryPlacement {
            kind: HistoryNodeKind::Sketch,
            ..before
        },
    ] {
        history.placements.insert(2, invalid);
        assert!(seed_feature_id(&history, 20, &source.identity).is_none());
    }
    history.placements.insert(2, before);
    let mut wrong_type = source.identity.clone();
    wrong_type.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
    assert!(seed_feature_id(&history, 20, &wrong_type).is_none());
}

#[test]
fn mirrors_refuse_unclosed_results_and_missing_plane_or_transform() {
    let fixture = Fixture::new(false);
    for slot in [0, 2, 5, 6, 7, 9, 12] {
        assert!(fixture
            .index()
            .resolve(
                &pattern(19, false, &[3], &[(slot, 0)]),
                &fixture.label,
                seed
            )
            .is_none());
    }
    let mut duplicate = Fixture::new(false);
    duplicate.properties.push(duplicate.properties[2].clone());
    assert!(duplicate
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &duplicate.label, seed)
        .is_none());
}

#[test]
fn mirror_projection_keeps_resource_refusals() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        fixture.index().project(&ctx, &source, &fixture.label, seed),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities
    ));
}

#[test]
fn mirror_result_and_plane_slots_do_not_shift_at_version_extensions() {
    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        let bytes =
            crate::feature::tests::pattern_feature_bytes(version, PmDcPatternFamily::Mirror);
        let parsed = parse_pattern_feature(
            cadmpeg_core::decode::View::over_retained(&bytes),
            version,
            PmDcPatternFamily::Mirror,
        )
        .expect("mirror carrier");
        assert_eq!(parsed.property_slots.len(), 13);
        for (slot, reference) in [(9, 0x43), (10, 0x44), (11, 0x50), (12, 0x51)] {
            assert_eq!(parsed.property_slots[slot].index(), reference);
        }
    }
}

#[test]
fn mirrors_refuse_suffix_shapes_that_disagree_with_the_version() {
    for version in [19, 21, 29] {
        let fixture = Fixture::new(false).at_version(version);
        let source = pattern(version, false, &[3], &[]);
        assert!(fixture
            .index()
            .resolve(&source, &fixture.label, seed)
            .is_some());
        for change in 0..3 {
            let mut payload = (*source).clone();
            match change {
                0 => {
                    payload.property_slots.pop();
                }
                1 => payload.property_slots.push(reference(0)),
                _ => payload.extension_values.push(0),
            }
            let malformed = Located::new(
                payload,
                RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
                segment(),
                20,
            );
            assert!(fixture
                .index()
                .resolve(&malformed, &fixture.label, seed)
                .is_none());
        }
    }
}

#[test]
fn mirrors_refuse_unsupported_record_versions() {
    let fixture = Fixture::new(false);
    for version in [0, 14, 24, 26, 27, 32] {
        assert!(fixture
            .index()
            .resolve(&pattern(version, false, &[3], &[]), &fixture.label, seed)
            .is_none());
    }
}

#[test]
fn mirrors_require_the_dimensions_family_and_operand_record_types() {
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[(5, 105)]), &fixture.label, seed)
        .is_none());
    for index in [0, 1, 2, 3, 4, 5, 6, 7] {
        let mut fixture = Fixture::new(false);
        fixture.properties[index].identity.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
        assert!(fixture
            .index()
            .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
            .is_none());
    }
    let mut fixture = Fixture::new(false);
    fixture.transforms[0].identity.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.planes[0].identity.type_id = RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
        .is_none());
}

#[test]
fn generated_instance_maps_remain_native() {
    let fixture = Fixture::new(true).at_version(25);
    assert!(fixture
        .index()
        .resolve(&pattern(25, true, &[], &[]), &fixture.label, |_| None)
        .is_some());
    assert!(fixture
        .index()
        .resolve(&pattern(25, true, &[], &[(8, 150)]), &fixture.label, |_| {
            None
        })
        .is_none());
    let losses = native_losses(&[pattern(25, true, &[], &[(8, 150)])], &[]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.mirror-operands-native"
    );
}

#[test]
fn mirrors_refuse_nonorthogonal_plane_frames_and_nonidentity_placements() {
    let source = pattern(19, false, &[3], &[]);
    let mut fixture = Fixture::new(false);
    let mut plane = (*fixture.planes[0]).clone();
    plane.u_axis = plane.v_axis;
    fixture.planes[0] = Located::new(
        plane,
        RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment(),
        103,
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    let mut transform = (*fixture.transforms[0]).clone();
    let mut rows = IDENTITY;
    rows[0][3] = 1.0;
    transform.matrix = crate::compact_matrix::CompactMatrix::try_from_rows(0x8429, 0x7bd6, rows)
        .expect("translated matrix");
    fixture.transforms[0] = Located::new(
        transform,
        RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
        segment(),
        100,
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
}

#[test]
fn native_validation_compares_the_reflection_seeds_and_result_identity() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (mut feature, mut result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    assert!(fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
    result.native_ref = Some(source.id());
    assert!(!fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
    result.native_ref = Some(fixture.properties[2].id());
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &feature,
        &result,
        |_| seed(4)
    ));
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Feature(seed(2).expect("seed"))],
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
                plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).expect("normal"),
            })
            .expect("reflection"),
        }),
    );
    assert!(!fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
}

#[test]
fn native_validation_requires_canonical_feature_and_result_ids() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    let mut changed_result = result.clone();
    changed_result.id = FeatureResultTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
        fixture.transforms[0].identity.key().expect("record key"),
    );
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &feature,
        &changed_result,
        seed
    ));
    let mut changed_feature = feature.clone();
    changed_feature.id = seed(4).expect("other feature id");
    changed_result = result.clone();
    changed_result.output_of = changed_feature.id.clone();
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &changed_feature,
        &changed_result,
        seed
    ));
    let mut changed_feature = feature.clone();
    changed_feature.native_ref = Some(fixture.transforms[0].id());
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &changed_feature,
        &result,
        seed
    ));
}

#[test]
fn mirrors_refuse_nonzero_variables_and_boolean_options() {
    let source = pattern(19, false, &[3], &[]);
    let mut fixture = Fixture::new(false);
    fixture.properties[4] = property(
        107,
        PmDcFeaturePropertyKind::RdxVariable {
            name: String::new(),
            name_value: 0,
            nominal_value: 1,
            model_value: 1,
        },
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.properties[1] = property(
        102,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[(3, 1)]), &fixture.label, seed)
        .is_none());
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 103)]),
            &fixture.label,
            seed
        )
        .is_some());
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 106)]),
            &fixture.label,
            seed
        )
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.properties.push(property(
        108,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 109)]),
            &fixture.label,
            seed
        )
        .is_none());
    let mut payload = (*source).clone();
    payload.control = 1;
    let source = Located::new(
        payload,
        RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
        segment(),
        20,
    );
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
}

#[test]
fn every_unresolved_mirror_operand_has_a_pinned_loss() {
    for (name, fixture, source) in crate::test_support::mirror::refusal_cases() {
        assert!(
            fixture
                .index()
                .resolve(&source, &fixture.label, seed)
                .is_none(),
            "{name}"
        );
        let losses = native_losses(&[source], &[]);
        assert_eq!(losses.len(), 1, "{name}");
        assert_eq!(
            losses[0].code.local_code(),
            "feature.mirror-operands-native",
            "{name}"
        );
    }
}

#[test]
fn mirror_auxiliary_records_require_the_exact_versioned_header_and_tail() {
    use crate::test_support::test_fixtures::{content_at, parse};

    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        let stamps: &[i32] = match version {
            0..=22 => &[],
            23 => &[-1],
            _ => &[-1, 0],
        };
        let mut selection = content_at(1, version, stamps);
        selection.extend_from_slice(&0x3000_0004u32.to_le_bytes());
        selection.extend_from_slice(&0u32.to_le_bytes());
        let parsed = parse(&selection, |ctx, source| {
            parse_selection(ctx, source, version)
        })
        .expect("selection control")
        .expect("fixed selection form");
        assert_eq!(
            parsed.kind,
            PmDcFeaturePropertyKind::Mirror {
                definition: Property::Selection {
                    control: 0x3000_0004,
                    value: 0
                },
            }
        );
        let mut options = content_at(2, version, stamps);
        options.extend_from_slice(&[0; 9]);
        let parsed = parse(&options, |ctx, source| parse_options(ctx, source, version))
            .expect("options control")
            .expect("fixed options form");
        assert_eq!(
            parsed.kind,
            PmDcFeaturePropertyKind::Mirror {
                definition: Property::Options {
                    bytes: [0; 9].into()
                },
            }
        );
        for (bytes, is_selection) in [(&selection, true), (&options, false)] {
            for end in 0..bytes.len() {
                let truncated = &bytes[..end];
                let result = parse(truncated, |ctx, source| {
                    if is_selection {
                        parse_selection(ctx, source, version)
                    } else {
                        parse_options(ctx, source, version)
                    }
                });
                assert!(result.is_err() || result.expect("unresolved shape").is_none());
            }
        }
        selection.push(0);
        options.push(0);
        assert!(parse(&selection, |ctx, source| parse_selection(
            ctx, source, version
        ))
        .expect("unknown selection tail")
        .is_none());
        assert!(
            parse(&options, |ctx, source| parse_options(ctx, source, version))
                .expect("unknown option tail")
                .is_none()
        );
    }
}

#[test]
fn a_neutral_mirror_has_no_native_operand_loss() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, _) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    assert!(native_losses(&[source], &[feature]).is_empty());
}

#[test]
fn mirror_operands_are_owned_by_mirror_references() {
    let source = pattern(19, false, &[3], &[]);
    let references = operand_references(std::slice::from_ref(&source));
    assert_eq!(references.len(), 2);
    assert!(references.contains(&(source.identity.segment_token.as_str(), 111)));
    assert!(references.contains(&(source.identity.segment_token.as_str(), 112)));
    let mut payload = (*source).clone();
    payload.family = PmDcPatternFamily::Rectangular;
    let rectangular = Located::new(
        payload,
        RecordTypeId::from_bytes(super::super::RECTANGULAR_PATTERN_FEATURE_TYPE),
        segment(),
        20,
    );
    assert!(operand_references(&[rectangular]).is_empty());
}

#[test]
fn an_unknown_selection_tail_stays_native_with_the_pinned_mirror_loss() {
    use crate::test_support::test_fixtures::{content_at, parse};

    let mut bytes = content_at(1, 19, &[]);
    bytes.extend_from_slice(&[0; 24]);
    assert!(
        parse(&bytes, |ctx, source| parse_selection(ctx, source, 19))
            .expect("framed unknown selection")
            .is_none()
    );
    let mut fixture = Fixture::new(false);
    fixture
        .properties
        .retain(|property| property.identity.record_ordinal != 111);
    let source = pattern(19, false, &[3], &[]);
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let losses = native_losses(&[source], &[]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.mirror-operands-native"
    );
}

#[test]
fn multi_operation_sheet_metal_parent_seeds_keep_the_pinned_native_loss() {
    use crate::feature::history::{History, HistoryNodeKind, HistoryPlacement};
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    let mut history = History::default();
    let before = HistoryPlacement {
        ordinal: 1,
        kind: HistoryNodeKind::Feature,
        below_end_of_part: false,
        model_history: true,
        model_root: 30,
    };
    history.placements.insert(2, before);
    history.placements.insert(
        20,
        HistoryPlacement {
            ordinal: 2,
            kind: HistoryNodeKind::Pattern,
            ..before
        },
    );
    for type_id in [
        "904bc1c4d311ff2860004da99dccefb0",
        "08dcddc3d311d0976000a7a99dccefb0",
    ] {
        let parent = crate::record_identity::RecordIdentity {
            segment_token: segment(),
            record_ordinal: 2,
            type_id: RecordTypeId::try_from(type_id.to_owned()).expect("type identity"),
        };
        assert!(fixture
            .index()
            .project(&ctx, &source, &fixture.label, |_| seed_feature_id(
                &history, 20, &parent
            ))
            .is_none());
        assert!(!fixture.index().matches_projection(
            &source,
            &fixture.label,
            &feature,
            &result,
            |_| seed_feature_id(&history, 20, &parent)
        ));
        let losses = native_losses(std::slice::from_ref(&source), &[]);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.mirror-operands-native"
        );
    }
}
