// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn scalar_equality_component_visits_refuse_before_present_variables() {
    let definition = super::definition(
        &super::equation_body(&[(1, 2, &[0, 1])]),
        vec![super::row(6, 10, Some(2.0)), super::row(6, 11, None)],
    );
    // The equality component contains both Result variables; only the first has a sample.
    let values = crate::test_support::assert_work_boundaries(
        &["creo scalar equality component variables"],
        |ctx| {
            let mut values = BTreeMap::new();
            assert!(super::super::propagate_section_equation_scalar_equality_values(ctx, &definition, &mut values)?);
            Ok(values)
        },
    );
    assert_eq!(values, BTreeMap::from([
        ((super::VariableType::Result, 10), Some(2.0)),
        ((super::VariableType::Result, 11), Some(2.0)),
    ]));
}

#[test]
fn empty_scalar_component_samples_charge_only_the_component_roster() {
    for count in 0..=3 {
        let components: Vec<_> = (0..count).map(|_| BTreeSet::new()).collect();
        let need = u64::try_from(count).expect("fixture count");
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            // Component roster admission is count units. All member and sample sources are empty.
            let result = super::super::scalar_equality_values_for_components(&ctx, &[], &components);
            let refused = result.is_err();
            if refused {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("component roster must refuse"); };
                assert_eq!((original.dimension, original.used, original.additional, original.limit),
                    (ResourceDimension::WorkUnits, 0, need, cap));
                assert_eq!(original.operation, "creo scalar equality value components");
                for _ in 0..2 {
                    assert!(matches!(super::super::scalar_equality_values_for_components(&ctx, &[], &components),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
                Err(original.into())
            } else {
                assert!(result.expect("component roster admitted").is_empty());
                ctx.finish_session().expect("active session");
                Ok(())
            }
        });
    }
}
