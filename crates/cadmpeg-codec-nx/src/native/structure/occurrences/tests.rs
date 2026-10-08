// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::native::NativeNamespace;
use serde_json::{json, Value};

use super::{FastLoadComponentOccurrenceWire, FastLoadOccurrences};

fn validate_native(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|ctx| {
        crate::NxCodec::validate_native(ctx, ir).expect("validation fits service policy")
    })
}

fn rows(form: u8) -> Value {
    json!([
        {"id": "nx:fast-load:occurrence#0", "ordinal": 0, "occurrence_lane_form": form,
         "marker": 49, "marker_source_offset": 12, "prototype": "nx:fast-load:prototype#0",
         "prototype_index": 1, "component_uuid": "nx:fast-load:uuid#0", "uuid_source_offset": 20,
         "source_entry": "/Root/FastLoad/Structure", "source_offset": 16},
        {"id": "nx:fast-load:occurrence#1", "ordinal": 1, "occurrence_lane_form": form,
         "marker": 57, "marker_source_offset": 13, "prototype": "nx:fast-load:prototype#0",
         "prototype_index": 1, "component_uuid": "nx:fast-load:uuid#0", "uuid_source_offset": 21,
         "source_entry": "/Root/FastLoad/Structure", "source_offset": 17}
    ])
}

fn invalid_second_prototype_rows() -> Value {
    let mut values = rows(0).as_array().unwrap().clone();
    values[1]["prototype"] = json!("nx:fast-load:prototype#missing");
    let suffix = values[1].clone();
    for ordinal in 2..128 {
        let mut record = suffix.clone();
        record["id"] = json!(format!("nx:fast-load:occurrence#{ordinal}"));
        record["ordinal"] = json!(ordinal);
        values.push(record);
    }
    json!(values)
}

#[test]
fn roster_hoists_each_legal_form_and_preserves_row_json() {
    for wire in [json!([]), rows(0), rows(1)] {
        let occurrences: FastLoadOccurrences = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&occurrences).unwrap(), wire);
        let namespace_wire = json!({"fast_load_component_occurrences": wire});
        let namespace: NativeNamespace = serde_json::from_value(namespace_wire.clone()).unwrap();
        assert_eq!(
            namespace.admit::<FastLoadOccurrences>().unwrap(),
            occurrences
        );
        assert_eq!(serde_json::to_value(namespace).unwrap(), namespace_wire);
    }
}

#[test]
fn roster_rejects_disagreeing_locally_valid_lane_forms_before_hoisting() {
    for first in [0, 1] {
        let mut wire = rows(first);
        wire[1]["occurrence_lane_form"] = json!(1 - first);
        let records: Vec<FastLoadComponentOccurrenceWire> =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(FastLoadOccurrences::try_from(records)
            .unwrap_err()
            .to_string()
            .contains("occurrence_lane_form"));
        assert!(serde_json::from_value::<FastLoadOccurrences>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("occurrence_lane_form"));
        let namespace: NativeNamespace =
            serde_json::from_value(json!({"fast_load_component_occurrences": wire})).unwrap();
        assert!(namespace
            .admit::<FastLoadOccurrences>()
            .unwrap_err()
            .to_string()
            .contains("occurrence_lane_form"));
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native.0.insert("nx".into(), namespace);
        let findings = validate_native(&ir);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("occurrence_lane_form"));
    }
}

#[test]
fn native_occurrence_admission_preserves_caller_resource_refusals() {
    let namespace: NativeNamespace =
        serde_json::from_value(json!({"fast_load_component_occurrences": rows(0)})).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.0.insert("nx".into(), namespace);
    for dimension in [
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        // Validation loads the records as temporary storage, so it charges materialized bytes.
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
    ] {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| match dimension {
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = 0;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = 0;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = 0;
                }
                _ => unreachable!(),
            },
            |ctx| {
                let error = crate::NxCodec::validate_native(ctx, &ir).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == dimension && Some(limit) == ctx.resource_refusal())
                );
            },
        );
    }
}

#[test]
fn roster_conversion_admits_each_record_before_converting_it() {
    let wire_value = invalid_second_prototype_rows();
    let row_count = wire_value.as_array().unwrap().len();
    let namespace: NativeNamespace =
        serde_json::from_value(json!({"fast_load_component_occurrences": wire_value})).unwrap();
    let invalid =
        "FastLoadComponentOccurrence.prototype disagrees with prototype_index";
    let decode_at_work = |max_work_units| {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = max_work_units,
            |ctx| FastLoadOccurrences::from_namespace_with_context(ctx, &namespace),
        )
    };

    // The namespace read and full-roster lane check have data-dependent costs.
    // Find the first work limit that reaches the unchanged second-row error.
    let mut lower = 0;
    let mut upper = u64::MAX;
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        match decode_at_work(middle) {
            Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(message))
                if message == invalid =>
            {
                upper = middle;
            }
            Err(cadmpeg_ir::native::NativeConvertError::Resource(
                cadmpeg_core::CodecError::ResourceLimit(limit),
            )) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                lower = middle + 1;
            }
            result => panic!("unexpected occurrence conversion result: {result:?}"),
        }
    }
    let semantic_work = upper;
    let message_work = u64::try_from(invalid.len()).expect("fixed error length fits u64");
    let before_second_record_work = message_work
        .checked_add(1)
        .and_then(|tail| semantic_work.checked_sub(tail))
        .expect("prefix conversion work fits");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = before_second_record_work,
        |ctx| {
            let error = FastLoadOccurrences::from_namespace_with_context(ctx, &namespace)
                .unwrap_err();
            assert!(matches!(
                error,
                cadmpeg_ir::native::NativeConvertError::Resource(
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                ) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "admit NX occurrence roster"
                    && limit.additional == 1
            ));
            assert!(ctx.resource_refusal().is_some());
        },
    );

    let result = decode_at_work(semantic_work);
    assert!(matches!(
        result,
        Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(message))
            if message == invalid
    ));
    assert_eq!(row_count, 128);
}
