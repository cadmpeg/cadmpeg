// SPDX-License-Identifier: Apache-2.0
//! The document tolerances a part states: the Parasolid modeller precision of
//! its bodies.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::CadIr;

use cadmpeg_ir::topology::BodyKind;

use crate::loss::SldprtLossCode;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::{
    entity51, fixture_vertex_owner_class, owned_triangle_with_resolution, untyped_triangle,
};
use crate::SldprtCodec;

const MODELLER_DEFAULT_LINEAR_MM: f64 = 1.0e-5;
const MODELLER_ANGULAR_RADIANS: f64 = 1.0e-11;
const COMPARISON_SLACK: f64 = 1.0e-18;

/// The body count and the stated linear and angular tolerances of `body`.
fn tolerances(body: &[u8]) -> (usize, f64, f64) {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(body)),
            &DecodeOptions::default(),
        )
        .expect("synthetic part decodes");
    let ir = decoded.ir();
    (
        ir.geometry.bodies.len(),
        ir.tolerances.linear.get(),
        ir.tolerances.angular.get(),
    )
}

#[test]
fn bodies_with_two_stored_resolutions_state_the_larger() {
    let mut body = owned_triangle_with_resolution(0, 700, 0.0, 1.0e-8);
    body.extend(owned_triangle_with_resolution(200, 701, 2.0, 1.0e-6));
    let (bodies, linear, angular) = tolerances(&body);
    assert_eq!(bodies, 2);
    assert!((linear - 1.0e-3).abs() <= COMPARISON_SLACK, "{linear}");
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}

#[test]
fn faces_without_a_body_node_state_the_modeller_default() {
    let mut body = untyped_triangle(0.0);
    let typed_start = body
        .windows(3)
        .position(|window| window == [0x00, 0x0c, 0xff])
        .expect("typed ownership suffix");
    body.truncate(typed_start);
    body.extend(fixture_vertex_owner_class(900));
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[10, 0, 0, 0, 0, 0]));
    let (bodies, linear, angular) = tolerances(&body);
    assert_eq!(bodies, 1);
    assert!(
        (linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK,
        "{linear}"
    );
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}

#[test]
fn a_negative_stored_resolution_is_not_a_tolerance() {
    let body = owned_triangle_with_resolution(0, 700, 0.0, -1.0e-6);
    let (bodies, linear, angular) = tolerances(&body);
    assert_eq!(bodies, 1);
    assert!(
        (linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK,
        "{linear}"
    );
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .expect("synthetic part decodes");
    let code = SldprtLossCode::BodyResolutionInvalid.kind();
    let invalid = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code)
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let body = decoded.ir().geometry.bodies[0].id.as_str();
    assert!(
        matches!(invalid.as_slice(), [message] if message.contains(body)),
        "{invalid:?}"
    );
}

/// The losses of a write of `ir` without a source, and the linear tolerance
/// of the written part read back.
fn written_linear_tolerance(ir: &CadIr) -> (Vec<LossNote>, f64) {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("part plans");
    let losses = plan.report().losses.clone();
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("part writes");
    let read_back = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written part decodes");
    (losses, read_back.ir().tolerances.linear.get())
}

/// Whether `losses` charge the linear tolerance as a value the write cannot hold.
fn charges_linear_tolerance(losses: &[LossNote]) -> bool {
    losses.iter().any(|loss| {
        loss.code.local_code() == "value.unwritable" && loss.message.contains("tolerances.linear")
    })
}

#[test]
fn a_written_part_keeps_the_resolution_its_bodies_store() {
    let mut body = owned_triangle_with_resolution(0, 700, 0.0, 1.0e-7);
    body.extend(owned_triangle_with_resolution(200, 701, 2.0, 1.0e-8));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .expect("synthetic part decodes");
    let mut ir = decoded.into_parts().0;
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert!((ir.tolerances.linear.get() - 1.0e-4).abs() <= COMPARISON_SLACK);
    // The writer refuses a body or face name without a material.
    for body in &mut ir.geometry.bodies {
        body.name = None;
    }
    for face in &mut ir.geometry.faces {
        face.name = None;
    }
    let (losses, linear) = written_linear_tolerance(&ir);
    assert_eq!(linear, ir.tolerances.linear.get());
    assert!(!charges_linear_tolerance(&losses), "{losses:?}");
}

#[test]
fn a_linear_tolerance_with_no_positive_value_in_metres_writes_the_modeller_default() {
    let mut ir = crate::test_support::ir::source_less_cube();
    ir.tolerances.linear = PositiveLength::new(f64::from_bits(1)).expect("positive length");
    let (losses, linear) = written_linear_tolerance(&ir);
    assert!(charges_linear_tolerance(&losses), "{losses:?}");
    assert!(
        (linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK,
        "{linear}"
    );
}

/// A linear tolerance that no `res_linear` value states reads back at the
/// tolerance its nearest value states, with a charge.
#[test]
fn a_linear_tolerance_no_resolution_states_is_charged() {
    const LINEAR_MM: f64 = 6.1e-5;
    let mut ir = crate::test_support::ir::source_less_cube();
    ir.tolerances.linear = PositiveLength::new(LINEAR_MM).expect("positive length");
    let (losses, linear) = written_linear_tolerance(&ir);
    assert!(charges_linear_tolerance(&losses), "{losses:?}");
    assert_ne!(linear, LINEAR_MM);
    assert!((linear - LINEAR_MM).abs() <= COMPARISON_SLACK, "{linear}");
}

/// A part regenerated from its source writes, in each BODY, the
/// `res_linear` its bodies store. The stored value is one that the
/// millimetre tolerance divided by 1000 misses by one unit in the last place.
#[test]
fn a_regenerated_part_writes_the_resolution_its_bodies_store() {
    const STORED_RESOLUTION_METRES: f64 = 1.0e-9;
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&owned_triangle_with_resolution(
                0,
                700,
                0.0,
                STORED_RESOLUTION_METRES,
            ))),
            &DecodeOptions::default(),
        )
        .expect("synthetic part decodes");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let linear = decoded.ir().tolerances.linear;
    assert_ne!(
        linear.get() / 1000.0,
        STORED_RESOLUTION_METRES,
        "the fixture value converts exactly"
    );
    let kind = {
        let mut ir = decoded.ir_mut();
        let body = &mut ir.geometry.bodies[0];
        body.kind = match body.kind {
            BodyKind::Sheet => BodyKind::Solid,
            _ => BodyKind::Sheet,
        };
        body.kind
    };
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .expect("part writes");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let scan = crate::container::scan(&ctx, cadmpeg_core::decode::View::over_retained(&written))
        .expect("written part scans");
    assert_eq!(
        crate::decode::stored_linear_resolutions(&ctx, &scan).expect("written bodies decode"),
        [STORED_RESOLUTION_METRES]
    );
    let read_back = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written part decodes");
    assert_eq!(read_back.ir().geometry.bodies[0].kind, kind);
    assert_eq!(read_back.ir().tolerances.linear, linear);
}
