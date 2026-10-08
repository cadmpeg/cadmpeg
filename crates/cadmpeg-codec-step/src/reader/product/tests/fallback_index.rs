// SPDX-License-Identifier: Apache-2.0
//! Parent mapping reuse and source warning replay.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

fn source(mapped_count: u64) -> String {
    let mut items = String::new();
    let mut records = String::new();
    for id in 100..100 + mapped_count {
        if id != 100 { items.push(','); }
        write!(items, "#{id}").expect("write source item");
        writeln!(records, "#{id}=MAPPED_ITEM('',#39,#35);").expect("write mapping");
    }
    String::from_utf8_lossy(include_bytes!("../../../../tests/fixtures/ap242_mapped_assembly.p21"))
        .replace("(#40)", &format!("({items})"))
        .replace("ENDSEC;\nEND-ISO-10303-21;", &format!("{records}ENDSEC;\nEND-ISO-10303-21;"))
}

#[test]
fn fallback_parent_mapping_list_is_resolved_once_for_sibling_usages() {
    let source = source(64);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("mapped parent exchange");
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    let usages = (1000..1064).map(|id| (id, super::super::Usage { parent_definition: 6, child_definition: 10, name: None })).collect::<BTreeMap<_, _>>();
    let mut policy = DecodePolicy::service();
    // Rescanning 64 mappings for 64 usages needs 4096 record lookups.
    // With more than 71 records, each lookup alone costs 33 * 8 units,
    // exceeding this cap before placement evaluation.
    policy.limits.max_work_units = 1_000_000;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut storage = ctx.reserve_scoped(0, "placement fixture").expect("scope");
        let placements = super::super::occurrence_placements(&exchange, &geometry.value, &usages, (&mut losses, &reports), (&mut BTreeMap::new(), &mut BTreeMap::new()), &mut storage, ctx).expect("indexed parent scan fits work budget");
        assert!(placements.is_empty());
        assert_eq!(losses.len(), 64);
        for (ordinal, loss) in losses.iter().enumerate() {
            assert_eq!(loss.code, crate::loss::StepLossCode::DecodeWarning.kind());
            assert_eq!(loss.message, format!("NAUO #{} has an ambiguous mapped-item placement", 1000 + ordinal));
        }
    });
}

#[test]
fn fallback_singular_warnings_keep_source_order_for_each_usage() {
    let source = source(2);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("mapped parent exchange");
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let mut geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    geometry.value.placements.remove(&34);
    geometry.value.transformation_operators.insert(34, cadmpeg_ir::transform::Transform::affine([[0.0; 4]; 3]).expect("finite singular affine transform"));
    let usages = BTreeMap::from([(1000, super::super::Usage { parent_definition: 6, child_definition: 10, name: None }), (1001, super::super::Usage { parent_definition: 6, child_definition: 10, name: None })]);
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut storage = ctx.reserve_scoped(0, "placement fixture").expect("scope");
        let placements = super::super::occurrence_placements(&exchange, &geometry.value, &usages, (&mut losses, &reports), (&mut BTreeMap::new(), &mut BTreeMap::new()), &mut storage, ctx).expect("singular source scan");
        assert!(placements.is_empty());
        assert_eq!(losses.iter().map(|loss| loss.message.as_str()).collect::<Vec<_>>(), ["MAPPED_ITEM #100 has a singular placement", "MAPPED_ITEM #101 has a singular placement", "MAPPED_ITEM #100 has a singular placement", "MAPPED_ITEM #101 has a singular placement"]);
    });
}
