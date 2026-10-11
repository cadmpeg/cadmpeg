// SPDX-License-Identifier: Apache-2.0
//! Pcurve witnesses reused after independent roots commit shared source faces.

use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const CARRIERS_PER_ROOT: u64 = 2048;

fn shared_selection_work(count: u64) -> u64 {
    let fixture = include_str!("data/pc04_shared_pcurve.p21");
    let (header, _) = fixture.split_once("DATA;\n").expect("fixture DATA section");
    let mut source = format!("{header}DATA;\n");
    for record in fixture.lines().filter(|line| line.starts_with('#')) {
        if ["#57=", "#58=", "#59="].iter().any(|prefix| record.starts_with(prefix)) {
            continue;
        }
        writeln!(source, "{record}").expect("append shared geometry");
    }
    for root in 0..count {
        let shell = 100_000 + root;
        let body = 200_000 + root;
        writeln!(source, "#{shell}=OPEN_SHELL('',(#56));\n#{body}=SHELL_BASED_SURFACE_MODEL('',(#{shell}));")
            .expect("append independent topology root");
        for carrier in 0..CARRIERS_PER_ROOT {
            let point = 1_000_000 + root * CARRIERS_PER_ROOT + carrier;
            let vertex = 2_000_000 + root * CARRIERS_PER_ROOT + carrier;
            writeln!(source, "#{point}=CARTESIAN_POINT('',(0.,0.,0.));\n#{vertex}=VERTEX_POINT('',#{point});")
                .expect("append unrelated carrier");
        }
    }
    source.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_work_units = 16_000_000_000;
    policy.limits.max_collection_items = 128_000_000;
    let (exchange, _) = crate::test_support::with_policy_context(source.as_bytes(), &policy, crate::parse::parse_inner)
        .expect("shared pcurve roots parse");
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(source.as_bytes(), &setup_arena, &policy)
        .expect("carrier setup fits policy");
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("shared geometry");
    assert_eq!(ir.model.points.len(), usize::try_from(count * CARRIERS_PER_ROOT + 6).expect("point count"));
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("predecoded carriers");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("source fits policy");
    let outcome = super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("shared roots commit");
    let roots = usize::try_from(count).expect("root count");
    assert_eq!(ir.model.bodies.len(), roots);
    assert_eq!(ir.model.faces.len(), roots);
    assert_eq!(ir.model.coedges.iter().filter(|coedge| !coedge.pcurves.is_empty()).count(), roots);
    assert!(ir.model.coedges.iter().flat_map(|coedge| &coedge.pcurves)
        .all(|use_| use_.pcurve.as_str() == "step:data:pcurve#33"));
    assert_eq!(outcome.losses.len(), roots + 1);
    assert_eq!(outcome.losses.iter().filter(|loss| loss.code
        == crate::loss::StepLossCode::PcurveEndpointsDiscontinuous.kind()
        && loss.message == "curve #35 has one optional pcurve on surface #26 whose mapped endpoints are not continuous with the edge vertices; the pcurve is omitted").count(), roots);
    assert!(outcome.losses.iter().any(|loss| loss.code
        == crate::loss::StepLossCode::PcurveGlobalFidelityUnproved.kind()
        && loss.message.starts_with(&format!("a finite endpoint and locus witness admits {count} pcurve relation(s);"))));
    drop(outcome);
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "shared selection work probe")
        .expect_err("work counter") else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn topology_roots_reuse_completed_pcurve_witnesses_after_commits() {
    let small = shared_selection_work(64);
    let large = shared_selection_work(128);
    eprintln!("shared selection work: 64 roots = {small}, 128 roots = {large}");
    assert!(large < 3 * small, "shared pcurve selection work grew from {small} to {large}");
}
