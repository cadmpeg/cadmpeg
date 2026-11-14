use super::*;
use crate::records::{
    feature::{
        scope::DesignScopePayloadMut,
        sheet_metal::fold::{DesignRefoldOperation, DesignUnfoldOperation},
    },
    identity::Located,
};

fn scope(kind: DesignFeatureKind, index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("f3d:test/Design1/BulkStream.dat:scope#{index}"),
        kind,
        index,
    )
}

#[test]
fn unfold_projects_partial_inputs_and_preserves_flag_and_rule_scalar() {
    let mut source = scope(DesignFeatureKind::Unfold, 80);
    let mut losses = Vec::new();
    let partial = unfold(&source, &ProjectInputs::default(), &mut losses).unwrap();
    assert!(
        matches!(partial, FeatureDefinition::Operation(FeatureOperation::SheetMetalUnfold { construction }) if construction == SheetMetalUnfoldConstruction::default())
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::UnfoldEnvelopeUnresolved.kind()));
    crate::design::test_support::name_scope_references(&mut source, [20, 30, 40]);
    source
        .try_edit(|draft| crate::design::test_support::lengthen_scope_frame(draft, 512))
        .unwrap();
    for (flag, expected) in [(0, Some(false)), (1, Some(true)), (7, None)] {
        if let DesignScopePayloadMut::Unfold(slot) = source.payload_mut() {
            *slot = Some(DesignUnfoldOperation {
                prefix_end: 66,
                control: Located {
                    value: flag,
                    offset: 21,
                },
                bend_group: Located {
                    value: 20,
                    offset: 22,
                },
                settings: Located {
                    value: 30,
                    offset: 33,
                },
                wrapper: Located {
                    value: 35,
                    offset: 44,
                },
                stationary_group: Located {
                    value: 40,
                    offset: 55,
                },
                rule_record_index: Some(60),
                rule_reference_assumed: true,
                rule_material_scalar: Some(Located {
                    value: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                    offset: 900,
                }),
            });
        }
        assert!(source.unfold_operation().unwrap().valid_for_scope(&source));
        let mut losses = Vec::new();
        let definition = unfold(&source, &ProjectInputs::default(), &mut losses).unwrap();
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::UnfoldSettingsRuleReferenceAssumed.kind()));
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalUnfold { construction }) =
            definition
        else {
            panic!("Unfold");
        };
        assert_eq!(construction.all_bends, expected);
        assert_eq!(construction.neutral_factor.unwrap().get(), 1.0);
        assert_eq!(
            construction.stationary,
            Some(FaceSelection::Native(source.id.clone()))
        );
        assert_eq!(
            construction.bends,
            Some(FaceSelection::Native(source.id.clone()))
        );
    }
}

#[test]
fn refold_requires_one_emitted_earlier_unfold_in_the_same_stream() {
    let source = scope(DesignFeatureKind::Unfold, 80);
    let mut target = scope(DesignFeatureKind::Refold, 20);
    let stream = native_stream(&source.id).unwrap();
    let id = FeatureId::mint("test:model:feature#unfold").unwrap();
    let ids = HashMap::from([((stream, 80), id.clone())]);
    let ordinals = HashMap::from([((stream, 80), 0), ((stream, 20), 3)]);
    for candidates in [vec![80], vec![], vec![80, 100]] {
        if let DesignScopePayloadMut::Refold(slot) = target.payload_mut() {
            *slot = Some(DesignRefoldOperation {
                group_record_index: 90,
                group_byte_offset: 0,
                group_paired_byte_offset: 125,
                members: [80, 100, 150, 20]
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| Located {
                        value,
                        offset: 76 + u64::try_from(ordinal).unwrap() * 11,
                    })
                    .collect(),
                preceding_unfolds: candidates.clone(),
            });
        }
        let scopes = [source.clone(), target.clone()];
        let inputs = ProjectInputs {
            scopes: &scopes,
            ..Default::default()
        };
        let definition = refold(&target, &inputs, &ids, &ordinals, &mut Vec::new());
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalRefold { construction }) =
            definition
        else {
            panic!("Refold");
        };
        assert_eq!(
            construction.unfold,
            (candidates.len() == 1).then(|| id.clone())
        );
    }
    if let DesignScopePayloadMut::Refold(slot) = target.payload_mut() {
        slot.as_mut().unwrap().preceding_unfolds = vec![80];
    }
    for (sources, emitted, order) in [
        (vec![], ids.clone(), ordinals.clone()),
        (
            vec![scope(DesignFeatureKind::Bend, 80)],
            ids.clone(),
            ordinals.clone(),
        ),
        (vec![source.clone()], HashMap::new(), ordinals.clone()),
        (
            vec![source.clone()],
            ids.clone(),
            HashMap::from([((stream, 80), 4), ((stream, 20), 3)]),
        ),
        (
            vec![DesignParameterScope::empty(
                "f3d:other/Design1/BulkStream.dat:scope#80",
                DesignFeatureKind::Unfold,
                80,
            )],
            ids.clone(),
            ordinals.clone(),
        ),
    ] {
        let inputs = ProjectInputs {
            scopes: &sources,
            ..Default::default()
        };
        assert!(
            matches!(refold(&target, &inputs, &emitted, &order, &mut Vec::new()), FeatureDefinition::Operation(FeatureOperation::SheetMetalRefold { construction }) if construction.unfold.is_none())
        );
    }
}

#[test]
fn fold_projection_binds_source_dependency_without_sketch_geometry() {
    crate::test_support::with_decode_context(|ctx| {
        let source = scope(DesignFeatureKind::Unfold, 80);
        let middle = scope(DesignFeatureKind::Bend, 200);
        let mut target = scope(DesignFeatureKind::Refold, 20);
        if let DesignScopePayloadMut::Refold(slot) = target.payload_mut() {
            *slot = Some(DesignRefoldOperation {
                group_record_index: 90,
                group_byte_offset: 0,
                group_paired_byte_offset: 114,
                members: vec![
                    Located {
                        value: 80,
                        offset: 76,
                    },
                    Located {
                        value: 200,
                        offset: 87,
                    },
                    Located {
                        value: 20,
                        offset: 98,
                    },
                ],
                preceding_unfolds: vec![80],
            });
        }
        let scopes = [source, middle, target];
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
        let inputs = ProjectInputs {
            scopes: &scopes,
            timelines: &timelines,
            ..Default::default()
        };
        let projected =
            super::super::project_parameter_design_with_edge_identities(ctx, &inputs).unwrap();
        let unfold = projected
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Unfold"))
            .unwrap();
        let refold = projected
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Refold"))
            .unwrap();
        assert_eq!(unfold.ordinal, 0);
        assert_eq!(refold.ordinal, 2);
        assert!(refold.dependencies.contains(&unfold.id));
        assert!(
            matches!(refold.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::SheetMetalRefold { construction }) if construction.unfold.as_ref() == Some(&unfold.id))
        );
    });
}
