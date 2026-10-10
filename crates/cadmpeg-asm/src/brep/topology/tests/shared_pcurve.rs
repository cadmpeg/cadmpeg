// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn shared_records() -> Vec<Record<'static>> {
    let edge = |index, range: [f64; 2]| {
        let mut tokens = vec![Token::Ref(-1); 10];
        tokens[4] = Token::Double(range[0]);
        tokens[6] = Token::Double(range[1]);
        crate::test_support::sab::record(index, "edge".into(), tokens.into(), 0, 0)
    };
    vec![
        ref_record(0, "face", &[-1, -1, -1, -1, 2]),
        ref_record(1, "face", &[-1, -1, -1, -1, 3]),
        ref_record(2, "loop", &[-1, -1, -1, -1, 4]),
        ref_record(3, "loop", &[-1, -1, -1, -1, 5]),
        ref_record(4, "coedge", &[-1, -1, -1, 4, -1, -1, 6, -1, -1, 8]),
        ref_record(5, "coedge", &[-1, -1, -1, 5, -1, -1, 7, -1, -1, 8]),
        edge(6, [0.0, 0.25]),
        edge(7, [0.5, 1.0]),
        crate::test_support::sab::record(
            8,
            "pcurve".into(),
            vec![
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Long(0),
                Token::True,
                Token::SubtypeOpen,
                Token::Ident("exp_par_cur".into()),
                Token::Ident("nubs".into()),
                Token::Long(1),
                Token::Enum(0),
                Token::Long(2),
                Token::Double(0.0),
                Token::Long(1),
                Token::Double(1.0),
                Token::Long(1),
                Token::Double(0.0),
                Token::Double(0.0),
                Token::Double(1.0),
                Token::Double(0.0),
                Token::SubtypeClose,
            ]
            .into(),
            0,
            0,
        ),
    ]
}
#[test]
fn shared_explicit_pcurve_decodes_once_and_keeps_each_edge_range() {
    let records = shared_records();
    let by_index = records
        .iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect();
    let table = nurbs::toks::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test pcurve carriers").unwrap();
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable {
        faces: [0, 1].into(),
        ..Reachable::default()
    };
    let inputs = TopologyContext {
        ctx: &ctx,
        by_index: &by_index,
        token_table: &table,
        purpose: DecodePurpose::Model,
        format: crate::asm_format!("sat"),
    };
    walk_reachable_topology(
        inputs,
        &mut out,
        &records[..1],
        &mut carriers,
        &mut reach,
        &mut storage,
    )
    .unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "ASM pcurve block with end entries",
        None,
    );
    walk_reachable_topology(
        inputs,
        &mut out,
        &records[1..2],
        &mut carriers,
        &mut reach,
        &mut storage,
    )
    .unwrap();
    drop(probe);
    assert_eq!(carriers.pcurve_geo.len(), 1);
    assert_eq!(reach.pcurves, [8].into());
    assert_eq!(
        carriers.pcurve_parameter_ranges[&crate::brep::CoedgeRecordIndex(4)],
        [0.0, 0.25]
    );
    assert_eq!(
        carriers.pcurve_parameter_ranges[&crate::brep::CoedgeRecordIndex(5)],
        [0.5, 1.0]
    );
    drop((carriers, reach));
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn shared_pcurve_keeps_the_last_faces_support_chart() {
    for (first, second, expected_u) in [("plane", "sphere", 1.0), ("sphere", "plane", 10.0)] {
    let mut records = shared_records();

        for (index, name) in [(9, first), (10, second)] {
            records.push(crate::test_support::sab::record(index, name.into(), Vec::new().into(), 0, 0));
        }
        records[0] = ref_record(0, "face", &[-1, -1, -1, -1, 2, -1, -1, 9]);
        records[1] = ref_record(1, "face", &[-1, -1, -1, -1, 3, -1, -1, 10]);
        let by_index = indexed_records(&records);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let table = nurbs::toks::SubtypeTable::from_records(&ctx, &records).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test pcurve carriers").unwrap();
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable { faces: [0, 1].into(), ..Reachable::default() };
        walk_reachable_topology(TopologyContext { ctx: &ctx, by_index: &by_index,
            token_table: &table, purpose: DecodePurpose::Model, format: crate::asm_format!("sat") },
            &mut out, &records, &mut carriers, &mut reach, &mut storage).unwrap();
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
            &carriers.pcurve_geo[&crate::brep::PcurveRecordIndex(8)] else { panic!("decoded pcurve"); };
        assert_eq!(nurbs.pole_rows().point_at(1).unwrap().get().u, expected_u);
        assert_eq!(carriers.pcurve_parameter_ranges[&crate::brep::CoedgeRecordIndex(4)], [0.0, 0.25]);
        assert_eq!(carriers.pcurve_parameter_ranges[&crate::brep::CoedgeRecordIndex(5)], [0.5, 1.0]);
        drop((carriers, reach));
        drop(storage);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn shared_pcurve_caches_an_unavailable_decode() {
    let mut records = shared_records();

    records[8] = crate::test_support::sab::record(8, "pcurve".into(),
        vec![Token::Ref(-1), Token::Ref(-1), Token::Ref(-1), Token::Long(0), Token::True,
            Token::SubtypeOpen, Token::Ident("exp_par_cur".into()), Token::SubtypeClose].into(), 0, 0);
    let by_index = indexed_records(&records);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let table = nurbs::toks::SubtypeTable::from_records(&ctx, &records).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test pcurve carriers").unwrap();
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable { faces: [0, 1].into(), ..Reachable::default() };
    let inputs = TopologyContext { ctx: &ctx, by_index: &by_index, token_table: &table,
        purpose: DecodePurpose::Model, format: crate::asm_format!("sat") };
    walk_reachable_topology(inputs, &mut out, &records[..1], &mut carriers, &mut reach, &mut storage).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "ASM payload subtype token scan", None);
    walk_reachable_topology(inputs, &mut out, &records[1..2], &mut carriers, &mut reach, &mut storage).unwrap();
    drop(probe);
    assert!(carriers.pcurve_geo.is_empty());
    assert!(reach.pcurves.is_empty());
    assert_eq!(out.stats.undecoded_pcurve_kinds.get("pcurve"), Some(&2));
    drop((carriers, reach));
    drop(storage);
    ctx.finish_session().unwrap();
}
