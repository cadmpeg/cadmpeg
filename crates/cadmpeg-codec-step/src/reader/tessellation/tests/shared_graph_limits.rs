// SPDX-License-Identifier: Apache-2.0
//! Shared graph visits, placement expansion, and sticky refusals.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::super::{
    AssociationMemo, AssociationMode, AssociationReservations, TessellationItemAssociator,
};

fn diamond(layers: u32, cycle: bool, repositioned: bool) -> String {
    let mut records = String::from(super::ONE_TRIANGLE);
    records.push_str("#3=TESSELLATED_GEOMETRIC_SET('',(#10,#11));");
    for layer in 0..layers {
        let left = 10 + 2 * layer;
        let children = if layer + 1 == layers {
            if cycle {
                String::from("#2,#3")
            } else {
                String::from("#2")
            }
        } else {
            format!("#{},#{}", left + 2, left + 3)
        };
        for branch in 0..2 {
            let id = left + branch;
            if repositioned {
                let point = 100 + id;
                let axis = 200 + id;
                let translation = branch * (1 << layer);
                writeln!(records, "#{point}=CARTESIAN_POINT('',({translation}.,0.,0.));\n#{axis}=AXIS2_PLACEMENT_3D('',#{point},$,$);\n#{id}=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#{axis}) REPRESENTATION_ITEM('') TESSELLATED_GEOMETRIC_SET(({children})) TESSELLATED_ITEM());").unwrap();
            } else {
                writeln!(records, "#{id}=TESSELLATED_GEOMETRIC_SET('',({children}));").unwrap();
            }
        }
    }
    records
}

fn association_refuses(records: &str, distinct_contexts: bool) {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap();
    crate::test_support::with_service_context(&[], |_, setup| {
        let geometry =
            crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), setup).unwrap();
        // Begin well inside the walk, then admit only the failed boundary until
        // the next node visit refuses. No assumption about keyed-work prices.
        let mut cap = 50_000;
        loop {
            assert!(
                cap < 100_000,
                "the graph walk must refuse at a modest budget"
            );
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = (|| {
                let mut memo = AssociationMemo::new(&ctx)?;
                let mut reservations = AssociationReservations::new(&ctx)?;
                let mut claims = ctx.reserve_scoped(0, "association claims")?;
                let mut item_bodies = BTreeMap::new();
                let mut declared = BTreeSet::new();
                let mut unresolved_containers = BTreeSet::new();
                let mut typed = BTreeSet::new();
                let mut placements = BTreeMap::new();
                let mut unresolved_placements = BTreeSet::new();
                let mut body_context = BTreeSet::new();
                for context in 0..if distinct_contexts { 128 } else { 1 } {
                    let body =
                        cadmpeg_ir::ids::BodyId::mint(format!("step:data:body#{context}")).unwrap();
                    let bodies = if distinct_contexts {
                        vec![&body]
                    } else {
                        Vec::new()
                    };
                    let context = memo.context(&bodies, &ctx)?;
                    let mut associator = TessellationItemAssociator {
                        bodies: &bodies,
                        exchange: &exchange,
                        item_bodies: &mut item_bodies,
                        declared_items: &mut declared,
                        unresolved_containers: &mut unresolved_containers,
                        typed: &mut typed,
                        claim_storage: &mut claims,
                        geometry: &geometry.value,
                        placements: &mut placements,
                        unresolved_placements: &mut unresolved_placements,
                        body_context_items: &mut body_context,
                        mode: AssociationMode::BodyItems,
                        active: HashSet::new(),
                        context,
                        memo: &mut memo,
                        cycle_count: 0,
                        reservations: &mut reservations,
                        ctx: &ctx,
                        active_storage: ctx.reserve_scoped(0, "association active")?,
                    };
                    associator.visit(3, None)?;
                }
                Ok::<_, CodecError>(())
            })();
            let CodecError::ResourceLimit(limit) =
                result.expect_err("expanded walk exceeds the cap")
            else {
                panic!("work refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
            if limit.operation == "STEP tessellation association visit" {
                assert_eq!(limit.additional, 1);
                break;
            }
            cap = limit.used + limit.additional;
        }
    });
}

