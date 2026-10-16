// SPDX-License-Identifier: Apache-2.0
//! Product occurrence expansion stops at its local limits and reports a loss.

use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;

use crate::loss::StepLossCode;

fn occurrence_output_truncates(existing: usize) {
    crate::test_support::with_service_context(super::PRODUCT_STRING_LIMIT_SOURCE, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("valid product source");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, ctx).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir);
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).unwrap();
        let mut admitted = 0;
        super::super::decode(
            &exchange,
            &geometry.value,
            &topology.value,
            &mut ir,
            ctx,
            &mut admitted,
        )
        .expect("initial assembly fits");
        ir.model
            .occurrences
            .resize(existing, ir.model.occurrences[0].clone());
        let model_input =
            serde_json::to_vec(&ir).expect("synthetic existing assembly budget envelope");
        crate::test_support::with_service_context(&model_input, |_, ctx| {
            let outcome = super::super::decode(
                &exchange,
                &geometry.value,
                &topology.value,
                &mut ir,
                ctx,
                &mut admitted,
            )
            .expect("the occurrence limit truncates instead of refusing");
            assert!(outcome.losses.iter().any(|loss| {
                loss.code == StepLossCode::OccurrenceExpansionOutputTruncated.kind()
                    && loss.message.contains("100000 occurrences")
            }));
            assert_eq!(ctx.resource_refusal(), None);
        });
        assert_eq!(ir.model.occurrences.len(), 100_000);
    });
}

#[test]
fn root_occurrence_output_limit_truncates_with_a_loss() {
    occurrence_output_truncates(100_000);
}

#[test]
fn child_occurrence_output_limit_truncates_with_a_loss() {
    occurrence_output_truncates(99_999);
}

/// A chain of `levels` products, each the single child of the previous one.
fn assembly_chain(levels: u64) -> Vec<u8> {
    let mut data = String::from(
        "#1=APPLICATION_CONTEXT('mechanical design');#2=PRODUCT_CONTEXT('',#1,'mechanical');#3=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');",
    );
    for level in 0..levels {
        let base = 10 + level * 10;
        write!(
            data,
            "#{base}=PRODUCT('P{level}','Part {level}','',(#2));#{}=PRODUCT_DEFINITION_FORMATION('','',#{base});#{}=PRODUCT_DEFINITION('part','',#{},#3);",
            base + 1,
            base + 2,
            base + 1,
        )
        .expect("write product fixture");
        if level > 0 {
            write!(
                data,
                "#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u{level}','','',#{},#{},$);",
                base + 3,
                base - 8,
                base + 2,
            )
            .expect("write usage fixture");
        }
    }
    format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;").into_bytes()
}

#[test]
fn occurrence_paths_stop_at_the_session_depth_with_a_loss() {
    let source = assembly_chain(6);
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 4;
    crate::test_support::with_policy_context(&source, &policy, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("valid product source");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, ctx).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir);
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).unwrap();
        let mut admitted = 0;
        let outcome = super::super::decode(
            &exchange,
            &geometry.value,
            &topology.value,
            &mut ir,
            ctx,
            &mut admitted,
        )
        .expect("a deep assembly truncates instead of refusing");
        assert_eq!(ir.model.occurrences.len(), 4);
        assert!(outcome.losses.iter().any(|loss| {
            loss.code == StepLossCode::OccurrenceExpansionDepthTruncated.kind()
                && loss.message.contains("1 usage(s) from NAUO #53")
        }));
        assert_eq!(ctx.resource_refusal(), None);
    });
}
