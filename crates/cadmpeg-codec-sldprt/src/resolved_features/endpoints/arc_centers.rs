//! Quantized candidate positions for bounded arc-center queries.

use crate::records::{SketchInputEntity, SketchInputKind};
use crate::resolved_features::grid::{quantize, GridPoint};
use crate::resolved_features::SKETCH_ANGLE_TOLERANCE;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;

#[derive(Clone, Copy)]
struct ArcCenterRecord<'a> {
    ordinal: usize,
    reference: Option<&'a str>,
    point: Point2,
    cell: GridPoint,
}

struct ArcCenterNode {
    bounds: [f64; 4],
    finite: bool,
    range: std::ops::Range<usize>,
    children: Option<(usize, usize)>,
}

/// Quantized position partitions and their coordinate bounds, built once for a sketch.
pub(in crate::resolved_features) struct ArcCenterIndex<'a, 'ctx> {
    records: Vec<ArcCenterRecord<'a>>,
    order: Vec<usize>,
    tolerance: f64,
    nodes: Vec<ArcCenterNode>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> ArcCenterIndex<'a, 'ctx> {
    pub(in crate::resolved_features) fn from_points(
        ctx: &'ctx DecodeContext<'_>,
        points: &[(Option<&'a str>, Point2)],
        tolerance: f64,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT arc center positions";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let records = storage.with_storage(|| ctx.collect_vec(points.iter().enumerate().map(|(ordinal, &(reference, point))| {
            ArcCenterRecord { ordinal, reference, point, cell: quantize(point, tolerance) }
        }), OPERATION))?;
        Self::build(ctx, records, tolerance, storage)
    }

    pub(super) fn from_markers(
        ctx: &'ctx DecodeContext<'_>,
        markers: &[&'a SketchInputEntity],
        scale: f64,
        arcs_only: bool,
        tolerance: f64,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT arc center positions";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut records = Vec::new();
        storage.with_storage(|| {
        for (ordinal, marker) in ctx.admit_iter(markers, OPERATION)?.enumerate() {
            if arcs_only && marker.kind() != SketchInputKind::Arc { continue; }
            let Some([u, v]) = marker.coordinates_m.map(cadmpeg_ir::units::FiniteVector::get) else { continue; };
            let point = Point2::new(u * scale, v * scale);
            ctx.push_vec(&mut records, ArcCenterRecord {
                ordinal, reference: Some(marker.id()), point, cell: quantize(point, tolerance),
            }, OPERATION)?;
        }
        Ok::<(), CodecError>(())
        })?;
        Self::build(ctx, records, tolerance, storage)
    }

    fn build(ctx: &DecodeContext<'_>, records: Vec<ArcCenterRecord<'a>>, tolerance: f64, mut storage: cadmpeg_core::decode::ScopedReservation<'ctx>) -> Result<Self, CodecError> {
        let mut order = storage.with_storage(|| ctx.collect_vec(0..records.len(), "index SLDPRT arc center positions"))?;
        let mut nodes = Vec::new();
        if !order.is_empty() { storage.with_storage(|| Self::partition(ctx, &records, &mut order, 0, 0, &mut nodes))?; }
        Ok(Self { records, order, tolerance, nodes, _storage: storage })
    }

    // Each split halves the range. The stack depth is at most usize::BITS.
    fn partition(
        ctx: &DecodeContext<'_>,
        records: &[ArcCenterRecord<'a>],
        order: &mut [usize],
        start: usize,
        axis: usize,
        nodes: &mut Vec<ArcCenterNode>,
    ) -> Result<usize, CodecError> {
        const OPERATION: &str = "partition SLDPRT arc center positions";
        let index = nodes.len();
        ctx.push_vec(nodes, ArcCenterNode {
            bounds: [0.0; 4], finite: true, range: start..start + order.len(), children: None,
        }, OPERATION)?;
        let (bounds, finite, children) = if order.len() <= 8 {
            let (bounds, finite) = ctx.fold(&*order, ([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], true),
                |(bounds, finite), index| { let record = &records[*index]; Ok(([
                    bounds[0].min(record.point.u), bounds[1].max(record.point.u),
                    bounds[2].min(record.point.v), bounds[3].max(record.point.v),
                ], finite && record.point.u.is_finite() && record.point.v.is_finite())) }, OPERATION)?;
            (bounds, finite, None)
        } else {
            ctx.sort_unstable_by_key(order, |index| if axis == 0 { records[*index].point.u } else { records[*index].point.v }, f64::total_cmp, OPERATION)?;
            let middle = order.len() / 2;
            let (left, right) = order.split_at_mut(middle);
            let left = Self::partition(ctx, records, left, start, 1 - axis, nodes)?;
            let right = Self::partition(ctx, records, right, start + middle, 1 - axis, nodes)?;
            let a = &nodes[left];
            let b = &nodes[right];
            ([a.bounds[0].min(b.bounds[0]), a.bounds[1].max(b.bounds[1]),
                a.bounds[2].min(b.bounds[2]), a.bounds[3].max(b.bounds[3])],
                a.finite && b.finite, Some((left, right)))
        };
        nodes[index].bounds = bounds;
        nodes[index].finite = finite;
        nodes[index].children = children;
        Ok(index)
    }

    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        query: &mut ArcCenterQuery<'_, '_>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "resolve SLDPRT unique arc center";
        ctx.any_by(std::iter::once(&self.nodes[index]), |node| {
            if !node.may_match(query.start, query.end, query.tolerance) { return Ok(false); }
            if let Some((left, right)) = node.children {
                return Ok(self.visit(ctx, left, query)? || self.visit(ctx, right, query)?);
            }
            ctx.any_by(&self.order[node.range.clone()], |index| {
                let record = &self.records[*index];
                if let Some(reference) = record.reference {
                    for excluded in query.excluded.into_iter().flatten() {
                        if ctx.equal(reference, excluded, OPERATION)? { return Ok(false); }
                    }
                }
                let center = record.point;
                let radius = (query.start.u - center.u).hypot(query.start.v - center.v);
                let end_radius = (query.end.u - center.u).hypot(query.end.v - center.v);
                if radius <= query.tolerance || (radius - end_radius).abs()
                    > query.tolerance * radius.abs().max(end_radius.abs()).max(1.0) { return Ok(false); }
                let start_angle = (query.start.v - center.v).atan2(query.start.u - center.u);
                let end_angle = (query.end.v - center.v).atan2(query.end.u - center.u);
                let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
                if sweep <= SKETCH_ANGLE_TOLERANCE || (std::f64::consts::TAU - sweep) <= SKETCH_ANGLE_TOLERANCE {
                    return Ok(false);
                }
                let cell = if self.tolerance == query.tolerance { record.cell } else { quantize(center, query.tolerance) };
                if query.cell.is_some_and(|previous| previous != cell) { return Ok(true); }
                query.cell = Some(cell);
                query.storage.with_storage(|| ctx.push_vec(&mut query.accepted, *index, OPERATION))?;
                Ok(false)
            }, OPERATION)
        }, OPERATION)
    }
}

