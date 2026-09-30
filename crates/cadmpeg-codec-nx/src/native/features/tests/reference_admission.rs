// SPDX-License-Identifier: Apache-2.0

use crate::native::features::construction_records::feature_block_construction_references;
use crate::native::features::construction_records::feature_extrude_construction_profiles;
use crate::native::features::construction_records::feature_extrude_payload_32_branches;
use crate::native::features::construction_records::feature_extrude_payload_headers;
use crate::native::features::construction_records::feature_extrude_profile_references;
use crate::native::features::construction_records::feature_operation_body_11_continuations;
use crate::native::features::construction_records::feature_operation_body_members;
use crate::native::features::construction_records::feature_operation_body_reference_lanes;
use crate::native::features::construction_records::feature_operation_body_scalar_triples;
use crate::native::features::construction_records::feature_operation_terminal_discriminators;
use crate::native::features::construction_records::feature_point_construction_headers;
use crate::native::features::construction_records::feature_point_construction_scalar_lanes;
use crate::native::features::construction_records::feature_projected_curve_construction_payloads;
use crate::native::features::construction_records::feature_projected_curve_construction_strings;
use crate::native::features::construction_records::feature_projected_curve_references;
use crate::native::features::construction_records::feature_surface_construction_payloads;
use crate::native::features::construction_records::feature_surface_construction_references;
use crate::native::features::construction_records::feature_swp104_leading_branches;
use crate::native::features::construction_records::feature_thru_curve_construction_envelopes;
use crate::native::features::draft::feature_draft_construction_binary32_lanes;
use crate::native::features::draft::feature_draft_construction_fixed_lanes;
use crate::native::features::draft::feature_draft_construction_graph_payloads;
use crate::native::features::draft::feature_draft_construction_graph_strings;
use crate::native::features::draft::feature_draft_construction_identity_frames;
use crate::native::features::draft::feature_draft_construction_index_lanes;
use crate::native::features::draft::feature_draft_construction_payloads;
use crate::native::features::draft::feature_draft_construction_references;
use crate::native::features::draft::feature_draft_construction_terminal_lanes;
use crate::native::features::draft::FeatureDraftConstructionGraphPayload;
use crate::native::features::draft::FeatureDraftConstructionIndexLane;
use crate::native::features::draft::FeatureDraftConstructionReference;
use crate::native::features::feature_operation_labels;
use crate::native::features::FeatureExtrudeProfileReference;

fn reference_container(
    label: &'static str,
    payload: Vec<u8>,
) -> crate::container::Container<'static> {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], label, payload)],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic feature reference container")
}

fn projected_curve_container() -> crate::container::Container<'static> {
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    reference_container("CPROJ", payload)
}

fn point_header_container() -> crate::container::Container<'static> {
    reference_container("POINT",
        b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa".to_vec())
}

fn point_lane_container() -> crate::container::Container<'static> {
    let mut store = vec![vec![b'A']; 7311];
    let mut encoded = Vec::new();
    for value in [1.0_f64, -2.0, 3.5, 4.0, 5.25, -6.0] {
        encoded.extend_from_slice(&crate::test_support::test_bytes::shifted_f64_bytes(value));
    }
    let mut preceding = vec![0xaa, 0xbb];
    preceding.extend_from_slice(&encoded[..3]);
    store[7309] = preceding;
    let mut target = encoded[3..].to_vec();
    target.extend_from_slice(&[
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00, 0xcc,
    ]);
    store[7310] = target;
    let blocks = store.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa".to_vec();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "POINT", payload)],
        &blocks,
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic point scalar lane container")
}

fn swp104_container() -> crate::container::Container<'static> {
    let mut payload = vec![33, 0, 0, 1, 0];
    for _ in 0..4 {
        payload.extend([47, 164, 122, 225, 71, 174, 20, 123]);
    }
    payload.extend([35, 1, 2, 240, 1]);
    payload.extend([0; 5]);
    payload.extend([255, 1, 2, 241, 1, 0, 0]);
    reference_container("SWP104", payload)
}

