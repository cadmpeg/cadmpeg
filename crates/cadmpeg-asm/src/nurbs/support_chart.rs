// SPDX-License-Identifier: Apache-2.0
//! Native pcurve charts of analytic support carriers ([spec §6.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#62-analytic-surface-byte-layouts)).
//!
//! A pcurve on a `plane`, `cone`, `sphere`, or `torus` stores its control
//! points in that carrier's native parameterization. [`NativeChart`] is that
//! parameterization in the carrier's own frame. [`SupportChart`] adds the
//! frame the decoder gives the neutral surface and converts a decoded pcurve to
//! the neutral chart of [`cadmpeg_ir::geometry::SurfaceGeometry`].

use crate::nurbs::reader::LEN_TO_MM;
use crate::nurbs::toks::Cur;
use crate::sab::Token;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;

/// The native pcurve chart of an analytic carrier in the carrier's own frame.
///
/// Frame coordinates are `(u, v)` document lengths on a plane, `(azimuth,
/// axial distance)` on a cone, and `(longitude, latitude)` on a sphere or
/// torus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeChart {
    /// Native lengths along `u_dir` and along `normal × u_dir`; `reverse_v`
    /// negates the second coordinate.
    Plane {
        /// The surface v-sense slot.
        reverse_v: bool,
    },
    /// Generator coordinate first and azimuth about the native axis from the
    /// major axis second.
    Cone {
        /// Nonzero document-length axial distance along the native axis per
        /// unit of the first coordinate.
        axial_scale: f64,
    },
    /// Latitude first and longitude about the axis from the reference
    /// direction second; `reverse_v` negates the longitude.
    LatitudeFirst {
        /// The surface v-sense slot.
        reverse_v: bool,
    },
}

impl NativeChart {
    /// The chart of a cone or cylinder with stored `cosine`, native-length
    /// `u_scale`, and u-sense slot `reverse_u`. The first coordinate times
    /// `u_scale` is a generator distance; its axial component is `|cosine|`
    /// of it, toward the native axis. The sign of `cosine` sets the surface
    /// normal and leaves the chart unchanged. A zero or non-finite axial
    /// scale defines no chart.
    #[must_use]
    pub fn cone(cosine: f64, u_scale: f64, reverse_u: bool) -> Option<Self> {
        let axial_scale = sense_sign(reverse_u) * cosine.abs() * u_scale * LEN_TO_MM;
        (axial_scale.is_finite() && axial_scale != 0.0).then_some(Self::Cone { axial_scale })
    }

    /// Map native pcurve coordinates to frame coordinates.
    #[must_use]
    pub fn frame_point(self, native: Point2) -> Point2 {
        match self {
            Self::Plane { reverse_v } => Point2::new(
                native.u * LEN_TO_MM,
                native.v * sense_sign(reverse_v) * LEN_TO_MM,
            ),
            Self::Cone { axial_scale } => Point2::new(native.v, native.u * axial_scale),
            Self::LatitudeFirst { reverse_v } => {
                Point2::new(native.v * sense_sign(reverse_v), native.u)
            }
        }
    }

    /// Map frame coordinates back to native pcurve coordinates.
    #[must_use]
    pub fn native_point(self, frame: Point2) -> Point2 {
        match self {
            Self::Plane { reverse_v } => Point2::new(
                frame.u / LEN_TO_MM,
                frame.v * sense_sign(reverse_v) / LEN_TO_MM,
            ),
            Self::Cone { axial_scale } => Point2::new(frame.v / axial_scale, frame.u),
            Self::LatitudeFirst { reverse_v } => {
                Point2::new(frame.v, frame.u * sense_sign(reverse_v))
            }
        }
    }
}

fn sense_sign(reversed: bool) -> f64 {
    if reversed {
        -1.0
    } else {
        1.0
    }
}

/// Whether a native cone decodes as a circular cylinder.
pub(crate) fn cone_is_circular_cylinder(sine: f64, ratio: f64) -> bool {
    sine.abs() <= f64::EPSILON && ratio == 1.0
}

/// Whether the decoded cone axis opposes the native axis. The neutral cone
/// radius grows along its axis, so a negative native slope `sine / cosine`
/// reverses it; a cylinder keeps the native axis.
pub(crate) fn cone_axis_reversed(sine: f64, cosine: f64, ratio: f64) -> bool {
    !cone_is_circular_cylinder(sine, ratio) && sine * cosine < 0.0
}

/// The map from a support's native pcurve chart to the neutral chart of its
/// decoded surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SupportChart {
    /// A spline, procedural, or absent support whose chart is already
    /// neutral.
    Canonical,
    /// An analytic support. `axis_reversed` states a decoded cone axis
    /// opposite the native axis, which negates both frame coordinates.
    Native {
        chart: NativeChart,
        axis_reversed: bool,
    },
}

impl SupportChart {
    /// The chart of a plane with v-sense slot `reverse_v`.
    pub(crate) fn plane(reverse_v: bool) -> Self {
        Self::Native {
            chart: NativeChart::Plane { reverse_v },
            axis_reversed: false,
        }
    }

    /// The chart of a sphere or torus with v-sense slot `reverse_v`.
    pub(crate) fn latitude_first(reverse_v: bool) -> Self {
        Self::Native {
            chart: NativeChart::LatitudeFirst { reverse_v },
            axis_reversed: false,
        }
    }

    /// The chart of a cone or cylinder from its stored `sine`, `cosine`,
    /// `ratio`, `u_scale`, and u-sense slot `reverse_u`, absent when the
    /// axial scale defines no chart.
    pub(crate) fn cone(
        sine: f64,
        cosine: f64,
        ratio: f64,
        u_scale: f64,
        reverse_u: bool,
    ) -> Option<Self> {
        Some(Self::Native {
            chart: NativeChart::cone(cosine, u_scale, reverse_u)?,
            axis_reversed: cone_axis_reversed(sine, cosine, ratio),
        })
    }

    /// The chart of a standalone surface record. An analytic record's fields
    /// start at its first position token and use the embedded support layout;
    /// a record whose analytic fields do not parse has no chart.
    pub(crate) fn of_surface_record(head: &str, tokens: &[Token]) -> Option<Self> {
        if !matches!(head, "plane" | "cone" | "sphere" | "torus") {
            return Some(Self::Canonical);
        }
        let start = tokens
            .iter()
            .position(|token| matches!(token, Token::Position(_)))
            .unwrap_or(tokens.len());
        crate::nurbs::proc_curve::analytic_support(head, &mut Cur::at(tokens, start))
            .map(|(_, chart)| chart)
    }

    fn neutral_point(self, native: Point2) -> Point2 {
        match self {
            Self::Canonical => native,
            Self::Native {
                chart,
                axis_reversed,
            } => {
                let frame = chart.frame_point(native);
                if axis_reversed {
                    Point2::new(-frame.u, -frame.v)
                } else {
                    frame
                }
            }
        }
    }

    /// Convert `pcurve` from this native chart to the neutral chart, absent
    /// when a converted pole is not finite. An absent conversion leaves
    /// `pcurve` unchanged.
    pub(crate) fn normalize(self, pcurve: &mut PcurveNurbs) -> Option<()> {
        if let Self::Native { .. } = self {
            pcurve
                .try_map_control_points(|_, point| {
                    cadmpeg_ir::units::FinitePoint2::new(self.neutral_point(point.get())).ok_or(())
                })
                .ok()?;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests;