impl ArcCenterNode {
    fn may_match(&self, start: Point2, end: Point2, tolerance: f64) -> bool {
        let du = end.u - start.u;
        let dv = end.v - start.v;
        let chord = du.hypot(dv);
        if !self.finite || !chord.is_finite() || chord == 0.0 || !tolerance.is_finite() || tolerance < 0.0 { return true; }
        let normal = [du / chord, dv / chord];
        let midpoint = [start.u + du * 0.5, start.v + dv * 0.5];
        let [umin, umax, vmin, vmax] = self.bounds;
        let (u0, u1) = if normal[0] < 0.0 { (umax, umin) } else { (umin, umax) };
        let (v0, v1) = if normal[1] < 0.0 { (vmax, vmin) } else { (vmin, vmax) };
        let lower = normal[0] * (u0 - midpoint[0]) + normal[1] * (v0 - midpoint[1]);
        let upper = normal[0] * (u1 - midpoint[0]) + normal[1] * (v1 - midpoint[1]);
        let separation = if lower > 0.0 { lower } else if upper < 0.0 { -upper } else { 0.0 };
        let radius_bound = [start, end].into_iter().map(|point| {
            (point.u - umin).abs().max((point.u - umax).abs())
                .hypot((point.v - vmin).abs().max((point.v - vmax).abs()))
        }).fold(0.0_f64, f64::max);
        // |r_start-r_end| = 2*chord*|projection|/(r_start+r_end).
        // Enlarge the bound for coordinate subtraction, projection and hypot rounding.
        let threshold = tolerance * radius_bound.max(1.0) * radius_bound / chord;
        let rounding = 128.0 * f64::EPSILON * (radius_bound + midpoint[0].abs() + midpoint[1].abs()
            + umin.abs() + umax.abs() + vmin.abs() + vmax.abs());
        !separation.is_finite() || !threshold.is_finite() || separation <= threshold + rounding
    }
}

struct ArcCenterQuery<'excluded, 'ctx> {
    start: Point2,
    end: Point2,
    tolerance: f64,
    excluded: [Option<&'excluded str>; 2],
    cell: Option<GridPoint>,
    accepted: Vec<usize>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(in crate::resolved_features) fn unique_arc_center_marker(
    ctx: &DecodeContext<'_>,
    start: Point2,
    end: Point2,
    candidates: &ArcCenterIndex<'_, '_>,
    tolerance: f64,
    excluded: [Option<&str>; 2],
) -> Result<Option<Point2>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT unique arc center";
    if start == end || candidates.nodes.is_empty() { return Ok(None); }
    let mut query = ArcCenterQuery {
        start, end, tolerance, excluded, cell: None, accepted: Vec::new(),
        storage: ctx.reserve_scoped(0, OPERATION)?,
    };
    if candidates.visit(ctx, 0, &mut query)? { return Ok(None); }
    ctx.sort_unstable_by_key(&mut query.accepted, |index| candidates.records[*index].ordinal, Ord::cmp, OPERATION)?;
    let mut centers = query.storage.with_storage(|| ctx.collect_vec(query.accepted.iter()
        .map(|index| { let record = &candidates.records[*index]; (quantize(record.point, tolerance), record.point) }), OPERATION))?;
    ctx.sort_unstable_by(&mut centers, |value| &value.0, Ord::cmp, OPERATION)?;
    ctx.dedup_by_key(&mut centers, |(center, _)| Ok(*center), "deduplicate SLDPRT unique arc center cells")?;
    Ok(match centers.as_slice() { [(_, center)] => Some(*center), _ => None })
}

#[cfg(test)]
mod tests;