#[derive(Clone, Copy)]
enum ExtrudeRoute {
    Profile,
    Header,
}

fn extrude_profile_join_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let references = [100_u32, 101].map(|object_index| FeatureExtrudeProfileReference {
        id: format!("profile-{object_index}"),
        operation_label: "operation".into(),
        ordinal: object_index - 100,
        field_tag: 0x16,
        witness_source_offset: Some(u64::from(object_index + 20)),
        token: crate::om::reference_index::PayloadIndexToken::from_wire(
            object_index,
            &[
                0xf0,
                u8::try_from(object_index).expect("small object index"),
            ],
        )
        .expect("payload index token"),
        data_block: Some(format!("block-{object_index}")),
        source_offset: u64::from(object_index),
    });
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_extrude_construction_profiles(ctx, &references)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted extrude profile join")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("extrude profile join resource limit"),
    )
}

fn extrude_32_branch_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let container = reference_container("EXTRUDE", bytes.to_vec());
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_extrude_payload_32_branches(ctx, &container)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted extrude 32 branches")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("extrude 32 branch resource limit"),
    )
}

fn block_reference_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let container = reference_container("BLOCK", payload);
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_block_construction_references(ctx, &container)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block references")
            .len(),
        19
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("block reference resource limit"),
    )
}

