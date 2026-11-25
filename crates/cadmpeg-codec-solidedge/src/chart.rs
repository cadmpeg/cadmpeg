// SPDX-License-Identifier: Apache-2.0
//! Native intersection chart caches and parameter intervals.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::legacy_cache::LegacyCache;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_parasolid::records::primary::Node;

use crate::geometry::Nodes;

const MM_PER_METRE: f64 = cadmpeg_ir::scalar::Magnification::MILLIMETERS_PER_METER.get();

#[derive(Debug)]
pub(crate) enum ChartError {
    Data(&'static str),
    Refusal(CodecError),
}

impl From<CodecError> for ChartError {
    fn from(error: CodecError) -> Self {
        Self::Refusal(error)
    }
}

pub(crate) struct Chart {
    pub(crate) supports: [u32; 2],
    pub(crate) curve: NurbsCurve,
    pub(crate) range: [f64; 2],
    pub(crate) fit: Option<LegacyCache>,
}

fn invalid(message: &'static str) -> ChartError {
    ChartError::Data(message)
}

fn terminator(nodes: &Nodes<'_>, curve: Node<'_>, name: &str) -> Option<Point3> {
    let node = nodes.get(&curve.pointer(name)?)?;
    if !matches!(node.char("type"), Some(b'L' | b'H' | b'T' | b'B')) {
        return None;
    }
    let points = node.hvec_points("hvec")?;
    if !(1..=2).contains(&points.len()) {
        return None;
    }
    let [x, y, z] = *points.first()?;
    cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(x, y, z))
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    nodes: &Nodes<'_>,
    source: Node<'_>,
) -> Result<Chart, ChartError> {
    let supports = source
        .pointer_array::<2>("surface")
        .filter(|supports| supports[0] > 1 && supports[1] > 1 && supports[0] != supports[1])
        .ok_or_else(|| invalid("intersection supports are unresolved"))?;
    let chart = source
        .pointer("chart")
        .and_then(|index| nodes.get(&index))
        .copied()
        .ok_or_else(|| invalid("intersection chart is unresolved"))?;
    let raw = chart
        .hvec_points("hvec")
        .ok_or_else(|| invalid("intersection chart points are absent"))?;
    if raw.len() < 2 || u32::try_from(raw.len()).ok() != chart.integer("chart_count") {
        return Err(invalid("intersection chart count is inconsistent"));
    }
    let base = chart
        .real("base_parameter")
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid("intersection chart parameter is invalid"))?;
    let scale = chart
        .real("base_scale")
        .filter(|value| value.is_finite() && *value != 0.0)
        .ok_or_else(|| invalid("intersection chart scale is invalid"))?;
    if chart.is_null("base_scale") == Some(true) {
        return Err(invalid("intersection chart scale is absent"));
    }
    let mut points = ctx.alloc_filled(
        raw.len(),
        Point3::new(0.0, 0.0, 0.0),
        "project intersection chart points",
    )?;
    for (point, [x, y, z]) in points.iter_mut().zip(raw) {
        *point = cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(x, y, z))
            .ok_or_else(|| invalid("intersection chart point is nonfinite"))?
            .get();
    }
    let mut parameters =
        ctx.alloc_filled(points.len(), base, "project intersection chart parameters")?;
    for (index, pair) in points.windows(2).enumerate() {
        parameters[index + 1] = parameters[index] + pair[1].distance(pair[0]) * scale;
    }
    if !parameters.iter().all(|value| value.is_finite())
        || !parameters.windows(2).all(|pair| {
            if scale > 0.0 {
                pair[0] < pair[1]
            } else {
                pair[0] > pair[1]
            }
        })
    {
        return Err(invalid("intersection chart parameters are not monotone"));
    }
    let last = points.len() - 1;
    let mut displacement = 0.0_f64;
    for (name, index) in [("start", 0), ("end", last)] {
        if let Some(point) = terminator(nodes, source, name) {
            displacement = displacement.max(point.distance(points[index]));
            points[index] = point;
        }
    }
    for point in &mut points {
        *point = cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
            point.x * MM_PER_METRE,
            point.y * MM_PER_METRE,
            point.z * MM_PER_METRE,
        ))
        .ok_or_else(|| invalid("intersection chart point overflows millimetres"))?
        .get();
    }
    if scale < 0.0 {
        parameters.reverse();
        points.reverse();
    }
    let range = [parameters[0], parameters[last]];
    let knot_count = points
        .len()
        .checked_add(2)
        .ok_or_else(|| invalid("intersection chart knot count overflows"))?;
    let mut knots = ctx.alloc_filled(knot_count, range[0], "project intersection chart knots")?;
    knots[1..=points.len()].copy_from_slice(&parameters);
    knots[knot_count - 1] = range[1];
    let closed = points[0] == points[last];
    let curve = NurbsCurve::from_lanes(1, knots, points, None, closed)
        .map_err(|_| invalid("intersection chart curve is invalid"))?;
    let fit = if chart.is_null("chordal_error") == Some(true) {
        None
    } else {
        let error = chart
            .real("chordal_error")
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| invalid("intersection chart fit tolerance is invalid"))?;
        Some(
            LegacyCache::try_new((error + displacement) * MM_PER_METRE)
                .map_err(|_| invalid("intersection chart fit tolerance overflows millimetres"))?,
        )
    };
    Ok(Chart {
        supports,
        curve,
        range,
        fit,
    })
}

