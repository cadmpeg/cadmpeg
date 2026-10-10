// SPDX-License-Identifier: Apache-2.0
//! Decode work-refusal tests for feature completeness.

use cadmpeg_core::decode::ResourceDimension;

#[test]
fn expression_completeness_refuses_dependency_pass_work_limit() {
    let mut ir = super::one_incomplete_expression_parameter();
    // Empty input incurs no parser byte-probe work in the settled OM iterator.
    ir.model.parameters[0].expression.clear();

    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx expression parameter evaluation pass",
        |ctx| super::super::incomplete_expression_parameters(ctx, &ir).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "nx expression parameter evaluation pass"
    ));
}
