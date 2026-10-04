use cadmpeg_core::CodecError;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily};
use std::collections::BTreeMap;

fn identity_lookup_input() -> [Feature; 2] {
    ["first", "second"].map(|name| Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        ordinal: 0, name: None, suppressed: Some(false), dependencies: Default::default(),
        source_properties: BTreeMap::new(), source_tag: None, source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved { family: UnresolvedFamily::Draft }),
        ),
        native_ref: Some("native".into()),
    })
}

fn assert_identity_lookup_refusal(
    operation: &str,
    project: impl Fn(&DecodeContext<'_>, &mut [Feature]) -> Result<(), CodecError>,
) {
    crate::test_support::work_refusal_at(operation, |ctx| project(ctx, &mut identity_lookup_input()));
    let mut features = identity_lookup_input();
    project(&cadmpeg_test_support::service_decode_context(), &mut features).unwrap();
    assert_eq!(features[0].id.as_str(), "synthetic:test:id#first");
    assert_eq!(features[1].id.as_str(), "synthetic:test:id#second");
    assert_eq!(features[0].native_ref.as_deref(), Some("native"));
    assert_eq!(features[1].native_ref.as_deref(), Some("native"));
}

#[test]
fn compact_edge_identity_lookup_propagates_work_refusal() {
    assert_identity_lookup_refusal("lookup SLDPRT compact edge feature identity", |ctx, features| {
        super::super::project_compact_edge_selections(ctx, features, &[], &[])
    });
}

#[test]
fn compact_surface_identity_lookup_propagates_work_refusal() {
    assert_identity_lookup_refusal("lookup SLDPRT compact surface feature identity", |ctx, features| {
        super::super::project_compact_surface_selections(ctx, features, &[], &[])
    });
}

#[test]
fn draft_identity_lookup_propagates_work_refusal() {
    assert_identity_lookup_refusal("lookup SLDPRT draft feature identity", |ctx, features| {
        super::super::project_draft_operands(ctx, features, &[], &[])
    });
}

#[test]
fn cosmetic_thread_identity_lookup_propagates_work_refusal() {
    assert_identity_lookup_refusal("lookup SLDPRT cosmetic thread feature identity", |ctx, features| {
        super::super::project_unbound_cosmetic_thread_faces(ctx, features, &[], &[], &[], &[])
    });
}
