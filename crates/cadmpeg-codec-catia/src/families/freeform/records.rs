//! Freeform surface-carrier vocabulary.
//!
//! Collects a5a8/b2 surface carriers and defines the consolidated-carrier key,
//! parametric chart, and the jet-pcurve and rolling-ball derivative
//! constructors the freeform pools build from those carrier records.

use cadmpeg_ir::geometry::{PcurveGeometry, RollingBallJetDerivative, SurfaceGeometry};
use cadmpeg_ir::math::Vector3;

use crate::assemble::quintic_jet_pcurve;

pub(crate) fn freeform_surface_carriers(
    data: &[u8],
) -> Vec<(usize, u32, SurfaceGeometry, &'static str)> {
    let mut surfaces: Vec<(usize, u32, SurfaceGeometry, &str)> =
        crate::families::a5a8::records::resolved_a8_surfaces(data)
            .into_iter()
            .chain(crate::families::a5a8::records::a5_surfaces(data))
            .map(|surface| (surface.pos, surface.object_id, surface.geometry, "freeform"))
            .collect();
    surfaces.extend(
        crate::families::b2::records::b2_cylinders(data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .geometry
                    .map(|geometry| (surface.pos, 0, geometry, "b2_03_28"))
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_embedded_cylinders(data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .cylinder
                    .geometry
                    .map(|geometry| (surface.pos, surface.object_id, geometry, "b2_03_60"))
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_cones(data)
            .into_iter()
            .map(|surface| {
                (
                    surface.pos,
                    0,
                    crate::families::b2::records::b2_cone_geometry(&surface),
                    "b2_03_29",
                )
            }),
    );
    surfaces
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConsolidatedCarrierKey {
    Cylinder(usize),
    EmbeddedCylinder(usize),
    Cone(usize),
    NurbsOffset(usize, u64),
}

pub(crate) enum ConsolidatedCarrierChart<'a> {
    Identity,
    Cylinder {
        radius: f64,
    },
    Cone {
        cone: &'a crate::families::b2::records::B2Cone,
    },
}

impl ConsolidatedCarrierChart<'_> {
    fn point(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [
                u / cone.angular_scale,
                (v - cone.slant_range[0]) * cone.half_angle.cos(),
            ],
        }
    }

    fn derivative(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [u / cone.angular_scale, v * cone.half_angle.cos()],
        }
    }
}

pub(crate) fn consolidated_jet_pcurve(
    pcurve: &crate::wire::records::ConsolidatedPcurve,
    chart: &ConsolidatedCarrierChart<'_>,
) -> Option<PcurveGeometry> {
    let points = pcurve
        .points
        .iter()
        .copied()
        .map(|point| chart.point(point))
        .collect::<Vec<_>>();
    let first = pcurve
        .first_derivatives
        .iter()
        .copied()
        .map(|derivative| chart.derivative(derivative))
        .collect::<Vec<_>>();
    let second = pcurve
        .second_derivatives
        .iter()
        .copied()
        .map(|derivative| chart.derivative(derivative))
        .collect::<Vec<_>>();
    quintic_jet_pcurve(pcurve.degree, &pcurve.knots, &points, &first, &second)
}

pub(crate) fn rolling_ball_derivative(values: [f64; 10]) -> RollingBallJetDerivative {
    RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    }
}
