// SPDX-License-Identifier: Apache-2.0
//! Second moments, principal moments and axes, mass and inertia of closed
//! bodies, with closed-form expectations.

use std::f64::consts::PI;

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::{Category, UnavailableReason};
use crate::report::{BodyMeasures, MeasureReport, RowStatus};
use crate::select::Selection;
use crate::session::{Density, MeasureInput, MeasureOptions, Measurer};
use crate::test_support::{
    cuboid, cylinder_solid, service_context, Builder, Frame, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Measured, Quantity, Tensor};

/// Steel, in kilograms per cubic metre.
const STEEL: f64 = 7850.0;

/// Kilograms per cubic millimetre of steel.
const STEEL_PER_CUBIC_MILLIMETRE: f64 = STEEL / 1.0e9;

fn measure_with(ir: &CadIr, density: Option<f64>) -> MeasureReport {
    let ctx = service_context();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let options = MeasureOptions {
        density: density.map(|density| Density::new(density).expect("density")),
        ..MeasureOptions::default()
    };
    let report = Measurer::new(input, options)
        .measure(&ctx, &Selection::default())
        .expect("measurement completes");
    ctx.finish_session().expect("session stays unrefused");
    report
}

fn only_body(report: &MeasureReport) -> &BodyMeasures {
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body
}

/// `value` agrees with `expected` within the numeric error plus the test
/// floor relative to `max(|expected|, scale)`.
fn assert_close(value: f64, expected: f64, numeric: f64, scale: f64) {
    let bound = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
    assert!(
        (value - expected).abs() <= bound,
        "{value} differs from {expected} by more than {bound}"
    );
}

fn assert_tensor(quantity: &Quantity<Tensor>, expected: [f64; 6], scale: f64) {
    let measured: &Measured<Tensor> = quantity.measured().expect("a measured tensor");
    let value = measured.value();
    let entries = [value.xx, value.yy, value.zz, value.xy, value.yz, value.zx];
    for (entry, expected) in entries.into_iter().zip(expected) {
        assert_close(entry.get(), expected, measured.numeric_error().get(), scale);
    }
}

/// The box 2 × 3 × 4 from (1, 2, 3): `V = 24`, second moments about the
/// centroid `V/12 · diag(a², b², c²) = diag(8, 18, 32)`, and with density
/// `ρ` the mass `m = ρV` and `I_xx = m(b² + c²)/12`, `I_yy = m(c² + a²)/12`,
/// `I_zz = m(a² + b²)/12` with no products of inertia.
#[test]
fn a_box_has_closed_form_second_moments_inertia_and_axes() {
    let [a, b, c] = [2.0, 3.0, 4.0];
    let ir = cuboid([1.0, 2.0, 3.0], [1.0 + a, 2.0 + b, 3.0 + c]);
    let report = measure_with(&ir, Some(STEEL));
    let body = only_body(&report);
    let volume = a * b * c;
    let scale = volume * c * c;
    let moments = [a * a, b * b, c * c].map(|square| volume * square / 12.0);
    assert_tensor(
        body.second_moments().as_ref().expect("second moments"),
        [moments[0], moments[1], moments[2], 0.0, 0.0, 0.0],
        scale,
    );
    let principal = body
        .principal_moments()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("principal moments");
    for (value, expected) in principal.value().value.iter().zip(moments) {
        assert_close(
            value.get(),
            expected,
            principal.numeric_error().get(),
            scale,
        );
    }
    let axes = body
        .principal_axes()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("principal axes");
    let expected_axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for (axis, expected) in axes.value().value.iter().zip(expected_axes) {
        let axis = axis.get();
        for (component, expected) in [axis.x, axis.y, axis.z].into_iter().zip(expected) {
            assert_close(component, expected, axes.numeric_error().get(), 1.0);
        }
    }
    let mass = STEEL_PER_CUBIC_MILLIMETRE * volume;
    let measured_mass = body
        .mass()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("mass");
    assert_close(
        measured_mass.value().value.get(),
        mass,
        measured_mass.numeric_error().get(),
        mass,
    );
    assert_tensor(
        body.inertia().as_ref().expect("inertia"),
        [
            mass * (b * b + c * c) / 12.0,
            mass * (c * c + a * a) / 12.0,
            mass * (a * a + b * b) / 12.0,
            0.0,
            0.0,
            0.0,
        ],
        mass * c * c,
    );
    assert_eq!(*body.status(), RowStatus::Measured);
}

