// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::features::BodySelection;

fn body_pattern(version: u8, second: bool) -> PmDcPatternFeature {
    let original = pattern(version, second);
    let mut source = (*original).clone();
    source.participants = reference_list(&[]);
    source.property_slots[6] = reference(99);
    source.property_slots[9] = reference(0);
    source.property_slots[10] = reference(86);
    Located::new(source, original.identity.type_id.clone(), segment(), 20)
}

fn mode_property(properties: &mut Vec<PmDcFeatureProperty>) {
    properties.push(test_property(
        98,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
}

#[test]
fn rectangular_body_pattern_uses_one_local_seed_and_the_same_body_result() {
    for version in [19, 25, 28, 29, 30, 31] {
        for second in [false, true] {
            let feature = project_fixture(&body_pattern(version, second), |_, _, properties| {
                mode_property(properties);
            })
            .expect("fixed-spacing body pattern");
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
                feature.evaluation.definition()
            else {
                panic!("pattern");
            };
            assert_eq!(
                seeds,
                &vec![PatternSeed::Bodies(
                    BodySelection::local(
                        vec![test_property(
                            95,
                            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) }
                        )
                        .id()],
                        test_property(
                            85,
                            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) }
                        )
                        .id(),
                    )
                    .expect("one wrapper")
                )]
            );
        }
    }
}

#[test]
fn rectangular_body_pattern_refuses_participants_and_non_body_members() {
    let original = body_pattern(19, false);
    let mut payload = (*original).clone();
    payload.participants = reference_list(&[3]);
    let source = Located::new(payload, original.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&source, |_, _, properties| mode_property(properties)).is_none());
    assert!(
        project_fixture(&body_pattern(19, false), |_, _, properties| {
            mode_property(properties);
            let wrapper = properties
                .iter_mut()
                .find(|property| property.identity.record_ordinal == 95)
                .expect("result wrapper");
            let mut payload = (**wrapper).clone();
            payload.kind = PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            };
            *wrapper = Located::new(payload, wrapper.identity.type_id.clone(), segment(), 95);
        })
        .is_none()
    );
    let mut payload = (*original).clone();
    payload.property_slots[11] = reference(99);
    let source = Located::new(payload, original.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&source, |_, _, properties| mode_property(properties)).is_none());
    let mut payload = (*original).clone();
    payload.property_slots[9] = reference(86);
    let source = Located::new(payload, original.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&source, |_, _, properties| mode_property(properties)).is_none());
}

#[test]
fn rectangular_feature_pattern_keeps_ordered_feature_seeds() {
    let feature = project_fixture(&pattern(19, false), |_, _, _| {}).expect("feature pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        feature.evaluation.definition()
    else {
        panic!("pattern")
    };
    assert_eq!(seeds.len(), 2);
    assert!(seeds
        .iter()
        .all(|seed| matches!(seed, PatternSeed::Feature(_))));
    let original = pattern(19, false);
    let mut payload = (*original).clone();
    payload.property_slots[10] = reference(86);
    let source = Located::new(payload, original.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&source, |_, _, _| {}).is_none());
}

#[test]
fn rectangular_body_seed_missing_join_stays_local_with_one_body_selection_loss() {
    let feature = project_fixture(&body_pattern(19, false), |_, _, properties| {
        mode_property(properties);
    })
    .expect("body pattern");
    let mut expected = feature.evaluation.definition().clone();
    super::super::super::body_selection::definition(&mut expected, &HashMap::new(), None);
    assert_eq!(&expected, feature.evaluation.definition());
    let mut model = cadmpeg_ir::document::Model::default();
    model.features.push(feature);
    let losses = super::super::super::body_selection::native_losses(&model, &HashMap::new());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.body-selection-native");
}
