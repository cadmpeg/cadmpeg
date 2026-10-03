// SPDX-License-Identifier: Apache-2.0
use crate::geometry::pcurve::PcurveNurbs;
use crate::math::Point2;
use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

fn copy_with_policy(
    geometry: &SketchGeometry,
    policy: &DecodePolicy,
) -> Result<SketchGeometry, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).expect("empty root");
    geometry.try_clone_for_decode(&ctx, "sketch geometry copy")
}

#[test]
fn sketch_nurbs_copy_refuses_each_nested_collection() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("linear NURBS fixture");
    let geometry = SketchGeometry::nurbs(curve);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = copy_with_policy(&geometry, &policy).expect_err("four knots exceed zero items");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "sketch geometry copy"));
    policy.limits.max_collection_items = 4;
    let error = copy_with_policy(&geometry, &policy).expect_err("two poles exceed four knot items");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "sketch geometry copy"));
    assert_eq!(
        copy_with_policy(&geometry, &DecodePolicy::service()).expect("service copy"),
        geometry
    );
}

#[test]
fn sketch_native_copy_refuses_retained_text() {
    let geometry = SketchGeometry::native(NonBlankString::new("native").expect("nonblank"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 5;
    let error = copy_with_policy(&geometry, &policy).expect_err("six bytes exceed five");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "sketch geometry copy"));
    assert_eq!(
        copy_with_policy(&geometry, &DecodePolicy::service()).expect("service copy"),
        geometry
    );
}

#[test]
fn sketch_external_copy_refuses_nested_text_and_subelements() {
    let geometry =
        SketchGeometry::from_admitted_definition(SketchGeometryDefinition::ExternalReference {
            document: Some("doc".to_owned()),
            object: NonBlankString::new("object").expect("nonblank"),
            subelements: vec!["edge".to_owned()],
        });
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let error = copy_with_policy(&geometry, &policy).expect_err("document exceeds retained cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "sketch geometry copy"));
    policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
    policy.limits.max_collection_items = 0;
    let error =
        copy_with_policy(&geometry, &policy).expect_err("subelement exceeds collection cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "sketch geometry copy"));
    assert_eq!(
        copy_with_policy(&geometry, &DecodePolicy::service()).expect("service copy"),
        geometry
    );
}
