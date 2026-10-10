// SPDX-License-Identifier: Apache-2.0
//! Shared characteristic values preserve selection and diagnostic replay.

use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::pmi::{PmiQuantity, PmiValue};
use cadmpeg_ir::CadIr;

fn exchange(records: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("characteristic exchange")
        .0
}

#[test]
fn characteristic_value_analysis_reuses_shared_wide_representations() {
    for count in [16, 32, 64] {
        let mut records =
            String::from("#1=DIMENSIONAL_SIZE($,'width');#2=SHAPE_DIMENSION_REPRESENTATION('',(");
        for id in 10..10 + count {
            if id != 10 {
                records.push(',');
            }
            write!(records, "#{id}").expect("measure reference");
        }
        records.push_str("),$);");
        for id in 10..10 + count {
            let name = if id == 10 { "nominal value" } else { "" };
            write!(records, "#{id}=MEASURE_REPRESENTATION_ITEM('{name}',1.);")
                .expect("measure item");
        }
        for id in 1000..1000 + count {
            write!(
                records,
                "#{id}=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);"
            )
            .expect("relation");
        }
        let exchange = exchange(&records);
        let setup = cadmpeg_test_support::service_decode_context();
        let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup)
            .expect("geometry");
        let mut policy = DecodePolicy::service();
        // At 64 relations and measures, repeated extraction needs 4096 measure
        // visits. The admitted work bound grows with relations plus measures.
        policy.limits.max_work_units = 4000 * (2 * count) + 50_000;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut losses = Vec::new();
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut storage = ctx.reserve_scoped(0, "test values").expect("scope");
            let values = super::super::characteristic_values(
                &exchange,
                &geometry.value,
                (&mut losses, &reports),
                64,
                &mut storage,
                ctx,
            )
            .expect("shared extraction fits linear work bound");
            assert_eq!(values.len(), 1);
            assert_eq!(
                values[&1],
                PmiValue::new(1.0, PmiQuantity::Ratio).expect("nominal")
            );
            assert!(losses.is_empty());
        });
    }
}

#[test]
fn characteristic_value_analysis_replays_measure_and_name_losses_in_order() {
    let exchange = exchange("#1=DIMENSIONAL_SIZE($,'width');#2=SHAPE_DIMENSION_REPRESENTATION('',(#10),$);#10=MEASURE_REPRESENTATION_ITEM('\\X2\\D83D\\X0\\',LENGTH_MEASURE(1.),$);#100=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);#101=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);");
    let setup = cadmpeg_test_support::service_decode_context();
    let geometry =
        crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut storage = ctx.reserve_scoped(0, "test values").expect("scope");
        let values = super::super::characteristic_values(
            &exchange,
            &geometry.value,
            (&mut losses, &reports),
            64,
            &mut storage,
            ctx,
        )
        .expect("cached diagnostic replay");
        assert_eq!(
            values[&1],
            PmiValue::new(1.0, PmiQuantity::Length).expect("nominal")
        );
        assert_eq!(losses.len(), 4);
        assert_eq!(
            losses[0].code,
            crate::loss::StepLossCode::PmiLengthUnitUnresolved.kind()
        );
        assert_eq!(
            losses[1].code,
            crate::loss::StepLossCode::MetadataStringInvalid.kind()
        );
        assert_eq!(&losses[..2], &losses[2..]);
        // Only replayed report strings survive the analysis cache.
        let report_bytes: usize = losses
            .iter()
            .map(|loss| {
                loss.message.len() + loss.code.namespace().len() + loss.code.local_code().len()
            })
            .sum();
        let cadmpeg_core::CodecError::ResourceLimit(refusal) = ctx
            .charge_retained(u64::MAX, "test characteristic retained observation")
            .expect_err("observation exceeds retained limit")
        else {
            panic!("retained refusal");
        };
        assert_eq!(
            refusal.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );
        assert_eq!(
            refusal.used,
            u64::try_from(report_bytes).expect("report size")
        );
    });
}

#[test]
fn characteristic_value_analysis_keeps_each_relation_ambiguity() {
    for name in ["", "nominal value"] {
        let exchange = exchange(&format!("#1=DIMENSIONAL_SIZE($,'width');#2=SHAPE_DIMENSION_REPRESENTATION('',(#10,#11),$);#10=MEASURE_REPRESENTATION_ITEM('{name}',1.);#11=MEASURE_REPRESENTATION_ITEM('{name}',2.);#100=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);#101=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);"));
        let setup = cadmpeg_test_support::service_decode_context();
        let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup)
            .expect("geometry");
        crate::test_support::with_service_context(b"", |_, ctx| {
            let mut losses = Vec::new();
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut storage = ctx.reserve_scoped(0, "test values").expect("scope");
            let values = super::super::characteristic_values(
                &exchange,
                &geometry.value,
                (&mut losses, &reports),
                64,
                &mut storage,
                ctx,
            )
            .expect("ambiguity replay");
            assert!(values.is_empty());
            assert_eq!(losses.len(), 2);
            let (code, measures) = if name.is_empty() {
                (
                    crate::loss::StepLossCode::DimensionalUnnamedMeasureAmbiguous,
                    "unnamed measure values",
                )
            } else {
                (
                    crate::loss::StepLossCode::DimensionalNominalAmbiguous,
                    "nominal value measures",
                )
            };
            for (loss, id) in losses.iter().zip([100, 101]) {
                assert_eq!(loss.code, code.kind());
                assert_eq!(loss.message, format!("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has 2 {measures}; the nominal is ambiguous"));
            }
        });
    }
}
