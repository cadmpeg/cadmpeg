// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

fn unknown_container_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/FastLoad/Structure",
        vec![0x5a; 2],
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("opaque container fixture");
    let scan = crate::decode::Scan {
        container,
        streams: Vec::new(),
    };
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::attach_container_layer(
        &ctx,
        &mut ir,
        &scan,
        &mut annotations,
        &mut unknowns,
        crate::native::TypedNative::ContainerOnly,
    )
    .unwrap_err()
}

#[test]
fn container_unknown_route_refuses_collection_limit() {
    let error = unknown_container_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "NX native unknown records"),
        "{error:?}"
    );
}

#[test]
fn container_unknown_route_refuses_retained_limit() {
    let error = unknown_container_refusal(|policy| policy.limits.max_retained_bytes = 2);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "NX native unknown records"),
        "{error:?}"
    );
}
