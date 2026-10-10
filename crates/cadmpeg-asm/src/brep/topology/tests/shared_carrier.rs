// SPDX-License-Identifier: Apache-2.0

use super::{indexed_records, ref_record, AsmBrep, Carriers, DecodePurpose, Reachable, Record, Token};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

fn carrier(name: &str) -> Record {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident(name.into())];
    if name == "exact_spl_sur" {
        tokens.extend([
            Token::Ident("nubs".into()), Token::Long(1), Token::Long(1),
            Token::Enum(0), Token::Enum(0), Token::Enum(0), Token::Enum(0),
            Token::Long(2), Token::Long(2),
        ]);
        for _ in 0..2 {
            tokens.extend([Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1)]);
        }
        for [x, y] in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
            tokens.extend([Token::Double(x), Token::Double(y), Token::Double(0.0)]);
        }
        tokens.extend([
            Token::Double(0.25), Token::Double(0.0), Token::Double(1.0), Token::Double(0.0), Token::Double(1.0), Token::Long(0),
        ]);
    }
    tokens.push(Token::SubtypeClose);
    crate::test_support::sab::record(
0,
"spline".into(),
tokens.into(),
0,
0
)
}

#[test]
fn shared_surface_reuses_decoded_and_unavailable_procedural_results() {
    for (purpose, name, operation) in [
        (DecodePurpose::Model, "exact_spl_sur", "scan ASM owned construction markers"),
        (DecodePurpose::Model, "unsupported", "scan ASM owned construction markers"),
        (DecodePurpose::Model, "unsupported", "scan ASM cache ownership"),
        (DecodePurpose::History, "blend_unsupported", "scan ASM owned construction markers"),
        (DecodePurpose::History, "unsupported", "scan ASM construction name"),
    ] {
        let records = [
            carrier(name),
            ref_record(1, "face", &[-1, -1, -1, -1, -1, -1, -1, 0]),
            ref_record(2, "face", &[-1, -1, -1, -1, -1, -1, -1, 0]),
        ];
        let by_index = indexed_records(&records);
        let table = crate::nurbs::toks::SubtypeTable::from_records(
            &cadmpeg_test_support::service_decode_context(), &records,
        ).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut scratch = ctx.reserve_scoped(0, "shared carrier scratch").unwrap();
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable::default();
        let inputs = super::TopologyContext {
            ctx: &ctx, by_index: &by_index, token_table: &table,
            purpose, format: crate::asm_format!("f3d"),
        };
        super::keep_faces_and_carriers(inputs, &mut out, &records[..2], &mut carriers, &mut reach, &mut scratch).unwrap();
        // Any repeated procedural parse (or history name scan) would refuse.
        // Both calls retain the same immutable record table and classification.
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        super::keep_faces_and_carriers(inputs, &mut out, &records[2..], &mut carriers, &mut reach, &mut scratch).unwrap();
        drop(probe);
        assert_eq!(reach.faces, [1, 2].into());
        if name == "exact_spl_sur" {
            assert_eq!(reach.surfaces, [0].into());
            assert_eq!(carriers.procedural_surface_defs.len(), 1);
            assert!(matches!(carriers.procedural_surface_defs[&0].definition(),
                crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Exact { .. }));
            assert_eq!(carriers.procedural_surface_defs[&0].cache_fit_tolerance(), Some(2.5));
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = &carriers.surface_geo[&0] else {
                panic!("expected the original solved surface cache");
            };
            assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
            assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
            assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            for (u, row) in [[(0.0, 0.0), (0.0, 10.0)], [(10.0, 0.0), (10.0, 10.0)]].into_iter().enumerate() {
                for (v, (x, y)) in row.into_iter().enumerate() {
                    assert_eq!(surface.pole(u, v).unwrap().get(), cadmpeg_ir::math::Point3::new(x, y, 0.0));
                    assert!(surface.weight(u, v).is_none());
                }
            }
            assert_eq!(out.stats.nurbs_surfaces, 1);
        } else if purpose == DecodePurpose::Model {
            assert_eq!(reach.unknown_surface_records, [0].into());
            assert!(reach.surfaces.is_empty());
            assert!(carriers.procedural_surface_defs.is_empty());
            assert_eq!(out.stats.unknown_surface_kinds.get(name), Some(&2));
        } else {
            assert_eq!(reach.surfaces, [0].into());
            assert!(carriers.procedural_surface_defs.is_empty());
            assert!(matches!(carriers.surface_geo[&0], SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })));
        }
        drop((carriers, reach, out));
        drop(scratch);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn first_surface_parse_keeps_its_original_sticky_work_refusal() {
    for purpose in [DecodePurpose::Model, DecodePurpose::History] {
        let records = [carrier("blend_unsupported"), ref_record(1, "face", &[-1, -1, -1, -1, -1, -1, -1, 0])];
        let by_index = indexed_records(&records);
        let table = crate::nurbs::toks::SubtypeTable::from_records(
            &cadmpeg_test_support::service_decode_context(), &records,
        ).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut scratch = ctx.reserve_scoped(0, "shared carrier scratch").unwrap();
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable::default();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan ASM owned construction markers", None);
        let error = super::keep_faces_and_carriers(super::TopologyContext {
            ctx: &ctx, by_index: &by_index, token_table: &table,
            purpose, format: crate::asm_format!("f3d"),
        }, &mut out, &records, &mut carriers, &mut reach, &mut scratch).unwrap_err();
        drop(probe);
        let CodecError::ResourceLimit(first) = error else { panic!("expected original parser refusal"); };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "scan ASM owned construction markers");
        assert_eq!(first.additional, 1);
        assert_eq!(first.limit, first.used);
        assert!(carriers.procedural_surface_defs.is_empty());
        drop((carriers, reach, out));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
