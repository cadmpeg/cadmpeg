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
            .expect("indexed record fixture")
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
        )
        .expect("indexed record fixture"),
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
            records.push(crate::test_support::sab::record(index, name.into(), Vec::new().into(), 0, 0)
                    .expect("indexed record fixture"));
        }
        records[0] = ref_record(0, "face", &[-1, -1, -1, -1, 2, -1, -1, 9]);
        records[1] = ref_record(1, "face", &[-1, -1, -1, -1, 3, -1, -1, 10]);
        let by_index = indexed_records(&records);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let table = nurbs::toks::SubtypeTable::from_records(&ctx, &records).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test pcurve carriers").unwrap();
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable {
            faces: [0, 1].into(),
            ..Reachable::default()
        };
        walk_reachable_topology(
            TopologyContext {
                ctx: &ctx,
                by_index: &by_index,
                token_table: &table,
                purpose: DecodePurpose::Model,
                format: crate::asm_format!("sat"),
            },
            &mut out,
            &records,
            &mut carriers,
            &mut reach,
            &mut storage,
        )
        .unwrap();
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
            &carriers.pcurve_geo[&crate::brep::PcurveRecordIndex(8)].0
        else {
            panic!("decoded pcurve");
        };
        assert_eq!(nurbs.pole_rows().point_at(1).unwrap().get().u, expected_u);
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
}

#[test]
fn shared_pcurve_caches_an_unavailable_decode() {
    let mut records = shared_records();

    records[8] = crate::test_support::sab::record(
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
            Token::SubtypeClose,
        ]
        .into(),
        0,
        0,
    )
    .expect("indexed record fixture");
    let by_index = indexed_records(&records);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let table = nurbs::toks::SubtypeTable::from_records(&ctx, &records).unwrap();
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
        "ASM payload subtype token scan",
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
    assert!(carriers.pcurve_geo.is_empty());
    assert!(reach.pcurves.is_empty());
    assert_eq!(out.stats.undecoded_pcurve_kinds.get("pcurve"), Some(&2));
    drop((carriers, reach));
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn shared_pcurve_retains_one_geometry_for_any_coedge_count() {
    let mut retained = None;
    for count in [1, 2, 64] {
        let mut records = shared_records();
        records[4] = ref_record(
            4,
            "coedge",
            &[
                -1,
                -1,
                -1,
                if count == 1 { 4 } else { 9 },
                -1,
                -1,
                6,
                -1,
                -1,
                8,
            ],
        );
        for ordinal in 1..count {
            let index = 8 + ordinal;
            let next = if ordinal + 1 == count { 4 } else { index + 1 };
            records.push(ref_record(
                index,
                "coedge",
                &[
                    -1,
                    -1,
                    -1,
                    i64::try_from(next).unwrap(),
                    -1,
                    -1,
                    6,
                    -1,
                    -1,
                    8,
                ],
            ));
        }
        let by_index = indexed_records(&records);
        let table = nurbs::toks::SubtypeTable::from_records(
            &cadmpeg_test_support::service_decode_context(),
            &records,
        )
        .unwrap();
        let refusal = crate::test_support::resource_limit_at(
            &[],
            ResourceDimension::RetainedBytes,
            "test shared pcurve retained total",
            |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test pcurve carriers")?;
                let mut out = AsmBrep::default();
                let mut carriers = Carriers::default();
                let mut reach = Reachable {
                    faces: [0].into(),
                    ..Reachable::default()
                };
                walk_reachable_topology(
                    TopologyContext {
                        ctx,
                        by_index: &by_index,
                        token_table: &table,
                        purpose: DecodePurpose::Model,
                        format: crate::asm_format!("sat"),
                    },
                    &mut out,
                    &records,
                    &mut carriers,
                    &mut reach,
                    &mut storage,
                )?;
                assert_eq!(reach.coedges.len(), count);
                assert_eq!(carriers.pcurve_geo.len(), 1);
                crate::brep::emit::emit_pcurves(
                    ctx,
                    &mut out,
                    &records,
                    &mut carriers,
                    &reach,
                    crate::asm_format!("sat"),
                )?;
                assert_eq!(out.pcurves.len(), 1);
                assert!(carriers.pcurve_geo.is_empty());
                let geometry = out.pcurves[0].geometry.clone();
                let result = ctx.charge_retained(1, "test shared pcurve retained total");
                assert_eq!(out.pcurves.len(), 1);
                assert_eq!(out.pcurves[0].geometry, geometry);
                result
            },
        );
        assert_eq!(refusal.operation, "test shared pcurve retained total");
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.limit + 1, refusal.used + refusal.additional);
        assert_eq!(*retained.get_or_insert(refusal.used), refusal.used);
    }
}

#[test]
fn pcurve_output_slot_refusal_precedes_candidate_retention() {
    let mut records = shared_records();
    records[4] = ref_record(4, "coedge", &[-1, -1, -1, 4, -1, -1, -1, -1, -1, 8]);
    let by_index = indexed_records(&records);
    let table = nurbs::toks::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let refusal = crate::test_support::resource_limit_at(
        &[],
        ResourceDimension::RetainedBytes,
        "out.pcurves",
        |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test pcurve carriers")?;
            let mut out = AsmBrep::default();
            let mut carriers = Carriers::default();
            let mut reach = Reachable {
                faces: [0].into(),
                ..Reachable::default()
            };
            walk_reachable_topology(
                TopologyContext {
                    ctx,
                    by_index: &by_index,
                    token_table: &table,
                    purpose: DecodePurpose::Model,
                    format: crate::asm_format!("sat"),
                },
                &mut out,
                &records,
                &mut carriers,
                &mut reach,
                &mut storage,
            )?;
            assert_eq!(carriers.pcurve_geo.len(), 1);
            let result = crate::brep::emit::emit_pcurves(
                ctx,
                &mut out,
                &records,
                &mut carriers,
                &reach,
                crate::asm_format!("sat"),
            );
            assert!(out.pcurves.is_empty());
            result
        },
    );
    let size = std::mem::size_of::<cadmpeg_ir::geometry::pcurve::Pcurve>();
    let slots = if size <= 1024 { 4 } else { 1 };
    assert_eq!(refusal.operation, "out.pcurves");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, u64::try_from(size * slots).unwrap());
    assert_eq!(refusal.limit + 1, refusal.additional);
}
