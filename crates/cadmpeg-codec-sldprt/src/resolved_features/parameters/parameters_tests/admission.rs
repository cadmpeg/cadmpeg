use super::{enrich_history_parameters, parameter_limit_fixture};
use crate::records::{FeatureHistory, FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn typed_scalar_fixture(class: &str) -> (Vec<FeatureHistory>, FeatureInputLane) {
    let (histories, mut lane) = parameter_limit_fixture();
    lane.classes.push(FeatureInputClass {
        id: "parameter-class".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 5,
        name: class.into(),
    });
    (histories, lane)
}

fn assert_typed_scalar_formatting(class: &str, expected: &str) {
    let (mut histories, lane) = typed_scalar_fixture(class);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    enrich_history_parameters(&ctx, &mut histories, std::slice::from_ref(&lane), true).unwrap();
    assert_eq!(histories[0].features[0].parameters["D1"], expected);

    let (mut histories, lane) = typed_scalar_fixture(class);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let error = enrich_history_parameters(&ctx, &mut histories, std::slice::from_ref(&lane), true)
        .expect_err("a typed scalar expression requires retained output storage");
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected retained-byte refusal: {error:?}");
    };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.operation, "format SLDPRT native scalar");
    assert!(histories[0].features[0].parameters.is_empty());
    let CodecError::ResourceLimit(repeated) = ctx
        .charge_work(0, "check sticky typed scalar refusal")
        .expect_err("scalar formatting refusal must fuse the session")
    else {
        panic!("expected sticky refusal");
    };
    assert_eq!(first, repeated);
}

#[test]
fn length_scalar_formatting_admits_retained_output_and_preserves_units() {
    assert_typed_scalar_formatting("moLengthParameter_c", "500mm");
}

#[test]
fn angle_scalar_formatting_admits_retained_output_and_preserves_units() {
    assert_typed_scalar_formatting("moAngleParameter_c", "0.5rad");
}

#[test]
fn preserved_parameter_does_not_allocate_an_unused_scalar_expression() {
    for class in ["moLengthParameter_c", "moAngleParameter_c"] {
        let (mut histories, lane) = typed_scalar_fixture(class);
        histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "kept".into());
        let expected = histories.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
        enrich_history_parameters(&ctx, &mut histories, std::slice::from_ref(&lane), false)
            .expect("preserving an existing parameter creates no retained expression or key");
        assert_eq!(histories, expected);
        assert!(ctx.resource_refusal().is_none());
    }
}
