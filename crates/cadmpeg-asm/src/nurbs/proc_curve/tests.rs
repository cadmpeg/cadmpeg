// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use std::mem::size_of;

#[test]
fn vector_offset_cache_releases_failed_candidates_and_preserves_its_source() {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("offset_int_cur".into()), Token::True];
    // The wrapper owns an independent source curve before its solved cache.
    for (ordinal, x) in [0.0, 2.0].into_iter().enumerate() {
        tokens.extend([
            Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
            Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
            Token::Double(x), Token::Double(0.0), Token::Double(0.0),
            Token::Double(x + 1.0), Token::Double(0.0), Token::Double(0.0),
        ]);
        if ordinal == 0 {
            tokens.extend([
                Token::Double(0.0), Token::Double(1.0), Token::Vector3([1.0, 0.0, 0.0]),
                Token::Str("source".into()), Token::Long(1), Token::Str("offset".into()), Token::Long(2),
            ]);
        }
    }
    tokens.extend([
        Token::Double(0.25),
        // A later 512-pole cache states no coordinates, so recovery selects
        // the preceding solved cache rather than the wrapper source.
        Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
        Token::Double(0.0), Token::Long(256), Token::Double(1.0), Token::Long(256),
        Token::SubtypeClose,
    ]);
    let marker_bytes = 4 * size_of::<usize>();
    let knot_bytes = 514 * size_of::<f64>();
    let pole_bytes = 512 * size_of::<Point3>();
    let peak = u64_from_index(marker_bytes + knot_bytes + pole_bytes);
    let curve_bytes = u64_from_index(4 * size_of::<f64>() + 2 * size_of::<FinitePoint3>());
    let retained = 2 * curve_bytes;
    for (material_cap, retained_cap) in [(peak - 1, retained), (peak, retained - 1), (peak, retained)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = material_cap;
        policy.limits.max_retained_bytes = retained_cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let table = crate::nurbs::toks::SubtypeTable::from_records(&ctx, &[]).unwrap();
        let result = super::procedural_curve_resolving_refs(&ctx, &tokens, &table).transpose();
        if material_cap < peak || retained_cap < retained {
            let first = match result {
                Err(CodecError::ResourceLimit(first)) => first,
                _ => panic!("expected failed-cache backing or selected-cache retention refusal"),
            };
            if material_cap < peak {
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "ASM polynomial NURBS poles");
                assert_eq!((first.limit, first.used, first.additional),
                    (material_cap, u64_from_index(marker_bytes + knot_bytes), u64_from_index(pole_bytes)));
            } else {
                assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(first.operation, "ASM procedural curve cache candidate");
                assert_eq!((first.limit, first.used, first.additional), (retained_cap, curve_bytes, curve_bytes));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let decoded = result.unwrap().unwrap();
            assert_eq!(decoded.curve.pole_rows().point_at(0).unwrap().get().x, 20.0);
            assert_eq!(decoded.cache_fit_tolerance, Some(2.5));
            let super::ProceduralCurveConstruction::VectorOffset((source, interval, offset, _)) = &decoded.construction else {
                panic!("expected retained vector-offset construction");
            };
            assert_eq!(source.pole_rows().point_at(0).unwrap().get().x, 0.0);
            assert_eq!(*interval, [0.0, 1.0]);
            assert_eq!(*offset, Vector3::new(10.0, 0.0, 0.0));
            let available = ctx.reserve_scoped(peak, "discarded vector-offset cache scratch released").unwrap();
            drop(available);
            drop(decoded);
            ctx.finish_session().unwrap();
        }
    }
}
