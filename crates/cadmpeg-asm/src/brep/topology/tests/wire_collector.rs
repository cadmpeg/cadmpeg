// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::brep::records::WireMembers;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn wire_member_collector_admits_each_member_without_source_preadmission() {
    let mut wire_tokens = vec![Token::Ref(-1); 8];
    wire_tokens[4] = Token::Ref(2);
    wire_tokens[7] = Token::True;
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, -1, 1]),
        crate::test_support::sab::record(1, "wire".into(), wire_tokens.into(), 0, 0),
        ref_record(2, "coedge", &[-1, -1, -1, 2, -1, -1, 3]),
        ref_record(3, "edge", &[]),
    ];
    let by_index = indexed_records(&records);
    let table = nurbs::toks::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let run = |ctx: &DecodeContext<'_>| {
        let mut storage = ctx.reserve_scoped(0, "test wire scratch")?;
        let mut carriers = Carriers::default();
        let mut reach = Reachable::default();
        let mut out = AsmBrep::default();
        super::super::collect_wire_topology(
            TopologyContext {
                ctx,
                by_index: &by_index,
                token_table: &table,
                purpose: DecodePurpose::Model,
                format: crate::asm_format!("sat"),
            },
            &mut out,
            &records,
            None,
            &mut carriers,
            &mut reach,
            &mut storage,
        )?;
        Ok::<_, CodecError>(out)
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "ASM wire member sources",
        None,
    );
    let out = run(&ctx).unwrap();
    drop(probe);
    assert_eq!(out.wire_topologies.len(), 1);
    let WireMembers::Edges(edges) = &out.wire_topologies[0].members else {
        panic!("wire edges");
    };
    assert_eq!(
        edges
            .iter()
            .map(cadmpeg_ir::ids::EdgeId::as_str)
            .collect::<Vec<_>>(),
        ["sat:brep:entity#3"]
    );
    ctx.finish_session().unwrap();

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM wire member edges",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            run(&ctx)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("wire member refusal");
    };
    assert_eq!(limit.operation, "ASM wire member edges");
    assert_eq!(limit.additional, 1);
}
