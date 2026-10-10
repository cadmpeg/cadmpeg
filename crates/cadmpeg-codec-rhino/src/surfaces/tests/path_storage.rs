// SPDX-License-Identifier: Apache-2.0
use super::super::extrusion_nurbs;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::FiniteVector;

const INPUT_BYTES: u64 = cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<f64>());
const PROFILE_OUTPUT_BYTES: u64 = cadmpeg_core::decode::u64_from_index(
    2 * std::mem::size_of::<Vec<FinitePoint3>>()
        + 4 * std::mem::size_of::<FinitePoint3>()
        + 4 * std::mem::size_of::<f64>(),
);

fn surface(ctx: &DecodeContext<'_>) -> Result<NurbsSurface, crate::curves::GeometryError> {
    let (start, end) = super::simple_extrusion_curves();
    extrusion_nurbs(
        ctx,
        &start,
        &end,
        FiniteVector::new([2.0, 5.0]).expect("finite path domain"),
        false,
        0,
    )
}

fn refusal(dimension: ResourceDimension, cap: u64, operation: &str, used: u64, additional: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
        _ => panic!("storage dimension"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        surface(&ctx).expect_err("the real path buffer exceeds its selected dimension")
    else {
        panic!("path storage resource refusal");
    };
    assert_eq!(limit.dimension, dimension);
    assert_eq!(limit.operation, operation);
    assert_eq!(
        (limit.limit, limit.used, limit.additional),
        (cap, used, additional)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn extrusion_path_input_refuses_before_its_four_slot_allocation() {
    // Two row slots, four pole slots and four copied profile knots precede this input.
    refusal(
        ResourceDimension::CollectionItems,
        10,
        "Rhino extrusion path knot input",
        10,
        4,
    );
}

#[test]
fn extrusion_path_input_refuses_one_byte_below_actual_backing() {
    refusal(
        ResourceDimension::MaterializedBytes,
        INPUT_BYTES - 1,
        "Rhino extrusion path knot input",
        0,
        INPUT_BYTES,
    );
}

#[test]
fn extrusion_path_output_preserves_the_original_retained_refusal() {
    refusal(
        ResourceDimension::RetainedBytes,
        PROFILE_OUTPUT_BYTES + INPUT_BYTES - 1,
        "IR finite knot values",
        PROFILE_OUTPUT_BYTES,
        INPUT_BYTES,
    );
}

#[test]
fn extrusion_path_input_releases_after_conversion_and_output_stays_retained() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = INPUT_BYTES;
    policy.limits.max_retained_bytes = PROFILE_OUTPUT_BYTES + INPUT_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let value = surface(&ctx).expect("actual input and output backing fit exactly");
    assert_eq!(value.v_knots().as_slice(), &[2.0, 2.0, 5.0, 5.0]);
    assert_eq!((value.u_count(), value.v_count()), (2, 2));
    assert_eq!(
        value.poles().into_iter().nth(3).expect("four tensor poles"),
        Point3::new(1.0, 0.0, 1.0)
    );
    let reuse = ctx
        .reserve_scoped(INPUT_BYTES, "path input release control")
        .expect("consumed finite input was destroyed before its reservation released");
    drop(reuse);
    let CodecError::ResourceLimit(limit) = ctx
        .vector_storage::<f64>(1, "path output control")
        .expect_err("the returned surface still occupies retained backing")
    else {
        panic!("retained output control");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (limit.used, limit.additional),
        (
            PROFILE_OUTPUT_BYTES + INPUT_BYTES,
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<f64>())
        )
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert_eq!(value.v_knots().as_slice(), &[2.0, 2.0, 5.0, 5.0]);
}
