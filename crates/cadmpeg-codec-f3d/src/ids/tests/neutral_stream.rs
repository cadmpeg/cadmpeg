// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const OWNERS: [(&str, &str); 4] = [
    ("parameter", "resolve F3D neutral parameter stream"),
    ("sketch", "resolve F3D neutral sketch stream"),
    ("spatial", "resolve F3D neutral spatial sketch stream"),
    ("constraint", "resolve F3D neutral sketch constraint stream"),
];

fn construct(owner: &str, native: &str, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
    let placement = crate::records::sketch_placement::DesignSketchPlacement {
        id: native.to_owned(), scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 7),
        visibility: None, class_tag: "330".to_owned().try_into().unwrap(),
        record_index: 4, paired_class_tag: "330".to_owned().try_into().unwrap(),
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0, crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        ).unwrap(),
    };
    match owner {
        "parameter" => {
            use crate::records::parameters::{DesignParameter, DesignParameterDraft, DesignParameterSource};
            let parameter = DesignParameter::try_from(DesignParameterDraft::<String> {
                id: native.to_owned(), byte_offset: 0,
                class_tag: "305".to_owned().try_into().unwrap(), record_index: 0,
                source_ordinal: 0,
                source: DesignParameterSource::new::<String>("AlongDistance".into(), Some(40), None).unwrap(),
                expression: "1 cm".into(), expression_offset: 40, source_kind_offset: 60,
                unit: Some(crate::records::identity::RecordedValue { value: "cm".into(), offset: 90 }),
                name: "d9".into(), name_offset: 100,
                evaluated_value: 1.0, evaluated_value_offset: 110,
            }).unwrap();
            super::super::neutral_parameter_id_charged(ctx, &parameter).map(|id| cadmpeg_ir::ids::Identity::from(id).into_string())
        }
        "sketch" => super::super::neutral_sketch_id_charged(ctx, &placement).map(|id| id.into_string()),
        "spatial" => super::super::neutral_spatial_sketch_id_charged(ctx, &placement).map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        "constraint" => super::super::neutral_sketch_constraint_id_charged(ctx, native, 4).map(|id| cadmpeg_ir::ids::Identity::from(id).into_string()),
        _ => panic!("unknown container identity test owner"),
    }
}

fn assert_source_refusal(owner: &str, operation: &str) {
    let native = format!("f3d:test:tail{}", "ç".repeat(1024));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = construct(owner, &native, ctx)
        else { panic!("actual suffix step precedes output allocation"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, operation);
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(construct(owner, "", ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn container_identity_native_stream_parameter_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[0].0, OWNERS[0].1);
}
#[test]
fn container_identity_native_stream_sketch_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[1].0, OWNERS[1].1);
}
#[test]
fn container_identity_native_stream_spatial_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[2].0, OWNERS[2].1);
}
#[test]
fn container_identity_native_stream_constraint_refuses_before_suffix_visit() {
    assert_source_refusal(OWNERS[3].0, OWNERS[3].1);
}

fn measure(owner: &str, native: &str) -> (String, u64) {
    crate::test_support::with_decode_context(|ctx| {
        let value = construct(owner, native, ctx).unwrap();
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            DecodePolicy::service().limits.max_work_units + 1, "measure container native stream work",
        ) else { panic!("finite ceiling exposes actual work"); };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        (value, usage.used)
    })
}

#[test]
fn container_identity_native_stream_arbitrary_suffix_keeps_exact_output_and_added_byte_work() {
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
fn container_identity_native_stream_missing_and_empty_stream_keep_distinct_native_meaning() {
    for (owner, _) in OWNERS {
        let empty = measure(owner, "");
        let missing = measure(owner, "unqualified");
        assert_eq!(missing.0, empty.0);
        assert_eq!(missing.1 - empty.1, u64::try_from("unqualified".len()).unwrap());
        assert_eq!(empty.0, measure(owner, "f3d:design:tail").0);
        assert_ne!(measure(owner, ":tail").0, empty.0);
        assert_eq!(measure(owner, ":tail").0, measure(owner, ":").0);
        assert_ne!(measure(owner, "f3d:test:path:tail").0, measure(owner, "f3d:test:tail").0);
    }
}
