// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_csys_payloads;
use crate::native::features::feature_datum_plane_payloads;

#[derive(Clone, Copy)]
enum DatumPayloadRoute {
    Csys,
    Plane,
}

fn datum_payload_refusal(
    route: DatumPayloadRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let (constructions, headers) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_datum_csys_constructions(
                ctx,
                &crate::native::features::FeatureHistory::new(ctx, &container)?,
            )?,
            feature_datum_plane_headers(
                ctx,
                &crate::native::features::FeatureHistory::new(ctx, &container)?,
            )?,
        ))
    })
    .expect("datum construction inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        DatumPayloadRoute::Csys => {
            feature_datum_csys_payloads(ctx, &container, &constructions).map(|rows| rows.len())
        }
        DatumPayloadRoute::Plane => {
            feature_datum_plane_payloads(ctx, &container, &headers).map(|rows| rows.len())
        }
    };
    let admitted = crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted datum payload route");
    assert_eq!(admitted, 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("datum payload resource limit"),
    )
}

macro_rules! datum_payload_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = datum_payload_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = datum_payload_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $scoped() {
            let error = datum_payload_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }
        #[test]
        fn $work() {
            let error = datum_payload_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

datum_payload_limit_tests!(
    datum_csys_payload_refuses_collection_limit,
    datum_csys_payload_refuses_retained_limit,
    datum_csys_payload_refuses_scoped_limit,
    datum_csys_payload_refuses_work_limit,
    DatumPayloadRoute::Csys
);
datum_payload_limit_tests!(
    datum_plane_payload_refuses_collection_limit,
    datum_plane_payload_refuses_retained_limit,
    datum_plane_payload_refuses_scoped_limit,
    datum_plane_payload_refuses_work_limit,
    DatumPayloadRoute::Plane
);