#[test]
fn block_reference_route_refuses_collection_limit() {
    let error = block_reference_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn block_reference_route_refuses_retained_limit() {
    let error = block_reference_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn block_reference_route_refuses_scoped_limit() {
    let error = block_reference_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn block_reference_route_refuses_work_limit() {
    let error = block_reference_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn extrude_32_branch_route_refuses_collection_limit() {
    let error = extrude_32_branch_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn extrude_32_branch_route_refuses_retained_limit() {
    let error = extrude_32_branch_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn extrude_32_branch_route_refuses_scoped_limit() {
    let error = extrude_32_branch_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn extrude_32_branch_route_refuses_work_limit() {
    let error = extrude_32_branch_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn extrude_profile_join_refuses_collection_limit() {
    let error = extrude_profile_join_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn extrude_profile_join_refuses_retained_limit() {
    let error = extrude_profile_join_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn extrude_profile_join_refuses_scoped_limit() {
    let error = extrude_profile_join_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn extrude_profile_join_refuses_work_limit() {
    let error = extrude_profile_join_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[derive(Clone, Copy)]
enum OperationLaneRoute {
    Terminal,
    ScalarTriple,
    BodyMember,
    Continuation,
    CompactReferences,
    ObjectReferences,
}

fn operation_lane_refusal(
    route: OperationLaneRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (label, bytes, expected) = match route {
        OperationLaneRoute::Terminal => ("EXTRUDE",
            b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00".as_slice(), 1),
        OperationLaneRoute::ScalarTriple => ("TRIM BODY",
            b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00".as_slice(), 2),
        OperationLaneRoute::BodyMember => ("SEW",
            b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00".as_slice(), 2),
        OperationLaneRoute::Continuation => ("TRIM BODY",
            b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00".as_slice(), 1),
        OperationLaneRoute::CompactReferences => ("OFFSET",
            b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00".as_slice(), 1),
        OperationLaneRoute::ObjectReferences => ("OFFSET",
            b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00".as_slice(), 1),
    };
    let container = reference_container(label, bytes.to_vec());
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        OperationLaneRoute::Terminal => {
            feature_operation_terminal_discriminators(ctx, &container).map(|rows| rows.len())
        }
        OperationLaneRoute::ScalarTriple => {
            feature_operation_body_scalar_triples(ctx, &container).map(|rows| rows.len())
        }
        OperationLaneRoute::BodyMember => {
            feature_operation_body_members(ctx, &container).map(|rows| rows.len())
        }
        OperationLaneRoute::Continuation => {
            feature_operation_body_11_continuations(ctx, &container).map(|rows| rows.len())
        }
        OperationLaneRoute::CompactReferences | OperationLaneRoute::ObjectReferences => {
            feature_operation_body_reference_lanes(ctx, &container).map(|rows| rows.len())
        }
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted operation lane"),
        expected
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("operation lane resource limit"),
    )
}

macro_rules! operation_lane_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = operation_lane_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = operation_lane_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $scoped() {
            let error = operation_lane_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }
        #[test]
        fn $work() {
            let error = operation_lane_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

operation_lane_limit_tests!(
    operation_terminal_refuses_collection_limit,
    operation_terminal_refuses_retained_limit,
    operation_terminal_refuses_scoped_limit,
    operation_terminal_refuses_work_limit,
    OperationLaneRoute::Terminal
);
operation_lane_limit_tests!(
    operation_scalar_triple_refuses_collection_limit,
    operation_scalar_triple_refuses_retained_limit,
    operation_scalar_triple_refuses_scoped_limit,
    operation_scalar_triple_refuses_work_limit,
    OperationLaneRoute::ScalarTriple
);
operation_lane_limit_tests!(
    operation_body_member_refuses_collection_limit,
    operation_body_member_refuses_retained_limit,
    operation_body_member_refuses_scoped_limit,
    operation_body_member_refuses_work_limit,
    OperationLaneRoute::BodyMember
);
operation_lane_limit_tests!(
    operation_body_continuation_refuses_collection_limit,
    operation_body_continuation_refuses_retained_limit,
    operation_body_continuation_refuses_scoped_limit,
    operation_body_continuation_refuses_work_limit,
    OperationLaneRoute::Continuation
);
operation_lane_limit_tests!(
    operation_compact_reference_refuses_collection_limit,
    operation_compact_reference_refuses_retained_limit,
    operation_compact_reference_refuses_scoped_limit,
    operation_compact_reference_refuses_work_limit,
    OperationLaneRoute::CompactReferences
);
operation_lane_limit_tests!(
    operation_object_reference_refuses_collection_limit,
    operation_object_reference_refuses_retained_limit,
    operation_object_reference_refuses_scoped_limit,
    operation_object_reference_refuses_work_limit,
    OperationLaneRoute::ObjectReferences
);

fn extrude_route_refusal(
    route: ExtrudeRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let payload = match route {
        ExtrudeRoute::Profile =>
            b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00".to_vec(),
        ExtrudeRoute::Header =>
            b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb".to_vec(),
    };
    let container = reference_container("EXTRUDE", payload);
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        ExtrudeRoute::Profile => {
            feature_extrude_profile_references(ctx, &container).map(|rows| rows.len())
        }
        ExtrudeRoute::Header => {
            feature_extrude_payload_headers(ctx, &container).map(|rows| rows.len())
        }
    };
    let expected = match route {
        ExtrudeRoute::Profile => 2,
        ExtrudeRoute::Header => 1,
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted extrude route"),
        expected
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("extrude route resource limit"),
    )
}

macro_rules! extrude_route_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = extrude_route_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = extrude_route_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $scoped() {
            let error = extrude_route_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }
        #[test]
        fn $work() {
            let error = extrude_route_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

extrude_route_limit_tests!(
    extrude_profile_refuses_collection_limit,
    extrude_profile_refuses_retained_limit,
    extrude_profile_refuses_scoped_limit,
    extrude_profile_refuses_work_limit,
    ExtrudeRoute::Profile
);
extrude_route_limit_tests!(
    extrude_header_refuses_collection_limit,
    extrude_header_refuses_retained_limit,
    extrude_header_refuses_scoped_limit,
    extrude_header_refuses_work_limit,
    ExtrudeRoute::Header
);

fn swp104_branch_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = swp104_container();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_swp104_leading_branches(ctx, &container)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted SWP104 branches")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("SWP104 branch resource limit"),
    )
}

#[test]
fn swp104_branch_route_refuses_collection_limit() {
    let error = swp104_branch_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn swp104_branch_route_refuses_retained_limit() {
    let error = swp104_branch_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn swp104_branch_route_refuses_scoped_limit() {
    let error = swp104_branch_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn swp104_branch_route_refuses_work_limit() {
    let error = swp104_branch_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn point_lane_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = point_lane_container();
    let headers = crate::test_support::with_decode_context(|ctx| {
        feature_point_construction_headers(ctx, &container)
    })
    .expect("point construction headers");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_point_construction_scalar_lanes(ctx, &container, &headers)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted point scalar lanes")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("point scalar lane resource limit"),
    )
}

#[test]
fn point_lane_refuses_collection_limit() {
    let error = point_lane_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn point_lane_refuses_retained_limit() {
    let error = point_lane_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn point_lane_refuses_work_limit() {
    let error = point_lane_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn point_header_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = point_header_container();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_point_construction_headers(ctx, &container)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted point headers")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("point header resource limit"),
    )
}

#[test]
fn point_header_refuses_collection_limit() {
    let error = point_header_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn point_header_refuses_retained_limit() {
    let error = point_header_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn point_header_refuses_scoped_limit() {
    let error = point_header_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn point_header_refuses_work_limit() {
    let error = point_header_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn projected_curve_payload_container() -> crate::container::Container<'static> {
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    let mut store = (0..715).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    store[712] = b"\x66\x32\x03\x05ABC\0";
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "CPROJ", payload)],
        &store,
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic projected curve payload container")
}

fn projected_curve_payload_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = projected_curve_payload_container();
    let (labels, references) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_operation_labels(ctx, &container)?,
            feature_projected_curve_references(ctx, &container)?,
        ))
    })
    .expect("projected curve payload inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_projected_curve_construction_payloads(ctx, &container, &labels, &references)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted projected curve payloads")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("projected curve payload resource limit"),
    )
}

fn projected_curve_string_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = projected_curve_payload_container();
    let payloads = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(ctx, &container)?;
        let references = feature_projected_curve_references(ctx, &container)?;
        feature_projected_curve_construction_payloads(ctx, &container, &labels, &references)
    })
    .expect("projected curve construction payloads");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_projected_curve_construction_strings(ctx, &container, &payloads)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted projected curve strings")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("projected curve string resource limit"),
    )
}

#[test]
fn projected_curve_string_refuses_collection_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_string_refuses_retained_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_string_refuses_scoped_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_string_refuses_work_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn projected_curve_payload_refuses_collection_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_payload_refuses_retained_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_payload_refuses_scoped_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_payload_refuses_work_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn surface_container() -> crate::container::Container<'static> {
    reference_container("SKIN", surface_payload_bytes())
}

pub(super) fn surface_payload_bytes() -> Vec<u8> {
    b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02".to_vec()
}

fn surface_payload_container() -> crate::container::Container<'static> {
    let store = (0..600).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "SKIN", surface_payload_bytes())],
        &store,
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic surface payload container")
}

fn draft_container() -> crate::container::Container<'static> {
    reference_container("DRAFT", draft_payload_bytes())
}

fn draft_payload_bytes() -> Vec<u8> {
    let mut payload = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49".to_vec();
    payload.extend_from_slice(b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff");
    payload.extend_from_slice(
        b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00",
    );
    payload
}

fn draft_index_container() -> crate::container::Container<'static> {
    let store = (0..7039).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "DRAFT", draft_payload_bytes())],
        &store,
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic draft index container")
}

fn thru_curve_container() -> crate::container::Container<'static> {
    reference_container("THRU_CURVE", b"\x13\x00\x00\x01\x00\xf1\x01\x21\xf1\x01\x22\xf1\x01\x23\x01\x08\x02\x03\x03\x04\x01\x01\x01\x01\x07\xf1\x01\x24\xf1\x01\x25\xf1\x01\x26\xf1\x01\x27\xf1\x01\x28\xf1\x01\x29\x04\x01\xa0\x5e\x38\x13\x01\x03".to_vec())
}

fn projected_curve_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = projected_curve_container();
    let records = crate::test_support::with_decode_context(|ctx| {
        feature_projected_curve_references(ctx, &container)
    })
    .expect("admitted projected curve references");
    assert_eq!(records.len(), 3);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_projected_curve_references(ctx, &container)
                .expect_err("projected curve reference resource limit")
        },
    )
}

