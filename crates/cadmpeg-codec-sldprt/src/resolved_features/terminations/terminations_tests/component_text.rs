//! Charged component selection text.

use crate::resolved_features::terminations::{
    compact_surface_selection_value, component_local_ids,
};

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

#[test]
fn component_local_ids_preserves_text_and_zero_cap_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const OPERATION: &str = "test component local IDs";
    let components = [None, Some(0), Some(u32::MAX)].map(|local_id| {
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
        component_local_ids(&ctx, &components, OPERATION).expect("component local IDs"),
        "_,0,4294967295"
    );
    assert_eq!(
        component_local_ids(&ctx, &[], OPERATION).expect("empty component local IDs"),
        ""
    );

    let mut work_policy = DecodePolicy::service();
    work_policy.limits.max_work_units = 0;
    let (work_ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &work_policy).expect("work-limited context");
    let work_error = component_local_ids(&work_ctx, &components, OPERATION)
        .expect_err("component local IDs exceed the work limit");
    assert!(matches!(
        work_error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == OPERATION
    ));

    let mut retained_policy = DecodePolicy::service();
    retained_policy.limits.max_retained_bytes = 0;
    let (retained_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &retained_policy)
        .expect("retained-limited context");
    let retained_error = component_local_ids(&retained_ctx, &components, OPERATION)
        .expect_err("component local IDs exceed the retained limit");
    assert!(matches!(
        retained_error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == OPERATION
    ));
}

#[test]
fn compact_surface_selection_value_propagates_first_component_char_work_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const PREFIX: &str = "sldprt:feature-input:surface-component-ids:";
    const OPERATION: &str = "format SLDPRT surface component selection";
    let components = [None, None].map(|local_id| crate::records::FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id,
    });
    let prefix_work = u64::try_from(PREFIX.len()).expect("prefix length fits u64");
    let visit_work = u64::try_from(components.len()).expect("component count fits u64");
    // The prefix copy and both component visits precede the first character append.
    let work_limit = prefix_work
        .checked_add(visit_work)
        .expect("test work limit fits u64");

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("work-limited context");
    let error = compact_surface_selection_value(&ctx, &components)
        .expect_err("first component character exceeds the work limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == OPERATION
    ));
}
