// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ONE_COMMENT: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";
const WITH_LOCAL_SYSTEM: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";

fn parse(
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<Vec<super::super::CurveExpressionRecord>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root input is admitted");
    super::super::expression_records_with_model_name(&ctx, payload, None)
}

#[test]
fn curve_expression_labels_refuse_before_vector_growth() {
    assert_eq!(
        parse(ONE_COMMENT, DecodePolicy::service())
            .expect("service profile admits expression")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = parse(ONE_COMMENT, policy).expect_err("one label needs one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record labels"
    ));
}

#[test]
fn curve_expression_local_system_body_refuses_before_copy() {
    assert_eq!(
        parse(WITH_LOCAL_SYSTEM, DecodePolicy::service())
            .expect("service profile admits local system")
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error =
        parse(WITH_LOCAL_SYSTEM, policy).expect_err("the local-system body needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression local-system body"
    ));
}

#[test]
fn curve_expression_lines_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = parse(ONE_COMMENT, policy).expect_err("the line follows one label");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression record lines"
    ));
}

#[test]
fn curve_expression_line_text_refuses_before_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = parse(ONE_COMMENT, policy).expect_err("the source line needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo expression record line text"
    ));
}

#[test]
fn curve_expression_records_refuse_before_vector_growth() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = parse(ONE_COMMENT, policy).expect_err("the record follows its label and line");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo expression records"
    ));
}

#[test]
fn curve_parameter_scalar_cache_refuses_before_unique_image_growth() {
    let payload = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root input is admitted");
        super::super::parameter_records_with_face_ids(&ctx, &payload, None)
    };
    assert!(run(3).expect("service admits scalar image").is_empty());
    let error = run(0).expect_err("scalar image requires a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar cache unique images"
    ));
}
