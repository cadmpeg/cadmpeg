use crate::native::attach::preceding_operation_dependency;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId;
use std::collections::BTreeMap;

#[test]
fn nx_construction_dependency_preserves_lookup_refusals() {
    for operation in [
        "NX preceding operation position lookup",
        "NX preceding operation feature lookup",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| policy.limits.max_work_units = cap,
                    |ctx| {
                        let positions = BTreeMap::from([("csys", 1)]);
                        let feature = FeatureId::mint("nx:test:feature#csys").unwrap();
                        let features = BTreeMap::from([("csys", feature)]);
                        let result = preceding_operation_dependency(ctx, "csys", 2, &positions, &features);
                        if let Err(CodecError::ResourceLimit(limit)) = &result {
                            assert_eq!(ctx.resource_refusal(), Some(*limit));
                        }
                        let dependency = result?;
                        assert_eq!(dependency, features.get("csys"));
                        Ok(())
                    },
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}

#[test]
fn hole_package_member_lookup_refusals_propagate() {
    for operation in [
        "NX hole package member output lookup",
        "NX hole package member diameters lookup",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| super::hole_package_result(|policy| policy.limits.max_work_units = cap),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}