#[derive(Clone, Copy)]
enum ReferenceRoute {
    Surface,
    Draft,
    ThruCurveEnvelope,
}

fn reference_route_refusal(
    container: &crate::container::Container<'static>,
    expected_count: usize,
    route: ReferenceRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let call = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        ReferenceRoute::Surface => {
            feature_surface_construction_references(ctx, container).map(|records| records.len())
        }
        ReferenceRoute::Draft => {
            feature_draft_construction_references(ctx, container).map(|records| records.len())
        }
        ReferenceRoute::ThruCurveEnvelope => {
            feature_thru_curve_construction_envelopes(ctx, container).map(|records| records.len())
        }
    };
    let records =
        crate::test_support::with_decode_context(call).expect("admitted feature references");
    assert_eq!(records, expected_count);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| call(ctx).expect_err("feature reference resource limit"),
    )
}

#[test]
fn surface_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(
        &surface_container(),
        14,
        ReferenceRoute::Surface,
        |policy| policy.limits.max_collection_items = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn surface_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(
        &surface_container(),
        14,
        ReferenceRoute::Surface,
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn surface_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(
        &surface_container(),
        14,
        ReferenceRoute::Surface,
        |policy| policy.limits.max_materialized_bytes = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn surface_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(
        &surface_container(),
        14,
        ReferenceRoute::Surface,
        |policy| policy.limits.max_work_units = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn draft_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(&draft_container(), 4, ReferenceRoute::Draft, |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(&draft_container(), 4, ReferenceRoute::Draft, |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(&draft_container(), 4, ReferenceRoute::Draft, |policy| {
        policy.limits.max_materialized_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(&draft_container(), 4, ReferenceRoute::Draft, |policy| {
        policy.limits.max_work_units = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn thru_curve_envelope_route_refuses_collection_limit() {
    let error = reference_route_refusal(
        &thru_curve_container(),
        1,
        ReferenceRoute::ThruCurveEnvelope,
        |policy| policy.limits.max_collection_items = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn thru_curve_envelope_route_refuses_retained_limit() {
    let error = reference_route_refusal(
        &thru_curve_container(),
        1,
        ReferenceRoute::ThruCurveEnvelope,
        |policy| policy.limits.max_retained_bytes = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn thru_curve_envelope_route_refuses_scoped_limit() {
    let error = reference_route_refusal(
        &thru_curve_container(),
        1,
        ReferenceRoute::ThruCurveEnvelope,
        |policy| policy.limits.max_materialized_bytes = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn thru_curve_envelope_route_refuses_work_limit() {
    let error = reference_route_refusal(
        &thru_curve_container(),
        1,
        ReferenceRoute::ThruCurveEnvelope,
        |policy| policy.limits.max_work_units = 0,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn surface_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = surface_payload_container();
    let references = crate::test_support::with_decode_context(|ctx| {
        feature_surface_construction_references(ctx, &container)
    })
    .expect("admitted surface references");
    assert_eq!(references.len(), 14);
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_surface_construction_payloads(ctx, &container, &references)
    })
    .expect("admitted surface payload");
    assert_eq!(payloads.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_surface_construction_payloads(ctx, &container, &references)
                .expect_err("surface payload resource limit")
        },
    )
}

fn draft_resolved_lane() -> FeatureDraftConstructionIndexLane {
    serde_json::from_str(
        r#"{"id":"nx:feature-history:draft-construction-index-lane#0-0000000000","operation_label":"nx:feature-history:operation-label#0-0000000000","declared_count":3,"indices":[1,2],"raw_indices":[[1],[2]],"data_blocks":["nx:om-data-blocks-0:block#1","nx:om-data-blocks-0:block#2"],"source_offsets":[24,25]}"#,
    ).expect("resolved draft lane")
}

fn draft_small_store_container() -> crate::container::Container<'static> {
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &[b"A", b"B"]);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic draft payload container")
}

fn draft_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let lane = draft_resolved_lane();
    let container = draft_small_store_container();
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_payloads(ctx, &container, std::slice::from_ref(&lane))
    })
    .expect("admitted draft payload");
    assert_eq!(payloads.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_payloads(ctx, &container, &[lane])
                .expect_err("draft payload resource limit")
        },
    )
}

#[test]
fn draft_payload_route_refuses_collection_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_payload_route_refuses_retained_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_payload_route_refuses_scoped_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_payload_route_refuses_work_limit() {
    let error = draft_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_graph_references() -> Vec<FeatureDraftConstructionReference> {
    (0..4).map(|ordinal| {
        let index = if ordinal == 0 { 1 } else { 2 };
        serde_json::from_value(serde_json::json!({
            "id": format!("nx:feature-history:draft-construction-reference#0-0000000000-{ordinal:010}"),
            "operation_label": "nx:feature-history:operation-label#0-0000000000",
            "ordinal": ordinal,
            "object_index": index,
            "raw_object_index": [240, index],
            "data_block": format!("nx:om-data-blocks-0:block#{index}"),
            "source_offset": 100 + ordinal,
        })).expect("resolved draft graph reference")
    }).collect()
}

fn draft_graph_fixture_with_content(
    bytes: &[u8],
) -> (
    crate::container::Container<'static>,
    FeatureDraftConstructionGraphPayload,
) {
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &[bytes, b""]);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    })
    .expect("synthetic draft graph content container");
    let lane = draft_resolved_lane();
    let references = draft_graph_references();
    let mut payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_graph_payloads(ctx, &container, &[lane], &references)
    })
    .expect("admitted draft graph content");
    assert_eq!(payloads.len(), 1);
    (container, payloads.remove(0))
}

fn draft_fixed_bytes() -> Vec<u8> {
    let discriminator = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes
}

fn draft_fixed_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, payload) = draft_graph_fixture_with_content(&draft_fixed_bytes());
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_fixed_lanes(ctx, &container, std::slice::from_ref(&payload))
    })
    .expect("admitted draft fixed lane");
    assert_eq!(lanes.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_fixed_lanes(ctx, &container, &[payload])
                .expect_err("draft fixed lane resource limit")
        },
    )
}

