// SPDX-License-Identifier: Apache-2.0
//! Bounded-curve wrappers.

use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::index::CarrierIndex;
use super::sp_curve::SurfaceCurveCarrier;
use super::{CurveCarrier, LEN_TO_MM};

const TAG: u8 = 0x85;
const PAYLOAD_LEN: usize = 2 + 8 * 8;
const POINT_TOLERANCE_MM: f64 = 1.0e-7;
/// The departure admitted between a bounded `SP_CURVE`'s stored end points
/// and the support evaluated through its pcurve (spec §7.6).
pub(super) const SURFACE_POINT_TOLERANCE_MM: f64 = 1.0e-5;

pub(super) fn point_at(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    parameter: f64,
) -> Result<Option<Point3>, CodecError> {
    Ok(match curve {
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
            let Ok(degree) = usize::try_from(curve.degree()) else {
                return Ok(None);
            };
            let domain = [curve.knots()[degree], curve.knots()[curve.pole_count()]];
            if !(domain[0]..=domain[1]).contains(&parameter) {
                return Ok(None);
            }
            super::evaluation::nurbs_curve_point(ctx, curve, parameter)?
                .map(cadmpeg_ir::features::FinitePoint3::get)
        }
        _ => None,
    })
}

fn close(left: Point3, right: Point3) -> bool {
    (left.x - right.x).abs() <= POINT_TOLERANCE_MM
        && (left.y - right.y).abs() <= POINT_TOLERANCE_MM
        && (left.z - right.z).abs() <= POINT_TOLERANCE_MM
}

/// One decoded `00 85` wrapper.
pub(super) enum Wrapper {
    /// A wrapper of a curve whose stored bounds agree with it.
    Curve(CurveCarrier),
    /// A wrapper of an `SP_CURVE` whose stored ends are its support through
    /// its pcurve at the stored parameters (spec §7.6).
    SurfaceCurve(Box<SurfaceCurveCarrier>),
}

/// Which wrappers a scan decodes: those of curves, or those of `SP_CURVE`s,
/// which are indexed after the supports they are read on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    Curves,
    SurfaceCurves,
}

