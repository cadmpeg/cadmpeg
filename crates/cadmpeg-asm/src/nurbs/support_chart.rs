// SPDX-License-Identifier: Apache-2.0
//! Native pcurve charts of analytic support carriers ([spec §6.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#62-analytic-surface-byte-layouts)).
//!
//! A pcurve on a `plane`, `cone`, `sphere`, or `torus` stores its control
//! points in that carrier's native parameterization. [`NativeChart`] is that
//! parameterization in the carrier's own frame. [`SupportChart`] converts a
//! decoded pcurve to the neutral chart of
//! [`cadmpeg_ir::geometry::SurfaceGeometry`].

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
    /// Generator coordinate first and azimuth second.
    Cone {
        /// Nonzero document-length axial distance per unit of the first
        /// coordinate.
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
    /// The chart of a cone or cylinder with stored `sine`, `cosine`, and
    /// native-length `u_scale`. A zero or non-finite axial scale defines no
    /// chart.
    #[must_use]
    pub fn cone(sine: f64, cosine: f64, u_scale: f64) -> Option<Self> {
        let direction = if sine * cosine < 0.0 { -1.0 } else { 1.0 };
        let axial_scale = direction * cosine * u_scale * LEN_TO_MM;
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

/// The map from a support's native pcurve chart to the neutral chart of its
/// decoded surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SupportChart {
    /// A spline, procedural, or absent support whose chart is already
    /// neutral.
    Canonical,
    /// An analytic support whose frame is the decoded surface frame.
    Native(NativeChart),
}

impl SupportChart {
    /// The chart of a carrier `kind` whose fields start at the cursor, absent
    /// when an analytic carrier's fields do not parse.
    pub(crate) fn from_fields(kind: &str, cur: &mut Cur<'_>) -> Option<Self> {
        match kind {
            "plane" => {
                cur.take_position()?;
                cur.take_vector3()?;
                cur.take_vector3()?;
                Some(Self::Native(NativeChart::Plane {
                    reverse_v: cur.take_bool()?,
                }))
            }
            "cone" => {
                cur.take_position()?;
                cur.take_vector3()?;
                cur.take_vector3()?;
                cur.take_f64()?;
                cur.take_bool()?;
                cur.take_bool()?;
                let sine = cur.take_f64()?;
                let cosine = cur.take_f64()?;
                let u_scale = cur.take_f64()?;
                Some(Self::Native(NativeChart::cone(sine, cosine, u_scale)?))
            }
            "sphere" => {
                cur.take_position()?;
                cur.take_f64()?;
                cur.take_vector3()?;
                cur.take_vector3()?;
                Some(Self::Native(NativeChart::LatitudeFirst {
                    reverse_v: cur.take_bool()?,
                }))
            }
            "torus" => {
                cur.take_position()?;
                cur.take_vector3()?;
                cur.take_f64()?;
                cur.take_f64()?;
                cur.take_vector3()?;
                Some(Self::Native(NativeChart::LatitudeFirst {
                    reverse_v: cur.take_bool()?,
                }))
            }
            _ => Some(Self::Canonical),
        }
    }

    /// The chart of the embedded support whose identifier is at token
    /// `position`, absent when an analytic support's fields do not parse.
    pub(crate) fn embedded_at(toks: &[Token], position: usize) -> Option<Self> {
        let mut cur = Cur::at(toks, position);
        match cur.take_ident() {
            Some(kind) => Self::from_fields(kind, &mut cur),
            None => Some(Self::Canonical),
        }
    }

    /// The chart of a standalone surface record. An analytic record's fields
    /// start at its first position token.
    pub(crate) fn of_surface_record(head: &str, tokens: &[Token]) -> Option<Self> {
        let start = tokens
            .iter()
            .position(|token| matches!(token, Token::Position(_)))
            .unwrap_or(tokens.len());
        Self::from_fields(head, &mut Cur::at(tokens, start))
    }

    /// Convert `pcurve` from this native chart to the neutral chart, absent
    /// when a converted pole is not finite. An absent conversion leaves
    /// `pcurve` unchanged.
    pub(crate) fn normalize(self, pcurve: &mut PcurveNurbs) -> Option<()> {
        if let Self::Native(chart) = self {
            pcurve
                .try_map_control_points(|_, point| {
                    cadmpeg_ir::units::FinitePoint2::new(chart.frame_point(point.get())).ok_or(())
                })
                .ok()?;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests;
