// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn normalization_rejection_releases_explicit_pcurve_storage() {
    let count = 4096;
    let mut records = vec![
        ref_record(0, "face", &[-1, -1, -1, -1, 1, -1, -1, 3]),
        ref_record(1, "loop", &[-1, -1, -1, -1, 4]),
        ref_record(2, "edge", &[]),
        ref_record(3, "plane", &[]),
    ];
    for index in 0..count {
        let next = i64::try_from(4 + (index + 1) % count).unwrap();
        let pcurve = i64::try_from(4 + count + index).unwrap();
        records.push(ref_record(4 + index, "coedge",
            &[-1, -1, -1, next, -1, -1, 2, -1, -1, pcurve]));
    }
    for index in 0..count {
        records.push(crate::test_support::sab::record(4 + count + index, "pcurve".into(), vec![
            Token::Ref(-1), Token::Ref(-1), Token::Ref(-1), Token::Long(0), Token::True,
            Token::SubtypeOpen, Token::Ident("exp_par_cur".into()), Token::Ident("nubs".into()),
            Token::Long(1), Token::Enum(0), Token::Long(2), Token::Double(0.0), Token::Long(1),
            Token::Double(1.0), Token::Long(1), Token::Double(1.0e308), Token::Double(0.0),
            Token::Double(1.0), Token::Double(0.0), Token::SubtypeClose,
        ].into(), 0, 0));
    }
    let service = cadmpeg_test_support::service_decode_context();
    let span = nurbs::toks::payload_subtype_toks(&service, &records[4 + count], 5, "exp_par_cur")
        .unwrap().unwrap();
    let candidate = nurbs::pcurve::explicit_pcurve_cache(&service, span).unwrap().unwrap();
    assert_eq!(candidate.pole_rows().count(), 2);
    let table = nurbs::toks::SubtypeTable::from_records(&service, &records).unwrap();
    let by_index = records.iter().map(|record| (i64::try_from(record.index).unwrap(), record)).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8192;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test pcurve carriers").unwrap();
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable { faces: [0].into(), ..Reachable::default() };
    let inputs = TopologyContext { ctx: &ctx, by_index: &by_index, token_table: &table,
        purpose: DecodePurpose::Model, format: crate::asm_format!("sat") };
    walk_reachable_topology(inputs, &mut out, &records[..1], &mut carriers, &mut reach,
        &mut storage).unwrap();
    assert!(carriers.pcurve_geo.is_empty());
    assert!(reach.pcurves.is_empty());
    assert_eq!(out.stats.undecoded_pcurve_kinds.get("pcurve"), Some(&count));
    drop((carriers, reach));
    drop(storage);
    ctx.finish_session().unwrap();
}