/// Decode `00 85` wrappers whose stored bounds agree with their source curve.
pub(super) fn scan(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carriers: &CarrierIndex,
    pass: Pass,
) -> Result<Vec<Wrapper>, CodecError> {
    // A buffer shorter than the two-byte tag holds no wrapper.
    let Some(scan_end) = bytes.len().checked_sub(2) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    if pass == Pass::SurfaceCurves && !carriers.has_sp_curves() {
        return Ok(out);
    }
    for off in 0..scan_end {
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
        let mut values = [cadmpeg_ir::scalar::FiniteReal::ZERO; 8];
        let mut valid = true;
        for (index, value) in values.iter_mut().enumerate() {
            match View::f64_be_at(bytes, marker_at + 3 + index * 8)
                .and_then(cadmpeg_ir::scalar::FiniteReal::new)
            {
                Some(parsed) => *value = parsed,
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
            values[0].get() * LEN_TO_MM,
            values[1].get() * LEN_TO_MM,
            values[2].get() * LEN_TO_MM,
        );
        let end = Point3::new(
            values[3].get() * LEN_TO_MM,
            values[4].get() * LEN_TO_MM,
            values[5].get() * LEN_TO_MM,
        );
        if pass == Pass::SurfaceCurves {
            if let Some(carrier) = surface_curve_at(ctx, carriers, off, attr, source_attr, &values)?
            {
                out.push(Wrapper::SurfaceCurve(Box::new(carrier)));
            }
            continue;
        }
        let Some(source) = carriers.curve(source_attr) else {
            continue;
        };
        let geometry = &source.carrier().geometry;
        let Some(evaluated_start) = point_at(ctx, geometry, values[6].get())? else {
            continue;
        };
        let Some(evaluated_end) = point_at(ctx, geometry, values[7].get())? else {
            continue;
        };
        if !close(start, evaluated_start) || !close(end, evaluated_end) {
            continue;
        }
        let copied_geometry = geometry.clone();
        out.push(Wrapper::Curve(CurveCarrier {
            marker_reversed: None,
            attr,
            offset: off,
            end: marker_at + 1 + PAYLOAD_LEN,
            geometry: copied_geometry,
            parameter_range: Some(cadmpeg_ir::units::FiniteVector::from([
                values[6], values[7],
            ])),
        }));
    }
    Ok(out)
}

/// The `SP_CURVE` wrapper at `off` when its stored ends are on the support
/// within [`SURFACE_POINT_TOLERANCE_MM`].
fn surface_curve_at(
    ctx: &DecodeContext<'_>,
    carriers: &CarrierIndex,
    off: usize,
    attr: u16,
    source_attr: u16,
    values: &[cadmpeg_ir::scalar::FiniteReal; 8],
) -> Result<Option<SurfaceCurveCarrier>, CodecError> {
    let Some(source) = carriers.sp_curve(source_attr) else {
        return Ok(None);
    };
    let Some(resolved) = carriers.sp_curve_support(source.surface) else {
        return Ok(None);
    };
    let support = resolved.support();
    let Some(pcurve) = support.pcurve(&source.pcurve) else {
        return Ok(None);
    };
    let point = |first: usize| {
        Point3::new(
            values[first].get() * LEN_TO_MM,
            values[first + 1].get() * LEN_TO_MM,
            values[first + 2].get() * LEN_TO_MM,
        )
    };
    let points = [point(0), point(3)];
    let parameter_range = [values[6].get(), values[7].get()];
    for (t, stored) in parameter_range.iter().zip(points) {
        let Some(on_support) = support.point(ctx, &pcurve, *t)? else {
            return Ok(None);
        };
        let departure = on_support.distance(stored);
        if departure.is_nan() || departure > SURFACE_POINT_TOLERANCE_MM {
            return Ok(None);
        }
    }
    Ok(Some(SurfaceCurveCarrier {
        attr,
        offset: off,
        surface: source.surface,
        pcurve,
        parameter_range,
        points,
        sweep: support.sweep().cloned(),
        offset_support: support.offset().cloned(),
    }))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
            marker_reversed: None,
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
            marker_reversed: None,
            attr: 10,
            offset: 100,
            end: 120,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range: None,
        }));
        carriers
    }

    #[test]
    fn parasolid_subset_decodes_low_and_high_degree_nurbs_sources() {
        let bytes = wrapper(0.005, false);
        let carriers = nurbs_carriers();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(
            scan(&ctx, &bytes, &carriers, super::Pass::Curves)
                .unwrap()
                .len(),
            1
        );

        // Degree 16 needs a heap basis of 17 `f64` slots; lower degrees
        // evaluate in inline storage.
        let degree = 16_u32;
        let slots = 17_usize;
        let mut knots = vec![0.0; slots];
        knots.extend(vec![0.005; slots]);
        let spline = NurbsCurve::from_lanes(
            degree,
            knots,
            (0..slots)
                .map(|pole| {
                    Point3::new(
                        0.0,
                        5.0 * f64::from(u32::try_from(pole).expect("pole")) / 16.0,
                        0.0,
                    )
                })
                .collect(),
            None,
            false,
        )
        .expect("degree-16 version of the same line");
        let mut spline_carriers = CarrierIndex::default();
        spline_carriers.insert(super::super::Carrier::Curve(CurveCarrier {
            marker_reversed: None,
            attr: 10,
            offset: 100,
            end: 120,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spline)),
            parameter_range: None,
        }));
        assert_eq!(
            scan(&ctx, &bytes, &spline_carriers, super::Pass::Curves)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn decodes_bounds_that_evaluate_on_the_source_curve() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let decoded = scan(
            &ctx,
            &wrapper(0.005, false),
            &carriers(),
            super::Pass::Curves,
        )
        .expect("subset scan");
        assert_eq!(decoded.len(), 1);
        let super::Wrapper::Curve(carrier) = &decoded[0] else {
            panic!("an exact curve wrapper");
        };
        assert_eq!(carrier.attr, 20);
        assert!(matches!(
            carrier.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        ));
        assert_eq!(
            carrier
                .parameter_range
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 0.005])
        );
    }

    #[test]
    fn decodes_optional_ff_header() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        assert_eq!(
            scan(
                &ctx,
                &wrapper(0.005, true),
                &carriers(),
                super::Pass::Curves
            )
            .expect("subset scan")
            .len(),
            1
        );
    }

    #[test]
    fn rejects_bounds_that_do_not_evaluate_on_the_source_curve() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = wrapper(0.005, false);
        bytes[21 + 3 * 8..21 + 4 * 8].copy_from_slice(&0.001f64.to_be_bytes());
        assert!(scan(&ctx, &bytes, &carriers(), super::Pass::Curves)
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
            &cadmpeg_test_support::service_decode_context(),
            &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            0.5,
        )
        .expect("evaluate the point")
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
            assert_eq!(
                point_at(
                    &cadmpeg_test_support::service_decode_context(),
                    &curve,
                    0.75 * d
                )
                .expect("evaluate the point"),
                Some(Point3::new(0.75, 0., 0.))
            );
            assert!(point_at(
                &cadmpeg_test_support::service_decode_context(),
                &curve,
                2. * d
            )
            .expect("evaluate the point")
            .is_none());
        }
    }
}
