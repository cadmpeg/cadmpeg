use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily,
};
use std::collections::BTreeMap;

fn identity_lookup_input() -> [Feature; 2] {
    ["first", "second"].map(|name| Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Draft,
            }),
        ),
        native_ref: Some("native".into()),
    })
}

#[test]
fn feature_identity_index_keeps_the_last_duplicate_and_propagates_work_refusal() {
    const OPERATION: &str = "index SLDPRT test feature identities";
    let features = identity_lookup_input();
    crate::test_support::work_refusal_at(OPERATION, |ctx| {
        super::super::model_feature_ids_by_native(ctx, &features, OPERATION).map(|_| ())
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    let (ids, _storage) =
        super::super::model_feature_ids_by_native(&ctx, &features, OPERATION).unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        ids.get("native").map(FeatureId::as_str),
        Some("synthetic:test:id#second")
    );
}

#[test]
fn feature_identity_index_refuses_materialized_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    const OPERATION: &str = "index SLDPRT test feature identities";
    let features = identity_lookup_input();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = super::super::model_feature_ids_by_native(&ctx, &features, OPERATION)
        .map(|_| ())
        .expect_err("the feature identity index exceeds the materialized limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == OPERATION));
}
