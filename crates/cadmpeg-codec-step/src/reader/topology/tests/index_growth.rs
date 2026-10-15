// SPDX-License-Identifier: Apache-2.0
//! Growth requirement for pcurve lookups shared across topology roots.

use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const CARRIERS_PER_ROOT: u64 = 2048;

fn root_source(count: u64) -> String {
    let fixture = include_str!("data/pc04_shared_pcurve.p21");
    let (header, _) = fixture.split_once("DATA;\n").expect("fixture DATA section");
    let mut source = format!("{header}DATA;\n");
    for root in 0..count {
        let offset = root * 100;
        for record in fixture.lines().filter(|line| line.starts_with('#')) {
            let mut chars = record.chars().peekable();
            while let Some(character) = chars.next() {
                source.push(character);
                if character == '#' {
                    let mut number = String::new();
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        number.push(chars.next().expect("peeked source digit"));
                    }
                    let id = number.parse::<u64>().expect("source record number");
                    write!(source, "{}", id + offset).expect("write source record number");
                }
            }
            source.push('\n');
        }
        // Keep carrier-index work visible beside the constant geometric witness work per root.
        for carrier in 0..CARRIERS_PER_ROOT {
            let point = 1_000_000 + root * CARRIERS_PER_ROOT + carrier;
            let vertex = 2_000_000 + root * CARRIERS_PER_ROOT + carrier;
            writeln!(
                source,
                "#{point}=CARTESIAN_POINT('',(0.,0.,0.));\n#{vertex}=VERTEX_POINT('',#{point});"
            )
            .expect("write independent carrier");
        }
    }
    source.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    source
}

fn root_index_work(count: u64) -> u64 {
    let source = root_source(count);
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_work_units = 16_000_000_000;
    policy.limits.max_collection_items = 128_000_000;
    let (exchange, _) = crate::test_support::with_policy_context(
        source.as_bytes(),
        &policy,
        crate::parse::parse_inner,
    )
    .expect("independent pcurve roots parse");
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(source.as_bytes(), &setup_arena, &policy)
        .expect("carrier setup fits desktop limits");
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("root carrier geometry");
    assert_eq!(
        ir.model.points.len(),
        usize::try_from(count * (CARRIERS_PER_ROOT + 6)).expect("carrier count fits")
    );
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("predecoded carriers");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits desktop limits");
    let outcome =
        super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("topology roots commit");
    assert_eq!(
        ir.model.bodies.len(),
        usize::try_from(count).expect("root count fits")
    );
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        usize::try_from(count).expect("one selected pcurve per root")
    );
    assert!(outcome.losses.iter().any(|loss| loss.code
        == crate::loss::StepLossCode::PcurveGlobalFidelityUnproved.kind()
        && loss.message.starts_with(&format!(
            "a finite endpoint and locus witness admits {count} pcurve relation(s);"
        ))));
    drop(outcome);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "root index work probe")
        .expect_err("probe work counter")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn pcurve_root_growth_fixture_commits_selected_relations() {
    assert!(root_index_work(2) > 0);
}

#[test]
#[ignore = "requires an IR carrier index reusable across topology commits"]
fn pcurve_root_indexes_do_not_rebuild_all_carriers() {
    let small = root_index_work(64);
    let large = root_index_work(128);
    eprintln!("topology work: 64 roots = {small}, 128 roots = {large}");
    // Carrier count and root count double; one shared index grows with n log n.
    assert!(
        large < 3 * small,
        "root index work grew from {small} to {large}"
    );
}