#[cfg(test)]
mod tests {
    use super::{project, ChartError};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_parasolid::records::{node_type, PrimaryRecords};

    const EPS_CHART_PARAMETER: f64 = 1.0e-12;
    const EPS_CHART_FIT_MM: f64 = 1.0e-12;

    #[test]
    fn native_chart_retains_closed_points_fit_and_parameter_direction() {
        for (scale, expected) in [(1.0, [0.0, 4.0]), (-2.0, [-8.0, 0.0])] {
            let bytes = crate::test_support::chart_partition(scale);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let records = PrimaryRecords::read(&bytes)
                .expect("header")
                .expect("partition");
            assert!(records.is_terminated());
            let nodes = records.nodes().map(|node| (node.index(), node)).collect();
            let source = records
                .nodes()
                .find(|node| node.kind() == node_type::INTERSECTION)
                .expect("intersection");
            let chart = project(&ctx, &nodes, source).expect("chart");
            for (actual, expected) in chart.range.into_iter().zip(expected) {
                assert!((actual - expected).abs() < EPS_CHART_PARAMETER);
            }
            assert!(chart.curve.periodic());
            assert_eq!(chart.curve.degree(), 1);
            assert_eq!(chart.curve.control_points().len(), 5);
            assert_eq!(
                chart.curve.control_points()[1].get().x,
                if scale > 0.0 { 1000.0 } else { 0.0 }
            );
            assert!((chart.fit.expect("fit").fit_tolerance.get() - 0.001).abs() < EPS_CHART_FIT_MM);
            ctx.finish_session().expect("session");
        }
    }

    #[test]
    fn inconsistent_chart_count_is_a_data_refusal() {
        let mut bytes = crate::test_support::chart_partition(1.0);
        let records = PrimaryRecords::read(&bytes)
            .expect("header")
            .expect("partition");
        let count = records
            .nodes()
            .find(|node| node.kind() == node_type::CHART)
            .expect("chart")
            .field_range("chart_count")
            .expect("count");
        bytes[count].copy_from_slice(&0u32.to_be_bytes());
        let records = PrimaryRecords::read(&bytes)
            .expect("header")
            .expect("partition");
        let nodes = records.nodes().map(|node| (node.index(), node)).collect();
        let source = records
            .nodes()
            .find(|node| node.kind() == node_type::INTERSECTION)
            .expect("intersection");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(matches!(
            project(&ctx, &nodes, source),
            Err(ChartError::Data(_))
        ));
        ctx.finish_session().expect("session");
    }
}
