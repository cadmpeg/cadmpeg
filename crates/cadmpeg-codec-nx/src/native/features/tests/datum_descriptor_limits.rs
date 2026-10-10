// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_csys_descriptors;
use crate::native::features::feature_datum_plane_csys_identity_uses;
use crate::native::features::feature_datum_plane_descriptors;

#[derive(Clone, Copy)]
enum DatumDescriptorRoute {
    Csys,
    Plane,
    IdentityUse,
}

fn datum_descriptor_refusal(
    route: DatumDescriptorRoute,
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
            {
                let history = crate::native::features::FeatureHistory::new(ctx, &container)?;
                let inputs = crate::native::features::feature_input_blocks(ctx, &history)?;
                feature_datum_csys_constructions(ctx, &history, &inputs)
            }?,
            {
                let history = crate::native::features::FeatureHistory::new(ctx, &container)?;
                let inputs = crate::native::features::feature_input_blocks(ctx, &history)?;
                feature_datum_plane_headers(ctx, &history, &inputs)
            }?,
        ))
    })
    .expect("datum descriptor inputs");
    let (csys, plane) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_datum_csys_descriptors(ctx, &container, &constructions)?,
            feature_datum_plane_descriptors(ctx, &container, &headers)?,
        ))
    })
    .expect("admitted datum descriptors");
    assert!(!csys.is_empty());
    assert!(!plane.is_empty());
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        DatumDescriptorRoute::Csys => {
            feature_datum_csys_descriptors(ctx, &container, &constructions).map(|rows| rows.len())
        }
        DatumDescriptorRoute::Plane => {
            feature_datum_plane_descriptors(ctx, &container, &headers).map(|rows| rows.len())
        }
        DatumDescriptorRoute::IdentityUse => {
            feature_datum_plane_csys_identity_uses(ctx, &plane, &csys).map(|rows| rows.len())
        }
    };
    assert!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted datum descriptor route")
            > 0
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("datum descriptor resource limit"),
    )
}

macro_rules! datum_descriptor_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = datum_descriptor_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = datum_descriptor_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $work() {
            let error = datum_descriptor_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

datum_descriptor_limit_tests!(
    datum_csys_descriptor_refuses_collection_limit,
    datum_csys_descriptor_refuses_retained_limit,
    datum_csys_descriptor_refuses_work_limit,
    DatumDescriptorRoute::Csys
);
datum_descriptor_limit_tests!(
    datum_plane_descriptor_refuses_collection_limit,
    datum_plane_descriptor_refuses_retained_limit,
    datum_plane_descriptor_refuses_work_limit,
    DatumDescriptorRoute::Plane
);
datum_descriptor_limit_tests!(
    datum_identity_use_refuses_collection_limit,
    datum_identity_use_refuses_retained_limit,
    datum_identity_use_refuses_work_limit,
    DatumDescriptorRoute::IdentityUse
);
