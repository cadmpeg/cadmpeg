// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn preserved_source_patch_uses_caller_input_limit() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic BREP");
    let mut input = EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()));
    input.limits.max_input_bytes = 0;
    let error = F3dCodec
        .plan(input, TargetRequest::Inherit)
        .expect_err("the retained source exceeds the caller's input limit");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("the input ceiling returns a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::InputBytes
    );
    assert_eq!(limit.limit, 0);

    let mut input = EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()));
    input.limits.max_entities = 0;
    let error = F3dCodec
        .plan(input, TargetRequest::Inherit)
        .expect_err("patch decoding exceeds the caller's entity limit");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("the entity ceiling returns a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.limit, 0);
}
