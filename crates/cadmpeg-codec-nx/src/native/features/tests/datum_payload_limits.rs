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
    let source_bytes_1 = crate::test_support::test_prt::composed_feature_history_prt();

    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, &source_bytes_1)
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
    ($collection:ident, $retained:ident, $work:ident, $route:expr $(, $scoped:ident)?) => {
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
        $(#[test]
        fn $scoped() {
            let error = datum_payload_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        })?
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
    datum_csys_payload_refuses_work_limit,
    DatumPayloadRoute::Csys
);
datum_payload_limit_tests!(
    datum_plane_payload_refuses_collection_limit,
    datum_plane_payload_refuses_retained_limit,
    datum_plane_payload_refuses_work_limit,
    DatumPayloadRoute::Plane,
    datum_plane_payload_refuses_scoped_limit
);

#[test]
fn native_datum_plane_unique_lane_transfers_only_token_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let mut file = crate::test_support::test_prt::composed_feature_history_prt();
    let identity = b"0123456789abcde0123456789abcde0";
    let at = file
        .windows(identity.len())
        .position(|window| window == identity)
        .unwrap();
    let end = at + identity.len() + 1;
    file[end - 10..end].copy_from_slice(&[0x7f, 1, 2, 0x80, 7, 0, 0x12, 0x34, 0x56, 0x78]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .unwrap();
    let headers = crate::test_support::with_decode_context(|ctx| {
        let history = crate::native::features::FeatureHistory::new(ctx, &container)?;
        let inputs = crate::native::features::feature_input_blocks(ctx, &history)?;
        feature_datum_plane_headers(ctx, &history, &inputs)
    })
    .unwrap();
    let rows = crate::test_support::with_decode_context(|ctx| {
        feature_datum_plane_payloads(ctx, &container, &headers)
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    let lane = rows[0].index_lane.as_ref().unwrap();
    assert_eq!(lane.declared_count(), 2);
    assert_eq!(lane.trailer(), 0x1234_5678);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "NX datum index token storage",
        |ctx| feature_datum_plane_payloads(ctx, &container, &headers),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "NX datum index token storage"
            && limit.additional == cadmpeg_core::decode::u64_from_index(std::mem::size_of::<crate::om::compact::CompactIndexAtom>())));
}