#[test]
fn draft_fixed_route_refuses_collection_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_fixed_route_refuses_retained_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_fixed_route_refuses_scoped_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_fixed_route_refuses_work_limit() {
    let error = draft_fixed_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_binary32_bytes() -> Vec<u8> {
    let discriminator = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x4f, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0xcf, 0x80, 0, 0]);
    bytes.push(0);
    bytes
}

fn draft_binary32_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, payload) = draft_graph_fixture_with_content(&draft_binary32_bytes());
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_binary32_lanes(ctx, &container, std::slice::from_ref(&payload))
    })
    .expect("admitted draft binary32 lane");
    assert_eq!(lanes.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_binary32_lanes(ctx, &container, &[payload])
                .expect_err("draft binary32 lane resource limit")
        },
    )
}

#[test]
fn draft_binary32_route_refuses_collection_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_binary32_route_refuses_retained_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_binary32_route_refuses_scoped_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_binary32_route_refuses_work_limit() {
    let error = draft_binary32_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_string_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, payload) = draft_graph_fixture_with_content(b"\x66\x32\x03\x03A\0");
    let strings = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_graph_strings(ctx, &container, std::slice::from_ref(&payload))
    })
    .expect("admitted draft graph string");
    assert_eq!(strings.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_graph_strings(ctx, &container, &[payload])
                .expect_err("draft graph string resource limit")
        },
    )
}

