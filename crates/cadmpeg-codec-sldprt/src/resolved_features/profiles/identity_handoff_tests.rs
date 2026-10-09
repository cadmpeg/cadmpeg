use super::detached_legacy_sketch_tests::compact_profile_projection_fixture;
use super::project_compact_sketch_profiles;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};

const ID: &str = "sldprt:model:sketch#compact:1:30";
const OPERATION: &str = "project SLDPRT compact sketch profiles";

fn existing_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint(ID).unwrap(), name: None, configuration: None,
        visible: None, placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::default(), native_ref: None,
    }
}

#[test]
fn compact_profile_binding_promotes_only_its_selected_identity() {
    let (history, lane, feature) = compact_profile_projection_fixture();
    let mut features = [feature];
    let prior = existing_sketch();
    let mut sketches = vec![prior.clone()];
    let mut entities = Vec::new();
    let mut losses = Vec::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // This route binds the existing sketch; only the feature's ID copy is new.
    policy.limits.max_retained_bytes = u64::try_from(ID.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    project_compact_sketch_profiles(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        &[history],
        std::slice::from_ref(&lane),
        &mut losses,
    )
    .unwrap();
    assert_eq!(sketches, [prior]);
    assert!(entities.is_empty());
    assert!(losses.is_empty());
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }) if id.as_str() == ID));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn compact_profile_binding_preserves_original_identity_promotion_refusal() {
    let (history, lane, feature) = compact_profile_projection_fixture();
    let mut features = [feature];
    let prior = existing_sketch();
    let mut sketches = vec![prior.clone()];
    let mut entities = Vec::new();
    let mut losses = Vec::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(ID.len() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let error = project_compact_sketch_profiles(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        &[history],
        std::slice::from_ref(&lane),
        &mut losses,
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("expected identity promotion refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, u64::try_from(ID.len()).unwrap());
    assert_eq!(limit.limit, policy.limits.max_retained_bytes);
    assert_eq!(sketches, [prior]);
    assert!(entities.is_empty());
    assert!(losses.is_empty());
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Unresolved,
        })));
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
}