#[test]
fn cyclic_tessellation_diamond_refuses_at_an_actual_visit() {
    association_refuses(&diamond(8, true, false), false);
}

#[test]
fn repositioned_tessellation_diamond_refuses_at_an_actual_visit() {
    association_refuses(&diamond(8, false, true), false);
}

#[test]
fn distinct_body_context_diamond_refuses_at_an_actual_visit() {
    association_refuses(&diamond(8, false, false), true);
}

#[test]
fn repositioned_tessellation_diamond_keeps_the_exact_distinct_count() {
    for layers in [2, 4] {
        let records = format!(
            "{}#500=TESSELLATED_ANNOTATION_OCCURRENCE('',(),#3);",
            diamond(layers, false, true)
        );
        let decoded = crate::test_support::exchange::decode_inline(&records);
        assert_eq!(decoded.ir().model.tessellations.len(), 1);
        assert_eq!(decoded.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
        let messages = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::StepLossCode::TessellationPlacementAmbiguous.kind()
            })
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>();
        let expected = format!("tessellation item #2 has {} distinct repositioning placements; mesh retained in source coordinates", 1_u32 << layers);
        assert_eq!(messages, [expected.as_str()]);
        super::assert_point3_close(
            decoded.ir().model.tessellations[0].vertices()[0].get(),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        );
    }
}

#[test]
fn memoized_tessellation_diamond_does_not_bill_the_expanded_paths() {
    let records = format!(
        "{}#500=TESSELLATED_ANNOTATION_OCCURRENCE('',(),#3);",
        diamond(24, false, false)
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 500_000;
    let ir =
        super::decode_tessellation_under_policy(&records, policy).expect("shared completions fit");
    assert_eq!(ir.model.tessellations.len(), 1);
    assert_eq!(ir.model.tessellations[0].triangles(), [[0, 1, 2]]);
}

#[test]
fn product_relationship_diamond_keeps_visited_work_and_original_refusal() {
    let mut records = String::from("#1=PRODUCT_DEFINITION_SHAPE('','',$);#2=SHAPE_DEFINITION_REPRESENTATION(#1,#3);#3=SHAPE_REPRESENTATION('',(),$);#4=SHAPE_REPRESENTATION('',(),$);");
    let mut relationship = 1000;
    for (left, right) in [(3, 10), (3, 11)] {
        writeln!(
            records,
            "#{relationship}=SHAPE_REPRESENTATION_RELATIONSHIP('','',#{left},#{right});"
        )
        .unwrap();
        relationship += 1;
    }
    for layer in 0..24 {
        let left = 10 + 2 * layer;
        let children = if layer == 23 {
            vec![4]
        } else {
            vec![left + 2, left + 3]
        };
        for id in [left, left + 1] {
            writeln!(records, "#{id}=SHAPE_REPRESENTATION('',(),$);").unwrap();
            for child in &children {
                writeln!(
                    records,
                    "#{relationship}=SHAPE_REPRESENTATION_RELATIONSHIP('','',#{id},#{child});"
                )
                .unwrap();
                relationship += 1;
            }
        }
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap();
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::product_linked_representations(&exchange, &ctx)
            .map(|(linked, _storage)| linked);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit)
            );
        } else {
            ctx.finish_session().unwrap();
        }
        result
    };
    let expected = [3, 4].into_iter().chain(10..58).collect::<BTreeSet<_>>();
    assert_eq!(
        run(500_000).expect("visited relationship walk fits"),
        expected
    );
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP product relationship walk",
        run,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "STEP product relationship walk" && limit.limit < 500_000)
    );
}
