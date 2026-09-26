// SPDX-License-Identifier: Apache-2.0
//! Bounded-curve wrappers.

use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::index::CarrierIndex;
use super::{CurveCarrier, LEN_TO_MM};

const TAG: u8 = 0x85;
const PAYLOAD_LEN: usize = 2 + 8 * 8;
const POINT_TOLERANCE_MM: f64 = 1.0e-7;

fn point_at(curve: &CurveGeometry, parameter: f64) -> Option<Point3> {
    match curve {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            Some(Point3::new(
                origin.x + parameter * direction.x * LEN_TO_MM,
                origin.y + parameter * direction.y * LEN_TO_MM,
                origin.z + parameter * direction.z * LEN_TO_MM,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            let tangent = axis.cross(*ref_direction);
            Some(Point3::new(
                center.x
                    + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
                center.y
                    + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
                center.z
                    + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let minor_direction = axis.cross(*major_direction);
            Some(Point3::new(
                center.x
                    + major_radius * parameter.cos() * major_direction.x
                    + minor_radius * parameter.sin() * minor_direction.x,
                center.y
                    + major_radius * parameter.cos() * major_direction.y
                    + minor_radius * parameter.sin() * minor_direction.y,
                center.z
                    + major_radius * parameter.cos() * major_direction.z
                    + minor_radius * parameter.sin() * minor_direction.z,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
            let degree = usize::try_from(curve.degree()).ok()?;
            let domain = [curve.knots()[degree], curve.knots()[curve.pole_count()]];
            if !(domain[0]..=domain[1]).contains(&parameter) {
                return None;
            }
            cadmpeg_ir::eval::nurbs_curve_point_at(curve, parameter)
                .ok()
                .map(cadmpeg_ir::features::FinitePoint3::get)
        }
        _ => None,
    }
}

fn close(left: Point3, right: Point3) -> bool {
    (left.x - right.x).abs() <= POINT_TOLERANCE_MM
        && (left.y - right.y).abs() <= POINT_TOLERANCE_MM
        && (left.z - right.z).abs() <= POINT_TOLERANCE_MM
}

/// Decode `00 85` wrappers whose stored bounds agree with their source curve.
pub(super) fn scan(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    carriers: &CarrierIndex,
) -> Result<Vec<CurveCarrier>, CodecError> {
    let mut out = Vec::new();
    for off in 0..bytes.len().checked_sub(2).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, TAG]) {
            continue;
        }
        let header = off + 2 + usize::from(bytes.get(off + 2) == Some(&0xff));
        let marker_at = header + 16;
        if !matches!(bytes.get(marker_at), Some(0x2b | 0x2d)) {
            continue;
        }
        let Some(attr) = View::u16_be_at(bytes, header) else {
            continue;
        };
        let Some(source_attr) = View::u16_be_at(bytes, marker_at + 1) else {
            continue;
        };
        let Some(source) = carriers.curve(source_attr) else {
            continue;
        };
        let geometry = &source.carrier().geometry;
        let mut values = [0.0_f64; 8];
        let mut valid = true;
        for (index, value) in values.iter_mut().enumerate() {
            match View::f64_be_at(bytes, marker_at + 3 + index * 8) {
                Some(parsed) if parsed.is_finite() => *value = parsed,
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let start = Point3::new(
            values[0] * LEN_TO_MM,
            values[1] * LEN_TO_MM,
            values[2] * LEN_TO_MM,
        );
        let end = Point3::new(
            values[3] * LEN_TO_MM,
            values[4] * LEN_TO_MM,
            values[5] * LEN_TO_MM,
        );
        let Some(evaluated_start) = point_at(geometry, values[6]) else {
            continue;
        };
        let Some(evaluated_end) = point_at(geometry, values[7]) else {
            continue;
        };
        if !close(start, evaluated_start) || !close(end, evaluated_end) {
            continue;
        }
        if let Some(ctx) = ctx {
            if let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = geometry {
                let lane_count = curve.knots().len() + curve.pole_count();
                ctx.charge_collection_items(
                    lane_count as u64,
                    "copy Parasolid subset curve lanes",
                )?;
            }
            ctx.charge_collection_items(1, "collect Parasolid subset curves")?;
        }
        out.push(CurveCarrier {
            attr,
            offset: off,
            end: marker_at + 1 + PAYLOAD_LEN,
            geometry: geometry.clone(),
            parameter_range: Some([values[6], values[7]]),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Vector3;

    use super::super::index::CarrierIndex;
    use super::super::CurveCarrier;
    use super::{point_at, scan, TAG};
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::math::Point3;

    fn wrapper(end_y: f64, has_ff: bool) -> Vec<u8> {
        let mut bytes = vec![0x00, TAG];
        if has_ff {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(&20u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for reference in [1u16, 2, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.extend_from_slice(&10u16.to_be_bytes());
        for value in [0.0, 0.0, 0.0, 0.0, end_y, 0.0, 0.0, end_y] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn carriers() -> CarrierIndex {
        let mut carriers = CarrierIndex::default();
        carriers.insert(super::super::Carrier::Curve(CurveCarrier {
            attr: 10,
            offset: 100,
            end: 120,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .expect("valid line fixture"),
            )),
            parameter_range: None,
        }));
        carriers
    }

    fn nurbs_carriers() -> CarrierIndex {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 0.005, 0.005],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 5.0, 0.0)],
            None,
            false,
        )
        .expect("valid NURBS fixture");
        let mut carriers = CarrierIndex::default();
        carriers.insert(super::super::Carrier::Curve(CurveCarrier {
            attr: 10,
            offset: 100,
            end: 120,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range: None,
        }));
        carriers
    }

    fn subset_limit(cap: u64, operation: &'static str) {
        let bytes = wrapper(0.005, false);
        let carriers = nurbs_carriers();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            scan(Some(&ctx), &bytes, &carriers).expect_err("subset collection exceeds its limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation),
            "{error:?}"
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            scan(Some(&ctx), &bytes, &carriers)
                .expect("service scan")
                .len(),
            1
        );
    }

    #[test]
    fn parasolid_subset_nurbs_lanes_refuse_before_clone() {
        subset_limit(5, "copy Parasolid subset curve lanes");
    }

    #[test]
    fn parasolid_subset_curves_refuse_before_collection() {
        subset_limit(6, "collect Parasolid subset curves");
    }

    #[test]
    fn decodes_bounds_that_evaluate_on_the_source_curve() {
        let decoded = scan(None, &wrapper(0.005, false), &carriers()).expect("subset scan");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].attr, 20);
        assert!(matches!(
            decoded[0].geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        ));
        assert_eq!(decoded[0].parameter_range, Some([0.0, 0.005]));
    }

    #[test]
    fn decodes_optional_ff_header() {
        assert_eq!(
            scan(None, &wrapper(0.005, true), &carriers())
                .expect("subset scan")
                .len(),
            1
        );
    }

    #[test]
    fn rejects_bounds_that_do_not_evaluate_on_the_source_curve() {
        let mut bytes = wrapper(0.005, false);
        bytes[21 + 3 * 8..21 + 4 * 8].copy_from_slice(&0.001f64.to_be_bytes());
        assert!(scan(None, &bytes, &carriers())
            .expect("subset scan")
            .is_empty());
    }

    #[test]
    fn evaluates_rational_nurbs_in_homogeneous_coordinates() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("valid rational test NURBS");
        let point = point_at(
            &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            0.5,
        )
        .expect("valid NURBS parameter");
        assert!((point.x - 20.0 / 3.0).abs() < 1.0e-12);
    }

    #[test]
    fn numerical_audit_subset_uses_shared_nurbs_evaluation() {
        for (d, w) in [(1., 1.), (1e-16, 1.), (1., 1e-20)] {
            let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0., 0., d, d],
                    vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                    Some(vec![w, w]),
                    false,
                )
                .unwrap(),
            ));
            assert_eq!(point_at(&curve, 0.75 * d), Some(Point3::new(0.75, 0., 0.)));
            assert!(point_at(&curve, 2. * d).is_none());
        }
    }
}
