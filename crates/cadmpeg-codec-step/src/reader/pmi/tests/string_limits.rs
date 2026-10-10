// SPDX-License-Identifier: Apache-2.0
//! Resource refusals from STEP PMI string fields.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::reader::RecordExt;

fn source(records: &str) -> String {
    format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;")
}

fn pmi_result(records: &str, dimension: ResourceDimension, limit: u64) -> Result<(), CodecError> {
    let source = source(records);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid PMI exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unsupported string storage dimension"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &ctx)?;
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx)?;
    super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx)?;
    Ok(())
}

macro_rules! pmi_string_limit_test {
    ($name:ident, $records:literal, $dimension:ident) => {
        #[test]
        fn $name() {
            let operation = match ResourceDimension::$dimension {
                ResourceDimension::WorkUnits => "STEP decoded string character",
                _ => "step_string_text",
            };
            assert!(matches!(
                Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::$dimension, operation, |cap| pmi_result($records, ResourceDimension::$dimension, cap))),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::$dimension
                        && refusal.operation == operation
            ));
        }
    };
}

pmi_string_limit_test!(
    datum_identification_refuses_retained_limit,
    "#1=DATUM('identifier');",
    RetainedBytes
);
pmi_string_limit_test!(
    datum_name_refuses_retained_limit,
    "#1=(DATUM('') SHAPE_ASPECT('datum name','',#2,.F.));#2=ITEM();",
    RetainedBytes
);
#[test]
fn datum_target_form_refuses_materialized_limit() {
    let source = source("#1=DATUM_TARGET('','rectangle',#2,.F.,'');#2=ITEM();");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("valid datum target exchange");
    let record = exchange.records().get(&1).expect("datum target");
    // The nine-byte form copy is below full-decode setup's materialized peak.
    // Isolate its owner so the limit can refuse this actual scratch boundary.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_string_text",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
                let mut losses = Vec::new();
                let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture")?);
                let mut storage = ctx.reserve_scoped(0, "form fixture")?;
                let parameter = super::super::shape_aspect_parameter(ctx, record, 1)?.expect("target form parameter");
                let form = super::super::super::decode_text_scoped(
                    &exchange, parameter, (&mut losses, &reports), 1,
                    ("datum target form", crate::loss::StepLossCode::MetadataStringInvalid),
                    ctx, &mut storage,
                )?.expect("decoded form");
                assert_eq!(super::super::datum_target_form(&form, ctx)?, cadmpeg_ir::pmi::DatumTargetForm::Rectangle);
                assert!(losses.is_empty());
                Ok(())
            })
        },
    );
    assert!(matches!(Err::<(), CodecError>(error),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::MaterializedBytes
                && refusal.operation == "step_string_text"));
}
pmi_string_limit_test!(
    datum_target_identification_refuses_retained_limit,
    "#1=DATUM_TARGET('','',#2,.F.,'identifier');#2=ITEM();",
    RetainedBytes
);
pmi_string_limit_test!(
    datum_target_name_refuses_retained_limit,
    "#1=DATUM_TARGET('target name','',#2,.F.,'');#2=ITEM();",
    RetainedBytes
);
pmi_string_limit_test!(
    datum_system_name_refuses_retained_limit,
    "#1=DATUM_SYSTEM('system name','',#2,.F.,());#2=ITEM();",
    RetainedBytes
);
pmi_string_limit_test!(
    dimension_name_refuses_retained_limit,
    "#1=DIMENSIONAL_SIZE(#2,'dimension name');#2=ITEM();",
    RetainedBytes
);
pmi_string_limit_test!(
    dimension_category_refuses_retained_limit,
    "#1=DIMENSIONAL_SIZE_WITH_DATUM_FEATURE(#2,'diameter');#2=ITEM();",
    RetainedBytes
);
// These scratch copies run below an earlier materialized peak. Each ASCII
// character write charges one work unit; the fields have 13, 13, 11 and 12 bytes.
pmi_string_limit_test!(
    limits_form_variance_refuses_string_work_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('form variance','','','');",
    WorkUnits
);
pmi_string_limit_test!(
    limits_zone_variance_refuses_string_work_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','zone variance','','');",
    WorkUnits
);
pmi_string_limit_test!(
    limits_grade_refuses_string_work_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','','grade value','');",
    WorkUnits
);
pmi_string_limit_test!(
    limits_source_refuses_string_work_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','','','source value');",
    WorkUnits
);
pmi_string_limit_test!(geometric_tolerance_name_refuses_retained_limit, "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(LENGTH_MEASURE_WITH_UNIT() MEASURE_WITH_UNIT(LENGTH_MEASURE(0.05),#1));#3=ITEM();#4=FLATNESS_TOLERANCE('tol name','',#2,#3);", RetainedBytes
);
pmi_string_limit_test!(
    presentation_annotation_name_refuses_retained_limit,
    "#1=ANNOTATION_TEXT_OCCURRENCE('annotation name',());",
    RetainedBytes
);

