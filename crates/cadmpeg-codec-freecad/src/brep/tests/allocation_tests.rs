// SPDX-License-Identifier: Apache-2.0
//! Input-sized BREP parser allocation tests.

use super::super::parse_text;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn text_brep_token_index_refuses_at_materialized_limit() {
    let bytes = b"CASCADE Topology V1, (c) Matra-Datavision Locations 0 Curve2ds 0 Curves 0 Polygon3D 0 PolygonOnTriangulations 0 Surfaces 0 Triangulations 0 TShapes 0 *";
    let token_count = bytes.split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty()).count();
    let required = u64::try_from(token_count * std::mem::size_of::<&str>())
        .expect("test token index size fits u64");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = required - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(parse_text(&ctx, bytes), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "FreeCAD text B-rep tokens"
            && limit.additional == required));
    policy.limits.max_materialized_bytes = required;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(parse_text(&ctx, bytes).is_ok());
}
