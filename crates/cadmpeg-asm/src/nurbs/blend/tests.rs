// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use std::mem::size_of;

#[test]
fn extrusion_tolerance_releases_a_failed_surface_before_the_next_candidate() {
    let mut tokens = vec![
        Token::SubtypeOpen, Token::Ident("cyl_spl_sur".into()),
        Token::Double(0.0), Token::Double(1.0),
        Token::Vector3([0.0, 0.0, 1.0]), Token::Position([0.0, 0.0, 0.0]),
        Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
        Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        Token::Double(0.0), Token::Double(0.0), Token::Double(0.0),
        Token::Double(1.0), Token::Double(0.0), Token::Double(0.0),
        Token::Ident("nubs".into()), Token::Long(1), Token::Long(1),
        Token::Enum(0), Token::Enum(0), Token::Enum(0), Token::Enum(0),
        Token::Long(2), Token::Long(2),
    ];
    for _ in 0..2 {
        tokens.extend([
            Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        ]);
    }
    for point in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]] {
        tokens.extend(point.map(Token::Double));
    }
    tokens.extend([
        Token::Double(0.125),
        // The final candidate has a 512 by 2 grid but no coordinates.
        Token::Ident("nubs".into()), Token::Long(1), Token::Long(1),
        Token::Enum(0), Token::Enum(0), Token::Enum(0), Token::Enum(0),
        Token::Long(2), Token::Long(2),
        Token::Double(0.0), Token::Long(256), Token::Double(1.0), Token::Long(256),
        Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        Token::SubtypeClose,
    ]);
    let directrix_bytes = 4 * size_of::<f64>() + 2 * size_of::<FinitePoint3>();
    let marker_bytes = 4 * size_of::<usize>();
    let knot_bytes = (514 + 4) * size_of::<f64>();
    let rows_bytes = 512 * size_of::<Vec<FinitePoint3>>();
    let row_bytes = 2 * size_of::<FinitePoint3>();
    let peak = u64_from_index(directrix_bytes + marker_bytes + knot_bytes + rows_bytes + 512 * row_bytes);
    // The directrix's raw/final pole overlap and the small valid surface are
    // both smaller than the malformed candidate's fully allocated grid.
    assert!(4 * size_of::<f64>() + 2 * size_of::<Point3>() + 2 * size_of::<FinitePoint3>()
        < knot_bytes + rows_bytes + 512 * row_bytes);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = ctx.with_scoped_storage("discarded extrusion definition", || {
            super::cyl_spl_sur(&ctx, &tokens, None).transpose()
        });
        if cap < peak {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected final malformed-candidate row refusal"),
            };
            drop(result);
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "ASM NURBS grid row poles");
            assert_eq!((first.limit, first.used, first.additional),
                (cap, peak - u64_from_index(row_bytes), u64_from_index(row_bytes)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let (decoded, storage) = result.unwrap();
            let decoded = decoded.unwrap();
            assert_eq!(decoded.legacy_cache_fit_tolerance(), Some(1.25));
            assert!(matches!(decoded.definition(),
                crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Extrusion { directrix, .. }
                    if directrix.degree() == 1));
            drop(decoded);
            drop(storage);
            let released = ctx.reserve_scoped(peak, "all extrusion tolerance storage released").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}
