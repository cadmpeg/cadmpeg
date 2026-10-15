// SPDX-License-Identifier: Apache-2.0
use super::super::{
    historical_face_id, historical_face_selection_with_native, historical_profile_native_group,
    resolved_extrude_profile_face_group, resolved_loft_edge_profile_group,
    resolved_profile_face_group,
};
use super::{edge_operand, loft_group, loft_scope, start_geometry_fixture};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FaceSelection, PlanarProfileRef, ProfileRef};

const NATIVE_COPY: &str = "test historical native input";

fn assert_refusal(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn profile_native_transfers_owned_buffer_under_one_copy_retained_limit() {
    let source = format!("native:{}", "x".repeat(16_384));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(source.len() + std::mem::size_of::<String>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let input = ctx.copy_retained_text(&source, NATIVE_COPY).unwrap();
        let original_buffer = input.as_ptr();
        let native = NonBlankString::from_ascii_leading(input).unwrap();
        let result = historical_profile_native_group(ctx, native).unwrap();
        assert_eq!(result.as_slice(), [source]);
        assert!(std::ptr::eq(result[0].as_ptr(), original_buffer));
    });
}

#[test]
fn profile_native_copies_static_borrow_through_budget() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = historical_profile_native_group(ctx, cadmpeg_core::nonblank_literal!("native"))
            .unwrap_err();
        assert_refusal(
            &error,
            ResourceDimension::RetainedBytes,
            "f3d historical profile native id",
        );
    });
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            historical_profile_native_group(ctx, cadmpeg_core::nonblank_literal!("native"))
                .unwrap()
                .as_slice(),
            ["native"]
        );
    });
}

#[test]
fn profile_native_group_and_validation_refuse_each_owned_storage_dimension() {
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "f3d historical profile native group",
        ),
        (
            ResourceDimension::RetainedBytes,
            "f3d historical profile native group",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d historical profile native validation",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d historical profile native validation",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d historical profile native validation",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            let text = ctx.copy_retained_text("native", NATIVE_COPY)?;
            historical_profile_native_group(ctx, NonBlankString::from_ascii_leading(text).unwrap())
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn successful_historical_selection_keeps_one_native_copy_and_same_state_faces() {
    let native = format!("native:{}", "x".repeat(16_384));
    let scope = loft_scope();
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 6);
    let mut policy = DecodePolicy::service();
    // The native allocation and all state/member storage fit; a second native copy cannot fit.
    policy.limits.max_retained_bytes = u64::try_from(native.len() + 4096).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let result =
            historical_face_selection_with_native(ctx, &scope, 6, &[10, 11], &native, NATIVE_COPY)
                .unwrap()
                .unwrap();
        let FaceSelection::Historical {
            state,
            faces,
            native: result_native,
        } = result
        else {
            panic!("expected historical selection");
        };
        assert_eq!(state, crate::ids::feature_input_topology_id(&feature, 6));
        assert_eq!(
            faces.as_slice(),
            [
                crate::ids::history_input_face_id(&prefix, 10),
                crate::ids::history_input_face_id(&prefix, 11),
            ]
        );
        assert_eq!(result_native.as_str(), native);
    });
}

#[test]
fn rejected_historical_members_and_blank_native_keep_original_fallback() {
    let scope = loft_scope();
    for (faces, native) in [(&[10, 10][..], "native"), (&[10][..], " \t\n")] {
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                historical_face_selection_with_native(ctx, &scope, 6, faces, native, NATIVE_COPY)
                    .unwrap(),
                Some(FaceSelection::Native(native.into()))
            );
        });
    }
}

#[test]
fn rejected_historical_fallback_refuses_retained_copy() {
    let scope = loft_scope();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "f3d historical face fallback id",
        0,
        |ctx| {
            historical_face_selection_with_native(ctx, &scope, 6, &[10, 10], "native", NATIVE_COPY)
        },
    );
    assert_refusal(
        &error,
        ResourceDimension::RetainedBytes,
        "f3d historical face fallback id",
    );
}

#[test]
fn historical_member_validation_refusal_propagates_without_native_fallback() {
    let scope = loft_scope();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate FaceSelection historical members",
        0,
        |ctx| historical_face_selection_with_native(ctx, &scope, 6, &[10], "native", NATIVE_COPY),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "validate FaceSelection historical members",
    );
}

#[test]
fn empty_historical_slots_skip_native_copy_and_identity_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let scope = loft_scope();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            historical_face_selection_with_native(ctx, &scope, 6, &[], "native", NATIVE_COPY)
                .unwrap(),
            None
        );
    });
}

#[test]
fn historical_slot_source_and_variable_prefix_refuse_work() {
    let scope = loft_scope();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d historical face slot source",
        0,
        |ctx| historical_face_selection_with_native(ctx, &scope, 6, &[10], "native", NATIVE_COPY),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d historical face slot source",
    );
    let prefix = cadmpeg_ir::ids::IdentityKey::try_from("x".repeat(16_384)).unwrap();
    let expected = crate::ids::history_input_face_id(&prefix, i64::MIN);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            historical_face_id(ctx, &prefix, i64::MIN).unwrap(),
            expected
        );
    });
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d historical face id",
        0,
        |ctx| historical_face_id(ctx, &prefix, i64::MIN),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d historical face id",
    );
}

fn historical_profile(
    route: usize,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<ProfileRef>, CodecError> {
    let scope = loft_scope();
    if route == 2 {
        return resolved_loft_edge_profile_group(
            ctx,
            &scope,
            &loft_group(),
            &[
                edge_operand(1, 100, 0, 3, 200),
                edge_operand(2, 100, 1, 3, 201),
                edge_operand(3, 100, 0, 3, 202),
            ],
        );
    }
    let (mut operand, mut group, _) = start_geometry_fixture();
    operand.resolved_face_slots = vec![10];
    if route == 0 {
        resolved_profile_face_group(ctx, &scope, &group, std::slice::from_ref(&operand))
    } else {
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        resolved_extrude_profile_face_group(
            ctx,
            &scope,
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
        )
    }
}

#[test]
fn all_historical_profile_routes_keep_same_native_and_refuse_output_group() {
    for route in 0..3 {
        let expected_native = if route == 2 {
            loft_group().id
        } else {
            start_geometry_fixture().1.id
        };
        crate::test_support::with_decode_context(|ctx| {
            let Some(ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
                native, faces, ..
            })) = historical_profile(route, ctx).unwrap()
            else {
                panic!("expected historical profile");
            };
            assert_eq!(native.as_slice(), [expected_native]);
            assert_eq!(faces.len(), 1);
        });
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
        ] {
            let error = crate::test_support::resource_refusal_at(
                dimension,
                "f3d historical profile native group",
                0,
                |ctx| historical_profile(route, ctx),
            );
            assert_refusal(&error, dimension, "f3d historical profile native group");
        }
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "f3d historical profile native validation",
            0,
            |ctx| historical_profile(route, ctx),
        );
        assert_refusal(
            &error,
            ResourceDimension::WorkUnits,
            "f3d historical profile native validation",
        );
    }
}
