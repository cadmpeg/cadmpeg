// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EPS_BINOMIAL_RELATIVE: f64 = 1e-12;

fn binomial_factors(count: usize, work: u64, refuses: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::bernstein_binomial(2 * count, count, &ctx);
    if refuses {
        let Err(CodecError::ResourceLimit(first)) = result else {
            panic!("expected the next binomial factor to refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges Bernstein binomial factors");
        assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
        for _ in 0..64 {
            for (n, k) in [(2 * count, count), (0, 1), (0, 0)] {
                assert!(matches!(super::super::bernstein_binomial(n, k, &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    } else {
        // C(8,4)=70; C(128,64)=23951146041928082866135587776380551750.
        let expected = match count {
            4 => 70.0,
            64 => 2.3951146041928085e37,
            _ => panic!("unsupported fixture size"),
        };
        let actual = result.unwrap().unwrap();
        assert!(actual.is_finite());
        assert!((actual - expected).abs() <= EPS_BINOMIAL_RELATIVE * expected);
        assert_eq!(super::super::bernstein_binomial(0, 1, &ctx).unwrap(), None);
        assert_eq!(super::super::bernstein_binomial(0, 0, &ctx).unwrap(), Some(1.0));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn surface_binomial_first_factor_refuses_work() {
    for count in [4, 64] { binomial_factors(count, 0, true); }
}

#[test]
fn surface_binomial_last_factor_refuses_work() {
    for count in [4, 64] {
        binomial_factors(count, u64::try_from(count - 1).unwrap(), true);
    }
}

#[test]
fn surface_binomial_accepts_exact_factor_visits() {
    for count in [4, 64] {
        binomial_factors(count, u64::try_from(count).unwrap(), false);
    }
}
