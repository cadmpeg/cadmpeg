// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbsPoles, PolarNurbsPoles};
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

/// Maximum adaptive geometry work admitted for one decoded model.
pub(super) const MAX_ADAPTIVE_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while completing intersection
/// pcurves for one decoded model.
///
/// Pcurve completion is a separate model-wide phase. Keeping its budget
/// independent prevents a large but valid completion set from exhausting the
/// carrier and topology-certification budget, while retaining a hard bound on
/// the completion phase itself.
pub(super) const MAX_PCURVE_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating serialized
/// EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating or completing
/// EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while continuing coupled
/// surface-intersection support lanes for one decoded model.
pub(super) const MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Compare two identity texts on a resource-only refusal channel. Texts of
/// unequal length differ without a byte comparison; equal lengths charge only
/// the byte pairs read before a mismatch.
pub(super) fn same_text(
    ctx: &DecodeContext<'_>,
    first: &str,
    second: &str,
    operation: &'static str,
) -> Result<bool, ResourceLimit> {
    ctx.equal_bytes_limit(first.as_bytes(), second.as_bytes(), operation)
}

/// Compare pcurve payloads without reading past the first unequal lane row.
pub(super) fn same_pcurve_geometry(
    ctx: &DecodeContext<'_>,
    mut first: &PcurveGeometry,
    mut second: &PcurveGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    loop {
        match (first, second) {
            (PcurveGeometry::Nurbs { nurbs: a }, PcurveGeometry::Nurbs { nurbs: b }) => {
                if a.degree() != b.degree()
                    || !same_knots(ctx, a.knots().as_slice(), b.knots().as_slice(), operation)?
                {
                    return Ok(false);
                }
                let poles_match = match (a.pole_rows(), b.pole_rows()) {
                    (
                        PcurveNurbsPoles::Polynomial { points: a },
                        PcurveNurbsPoles::Polynomial { points: b },
                    ) => {
                        a.len() == b.len()
                            && ctx.all_by(a.iter().zip(b), |(a, b)| Ok(a == b), operation)?
                    }
                    (
                        PcurveNurbsPoles::Rational { points: a },
                        PcurveNurbsPoles::Rational { points: b },
                    ) => {
                        a.len() == b.len()
                            && ctx.all_by(a.iter().zip(b), |(a, b)| Ok(a == b), operation)?
                    }
                    _ => false,
                };
                return Ok(poles_match && a.periodic() == b.periodic());
            }
            (PcurveGeometry::PolarNurbs { nurbs: a }, PcurveGeometry::PolarNurbs { nurbs: b }) => {
                if a.degree() != b.degree()
                    || !same_knots(ctx, a.knots().as_slice(), b.knots().as_slice(), operation)?
                {
                    return Ok(false);
                }
                let poles_match = match (a.pole_rows(), b.pole_rows()) {
                    (
                        PolarNurbsPoles::Polynomial { poles: a },
                        PolarNurbsPoles::Polynomial { poles: b },
                    ) => {
                        a.len() == b.len()
                            && ctx.all_by(a.iter().zip(b), |(a, b)| Ok(a == b), operation)?
                    }
                    (
                        PolarNurbsPoles::Rational { poles: a },
                        PolarNurbsPoles::Rational { poles: b },
                    ) => {
                        a.len() == b.len()
                            && ctx.all_by(a.iter().zip(b), |(a, b)| Ok(a == b), operation)?
                    }
                    _ => false,
                };
                return Ok(poles_match && a.periodic() == b.periodic());
            }
            (PcurveGeometry::Transformed(a), PcurveGeometry::Transformed(b)) => {
                if a.transform() != b.transform() {
                    return Ok(false);
                }
                first = a.basis();
                second = b.basis();
            }
            (PcurveGeometry::Trimmed(a), PcurveGeometry::Trimmed(b)) => {
                if a.parameter_range() != b.parameter_range() || a.same_sense() != b.same_sense() {
                    return Ok(false);
                }
                first = a.basis();
                second = b.basis();
            }
            (PcurveGeometry::Offset(a), PcurveGeometry::Offset(b)) => {
                if a.distance() != b.distance() {
                    return Ok(false);
                }
                first = a.basis();
                second = b.basis();
            }
            (PcurveGeometry::Line(a), PcurveGeometry::Line(b)) => return Ok(a == b),
            (PcurveGeometry::PolarHarmonic(a), PcurveGeometry::PolarHarmonic(b)) => {
                return Ok(a == b)
            }
            (PcurveGeometry::SphericalGreatCircle(a), PcurveGeometry::SphericalGreatCircle(b)) => {
                return Ok(a == b)
            }
            (PcurveGeometry::Circle(a), PcurveGeometry::Circle(b)) => return Ok(a == b),
            (PcurveGeometry::Ellipse(a), PcurveGeometry::Ellipse(b)) => return Ok(a == b),
            (PcurveGeometry::Harmonic(a), PcurveGeometry::Harmonic(b)) => return Ok(a == b),
            (PcurveGeometry::Parabola(a), PcurveGeometry::Parabola(b)) => return Ok(a == b),
            (PcurveGeometry::Hyperbola(a), PcurveGeometry::Hyperbola(b)) => return Ok(a == b),
            (PcurveGeometry::Hyperbolic(a), PcurveGeometry::Hyperbolic(b)) => return Ok(a == b),
            _ => return Ok(false),
        }
    }
}

