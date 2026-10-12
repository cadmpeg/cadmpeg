// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbsPoles, PolarNurbsPoles};
use cadmpeg_ir::geometry::{nurbs::NurbsPoleGrid, SolvedSurfaceGeometry, SurfaceGeometry};
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

/// Compare surface payloads without reading an unequal lane's unused suffix.
pub(super) fn same_surface_geometry(
    ctx: &DecodeContext<'_>,
    first: &SurfaceGeometry,
    second: &SurfaceGeometry,
    operation: &'static str,
) -> Result<bool, ResourceLimit> {
    let (mut first, mut second) = match (first, second) {
        (SurfaceGeometry::Solved(a), SurfaceGeometry::Solved(b)) => (a, b),
        (
            SurfaceGeometry::Procedural {
                construction: a,
                cache: a_cache,
            },
            SurfaceGeometry::Procedural {
                construction: b,
                cache: b_cache,
            },
        ) => {
            if !same_text(ctx, a.as_str(), b.as_str(), operation)? {
                return Ok(false);
            }
            match (a_cache, b_cache) {
                (Some(a), Some(b)) => (a, b),
                (None, None) => return Ok(true),
                _ => return Ok(false),
            }
        }
        _ => return Ok(false),
    };
    // PlacedSurface admission bounds the inline basis chain.
    loop {
        match (first, second) {
            (SolvedSurfaceGeometry::Nurbs(a), SolvedSurfaceGeometry::Nurbs(b)) => {
                if a.u_degree() != b.u_degree()
                    || a.v_degree() != b.v_degree()
                    || a.u_count() != b.u_count()
                    || a.v_count() != b.v_count()
                    || a.normal_reversed() != b.normal_reversed()
                    || a.u_periodic() != b.u_periodic()
                    || a.v_periodic() != b.v_periodic()
                    || std::mem::discriminant(a.pole_grid())
                        != std::mem::discriminant(b.pole_grid())
                {
                    return Ok(false);
                }
                if !same_fixed_values(
                    ctx,
                    a.u_knots().as_slice(),
                    b.u_knots().as_slice(),
                    operation,
                )? || !same_fixed_values(
                    ctx,
                    a.v_knots().as_slice(),
                    b.v_knots().as_slice(),
                    operation,
                )? {
                    return Ok(false);
                }
                return match (a.pole_grid(), b.pole_grid()) {
                    (
                        NurbsPoleGrid::Polynomial { rows: a },
                        NurbsPoleGrid::Polynomial { rows: b },
                    ) => same_fixed_rows(ctx, a, b, operation),
                    (NurbsPoleGrid::Rational { rows: a }, NurbsPoleGrid::Rational { rows: b }) => {
                        same_fixed_rows(ctx, a, b, operation)
                    }
                    _ => Ok(false),
                };
            }
            (SolvedSurfaceGeometry::Polygonal(a), SolvedSurfaceGeometry::Polygonal(b)) => {
                return Ok(a.chordal_deflection() == b.chordal_deflection()
                    && same_fixed_values(ctx, a.vertices(), b.vertices(), operation)?
                    && same_fixed_values(ctx, a.triangles(), b.triangles(), operation)?);
            }
            (SolvedSurfaceGeometry::Transformed(a), SolvedSurfaceGeometry::Transformed(b)) => {
                if a.transform() != b.transform() {
                    return Ok(false);
                }
                first = a.basis();
                second = b.basis();
            }
            (
                SolvedSurfaceGeometry::Unknown { record: a },
                SolvedSurfaceGeometry::Unknown { record: b },
            ) => {
                return match (a, b) {
                    (Some(a), Some(b)) => same_text(ctx, a.as_str(), b.as_str(), operation),
                    (None, None) => Ok(true),
                    _ => Ok(false),
                };
            }
            (SolvedSurfaceGeometry::Plane(a), SolvedSurfaceGeometry::Plane(b)) => return Ok(a == b),
            (SolvedSurfaceGeometry::Cylinder(a), SolvedSurfaceGeometry::Cylinder(b)) => {
                return Ok(a == b)
            }
            (SolvedSurfaceGeometry::Cone(a), SolvedSurfaceGeometry::Cone(b)) => return Ok(a == b),
            (SolvedSurfaceGeometry::Sphere(a), SolvedSurfaceGeometry::Sphere(b)) => {
                return Ok(a == b)
            }
            (SolvedSurfaceGeometry::Torus(a), SolvedSurfaceGeometry::Torus(b)) => return Ok(a == b),
            _ => return Ok(false),
        }
    }
}

/// Compare rows of fixed-size values, admitting each row and each read pair.
fn same_fixed_rows<T: Copy + PartialEq>(
    ctx: &DecodeContext<'_>,
    first: &[Vec<T>],
    second: &[Vec<T>],
    operation: &'static str,
) -> Result<bool, ResourceLimit> {
    if first.len() != second.len() {
        return Ok(false);
    }
    let mut second = second.iter();
    ctx.all_by_limit(
        first,
        |row| {
            let Some(other) = second.next() else {
                return Ok(false);
            };
            same_fixed_values(ctx, row, other, operation)
        },
        operation,
    )
}

/// Compare fixed-size lane values, admitting only the pairs read.
fn same_fixed_values<T: Copy + PartialEq>(
    ctx: &DecodeContext<'_>,
    first: &[T],
    second: &[T],
    operation: &'static str,
) -> Result<bool, ResourceLimit> {
    if first.len() != second.len() {
        return Ok(false);
    }
    let mut second = second.iter();
    ctx.all_by_limit(first, |value| Ok(Some(value) == second.next()), operation)
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
    mod surface;
    fn nurbs(points: Vec<cadmpeg_ir::math::Point2>) -> super::PcurveGeometry {
        crate::test_support::with_decode_context(|ctx| {
            let count = points.len();
            let mut knots = vec![0.0];
            knots.extend((0..count).map(|i| cadmpeg_core::convert::f64_from_index(i).unwrap()));
            knots.push(cadmpeg_core::convert::f64_from_index(count - 1).unwrap());
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
