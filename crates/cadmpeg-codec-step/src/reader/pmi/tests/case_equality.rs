// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);#6=SHAPE_ASPECT('feature','',#5,.T.);#10=DIMENSIONAL_SIZE(#6,'diameter');#13=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(5.0),#1) REPRESENTATION_ITEM('nominal value'));#14=SHAPE_DIMENSION_REPRESENTATION('value',(#13),#2);#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);#99=ITEM();ENDSEC;END-ISO-10303-21;";

fn case_refuses(operation: &str, characteristic: bool) {
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner).unwrap();
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).unwrap();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).unwrap();
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).unwrap();
        let result = if characteristic {
            super::super::characteristic_values(&exchange, &geometry.value, &mut Vec::new(), 64, &ctx).map(|_| ())
        } else {
            super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir.clone(), &ctx).map(|_| ())
        };
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn characteristic_name_case_equality_preserves_refusal() {
    case_refuses("STEP characteristic name case equality", true);
}

#[test]
fn dimension_category_case_equality_preserves_refusal() {
    case_refuses("STEP dimension category case equality", false);
}

#[test]
fn datum_target_form_case_equality_preserves_refusal() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "STEP datum target form case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::datum_target_form("POINT", &ctx).map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
        }
        result
    });
    let CodecError::ResourceLimit(refusal) = error else { panic!("comparison must preserve its refusal"); };
    assert_eq!(refusal.operation, "STEP datum target form case equality");
}

#[test]
fn datum_target_form_trim_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::datum_target_form(" POINT ", &ctx).unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else { panic!("datum form trim must return the refusal"); };
    assert_eq!(refusal.operation, "STEP datum target form trim");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn targeted_aspect_number_parse_preserves_refusal() {
    case_refuses("STEP PMI targeted aspect number parse", false);
}
