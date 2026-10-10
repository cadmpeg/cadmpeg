// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::{new_body_boolean_op, NewBodyEvidence};
use crate::native::history::BodyWriterHistory;
use cadmpeg_ir::features::{BooleanOp, FeatureId};
use cadmpeg_ir::ids::BodyId;

#[test]
fn nx_block_new_body_ignores_only_the_provisional_initial_writer() {
    crate::test_support::with_decode_context(|ctx| {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let provisional =
            FeatureId::mint("synthetic:test:id#initial-bodies").expect("identity grammar");
        let mut history = BodyWriterHistory::default();
        history
            .record_writer(ctx, None, None, std::slice::from_ref(&body), &provisional)
            .expect("admitted writer history");

        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 0,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: None,
                    history: &history,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::NewBody
        );

        let fallback_prior =
            FeatureId::mint("synthetic:test:id#fallback-prior-feature").expect("identity grammar");
        let mut fallback_history = BodyWriterHistory::default();
        fallback_history
            .record_writer(
                ctx,
                None,
                None,
                std::slice::from_ref(&body),
                &fallback_prior,
            )
            .expect("admitted writer history");
        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 0,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: None,
                    history: &fallback_history,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::Unresolved
        );

        let prior = FeatureId::mint("synthetic:test:id#prior-feature").expect("identity grammar");
        history
            .record_writer(ctx, Some(7), None, std::slice::from_ref(&body), &prior)
            .expect("admitted writer history");
        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 1,
                    provisional_feature: Some(&provisional),
                    native_primary_body: Some(7),
                    offset_store_primary_body: None,
                    history: &history,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::Unresolved
        );
        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: false,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 0,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: None,
                    history: &history,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::Unresolved
        );

        let offset_prior =
            FeatureId::mint("synthetic:test:id#offset-prior-feature").expect("identity grammar");
        let mut offset_history = BodyWriterHistory::default();
        offset_history
            .record_writer(ctx, None, Some("store:block#7"), &[], &offset_prior)
            .expect("admitted writer history");
        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 1,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: Some("store:block#7"),
                    history: &offset_history,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::Unresolved
        );

        let offset_without_prior = BodyWriterHistory::default();
        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 1,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: Some("store:block#8"),
                    history: &offset_without_prior,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::NewBody
        );

        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: false,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 2,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: None,
                    history: &offset_without_prior,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::Unresolved
        );

        assert_eq!(
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: true,
                    outputs: std::slice::from_ref(&body),
                    body_reference_count: 2,
                    provisional_feature: Some(&provisional),
                    native_primary_body: None,
                    offset_store_primary_body: None,
                    history: &offset_without_prior,
                }
            )
            .expect("admitted writer history"),
            BooleanOp::NewBody
        );
    });
}
