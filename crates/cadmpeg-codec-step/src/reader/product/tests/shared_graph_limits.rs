// SPDX-License-Identifier: Apache-2.0
//! Path-proportional assembly output and per-visit admission.

use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::codec::Codec;

fn diamond(layers: u64, cycle: bool) -> String {
    let mut records = String::new();
    for node in 0..=2 * layers + 1 {
        let product = 10 + 3 * node;
        writeln!(records, "#{product}=PRODUCT('p{node}','','',());\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{product});\n#{}=PRODUCT_DEFINITION('','',#{},$);", product + 1, 100 + node, product + 1).unwrap();
    }
    let mut usage = 1000;
    for node in 0..=2 * layers {
        let children = if node == 0 {
            vec![1, 2]
        } else if node > 2 * (layers - 1) {
            vec![2 * layers + 1]
        } else {
            let next = 1 + 2 * node.div_ceil(2);
            vec![next, next + 1]
        };
        for child in children {
            writeln!(records, "#{usage}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);", 100 + node, 100 + child).unwrap();
            usage += 1;
        }
    }
    if cycle {
        // Keep one resolvable root while every sink path cuts this self-edge.
        writeln!(records, "#{usage}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{0},#{0},$);", 101 + 2 * layers).unwrap();
    }
    format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;")
}

#[test]
fn assembly_diamond_refuses_at_an_actual_child_visit() {
    let source = diamond(8, false);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).unwrap();
    crate::test_support::with_service_context(&[], |_, setup| {
        let mut ir = CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, setup).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, setup).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, setup).unwrap();
        let mut cap = 1_000_000;
        loop {
            assert!(cap < 2_000_000, "path expansion refuses at a modest budget");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let result = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx, &mut 0);
            let CodecError::ResourceLimit(limit) = result.err().expect("expanded occurrences exceed the cap") else {
                panic!("work refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
            if limit.operation == "STEP child usage traversal" {
                assert!(ir.model.occurrences.len() > 18, "the walk has expanded shared definitions");
                assert_eq!(limit.additional, 1);
                break;
            }
            cap = limit.used + limit.additional;
        }
    });
}

#[test]
fn assembly_diamond_preserves_occurrences_and_cycle_warning_order() {
    let source = diamond(2, true);
    let decoded = crate::StepCodec::default().decode(&mut std::io::Cursor::new(source.as_bytes()), &cadmpeg_ir::codec::DecodeOptions::default()).unwrap();
    assert_eq!(decoded.ir().model.occurrences.len(), 11);
    let messages = decoded.report().losses.iter().filter(|loss| loss.code == crate::loss::StepLossCode::NauoPlacementUnresolved.kind() || loss.code == crate::loss::StepLossCode::DecodeWarning.kind()).map(|loss| loss.message.as_str()).collect::<Vec<_>>();
    let mut expected = (1000..1008).map(|id| format!("NAUO #{id} has no resolved occurrence transform; identity placement was used")).collect::<Vec<_>>();
    expected.extend(std::iter::repeat_n(String::from("NAUO #1008 closes an assembly definition cycle"), 4));
    assert_eq!(messages, expected);
    let ids = decoded.ir().model.occurrences.iter().map(|occurrence| occurrence.id.as_str()).collect::<Vec<_>>();
    // CadIr::finalize establishes canonical identity order at the codec boundary.
    let mut expected_ids = [
        "step:product:occurrence#definition-100",
        "step:product:occurrence#1000", "step:product:occurrence#1001",
        "step:product:occurrence#1002", "step:product:occurrence#1003",
        "step:product:occurrence#1004", "step:product:occurrence#1005",
        "step:product:occurrence#1006", "step:product:occurrence#1007",
        "step:product:occurrence#1006-instance-2", "step:product:occurrence#1007-instance-2",
    ];
    expected_ids.sort_unstable();
    assert_eq!(ids, expected_ids);
}

#[test]
fn occurrence_entity_refusal_precedes_insertion() {
    let source = diamond(2, false);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).unwrap();
    crate::test_support::with_service_context(&[], |_, setup| {
        let mut ir = CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, setup).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, setup).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, setup).unwrap();
        for roots in [0, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_entities = 6 + roots;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let result = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx, &mut 0);
            let CodecError::ResourceLimit(limit) = result.err().expect("next occurrence refuses") else { panic!("entity refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::Entities);
            assert_eq!(limit.operation, "step_assembly_occurrence");
            assert_eq!(ir.model.occurrences.len(), usize::try_from(roots).unwrap());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    });
}

#[test]
fn drawing_ownership_diamond_keeps_visited_work_and_original_refusal() {
    let mut records = String::from("#1=DRAUGHTING_MODEL('',(#10,#11),$);#2=MAPPED_ITEM();");
    for layer in 0..24 {
        let left = 10 + 2 * layer;
        let children = if layer == 23 { String::from("#2") } else { format!("#{},#{}", left + 2, left + 3) };
        for id in [left, left + 1] {
            writeln!(records, "#{id}=TESSELLATED_GEOMETRIC_SET('',({children}));").unwrap();
        }
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).unwrap();
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::drawing_owned_items(&exchange, &ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit));
        } else {
            ctx.finish_session().unwrap();
        }
        result
    };
    assert_eq!(run(500_000).expect("visited drawing walk fits"), [2].into());
    let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "STEP product worklist step", run);
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "STEP product worklist step" && limit.limit < 500_000));
}

#[test]
fn product_definition_entity_refusal_precedes_insertion() {
    let source = diamond(2, false);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).unwrap();
    crate::test_support::with_service_context(&[], |_, setup| {
        let mut ir = CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, setup).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, setup).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, setup).unwrap();
        for prototypes in [0, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_entities = prototypes;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let result = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx, &mut 0);
            let CodecError::ResourceLimit(limit) = result.err().expect("next product definition refuses") else { panic!("entity refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::Entities);
            assert_eq!(limit.operation, "step_product_definition");
            assert_eq!(ir.model.product_definitions.len(), usize::try_from(prototypes).unwrap());
            assert!(ir.model.occurrences.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    });
}
