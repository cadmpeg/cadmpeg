// SPDX-License-Identifier: Apache-2.0
//! A ruled chart defined by two aligned quintic guide jets.

use cadmpeg_core::decode::View;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::UnitVector3;

use crate::families::a5a8::records::A5GuideCurve;
use crate::wire::bytes::{allocation_reference, AllocationReference};

use super::super::rolling_ball_patch::fit_bezier_lanes;

const RULED_FIT_ERROR_MM: f64 = 0.0005;
const DEGREE: u32 = 5;

pub(super) struct Ruled {
    guides: [SolvedCurveGeometry; 2],
    pub(super) bounds: [[f64; 2]; 2],
    origin_at_first: bool,
    offset: f64,
}

impl Ruled {
    pub(super) fn new(guide: &A5GuideCurve, v_limits: [f64; 2]) -> Option<Self> {
        let knots = guide.knots();
        let bounds = [
            [*knots.first()?, *knots.last()?],
            [f64::NEG_INFINITY, f64::INFINITY],
        ];
        let points = guide
            .sites
            .iter()
            .map(|s| {
                let [x, y, z] = s.point.get();
                let [ref_x, ref_y, ref_z] = s.reference_point.get();
                [x, y, z, ref_x, ref_y, ref_z]
            })
            .collect::<Vec<_>>();
        let first = guide
            .sites
            .iter()
            .map(|s| s.first_derivative.get())
            .collect::<Vec<_>>();
        let second = guide
            .sites
            .iter()
            .map(|s| s.second_derivative.get())
            .collect::<Vec<_>>();
        let (knots, poles) =
            crate::nurbs::quintic_jet_bspline(guide.degree, &knots, &points, &first, &second)?;
        let build = |i| {
            NurbsCurve::from_lanes(
                DEGREE,
                knots.clone(),
                poles
                    .iter()
                    .map(|p| {
                        let p = p.get();
                        Point3::new(p[i], p[i + 1], p[i + 2])
                    })
                    .collect(),
                None,
                false,
            )
            .ok()
        };
        Some(Self {
            guides: [build(0)?, build(3)?].map(SolvedCurveGeometry::Nurbs),
            bounds,
            origin_at_first: super::near(v_limits[0], 0.0) && v_limits[1] > 0.0,
            offset: 0.0,
        })
    }

    pub(super) fn offset(
        guide: &A5GuideCurve,
        support_v_limits: [f64; 2],
        bounds: [[f64; 2]; 2],
        distance: f64,
    ) -> Option<Self> {
        let mut chart = Self::new(guide, support_v_limits)?;
        chart.bounds[1] = bounds[1];
        chart.offset = distance;
        (chart.bounds[0][0] < chart.bounds[0][1]).then_some(chart)
    }

    pub(super) fn point(&self, u: f64, v: f64) -> Option<Point3> {
        let evaluate = |curve| {
            eval::decode::curve_point_solved(EvaluationAdmission::Standard, curve, u)
                .ok()
                .map(cadmpeg_ir::features::FinitePoint3::get)
        };
        let p = evaluate(&self.guides[0])?;
        let q = evaluate(&self.guides[1])?;
        let rail = q.vector_from(p);
        let direction = UnitVector3::normalized_by_largest_component(rail)?;
        let origin = if self.origin_at_first { p } else { q };
        let point = origin.translated(*direction.as_raw(), v);
        if self.offset == 0.0 {
            return Some(point);
        }
        let tangent = |guide| {
            eval::curve_tangent_solved(EvaluationAdmission::Standard, guide, u)
                .ok()
                .map(cadmpeg_ir::features::FiniteVector3::get)
        };
        let p_tangent = tangent(&self.guides[0])?;
        let q_tangent = tangent(&self.guides[1])?;
        let along = *direction.as_raw();
        // The discarded derivative of the normalization is parallel to the ruling.
        let origin_tangent = if self.origin_at_first {
            p_tangent
        } else {
            q_tangent
        };
        let du = origin_tangent + (q_tangent - p_tangent).scale(v / rail.dot(along));
        let normal = UnitVector3::normalized_by_largest_component(du.cross(along))?;
        Some(point.translated(*normal.as_raw(), self.offset))
    }

    pub(super) fn fit(&self, [u, v]: [[f64; 2]; 2]) -> Option<NurbsSurface> {
        if self.offset != 0.0 {
            let SolvedCurveGeometry::Nurbs(guide) = &self.guides[0] else {
                return None;
            };
            return super::surface_fit::bounded(guide.knots().iter().copied(), [u, v], |u, v| {
                self.point(u, v)
            });
        }
        let (knots, poles) = fit_bezier_lanes(
            u,
            |t| {
                let a = self.point(t, v[0])?;
                let b = self.point(t, v[1])?;
                Some([a.x, a.y, a.z, b.x, b.y, b.z])
            },
            RULED_FIT_ERROR_MM,
        )?;
        NurbsSurface::new(
            NurbsSurfaceAxis::new(DEGREE, knots, false),
            NurbsSurfaceAxis::new(1, vec![v[0], v[0], v[1], v[1]], false),
            NurbsPoleGrid::Polynomial {
                rows: poles
                    .into_iter()
                    .map(|[a, b, c, d, e, f]| vec![Point3::new(a, b, c), Point3::new(d, e, f)])
                    .collect(),
            },
            false,
        )
        .ok()
    }
}

pub(super) fn bridge(bytes: &[u8]) -> Option<([AllocationReference; 2], [[f64; 2]; 2])> {
    if bytes.first() != Some(&0x82) {
        return None;
    }
    let mut at = 1;
    let guide = allocation_reference(bytes, &mut at)?;
    let profile = allocation_reference(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    if view.take(3)? != [0x05, 0x01, 0x01] || !view.f64_le()?.is_finite() {
        return None;
    }
    UnitVector3::new(Vector3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))?;
    let mut range = || IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?]);
    let bounds = [range()?.endpoints(), range()?.endpoints()];
    (view.remaining() == 0).then_some(([guide, profile], bounds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::families::a5a8::records::a5_guide_curves_from_records;
    use crate::test_support::test_a5a8::a5_guide_curve_stream;
    use crate::wire::records::consolidated_records;

    #[test]
    fn offset_ruled_chart_retains_guide_knots_beyond_result_range() {
        let bytes = a5_guide_curve_stream();
        let records = consolidated_records(&bytes);
        let guide = a5_guide_curves_from_records(&bytes, &records)
            .pop()
            .expect("synthetic guide");
        let result_bounds = [[0.25, 0.75], [0.0, 1.0]];
        let chart =
            Ruled::offset(&guide, [0.0, 1.0], result_bounds, 0.5).expect("offset ruled chart");
        assert_eq!(chart.bounds, [[0.0, 1.0], [0.0, 1.0]]);
        let patch = chart
            .fit([[0.1, 0.9], [0.0, 1.0]])
            .expect("fit a trim inside the guide domain");
        for u in [0.1, 0.5, 0.9] {
            for v in [0.0, 0.5, 1.0] {
                let expected = chart.point(u, v).expect("offset guide point");
                let actual =
                    eval::decode::nurbs_surface_point(EvaluationAdmission::Standard, &patch, u, v)
                        .expect("fitted chart point")
                        .get();
                assert!(actual.distance(expected) <= RULED_FIT_ERROR_MM);
            }
        }
    }
}
