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
                        let result =
                            preceding_operation_dependency(ctx, "csys", 2, &positions, &features);
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

fn parameter_property_membership_result(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), CodecError> {
    use crate::native::attach::feature_projection::insert_parameter_property;
    use cadmpeg_core::text::NonBlankString;

    let query = "property-name-".repeat(512);
    let key = NonBlankString::from_ascii_leading(query.clone()).unwrap();
    let mut properties = BTreeMap::from([(key, "old".to_string())]);
    crate::test_support::with_decode_context_over(&[], configure, |ctx| {
        let result = insert_parameter_property(
            ctx,
            &mut properties,
            format_args!("{query}"),
            "new".to_string(),
        );
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
            assert_eq!(properties.values().next().map(String::as_str), Some("old"));
        }
        result?;
        assert_eq!(properties.len(), 1);
        let (key, value) = properties.iter().next().unwrap();
        assert_eq!(key.as_str(), query);
        assert_eq!(value, "new");
        Ok(())
    })
}

#[test]
fn parameter_property_membership_preserves_existing_key() {
    parameter_property_membership_result(|_| {}).unwrap();
}

#[test]
fn parameter_property_membership_refuses_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "NX parameter property membership",
        |cap| parameter_property_membership_result(|policy| policy.limits.max_work_units = cap),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
        && limit.operation == "NX parameter property membership"));
}

#[test]
fn hole_output_relation_membership_refusal_propagates() {
    use crate::native::attach::feature_projection::hole_operations_by_body;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::BodyId;

    let operation = "hole-operation-".repeat(512);
    let body = BodyId::mint("test:model:entity#hole-body").unwrap();
    let outputs = BTreeMap::from([(operation.clone(), vec![body.clone()])]);
    let operations = [operation.clone()];
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "NX hole operations by body outputs membership",
        |cap| {
            crate::test_support::with_decode_context_over(
                &[],
                |policy| policy.limits.max_work_units = cap,
                |ctx| {
                    let result =
                        hole_operations_by_body(ctx, &CadIr::empty(), &operations, &outputs);
                    if let Err(CodecError::ResourceLimit(limit)) = &result {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    let groups = result?.unwrap();
                    assert_eq!(
                        groups,
                        BTreeMap::from([(body.clone(), vec![operation.clone()])])
                    );
                    Ok(())
                },
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
        && limit.operation == "NX hole operations by body outputs membership"));
}

#[test]
fn result_group_equality_cost_counts_member_text() {
    use cadmpeg_core::decode::cost::DecodeCost;
    let group = crate::native::attach::FeatureResultGroupMembers {
        faces: vec![cadmpeg_core::text::NonBlankString::try_from("face-μ".to_owned()).unwrap()],
        edges: vec![cadmpeg_core::text::NonBlankString::try_from("edge".to_owned()).unwrap()],
        vertices: Vec::new(),
    };
    // Member text costs seven UTF-8 face bytes and four edge bytes.
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            group
                .decode_cost(ctx, "NX group member equality cost")
                .unwrap(),
            7 + 4
        );
    });
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX group member equality cost",
        |ctx| ctx.equal(&group, &group, "NX group member equality cost"),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "NX group member equality cost")
    );
}
