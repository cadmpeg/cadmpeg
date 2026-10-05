use crate::history::combine_external_local_tools;

fn refusal(operation: &'static str) -> cadmpeg_core::CodecError {
    let scope = super::combine_external_scope();
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| combine_external_local_tools(ctx, &scope),
    )
}

#[test]
fn combine_external_tool_body_scan_refuses_work_limit() {
    let operation = "scan F3D Combine external tool identities";
    assert!(matches!(
        refusal(operation),
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn combine_external_tool_identity_comparison_refuses_work_limit() {
    let operation = "compare F3D Combine external tool identities";
    assert!(matches!(
        refusal(operation),
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
