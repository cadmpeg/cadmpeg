// SPDX-License-Identifier: Apache-2.0

const SCALAR_IMAGE: &[u8] = &[0x46, 0, 0, 0, 0, 0, 0, 0];

#[test]
fn named_prototype_scalar_image_alone_yields_no_records() {
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::named_prototype_records(
            ctx,
            SCALAR_IMAGE,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
    })
    .expect("named prototype records")
    .is_empty());
}

#[test]
fn positional_parameter_scalar_image_without_rows_yields_no_records() {
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::parameter_records_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("parameter records")
    .is_empty());
}

#[test]
fn contour_scalar_image_without_rows_yields_no_records() {
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::contour_records_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("contour records")
    .is_empty());
}

#[test]
fn plane_local_system_scalar_image_without_rows_yields_no_records() {
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::plane_local_systems_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("plane local systems")
    .is_empty());
}
