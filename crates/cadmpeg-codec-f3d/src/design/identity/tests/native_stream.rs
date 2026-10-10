// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const OWNERS: [(&str, &str); 7] = [
    ("feature", "f3d feature identifier"),
    ("parameter", "f3d parameter identifier"),
    ("sketch", "f3d sketch identifier"),
    ("spatial", "f3d spatial sketch identifier"),
    ("constraint", "f3d sketch constraint identifier"),
    ("occurrence", "f3d component insert occurrence identifier"),
    ("joint", "f3d assembly joint identifier"),
];

fn construct(owner: &str, native: &str, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
    let mut scope = super::encoded_scope();
    scope.id = native.to_owned();
    let mut placement = super::encoded_placement();
    placement.id = native.to_owned();
    match owner {
        "feature" => super::super::neutral_feature_id(ctx, &scope).map(|id| id.into_string()),
        "parameter" => {
            let mut parameter = crate::design::decode::parameters::parse_design_parameter_record(
                &crate::design::test_support::parameter_record(
                    Some(40),
                    "1 cm",
                    "AlongDistance",
                    Some("cm"),
                    "d9",
                    1.0,
                ),
            )
            .unwrap();
            parameter.id = native.to_owned();
            super::super::neutral_parameter_id(ctx, &parameter)
                .map(|id| cadmpeg_ir::ids::Identity::from(id).into_string())
        }
        "sketch" => super::super::neutral_sketch_id(ctx, &placement).map(|id| id.into_string()),
        "spatial" => super::super::neutral_spatial_sketch_id(ctx, &placement)
            .map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        "constraint" => super::super::neutral_sketch_constraint_id(ctx, native, u32::MAX)
            .map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        "occurrence" => super::super::neutral_component_insert_occurrence_id(ctx, &scope)
            .map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        "joint" => super::super::neutral_assembly_joint_id(ctx, &scope)
            .map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        _ => panic!("unknown identity test owner"),
    }
}

fn measure(owner: &str, native: &str) -> (String, u64) {
    crate::test_support::with_decode_context(|ctx| {
        let id = construct(owner, native, ctx).unwrap();
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            DecodePolicy::service().limits.max_work_units + 1,
            "measure identity native stream work",
        ) else {
            panic!("finite ceiling exposes executed work");
        };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        (id, usage.used)
    })
}

fn assert_source_refusal(owner: &str, operation: &str) {
    let native = format!("f3d:asset:#%{}", "🧪".repeat(1024));
    for work in [0, 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let Err(CodecError::ResourceLimit(original)) = construct(owner, &native, ctx) else {
                panic!("the next suffix byte must be admitted before source or output");
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, operation);
            assert_eq!((original.used, original.additional), (work, 1));
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(construct(owner, "f3d:test:tail", ctx),
                Err(CodecError::ResourceLimit(later)) if later == original));
        });
    }
}

#[test]
fn identity_native_stream_feature_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[0].0, OWNERS[0].1);
}
#[test]
fn identity_native_stream_parameter_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[1].0, OWNERS[1].1);
}
#[test]
fn identity_native_stream_sketch_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[2].0, OWNERS[2].1);
}
#[test]
fn identity_native_stream_spatial_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[3].0, OWNERS[3].1);
}
#[test]
fn identity_native_stream_constraint_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[4].0, OWNERS[4].1);
}
#[test]
fn identity_native_stream_occurrence_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[5].0, OWNERS[5].1);
}
#[test]
fn identity_native_stream_joint_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[6].0, OWNERS[6].1);
}

#[test]
fn identity_native_stream_arbitrary_suffix_pays_only_its_actual_added_bytes() {
    for (owner, _) in OWNERS {
        let short = measure(owner, "f3d:asset/a %\u{2003}ç:tail");
        for suffix in ["A".repeat(1024), "🧪".repeat(1024)] {
            let long = measure(owner, &format!("f3d:asset/a %\u{2003}ç:tail{suffix}"));
            assert_eq!(long.0, short.0);
            assert_eq!(long.1 - short.1, u64::try_from(suffix.len()).unwrap());
        }
    }
}

#[test]
fn identity_native_stream_unqualified_ids_preserve_default_and_executed_end_probe() {
    for (owner, _) in OWNERS {
        let empty = measure(owner, "");
        for native in ["unqualified".to_owned(), "ç".repeat(1024)] {
            let nonempty = measure(owner, &native);
            assert_eq!(nonempty.0, empty.0);
            assert_eq!(nonempty.1 - empty.1, u64::try_from(native.len()).unwrap());
        }
        assert_eq!(empty.0, measure(owner, "f3d:design:tail").0);
    }
}

#[test]
fn identity_native_stream_last_separator_and_empty_stream_preserve_identity_grammar() {
    for (owner, _) in OWNERS {
        assert_eq!(
            measure(owner, "f3d:asset:path:tail").0,
            measure(owner, "f3d:asset:path:other").0
        );
        assert_ne!(
            measure(owner, "f3d:asset:path:tail").0,
            measure(owner, "f3d:asset:tail").0
        );
        assert_eq!(measure(owner, ":tail").0, measure(owner, ":").0);
        assert_ne!(measure(owner, ":tail").0, measure(owner, "tail").0);
    }
}

#[test]
fn identity_native_stream_original_refusal_precedes_even_empty_source() {
    for (owner, _) in OWNERS {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let Err(CodecError::ResourceLimit(original)) =
                ctx.charge_work(1, "original identity scan refusal")
            else {
                panic!("zero ceiling refuses one executed unit");
            };
            for native in ["", ":", "unqualified", "f3d:test:tail"] {
                assert!(matches!(construct(owner, native, ctx),
                    Err(CodecError::ResourceLimit(later)) if later == original));
            }
            assert_eq!(ctx.resource_refusal(), Some(original));
        });
    }
}
