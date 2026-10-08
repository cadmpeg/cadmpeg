// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn transform_child_depth_refusal_destroys_path_before_frame_storage() {
    let identity_record = |sequence| {
        let values = [
            124.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        ParameterRecord::from_test_tokens(
            sequence,
            1..2,
            Vec::new(),
            values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::real(value),
                span: 0..0,
            }).collect(),
            Vec::new(),
        )
    };
    let parent = super::transform_entry(1, 0);
    let child = super::transform_entry(3, 1);
    let parent_record = identity_record(1);
    let child_record = identity_record(3);
    let entries = BTreeMap::from([(1, &parent), (3, &child)]);
    let records = BTreeMap::from([(1, &parent_record), (3, &child_record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    let mut policy = DecodePolicy::service();
    // The first frame inserts D3. Entering its D1 parent requests frame two.
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    let error = super::super::resolve_transform(
        3, &entries, &records, 1.0, precision, &mut path, &ctx,
    ).unwrap_err();
    let super::super::TransformResolutionError::Resource(CodecError::ResourceLimit(first)) = error
    else { panic!("expected child depth refusal") };
    assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_eq!(first.operation, "iges_transform_chain");
    assert!(path.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));

    policy.limits.max_recursion_depth = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    assert_eq!(super::super::resolve_transform(
        3, &entries, &records, 1.0, precision, &mut path, &ctx,
    ).unwrap(), cadmpeg_ir::transform::Transform::identity());
    assert!(path.is_empty());
    ctx.finish_session().unwrap();
}