#[test]
// Candidate names and text live in scratch storage; the selected output text is charged separately.
fn annotation_text_refuses_materialized_limit() {
    let source = source("#1=TEXT_LITERAL('annotation text',$,'left',.RIGHT.,$);");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid text exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            let mut visited = BTreeSet::new();
            let mut candidates = BTreeMap::new();
            let mut storage = ctx.reserve_scoped(0, "text fixture").expect("scope");
            let mut losses = Vec::<LossNote>::new();
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
            super::super::collect_annotation_text(
                1,
                &exchange,
                &mut visited,
                (&mut candidates, &mut storage),
                (&mut losses, &reports),
                0,
                &ctx,
            )
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::MaterializedBytes
                && refusal.operation == "step_string_text")
    );
}

#[test]
fn measure_item_name_refuses_materialized_limit() {
    let source = source("#1=(MEASURE_REPRESENTATION_ITEM() REPRESENTATION_ITEM('measure name'));");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let record = exchange.records().get(&1).expect("measure record");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("root");
            let mut storage = ctx.reserve_scoped(0, "measure fixture").expect("scope");
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
            super::super::measure_item_name(
                1,
                record,
                &exchange,
                (&mut Vec::new(), &reports),
                (&ctx, &mut storage),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::MaterializedBytes && refusal.operation == "step_string_text")
    );
}

fn annotation_collection_result(limit: u64, consume_text: bool) -> Result<(), CodecError> {
    let source = source("#1=TEXT_LITERAL('annotation text',$,'left',.RIGHT.,$);");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid text exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits collection policy");
    let mut visited = BTreeSet::new();
    let mut storage = ctx.reserve_scoped(0, "text fixture").expect("scope");
    let mut losses = Vec::new();
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    if consume_text {
        super::super::find_annotation_text(
            1,
            &exchange,
            &mut visited,
            (&mut BTreeSet::new(), &mut storage),
            (&mut losses, &reports),
            0,
            &ctx,
        )?;
    } else {
        super::super::collect_annotation_text(
            1,
            &exchange,
            &mut visited,
            (&mut BTreeMap::new(), &mut storage),
            (&mut losses, &reports),
            0,
            &ctx,
        )?;
    }
    Ok(())
}

