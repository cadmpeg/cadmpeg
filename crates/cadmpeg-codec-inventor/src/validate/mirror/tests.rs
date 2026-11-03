// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{is_mirror_construction, matches_history, requires_mirror_validation};
use crate::feature::history::{HistoryNodeKind, HistoryPlacement, SUPPRESSED_FLAG};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureDirection3,
    FeatureEvaluation, FeatureId, FeatureOperation, FinitePoint3, NativeFeatureKind,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;

fn feature() -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            cadmpeg_ir::ids::IdentityKey::try_new("test-1").expect("feature key"),
        ),
        ordinal: 3,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: NativeFeatureKind::Other("Mirror".into()),
                parameters: BTreeMap::default(),
            },
        )),
        native_ref: None,
    }
}

#[test]
fn mirror_construction_recognizes_patterns_and_shapes() {
    let mut feature = feature();
    let source = crate::test_support::mirror::pattern(19, false, &[3], &[]);
    assert!(!requires_mirror_validation(&feature, Some(&source)));
    assert!(!is_mirror_construction(&feature));
    let seeds = vec![PatternSeed::Feature(feature.id.clone())];
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: seeds.clone(),
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
                plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).expect("normal"),
            })
            .expect("mirror pattern"),
        },
    ));
    assert!(is_mirror_construction(&feature));
    assert!(requires_mirror_validation(&feature, Some(&source)));
    assert!(requires_mirror_validation(&feature, None));
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds,
            pattern: PatternKind::new(PatternTransform::Linear {
                direction: None,
                spacing: PositiveLength::new(1.0).expect("spacing"),
                count: 2,
                second: None,
                omitted_instances: Vec::new(),
            })
            .expect("linear pattern"),
        },
    ));
    assert!(!is_mirror_construction(&feature));
    assert!(requires_mirror_validation(&feature, Some(&source)));
    assert!(!requires_mirror_validation(&feature, None));
    let mut payload = (*source).clone();
    payload.family = crate::feature::PmDcPatternFamily::Rectangular;
    let changed = crate::record_identity::Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        source.identity.record_ordinal,
    );
    assert!(requires_mirror_validation(&feature, Some(&changed)));
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::MirrorShape {
            source: cadmpeg_ir::features::BodySelection::Native("source-body".into()),
            plane_origin: FinitePoint3::ZERO,
            plane_normal: cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))
                .expect("normal"),
            plane_reference: None,
        },
    ));
    assert!(is_mirror_construction(&feature));
    assert!(requires_mirror_validation(&feature, Some(&source)));
    assert!(requires_mirror_validation(&feature, None));
}

#[test]
fn mirror_history_uses_native_order_scope_and_suppression() {
    let placement = HistoryPlacement {
        ordinal: 3,
        kind: HistoryNodeKind::Pattern,
        below_end_of_part: false,
        model_history: true,
        model_root: 0,
    };
    let feature = feature();
    assert!(matches_history(&feature, placement, 0));
    let mut changed = feature.clone();
    changed.ordinal = 4;
    assert!(!matches_history(&changed, placement, 0));
    let mut changed = feature.clone();
    changed.suppressed = Some(true);
    assert!(!matches_history(&changed, placement, 0));
    assert!(!matches_history(&feature, placement, SUPPRESSED_FLAG));
    assert!(!matches_history(
        &feature,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        0
    ));
    assert!(!matches_history(
        &feature,
        HistoryPlacement {
            below_end_of_part: true,
            ..placement
        },
        0
    ));
    let mut changed = feature.clone();
    changed.source_properties.insert(
        NonBlankString::new("history".to_owned()).expect("property key"),
        "flat_pattern".into(),
    );
    assert!(!matches_history(&changed, placement, 0));
    assert!(matches_history(
        &changed,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        0
    ));
    changed.suppressed = Some(true);
    changed.source_properties.insert(
        NonBlankString::new("suppressed".to_owned()).expect("property key"),
        "true".into(),
    );
    assert!(matches_history(
        &changed,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        SUPPRESSED_FLAG
    ));
}

#[test]
fn validator_uses_all_mirror_operand_and_suffix_gates() {
    use crate::test_support::mirror::{pattern, seed, Fixture};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        for body in [false, true] {
            let fixture = Fixture::new(body).at_version(version);
            let source = pattern(version, body, if body { &[] } else { &[3] }, &[]);
            let (feature, result) = fixture
                .index()
                .project(&ctx, &source, &fixture.label, seed, |_| {})
                .expect("mirror form")
                .expect("projection");
            assert!(fixture.index().matches_projection(
                &source,
                &fixture.label,
                &feature,
                &result,
                seed
            ));
        }
    }
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed, |_| {})
        .expect("mirror")
        .expect("projection");
    for (name, changed, source) in crate::test_support::mirror::refusal_cases() {
        assert!(
            !changed
                .index()
                .matches_projection(&source, &changed.label, &feature, &result, seed),
            "{name}"
        );
    }
}

#[test]
fn validator_rejects_body_mirror_boolean_controls_against_a_body_projection() {
    use crate::test_support::mirror::{body_boolean_form, body_boolean_refusal_cases, seed};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for version in [19, 21, 29] {
        let (fixture, source) = body_boolean_form(version, false, Some(false));
        let (feature, result) = fixture
            .index()
            .project(&ctx, &source, &fixture.label, seed, |_| {})
            .expect("body mirror")
            .expect("projection");
        assert!(fixture.index().matches_projection(
            &source,
            &fixture.label,
            &feature,
            &result,
            seed
        ));
        for (name, changed, source) in body_boolean_refusal_cases()
            .into_iter()
            .filter(|(_, _, source)| source.save_version_major == version)
        {
            assert!(
                !changed.index().matches_projection(
                    &source,
                    &changed.label,
                    &feature,
                    &result,
                    seed
                ),
                "{name}"
            );
        }
    }
}

