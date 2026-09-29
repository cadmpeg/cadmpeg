// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_block_uses;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_plane_block_uses;
use crate::native::features::feature_input_blocks;

#[derive(Clone, Copy)]
enum DatumBlockUseRoute {
    Csys,
    Plane,
}

fn datum_block_use_refusal(
    route: DatumBlockUseRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let (constructions, headers, inputs) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_datum_csys_constructions(ctx, &container)?,
            feature_datum_plane_headers(ctx, &container)?,
            feature_input_blocks(ctx, &container)?,
        ))
    })
    .expect("datum block use inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        DatumBlockUseRoute::Csys => {
            feature_datum_csys_block_uses(ctx, &constructions, &inputs).map(|rows| rows.len())
        }
        DatumBlockUseRoute::Plane => {
            feature_datum_plane_block_uses(ctx, &headers, &inputs).map(|rows| rows.len())
        }
    };
    assert!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted datum block use")
            > 0
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("datum block use resource limit")

})
}

macro_rules! datum_block_use_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = datum_block_use_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = datum_block_use_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $work() {
            let error = datum_block_use_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

datum_block_use_limit_tests!(
    datum_csys_block_use_refuses_collection_limit,
    datum_csys_block_use_refuses_retained_limit,
    datum_csys_block_use_refuses_work_limit,
    DatumBlockUseRoute::Csys
);
datum_block_use_limit_tests!(
    datum_plane_block_use_refuses_collection_limit,
    datum_plane_block_use_refuses_retained_limit,
    datum_plane_block_use_refuses_work_limit,
    DatumBlockUseRoute::Plane
);