macro_rules! annotation_collection_limit_test {
    ($name:ident, $limit:expr, $consume:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(
                annotation_collection_result($limit, $consume),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

annotation_collection_limit_test!(
    annotation_text_visited_refuses_collection_limit,
    0,
    false,
    "step_pmi_annotation_text_visited"
);
annotation_collection_limit_test!(
    annotation_text_candidates_refuse_collection_limit,
    1,
    false,
    "step_pmi_annotation_text_candidates"
);
annotation_collection_limit_test!(
    annotation_text_used_refuses_collection_limit,
    2,
    true,
    "step_pmi_annotation_text_used"
);

#[test]
fn characteristic_measure_values_refuse_collection_limit() {
    let source = source("#1=ITEM();");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits collection policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        graph_limit: 64,
        losses: (
            &mut losses,
            &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
        ),
    };
    let value = crate::parse::Value::Real(
        cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite fixture"),
    );
    let mut storage = ctx.reserve_scoped(0, "value fixture").expect("scope");
    assert!(matches!(
        super::super::characteristic_measure_values(&super::super::MeasureParameters::Items(std::slice::from_ref(&value)), &exchange, &mut measurements, &mut storage, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_measure_values"
    ));
}

#[test]
fn characteristic_value_map_refuses_collection_limit() {
    const RECORDS: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);#6=SHAPE_ASPECT('feature','',#5,.T.);#10=DIMENSIONAL_SIZE(#6,'width');#13=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(5.0),#1) REPRESENTATION_ITEM('nominal value'));#14=SHAPE_DIMENSION_REPRESENTATION('value',(#13),#2);#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);#99=ITEM();";
    let source = source(RECORDS);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid characteristic exchange");
    let arena = DecodeArena::new();
    let refused = (0..512).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let Ok(geometry) = crate::reader::geometry::decode(&exchange, &mut ir, &ctx) else {
            return false;
        };
        let mut losses = Vec::new();
        let mut storage = ctx.reserve_scoped(0, "characteristic fixture").expect("scope");
        let refused = matches!(
            super::super::characteristic_values(&exchange, &geometry.value, (&mut losses, &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"))), 64, &mut storage, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_pmi_characteristic_values"
        );
        refused
    });
    assert!(
        refused,
        "no collection limit refused the characteristic map entry"
    );
}

#[test]
fn datum_target_form_direct_caller_preserves_materialized_refusal() {
    let source = source("#1=DATUM_TARGET('','rectangle',#2,.F.,'');#2=ITEM();");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid datum target exchange");
    let record = exchange.records().get(&1).expect("datum target");
    let form = record.parameters().get(1).expect("target form");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    let mut storage = ctx.reserve_scoped(0, "form fixture").expect("scope");
    let mut losses = Vec::new();
    let CodecError::ResourceLimit(refusal) = super::super::super::decode_text_scoped(
        &exchange,
        form,
        (&mut losses, &reports),
        1,
        ("datum target form", crate::loss::StepLossCode::MetadataStringInvalid),
        &ctx,
        &mut storage,
    ).expect_err("decoded form requires materialized storage") else {
        panic!("datum form resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(refusal.operation, "step_string_text");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, u64::try_from("rectangle".len()).expect("form size"));
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    drop(losses);
    drop(storage);
    drop(reports);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

fn pmi_only_result(records: &str, dimension: ResourceDimension, cap: u64) -> Result<cadmpeg_ir::CadIr, CodecError> {
    let source = source(records);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("PMI exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup)?;
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup)?;
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup)?;
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
        _ => panic!("fit storage dimension"),
    }
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx).map(|_| ())
    })?;
    Ok(ir)
}

#[test]
fn discarded_fit_fields_use_and_release_scratch_storage() {
    for field in 0..4 {
        let text = "x".repeat(16_384);
        let mut fields = ["", "", "", ""];
        fields[field] = &text;
        let records = format!("#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('{}','{}','{}','{}');", fields[0], fields[1], fields[2], fields[3]);
        let ir = pmi_only_result(&records, ResourceDimension::RetainedBytes, 0)
            .expect("discarded fit fields do not charge retained storage");
        assert!(ir.model.pmi.is_empty());
        let mut repeated = records.clone();
        for id in 3..35 {
            use std::fmt::Write as _;
            write!(repeated, "#{id}=PLUS_MINUS_TOLERANCE(#2);").expect("repeated candidate");
        }
        let ir = pmi_only_result(&repeated, ResourceDimension::MaterializedBytes, 65_536)
            .expect("live 16 KiB field plus 33 report slots fits; dead candidate fields would add 528 KiB");
        assert!(ir.model.pmi.is_empty());
        assert!(matches!(pmi_only_result(&records, ResourceDimension::MaterializedBytes, 8192),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::MaterializedBytes
                    && refusal.operation == "step_string_text"));
    }
}

#[test]
fn accepted_fit_fields_charge_retained_storage() {
    for field in 0..4 {
        let text = "x".repeat(16_384);
        let mut fields = ["", "", "", ""];
        fields[field] = &text;
        let records = format!("#1=DIMENSIONAL_SIZE(#4,'');#2=LIMITS_AND_FITS('{}','{}','{}','{}');#3=PLUS_MINUS_TOLERANCE(#1,#2);#4=ITEM();", fields[0], fields[1], fields[2], fields[3]);
        assert!(matches!(pmi_only_result(&records, ResourceDimension::RetainedBytes, 8192),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "STEP limits-and-fits candidate scratch"));
    }
}
