// SPDX-License-Identifier: Apache-2.0
//! Exact degree-one NURBS carriers for sampled polylines.

use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoles3},
    sampled::PolylineCurve,
    CurveGeometry, SolvedCurveGeometry,
};

use super::WriteError;

pub(super) fn convert(
    id: &str,
    polyline: &PolylineCurve,
    range: Option<[f64; 2]>,
) -> Result<NurbsCurve, WriteError> {
    let fail = || WriteError::Omit(format!("polyline {id} has no exact bounded NURBS carrier"));
    let count = polyline.point_count();
    if count < 2 {
        return Err(fail());
    }
    let mut rows = (0..count)
        .map(|index| {
            Some((
                polyline.parameter_at(index)?.get(),
                polyline.point_at(index)?,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(fail)?;
    if rows[0].0 > rows[count - 1].0 {
        rows.reverse();
    }
    if rows.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(fail());
    }
    let [lo, hi] = range.unwrap_or([rows[0].0, rows[count - 1].0]);
    if !(rows[0].0 <= lo && lo < hi && hi <= rows[count - 1].0) {
        return Err(fail());
    }
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline.clone()));
    let endpoint = |parameter| {
        cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &geometry,
            parameter,
        )
        .map_err(|failure| match failure {
            cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                WriteError::Encode(limit.into())
            }
            _ => fail(),
        })
    };
    let mut selected = vec![(lo, endpoint(lo)?)];
    selected.extend(rows.into_iter().filter(|row| lo < row.0 && row.0 < hi));
    selected.push((hi, endpoint(hi)?));
    let mut knots = Vec::with_capacity(selected.len() + 2);
    knots.push(lo);
    knots.extend(selected.iter().map(|row| row.0));
    knots.push(hi);
    NurbsCurve::new(
        1,
        knots,
        NurbsPoles3::Polynomial {
            points: selected.into_iter().map(|row| row.1).collect(),
        },
        false,
    )
    .map_err(|_| fail())
}

#[cfg(test)]
mod tests {
    use super::convert;
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::math::Point3;

    #[test]
    fn clipped_span_interpolates_endpoints_in_source_parameters() {
        let vertices = [
            (0.0, Point3::new(0.0, 0.0, 0.0)),
            (2.0, Point3::new(2.0, 2.0, 0.0)),
            (4.0, Point3::new(4.0, 0.0, 0.0)),
        ]
        .into_iter()
        .map(|(parameter, point)| PolylineVertex { parameter, point })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
        let polyline =
            PolylineCurve::new(PolylineSamples::Parameterized { vertices }, 0.0).unwrap();
        let nurbs = convert("test", &polyline, Some([0.5, 3.0])).unwrap();
        assert_eq!(nurbs.knots().as_slice(), &[0.5, 0.5, 2.0, 3.0, 3.0]);
        assert_eq!(
            nurbs
                .control_points()
                .iter()
                .map(|point| point.get())
                .collect::<Vec<_>>(),
            vec![
                Point3::new(0.5, 0.5, 0.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(3.0, 1.0, 0.0)
            ]
        );
    }
}
