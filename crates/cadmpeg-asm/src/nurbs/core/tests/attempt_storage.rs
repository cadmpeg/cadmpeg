// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use std::mem::size_of;

#[test]
fn surface_cache_keeps_only_the_selected_candidate_and_its_actual_retained_backing() {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("carrier".into())];
    tokens.extend(super::rectangular_surface_tokens(false));
    // The final candidate has a 512 by 2 grid but no coordinates.
    tokens.extend([
        Token::Ident("nubs".into()), Token::Long(1), Token::Long(1),
        Token::Enum(0), Token::Enum(0), Token::Enum(0), Token::Enum(0),
        Token::Long(2), Token::Long(2),
        Token::Double(0.0), Token::Long(256), Token::Double(1.0), Token::Long(256),
        Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        Token::SubtypeClose,
    ]);
    let marker_bytes = 4 * size_of::<usize>();
    let knot_bytes = (514 + 4) * size_of::<f64>();
    let rows_bytes = 512 * size_of::<Vec<FinitePoint3>>();
    let row_bytes = 2 * size_of::<FinitePoint3>();
    let peak = u64_from_index(marker_bytes + knot_bytes + rows_bytes + 512 * row_bytes);
    // The selected 2 by 3 surface keeps four U knots, five V knots, two
    // row slots and six already-admitted poles, with no lane copies.
    let retained = u64_from_index(9 * size_of::<f64>()
        + 2 * size_of::<Vec<FinitePoint3>>() + 6 * size_of::<FinitePoint3>());
    for (material_cap, retained_cap) in [
        (peak - 1, retained), (peak, retained - 1), (peak, retained),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = material_cap;
        policy.limits.max_retained_bytes = retained_cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::surface_cache(&ctx, &tokens).transpose();
        if material_cap < peak || retained_cap < retained {
            let first = match result {
                Err(CodecError::ResourceLimit(first)) => first,
                _ => panic!("expected actual candidate allocation or selected output refusal"),
            };
            if material_cap < peak {
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "ASM NURBS grid row poles");
                assert_eq!((first.limit, first.used, first.additional),
                    (material_cap, peak - u64_from_index(row_bytes), u64_from_index(row_bytes)));
            } else {
                assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(first.operation, "ASM surface cache candidate");
                assert_eq!((first.limit, first.used, first.additional), (retained_cap, 0, retained));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let selected = result.unwrap().unwrap();
            assert_eq!((selected.u_count(), selected.v_count()), (2, 3));
            assert_eq!(selected.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(selected.v_knots().as_slice(), [0.0, 0.0, 1.0, 2.0, 2.0]);
            // Keep the selected actual surface alive. Discarded candidates
            // and the marker index must no longer occupy materialized bytes.
            let available = ctx.reserve_scoped(peak, "discarded surface candidates released").unwrap();
            drop(available);
            drop(selected);
            ctx.finish_session().unwrap();
        }
    }
}