#[test]
fn mirror_options_have_exactly_nine_native_bytes() {
    use crate::feature::mirror::Property;

    for bytes in ["", "00", "0000000000000000", "00000000000000000000"] {
        assert!(serde_json::from_value::<Property>(
            serde_json::json!({"kind": "options", "bytes": bytes})
        )
        .is_err());
    }
    assert_eq!(
        serde_json::from_value::<Property>(
            serde_json::json!({"kind": "options", "bytes": "000000000000000000"})
        )
        .expect("nine bytes"),
        Property::Options {
            bytes: [0; 9].into()
        }
    );
}

#[test]
fn validator_checks_each_auxiliary_payload_length_against_the_versioned_header() {
    use crate::test_support::mirror::{pattern, seed, Fixture};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for (version, selection, options) in [
        (19, 30, 31),
        (23, 34, 35),
        (25, 38, 39),
        (29, 42, 43),
        (31, 42, 43),
    ] {
        let fixture = Fixture::new(false).at_version(version);
        let source = pattern(version, false, &[3], &[]);
        let (feature, result) = fixture
            .index()
            .project(&ctx, &source, &fixture.label, seed, |_| {})
            .expect("mirror")
            .expect("projection");
        let token = fixture.properties[0].identity.segment_token.as_str();
        let index = fixture
            .index()
            .with_record_lengths([(token, 111, selection), (token, 112, options)].into_iter());
        assert!(index.matches_projection(&source, &fixture.label, &feature, &result, seed));
        for (bad_selection, bad_options) in [
            (selection - 1, options),
            (selection + 1, options),
            (selection + 16, options),
            (selection, options - 1),
            (selection, options + 1),
        ] {
            let index = fixture.index().with_record_lengths(
                [(token, 111, bad_selection), (token, 112, bad_options)].into_iter(),
            );
            assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, seed));
        }
        let index = fixture
            .index()
            .with_record_lengths([(token, 111, selection)].into_iter());
        assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, seed));
    }
}

#[test]
fn validator_checks_the_body_copy_form_and_instance_map_lengths() {
    use crate::test_support::mirror::new_body_form;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (fixture, source) = new_body_form();
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, |_| None, |_| {})
        .expect("body copy")
        .expect("projection");
    assert!(is_mirror_construction(&feature));
    assert!(requires_mirror_validation(&feature, None));
    let token = source.identity.segment_token.as_str();
    let index = fixture.index().with_record_lengths(
        [
            (token, 111, 30),
            (token, 112, 31),
            (token, 113, 78),
            (token, 200, 66),
        ]
        .into_iter(),
    );
    assert!(index.matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    for length in [0, 77, 79, 82] {
        let index = fixture.index().with_record_lengths(
            [
                (token, 111, 30),
                (token, 112, 31),
                (token, 113, length),
                (token, 200, 66),
            ]
            .into_iter(),
        );
        assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    }
    let index = fixture
        .index()
        .with_record_lengths([(token, 111, 30), (token, 112, 31)].into_iter());
    assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    for length in [0, 65, 67, 80] {
        let index = fixture.index().with_record_lengths(
            [
                (token, 111, 30),
                (token, 112, 31),
                (token, 113, 78),
                (token, 200, length),
            ]
            .into_iter(),
        );
        assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    }
    let index = fixture
        .index()
        .with_record_lengths([(token, 111, 30), (token, 112, 31), (token, 113, 78)].into_iter());
    assert!(!index.matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    let mut changed = feature.clone();
    let mut definition = changed.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::MirrorShape {
        source: selection, ..
    }) = &mut definition
    else {
        panic!("mirror shape")
    };
    *selection = cadmpeg_ir::features::BodySelection::Native("other-source".into());
    changed.evaluation.set_definition(definition);
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &changed,
        &result,
        |_| None
    ));
}

#[test]
fn validator_rejects_each_unresolved_body_copy_operand() {
    use crate::test_support::mirror::{body_copy_refusal_cases, new_body_form};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (fixture, source) = new_body_form();
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, |_| None, |_| {})
        .expect("body copy")
        .expect("projection");
    for (name, changed, source) in body_copy_refusal_cases() {
        assert!(
            !changed
                .index()
                .matches_projection(&source, &changed.label, &feature, &result, |_| None),
            "{name}"
        );
    }
}

#[test]
fn validator_admits_each_measured_body_copy_outline() {
    use crate::test_support::mirror::new_body_form;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for outline in [3, 9, 15] {
        let (mut fixture, source) = new_body_form();
        let mut node = serde_json::to_value(&fixture.generated_features[0]).expect("node");
        node["outline_value"] = serde_json::json!(outline);
        fixture.generated_features[0] = serde_json::from_value(node).expect("outline form");
        let (feature, result) = fixture
            .index()
            .project(&ctx, &source, &fixture.label, |_| None, |_| {})
            .expect("body copy")
            .expect("projection");
        assert!(fixture.index().matches_projection(
            &source,
            &fixture.label,
            &feature,
            &result,
            |_| None
        ));
    }
}

#[test]
fn validator_checks_the_replacement_form_and_each_neighbouring_refusal() {
    use crate::test_support::mirror::{replacement_form, replacement_refusal_cases};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (fixture, source) = replacement_form();
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, |_| None, |_| {})
        .expect("replacement")
        .expect("projection");
    assert!(fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, |_| None));
    for (name, changed, source) in replacement_refusal_cases() {
        assert!(
            !changed
                .index()
                .matches_projection(&source, &changed.label, &feature, &result, |_| None),
            "{name}"
        );
    }
}
