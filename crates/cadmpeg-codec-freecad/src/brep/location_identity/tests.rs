// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

use super::{identities, LocationIdentity, LocationIdentityError, MAX_LOCATION_WORK};
use crate::brep::{LocationFactor, TextLocation};

fn context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

fn location(factors: &[(usize, i64)]) -> TextLocation {
    TextLocation {
        factors: factors
            .iter()
            .map(|&(location, power)| LocationFactor { location, power })
            .collect(),
        transform: Transform::identity(),
    }
}

#[test]
fn factor_order_and_cancellation_preserve_native_datum_identity() {
    context(|ctx| {
        let table = identities(
            ctx,
            &[
                location(&[]),
                location(&[]),
                location(&[(1, 1), (2, 1)]),
                location(&[(3, -1)]),
                location(&[(3, 1), (4, 1)]),
                location(&[(1, 2), (1, -1)]),
            ],
        )
        .unwrap();
        assert_eq!(table[0], LocationIdentity::identity());
        assert_ne!(table[1], table[0]);
        assert_ne!(table[1], table[2]);
        assert_eq!(table[3], LocationIdentity(vec![(2, 1), (1, 1)]));
        assert_eq!(table[4], LocationIdentity(vec![(1, -1), (2, -1)]));
        assert_eq!(table[5], table[0]);
        assert_eq!(table[6], table[1]);
    });
}

#[test]
fn nested_composition_is_associative_without_commuting_datums() {
    context(|ctx| {
        let a = LocationIdentity(vec![(1, 1), (2, 3)]);
        let b = LocationIdentity(vec![(2, -3), (1, -1)]);
        let c = LocationIdentity(vec![(3, 2)]);
        assert_eq!(
            a.compose(ctx, &b).unwrap().compose(ctx, &c).unwrap(),
            a.compose(ctx, &b.compose(ctx, &c).unwrap()).unwrap()
        );
        assert_eq!(a.compose(ctx, &b).unwrap(), LocationIdentity::identity());
        assert_ne!(a.compose(ctx, &c).unwrap(), c.compose(ctx, &a).unwrap());
    });
}

#[test]
fn powers_handle_inverse_and_large_single_datum_without_expansion() {
    context(|ctx| {
        let work = WorkBudget::new(MAX_LOCATION_WORK);
        let word = LocationIdentity(vec![(1, 1), (2, 2)]);
        assert_eq!(
            word.power(ctx, -2, &work).unwrap(),
            LocationIdentity(vec![(2, -2), (1, -1), (2, -2), (1, -1)])
        );
        assert_eq!(
            word.power(ctx, 0, &work).unwrap(),
            LocationIdentity::identity()
        );
        assert_eq!(
            LocationIdentity(vec![(1, 1)])
                .power(ctx, i64::MIN, &work)
                .unwrap(),
            LocationIdentity(vec![(1, i64::MIN)])
        );
    });
}

#[test]
fn expansion_and_exponent_overflow_refuse_without_unbounded_work() {
    context(|ctx| {
        let work = WorkBudget::new(MAX_LOCATION_WORK);
        for power in [i64::MAX, i64::MIN] {
            assert!(matches!(
                LocationIdentity(vec![(1, 1), (2, 1)]).power(ctx, power, &work),
                Err(LocationIdentityError::Bounded)
            ));
        }
        assert!(matches!(
            LocationIdentity(vec![(1, i64::MAX)]).compose(ctx, &LocationIdentity(vec![(1, 1)])),
            Err(LocationIdentityError::Bounded)
        ));
        for word in [
            LocationIdentity(vec![(1, i64::MIN)]),
            LocationIdentity(vec![(1, i64::MIN), (2, 1)]),
        ] {
            assert!(matches!(
                word.power(ctx, -1, &work),
                Err(LocationIdentityError::Bounded)
            ));
        }
        assert!(matches!(
            LocationIdentity(vec![(1, i64::MAX)]).power(ctx, 2, &work),
            Err(LocationIdentityError::Bounded)
        ));
    });
}

#[test]
fn zero_and_forward_location_references_are_refused() {
    context(|ctx| {
        for reference in [0, 1, usize::MAX] {
            assert!(matches!(
                identities(ctx, &[location(&[(reference, 1)])]),
                Err(LocationIdentityError::Decode(CodecError::Malformed(_)))
            ));
        }
    });
}

#[test]
fn local_work_exhaustion_keeps_the_decode_context_usable() {
    context(|ctx| {
        let datum = LocationIdentity(vec![(1, 1)]);
        assert!(matches!(
            datum.compose_bounded(ctx, &datum, &WorkBudget::new(1)),
            Err(LocationIdentityError::Bounded)
        ));
        assert_eq!(
            datum.compose(ctx, &datum).unwrap(),
            LocationIdentity(vec![(1, 2)])
        );
        ctx.charge_entities(1, "test_after_location_bound").unwrap();
    });
}

#[test]
fn canonical_transform_preserves_order_and_exact_cancellation() {
    context(|ctx| {
        let mut translate = location(&[]);
        translate.transform = Transform::affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let mut rotate = location(&[]);
        rotate.transform = Transform::affine([
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let locations = [translate, rotate, location(&[(1, 1), (2, 1)])];
        let table = identities(ctx, &locations).unwrap();
        let result = table[3].transform(&locations).unwrap();
        assert_eq!(result.affine_rows()[0][3], 0.0);
        assert_eq!(result.affine_rows()[1][3], 3.0);
        let inverse = table[3]
            .power(ctx, -1, &WorkBudget::new(MAX_LOCATION_WORK))
            .unwrap();
        assert_eq!(
            table[3]
                .compose(ctx, &inverse)
                .unwrap()
                .transform(&locations)
                .unwrap(),
            Transform::identity()
        );
        assert!(matches!(
            LocationIdentity(vec![(4, 1)]).transform(&locations),
            Err(CodecError::Malformed(_))
        ));
    });
}
