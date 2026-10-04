//! Charged component selection text.

use crate::resolved_features::terminations::compact_surface_selection_value;

#[test]
fn charged_surface_component_text_matches_native_text() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let entries = [None, Some(0), Some(u32::MAX)].map(|local_id| {
        crate::records::FeatureInputComponentPathEntry {
            instance: None,
            type_signature: [0; 12],
            local_id,
        }
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    assert_eq!(
        compact_surface_selection_value(&ctx, &entries).expect("charged surface text"),
        "sldprt:feature-input:surface-component-ids:_,0,4294967295"
    );
}

#[test]
fn charged_surface_component_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = compact_surface_selection_value(&ctx, &[])
        .expect_err("surface text exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT surface component selection")
    );
}
