// SPDX-License-Identifier: Apache-2.0
//! `SP_CURVE` records and their bounded wrappers read by the carrier scan
//! (spec §7.6).

use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point2;

use super::{scan_carriers, CarrierIndex};
use crate::test_support::parasolid::{planar_b_curve as planar_curve, sp_curve_records};

fn scan(bytes: &[u8]) -> CarrierIndex {
    let ctx = cadmpeg_test_support::service_decode_context();
    scan_carriers(&ctx, bytes).expect("carrier scan")
}

/// The plane `z = 0` (attr 180) with `u` along `x`.
fn plane() -> Vec<u8> {
    crate::test_support::parasolid::plane_carrier(
        180,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    )
}

fn plane_stream(end: [f64; 3]) -> Vec<u8> {
    let mut bytes = plane();
    bytes.extend(planar_curve([0.0, 0.0], [0.001, 0.002], false));
    bytes.extend(sp_curve_records(180, [[0.0, 0.0, 0.0], end]));
    bytes
}

#[test]
fn a_plane_sp_curve_reads_its_parameters_in_millimetres() {
    // A plane's parameters are lengths, stored in metres (spec §7.1, §7.6).
    let carriers = scan(&plane_stream([0.001, 0.002, 0.0]));
    let carrier = carriers.surface_curve(75).expect("the bounded SP_CURVE");
    assert_eq!(carrier.surface, 180);
    assert_eq!(carrier.parameter_range, [0.0, 1.0]);
    let PcurveGeometry::Nurbs { nurbs } = &carrier.pcurve else {
        panic!("{:?}", carrier.pcurve);
    };
    let end = nurbs.control_points()[1].get();
    assert!(
        (end.u - 1.0).abs() < 1e-12 && (end.v - 2.0).abs() < 1e-12,
        "{end:?}"
    );
    // The bounded wrapper is no model-space curve.
    assert!(carriers.curve(75).is_none());
}

#[test]
fn a_bounded_sp_curve_whose_ends_are_off_its_support_does_not_resolve() {
    // 5e-8 m is 5e-5 mm, five times the admitted departure.
    let carriers = scan(&plane_stream([0.001, 0.002, 5.0e-8]));
    assert!(carriers.surface_curve(75).is_none());
    assert!(carriers.sp_curve(70).is_some());
}

#[test]
fn an_sp_curve_on_no_decoded_b_curve_is_not_read() {
    let mut bytes = plane();
    bytes.extend(sp_curve_records(180, [[0.0; 3], [0.001, 0.002, 0.0]]));
    let carriers = scan(&bytes);
    assert!(carriers.sp_curve(70).is_none());
    assert!(carriers.surface_curve(75).is_none());
}

#[test]
fn a_rational_planar_descriptor_is_a_pcurve() {
    // `dimension` 3 with the `rational` logical set is `u·w v·w w`.
    let mut bytes = plane();
    bytes.extend(planar_curve([0.0, 0.0], [0.001, 0.001], true));
    bytes.extend(sp_curve_records(180, [[0.0; 3], [0.001, 0.001, 0.0]]));
    let carriers = scan(&bytes);
    assert!(carriers.curve(60).is_none());
    let pcurve = &carriers.sp_curve(70).expect("the SP_CURVE").pcurve;
    let weights = pcurve
        .weights()
        .expect("a rational curve")
        .into_iter()
        .map(cadmpeg_ir::scalar::NonZeroReal::get)
        .collect::<Vec<_>>();
    assert_eq!(weights, [1.0, 2.0]);
    assert_eq!(pcurve.control_points()[1].get(), Point2::new(0.001, 0.001));
    assert!(carriers.surface_curve(75).is_some());
}

#[test]
fn a_two_dimensional_descriptor_marked_rational_is_not_read() {
    // A `dimension` 2 descriptor with the `rational` logical has no weight.
    let mut bytes = plane();
    let mut curve = planar_curve([0.0, 0.0], [0.001, 0.002], false);
    let descriptor = curve
        .windows(2)
        .position(|window| window == [0x00, 0x88])
        .expect("the descriptor");
    curve[descriptor + 2 + 17] = 1;
    bytes.extend(curve);
    bytes.extend(sp_curve_records(180, [[0.0; 3], [0.001, 0.002, 0.0]]));
    assert!(scan(&bytes).sp_curve(70).is_none());
}
