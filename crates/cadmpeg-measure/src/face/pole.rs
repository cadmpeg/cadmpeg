// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Coedge;

use crate::bbox::curve_box;
use crate::boundary::FaceFrame;
use crate::canonical::curve::CurveShape;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::edges::EdgeGeometry;
use crate::failure::Measure;
use crate::geom::coordinate;
use crate::region::Region;
use crate::report::{Witness, WitnessKind};

fn at_pole(edge: &EdgeGeometry<'_>, point: Point3, allowance: f64) -> bool {
    let carrier_at_pole = match edge.curve.as_ref().map(|curve| &curve.shape) {
        Ok(CurveShape::Degenerate {
            point: carrier_point,
        }) => carrier_point.distance(point) <= allowance,
        _ => edge
            .bbox
            .as_ref()
            .is_ok_and(|bbox| bbox.aabb.reach_from(point) <= allowance + bbox.numeric_error),
    };
    edge.closed()
        && edge.degenerate
        && carrier_at_pole
        && edge
            .start
            .is_some_and(|start| start.distance(point) <= allowance)
        && edge.end.is_some_and(|end| end.distance(point) <= allowance)
}

fn clear(
    frame: &FaceFrame<'_, '_>,
    coedges: &[&Coedge],
    point: Point3,
    eval: &Eval<'_, '_>,
) -> Measure<bool> {
    for coedge in coedges {
        let Some(edge) = frame.edges.get(coedge.edge.as_str()) else {
            return Ok(false);
        };
        if at_pole(edge, point, frame.alpha) {
            continue;
        }
        if edge.length.is_err() {
            return Ok(false);
        }
        let uses = frame.owners.edge_coedges(coedge.edge.as_str());
        if let [first, second] = uses {
            if first.sense != second.sense
                && [first, second]
                    .iter()
                    .all(|other| frame.owners.coedge_face(other) == Some(frame.face.id.as_str()))
            {
                continue;
            }
        }
        let (Ok(curve), Ok(interval)) = (&edge.curve, &edge.interval) else {
            return Ok(false);
        };
        if !matches!(
            curve.shape,
            CurveShape::Line { .. } | CurveShape::Conic { .. } | CurveShape::Degenerate { .. }
        ) {
            return Ok(false);
        }
        let bbox = match &edge.bbox {
            Ok(bbox) => *bbox,
            Err(_) => curve_box(eval, curve, *interval)?,
        };
        let allowance = frame.alpha + edge.tau + bbox.numeric_error;
        if !(0..3).any(|axis| {
            let value = coordinate(point, axis);
            value < bbox.aabb.min[axis] - allowance || value > bbox.aabb.max[axis] + allowance
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn witness(
    frame: &FaceFrame<'_, '_>,
    coedges: &[&Coedge],
    region: &Region,
    eval: &Eval<'_, '_>,
) -> Measure<Option<Witness>> {
    if !matches!(frame.chart, Chart::Sphere(_)) {
        return Ok(None);
    }
    let (Some(poles), Some(closed)) = (frame.chart.poles(), &region.closed) else {
        return Ok(None);
    };
    let mut found = None;
    for (index, (_, point)) in poles.into_iter().enumerate() {
        let Some(coedge) = coedges.iter().find(|coedge| {
            frame
                .edges
                .get(coedge.edge.as_str())
                .is_some_and(|edge| at_pole(edge, point, frame.alpha))
        }) else {
            continue;
        };
        if !clear(frame, coedges, point, eval)? {
            continue;
        }
        let agrees = closed.membership.end(if index == 0 { -1.0 } else { 1.0 })?;
        let witness = Witness {
            kind: WitnessKind::OwnedPole,
            entity: coedge.edge.to_string(),
            agrees,
        };
        if !agrees {
            return Ok(Some(witness));
        }
        found = Some(witness);
    }
    Ok(found)
}
