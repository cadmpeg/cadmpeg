// SPDX-License-Identifier: Apache-2.0

use crate::design::identity::neutral_configuration_id;
use crate::ids::EscapedIdentityComponent;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn identity_singlepass_preserves_literal_unicode_and_ascii_lowercase_bytes() {
    for (source, lowercase, expected) in [
        ("", false, ""),
        ("plain/ABç", false, "plain/ABç"),
        ("plain/ABÇç", true, "plain/abÇç"),
        (":#%", false, "%3A%23%25"),
        (" \t\n\u{a0}\u{2003}", false, "%20%09%0A%C2%A0%E2%80%83"),
        (
            "AZÇç:#%\t\n\u{a0}\u{2003}🧪",
            true,
            "azÇç%3A%23%25%09%0A%C2%A0%E2%80%83🧪",
        ),
    ] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let encoded = ctx.format_retained(format_args!("{}", EscapedIdentityComponent(source, lowercase)), "encode identity control").unwrap();
        assert_eq!(encoded, expected);
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn identity_singlepass_refuses_before_first_write_and_unvisited_suffix() {
    let source = format!("A{}", ":#% \u{2003}".repeat(1024));
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d configuration identifier",
        0,
        |ctx| neutral_configuration_id(ctx, &source, "B"),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "f3d configuration identifier"));
}
