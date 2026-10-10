// SPDX-License-Identifier: Apache-2.0
//! Temporary history lookup keys do not publish session retention.

use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::{
    DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
    DesignWorkPointInputCarrier,
};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::{
    DatumPointConstruction, FeatureDefinition, FeatureOperation, VertexSelection,
};

const LOOKUP_SCRATCH: u64 = 4096;

#[test]
fn history_lookup_lifetime_work_point_key_is_scratch_and_keeps_native_dependency() {
    let mut predecessor = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(4);
        })
        .unwrap();
    let recipe_id = "f3d:native/BulkStream.dat:construction-recipe#vertex".to_string();
    let recipe = DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: 12,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("369".to_owned())
                .unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 15,
            recipe_record_byte_offset: 32,
            recipe_id: recipe_id.clone(),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 4,
            recipe_program: vec![0],
            resolution: Some(
                crate::records::feature::work_geometry::DesignVertexResolution::new(4, 43)
                    .expect("valid vertex slot"),
            ),
            next_record_index: 17,
            next_byte_offset: 200,
        },
    )
    .unwrap();
    let mut point = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#20",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        20,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPoint(slot) =
        point.payload_mut()
    {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 21,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([4.0, 3.0, 0.0]),
            position_offset: 0,
            rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex {
                    input: DesignWorkPointInput::try_new(
                        22,
                        0,
                        Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                            recipe,
                        })),
                    )
                    .unwrap(),
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:native/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 20,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let scopes = vec![predecessor, point];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = LOOKUP_SCRATCH;
    policy.limits.max_retained_bytes = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        // Both index construction and the later WorkPoint dependency query
        // borrow their key. The latter must not copy text into session output.
        let _probe = RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "f3d history lookup stream",
            None,
        );
        let (features, parameters) = project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
        .expect("temporary history lookup key stays in scratch");
        assert!(parameters.is_empty());
        assert_eq!(features.len(), 2);
        let predecessor = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(&scopes[0].id))
            .expect("projected predecessor");
        let point = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(&scopes[1].id))
            .expect("projected WorkPoint");
        let FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            construction: Some(construction),
            ..
        }) = point.evaluation.definition()
        else {
            panic!("typed datum-point construction")
        };
        let DatumPointConstruction::Vertex {
            vertex:
                VertexSelection::Historical {
                    state,
                    vertex,
                    native,
                },
        } = construction.as_ref()
        else {
            panic!("historical vertex construction")
        };
        let feature_key = point.id.key();
        let prefix = crate::ids::history_input_prefix(&feature_key, 4);
        assert_eq!(state, &crate::ids::feature_input_topology_id(&point.id, 4));
        assert_eq!(vertex, &crate::ids::history_input_vertex_id(&prefix, 43));
        assert_eq!(native.as_str(), &recipe_id);
        assert_eq!(point.dependencies.as_slice(), [predecessor.id.clone()]);
        assert_eq!(ctx.resource_refusal(), None);
        drop(
            ctx.reserve_scoped(
                LOOKUP_SCRATCH,
                "reuse query scratch while projected features live",
            )
            .unwrap(),
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

fn predecessor_chain() -> Vec<DesignParameterScope> {
    let stream = format!("f3d:{}/BulkStream.dat", "a".repeat(1024));
    (1..=8)
        .map(|state_id| {
            let mut scope = DesignParameterScope::empty(
                &format!("{stream}:parameter-scope#{state_id}"),
                crate::records::feature::scope::DesignFeatureKind::Extrude,
                u32::try_from(state_id).unwrap(),
            );
            scope
                .try_edit(|draft| {
                    draft.history_state_id = Some(state_id);
                    if state_id > 1 {
                        draft.previous_history_state_id = Some(state_id - 1);
                        draft.layout_fixture_tail();
                    }
                })
                .unwrap();
            scope
        })
        .collect()
}

#[test]
fn history_lookup_lifetime_predecessor_releases_each_consumed_key() {
    let scopes = predecessor_chain();
    let graph = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[])
    })
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = LOOKUP_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let selected = graph
                .predecessor(ctx, &scopes[7], |scope| Ok(scope.record_index == 1))
                .unwrap();
            let crate::design::feature_project::ScopeHistoryPredecessor::Scope(selected) = selected
            else {
                panic!("chain reaches its sole selected predecessor");
            };
            assert!(std::ptr::eq(selected, &scopes[0]));
            assert_eq!(selected.id, scopes[0].id);
            drop(
                ctx.reserve_scoped(LOOKUP_SCRATCH, "reuse complete predecessor query scratch")
                    .unwrap(),
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn history_lookup_lifetime_predecessor_key_one_below_keeps_original_refusal() {
    use cadmpeg_core::CodecError;
    let scopes = predecessor_chain();
    let graph = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::ScopeHistoryGraph::new(ctx, &scopes, &[], &[], &[], &[])
    })
    .unwrap();
    let bytes = u64::try_from(crate::ids::native_stream(&scopes[7].id).unwrap().len()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            graph.predecessor(ctx, &scopes[7], |_| Ok(true))
        else {
            panic!("actual first predecessor key text must refuse before copying");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d predecessor stream");
        assert_eq!((original.used, original.additional), (0, bytes));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(graph.predecessor(ctx, &scopes[7], |_| Ok(true)), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

mod callback_refusal;