#[test]
fn draft_string_route_refuses_collection_limit() {
    let error = draft_string_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_string_route_refuses_retained_limit() {
    let error = draft_string_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_string_route_refuses_scoped_limit() {
    let error = draft_string_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_string_route_refuses_work_limit() {
    let error = draft_string_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_identity_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x00A\x81\x54\xf0\x38\x02\x01abc123?A\xf0\x27\xff\x02\x01def456?\x00";
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &[bytes, b""]);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    })
    .expect("synthetic draft identity container");
    let lane = draft_resolved_lane();
    let mut payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_payloads(ctx, &container, &[lane])
    })
    .expect("admitted draft construction payload");
    assert_eq!(payloads.len(), 1);
    let payload = payloads.remove(0);
    let frames = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_identity_frames(ctx, &container, std::slice::from_ref(&payload))
    })
    .expect("admitted draft identity frames");
    assert_eq!(frames.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_identity_frames(ctx, &container, &[payload])
                .expect_err("draft identity frame resource limit")
        },
    )
}

#[test]
fn draft_identity_route_refuses_collection_limit() {
    let error = draft_identity_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_identity_route_refuses_retained_limit() {
    let error = draft_identity_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_identity_route_refuses_scoped_limit() {
    let error = draft_identity_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_identity_route_refuses_work_limit() {
    let error = draft_identity_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_terminal_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = draft_container();
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_terminal_lanes(ctx, &container)
    })
    .expect("admitted draft terminal lane");
    assert_eq!(lanes.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_terminal_lanes(ctx, &container)
                .expect_err("draft terminal lane resource limit")
        },
    )
}