fn assert_degenerate_axes(body: &BodyMeasures) {
    let axes = body
        .principal_axes()
        .as_ref()
        .and_then(Quantity::unavailable)
        .expect("unavailable axes");
    assert_eq!(
        axes.reason(),
        UnavailableReason::MassPrincipalAxesDegenerate
    );
    assert_eq!(axes.category(), Category::Undefined);
}

/// The cylinder `r = 10`, `h = 30`: `I_zz = m r²/2`,
/// `I_xx = I_yy = m(3r² + h²)/12`; two equal principal moments.
#[test]
fn a_cylinder_has_closed_form_inertia_and_degenerate_axes() {
    let [radius, height] = [10.0, 30.0];
    let report = measure_with(&cylinder_solid(radius, height), Some(STEEL));
    let body = only_body(&report);
    let volume = PI * radius * radius * height;
    let mass = STEEL_PER_CUBIC_MILLIMETRE * volume;
    let transverse = mass * (3.0 * radius * radius + height * height) / 12.0;
    assert_tensor(
        body.inertia().as_ref().expect("inertia"),
        [
            transverse,
            transverse,
            mass * radius * radius / 2.0,
            0.0,
            0.0,
            0.0,
        ],
        mass * height * height,
    );
    let axial = volume * height * height / 12.0;
    let radial = volume * radius * radius / 4.0;
    assert_tensor(
        body.second_moments().as_ref().expect("second moments"),
        [radial, radial, axial, 0.0, 0.0, 0.0],
        volume * height * height,
    );
    assert_degenerate_axes(body);
    assert_eq!(*body.status(), RowStatus::Measured);
}

/// The ball of radius 5 about (1, 2, 3): every moment of inertia is
/// `2/5 · m R²`, every second moment `V R²/5`.
#[test]
fn a_ball_has_two_fifths_m_r_squared_and_degenerate_axes() {
    let radius = 5.0;
    let mut builder = Builder::new();
    let south = builder.vertex([1.0, 2.0, 3.0 - radius]);
    let north = builder.vertex([1.0, 2.0, 3.0 + radius]);
    let sphere = builder.sphere(Frame::z_up([1.0, 2.0, 3.0]), radius);
    builder.vertex_face(&sphere, Sense::Forward, &[south, north]);
    let report = measure_with(&builder.finish(BodyKind::Solid), Some(STEEL));
    let body = only_body(&report);
    let volume = 4.0 * PI * radius.powi(3) / 3.0;
    let mass = STEEL_PER_CUBIC_MILLIMETRE * volume;
    let moment = 2.0 * mass * radius * radius / 5.0;
    assert_tensor(
        body.inertia().as_ref().expect("inertia"),
        [moment, moment, moment, 0.0, 0.0, 0.0],
        moment,
    );
    let second = volume * radius * radius / 5.0;
    assert_tensor(
        body.second_moments().as_ref().expect("second moments"),
        [second, second, second, 0.0, 0.0, 0.0],
        second,
    );
    assert_degenerate_axes(body);
}

/// The cube of edge 10 without a density: second moments
/// `V a²/12 · I`, no mass and no inertia, degenerate axes.
#[test]
fn without_a_density_a_cube_has_second_moments_and_no_mass() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let report = measure_with(&cube, None);
    let body = only_body(&report);
    let second = 1000.0 * 100.0 / 12.0;
    assert_tensor(
        body.second_moments().as_ref().expect("second moments"),
        [second, second, second, 0.0, 0.0, 0.0],
        second,
    );
    assert!(body.mass().is_none());
    assert!(body.inertia().is_none());
    assert_degenerate_axes(body);
}

#[test]
fn an_unavailable_volume_leaves_the_mass_properties_unavailable() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    cube.model.faces[0].sense = match cube.model.faces[0].sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    };
    let report = measure_with(&cube, Some(STEEL));
    let body = only_body(&report);
    for reason in [
        body.second_moments()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        body.principal_moments()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        body.principal_axes()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        body.mass()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        body.inertia()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
    ] {
        assert_eq!(reason, Some(UnavailableReason::VolumeOrientationConflict));
    }
}

#[test]
fn a_density_is_finite_and_positive() {
    assert_eq!(Density::new(STEEL).map(Density::get), Some(STEEL));
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Density::new(invalid).is_none(), "{invalid}");
    }
}