fn same_knots(
    ctx: &DecodeContext<'_>,
    first: &[f64],
    second: &[f64],
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(first.len() == second.len()
        && ctx.all_by(first.iter().zip(second), |(a, b)| Ok(a == b), operation)?)
}

/// Geometry work accounting plus the cache of successful blend-geometry
/// certificates earned within the same accounting scope.
pub(super) struct GeometryWorkBudget<'a> {
    work: WorkBudget<'a>,
    pub(super) charges: &'a DecodeContext<'a>,
    blend_frame_cache: Rc<RefCell<super::blend::BlendSurfaceFrameCache<'a>>>,
}

impl<'a> GeometryWorkBudget<'a> {
    pub(super) fn from_context(ctx: &'a DecodeContext<'_>, limit: u64) -> Self {
        Self {
            work: ctx.work_budget(limit),
            charges: ctx,
            blend_frame_cache: Rc::new(RefCell::new(
                super::blend::BlendSurfaceFrameCache::default(),
            )),
        }
    }

    pub(super) fn resource_refusal(&self) -> Option<ResourceLimit> {
        let charges = self.charges;
        if let Some(limit) = charges.resource_refusal() {
            return Some(limit);
        }
        if self.work.exhausted() {
            let limit = cadmpeg_core::decode::u64_from_index(self.work.consumed());
            let additional = limit + u64::from(limit != u64::MAX);
            drop(charges.refuse_codec_limit("nx adaptive geometry work", limit, additional));
        }
        charges.resource_refusal()
    }

    pub(super) fn exhausted(&self) -> bool {
        self.resource_refusal().is_some()
    }

    pub(super) fn child_slice(&self, limit: usize) -> GeometryWorkBudget<'a> {
        GeometryWorkBudget {
            work: self.work.session_child_slice(limit),
            charges: self.charges,
            blend_frame_cache: Rc::clone(&self.blend_frame_cache),
        }
    }

    pub(super) fn clear_blend_frame_cache(&self) {
        self.blend_frame_cache.borrow_mut().clear();
    }

    pub(super) fn blend_frame_cache(&self) -> &RefCell<super::blend::BlendSurfaceFrameCache<'a>> {
        self.blend_frame_cache.as_ref()
    }
}

impl<'a> Deref for GeometryWorkBudget<'a> {
    type Target = WorkBudget<'a>;

    fn deref(&self) -> &Self::Target {
        &self.work
    }
}

#[cfg(test)]
mod tests {
    fn nurbs(points: Vec<cadmpeg_ir::math::Point2>) -> super::PcurveGeometry {
        crate::test_support::with_decode_context(|ctx| {
            let count = points.len();
            let mut knots = vec![0.0];
            knots.extend((0..count).map(|i| i as f64));
            knots.push((count - 1) as f64);
            super::PcurveGeometry::Nurbs {
                nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    ctx, 1, knots, points, None, false,
                )
                .unwrap()
                .unwrap(),
            }
        })
    }

    #[test]
    fn pcurve_comparison_charges_equal_lanes_and_stops_at_first_unequal_pole() {
        use cadmpeg_ir::math::Point2;
        let first = nurbs(vec![Point2::new(0.0, 0.0); 1024]);
        let mut points = vec![Point2::new(0.0, 0.0); 1024];
        points[0] = Point2::new(1.0, 0.0);
        let second = nurbs(points);
        // 1026 knot pairs, their terminal probe, and the first pole pair.
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1028,
            |ctx| {
                assert!(!super::same_pcurve_geometry(
                    ctx,
                    &first,
                    &second,
                    "nx endpoint proof pcurve comparison"
                )
                .unwrap());
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "nx endpoint proof pcurve comparison",
            |ctx| {
                super::same_pcurve_geometry(
                    ctx,
                    &first,
                    &first,
                    "nx endpoint proof pcurve comparison",
                )
            },
        );
        crate::test_support::with_decode_context(|ctx| {
            assert!(
                super::same_pcurve_geometry(ctx, &first, &first, "nx equal pcurve comparison")
                    .unwrap()
            );
        });
    }

    #[test]
    fn pcurve_comparison_charges_only_the_unequal_knot_prefix() {
        let first = vec![0.0; 4096];
        let mut second = first.clone();
        second[0] = 1.0;
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                assert!(
                    !super::same_knots(ctx, &first, &second, "nx pcurve knot comparison").unwrap()
                );
            },
        );
    }

    #[test]
    fn identity_comparison_stops_before_an_unused_suffix() {
        let first = format!("a{}", "x".repeat(4096));
        let second = format!("b{}", "x".repeat(4096));
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                assert!(
                    !super::same_text(ctx, &first, &second, "nx test identity comparison").unwrap()
                );
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "nx test identity comparison",
            |ctx| {
                Ok(super::same_text(
                    ctx,
                    &first,
                    &first,
                    "nx test identity comparison",
                )?)
            },
        );
    }
}