#[test]
fn draft_terminal_route_refuses_collection_limit() {
    let error = draft_terminal_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_terminal_route_refuses_retained_limit() {
    let error = draft_terminal_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_terminal_route_refuses_scoped_limit() {
    let error = draft_terminal_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_terminal_route_refuses_work_limit() {
    let error = draft_terminal_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_index_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = draft_index_container();
    let lanes = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_index_lanes(ctx, &container)
    })
    .expect("admitted draft index lane");
    assert_eq!(lanes.len(), 1);
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_payloads(ctx, &container, &lanes)
    })
    .expect("resolved draft index target blocks");
    assert_eq!(payloads.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_index_lanes(ctx, &container)
                .expect_err("draft index lane resource limit")
        },
    )
}

#[test]
fn draft_index_route_refuses_collection_limit() {
    let error = draft_index_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_index_route_refuses_retained_limit() {
    let error = draft_index_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_index_route_refuses_scoped_limit() {
    let error = draft_index_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_index_route_refuses_work_limit() {
    let error = draft_index_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn draft_graph_payload_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let lane = draft_resolved_lane();
    let references = draft_graph_references();
    let container = draft_small_store_container();
    let payloads = crate::test_support::with_decode_context(|ctx| {
        feature_draft_construction_graph_payloads(
            ctx,
            &container,
            std::slice::from_ref(&lane),
            &references,
        )
    })
    .expect("admitted draft graph payload");
    assert_eq!(payloads.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_draft_construction_graph_payloads(ctx, &container, &[lane], &references)
                .expect_err("draft graph payload resource limit")
        },
    )
}

#[test]
fn draft_graph_payload_route_refuses_collection_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn draft_graph_payload_route_refuses_retained_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn draft_graph_payload_route_refuses_scoped_limit() {
    let error =
        draft_graph_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn draft_graph_payload_route_refuses_work_limit() {
    let error = draft_graph_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn surface_payload_route_refuses_collection_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn surface_payload_route_refuses_retained_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn surface_payload_route_refuses_scoped_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn surface_payload_route_refuses_work_limit() {
    let error = surface_payload_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn projected_curve_reference_route_refuses_collection_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_reference_route_refuses_retained_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_reference_route_refuses_scoped_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_reference_route_refuses_work_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
