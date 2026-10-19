// SPDX-License-Identifier: Apache-2.0
//! Exact edits of homogeneous NURBS control nets and parameter domains.
use cadmpeg_core::{
    decode::{index_from_u32, DecodeContext},
    CodecError,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::bezier::HomogeneousBezierSpan;
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{NonZeroReal, PositiveReal};

/// Control-point blends that one Bézier extraction, split, or degree
/// elevation of a curve may perform.
///
/// Splitting or elevating a span of degree `d` blends about `(d + 1)²`
/// controls, so [`bezier_blends`] bounds the work of a curve by its span
/// count. A curve over this limit is not processed, and its caller reports
/// a loss naming this limit.
pub(crate) const MAX_BEZIER_BLENDS: usize = 1 << 26;

/// The blends that processing `spans` Bézier spans of `degree` costs, or
/// `None` when the count does not fit `usize`.
pub(crate) fn bezier_blends(spans: usize, degree: usize) -> Option<usize> {
    let width = degree.checked_add(1)?;
    spans.checked_mul(width)?.checked_mul(width)
}

/// The nonempty spans of the non-decreasing `knots` inside `domain`: one
/// more than the distinct knots strictly between its ends.
pub(crate) fn bezier_span_count(knots: &[f64], domain: [f64; 2]) -> usize {
    let mut previous = None;
    let mut interior = 0_usize;
    for &knot in knots {
        if domain[0] < knot && knot < domain[1] && previous != Some(knot) {
            interior += 1;
        }
        previous = Some(knot);
    }
    interior + 1
}

/// How [`RefiningNet::insert`] admits each blended control.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlendCheck {
    /// Any blend whose knot denominator is finite and positive.
    Denominator,
    /// Also only an alpha in `[0, 1]` and a finite, positive-weight control.
    PositiveWeight,
    /// The rule of [`homogeneous_spans`]: alpha is the knot
    /// [`parameter_fraction`], and a knot may reach multiplicity
    /// `degree + 1`.
    ///
    /// [`homogeneous_spans`]: cadmpeg_ir::geometry::nurbs::bezier::homogeneous_spans
    /// [`parameter_fraction`]: cadmpeg_ir::math::parameter_fraction
    Extraction,
}

/// A vector split at a cursor: `head` holds a prefix in order and `tail`
/// holds the rest reversed, so moving the cursor by one is one push and pop.
struct SplitVec<T> {
    head: Vec<T>,
    tail: Vec<T>,
}

impl<T: Copy> SplitVec<T> {
    fn new(
        ctx: &DecodeContext<'_>,
        items: &[T],
        growth: usize,
        fill: T,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let capacity = items.len().checked_add(growth).ok_or_else(|| {
            CodecError::NotImplemented(format!("{operation} does not fit the platform"))
        })?;
        let mut head = ctx.alloc_filled(capacity, fill, operation)?;
        head.clear();
        let mut tail = ctx.alloc_filled(items.len(), fill, operation)?;
        tail.clear();
        tail.extend(items.iter().rev().copied());
        Ok(Self { head, tail })
    }

    fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    fn get(&self, index: usize) -> Option<T> {
        match index.checked_sub(self.head.len()) {
            None => self.head.get(index).copied(),
            Some(offset) => self
                .tail
                .len()
                .checked_sub(offset.checked_add(1)?)
                .map(|position| self.tail[position]),
        }
    }

    /// Moves the cursor so the head holds the first `len` items.
    fn seek(&mut self, len: usize) -> Option<()> {
        if len > self.len() {
            return None;
        }
        while self.head.len() < len {
            self.head.push(self.tail.pop()?);
        }
        while self.head.len() > len {
            self.tail.push(self.head.pop()?);
        }
        Some(())
    }

    /// The first index whose item fails `predicate`, which holds on a prefix.
    fn partition_point(&self, predicate: impl Fn(T) -> bool) -> Option<usize> {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if predicate(self.get(middle)?) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Some(low)
    }

    fn into_vec(mut self) -> Vec<T> {
        self.head.extend(self.tail.drain(..).rev());
        self.head
    }
}

/// A homogeneous control net and its knots under ascending knot insertion.
///
/// Each insertion blends the same controls with the same arithmetic as
/// Boehm insertion into contiguous vectors. Only the controls around the
/// insertion span sit at the cursors, so ascending insertions cost
/// `O(degree)` each instead of shifting the whole tail.
pub(crate) struct RefiningNet {
    knots: SplitVec<f64>,
    controls: SplitVec<[f64; 4]>,
}

impl RefiningNet {
    /// Stages `controls` and `knots` for `insertions` ascending insertions.
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        knots: &[f64],
        controls: &[[f64; 4]],
        insertions: usize,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            knots: SplitVec::new(ctx, knots, insertions, 0.0, "iges refined knots")?,
            controls: SplitVec::new(
                ctx,
                controls,
                insertions,
                [0.0; 4],
                "iges refined control points",
            )?,
        })
    }

    /// Inserts `value` once, below the multiplicity `check` admits.
    ///
    /// Insertions in ascending order move the cursors `O(degree)` each.
    pub(crate) fn insert(&mut self, value: f64, degree: usize, check: BlendCheck) -> Option<()> {
        let last_control = self.controls.len().checked_sub(1)?;
        let upper = self.knots.partition_point(|knot| knot <= value)?;
        if upper == self.knots.len() {
            return None;
        }
        let span = upper.checked_sub(1)?;
        let multiplicity = upper - self.knots.partition_point(|knot| knot < value)?;
        let left_end = span.checked_sub(degree)?;
        let tail_start = span.checked_sub(multiplicity)?;
        let ceiling = if check == BlendCheck::Extraction {
            degree.checked_add(1)?
        } else {
            degree
        };
        if multiplicity >= ceiling || last_control < degree || tail_start > last_control {
            return None;
        }
        let first_interior = left_end.checked_add(1)?;
        if check != BlendCheck::Extraction {
            for index in first_interior..=tail_start {
                let denominator = self.knots.get(index + degree)? - self.knots.get(index)?;
                if !denominator.is_finite() || denominator <= 0.0 {
                    return None;
                }
            }
        }
        self.controls.seek(tail_start + 1)?;
        let last = *self.controls.head.last()?;
        self.controls.head.push(last);
        for index in (first_interior..=tail_start).rev() {
            let low = self.knots.get(index)?;
            let high = self.knots.get(index + degree)?;
            let alpha = if check == BlendCheck::Extraction {
                cadmpeg_ir::math::parameter_fraction(value, low, high)?.get()
            } else {
                (value - low) / (high - low)
            };
            if check == BlendCheck::PositiveWeight
                && (!alpha.is_finite() || !(0.0..=1.0).contains(&alpha))
            {
                return None;
            }
            let previous = self.controls.head[index - 1];
            let current = self.controls.head[index];
            let point: [f64; 4] =
                std::array::from_fn(|axis| alpha * current[axis] + (1.0 - alpha) * previous[axis]);
            if check == BlendCheck::PositiveWeight && !homogeneous_point_is_valid(&point) {
                return None;
            }
            self.controls.head[index] = point;
        }
        self.knots.seek(upper)?;
        self.knots.head.push(value);
        Some(())
    }

    /// The refined knots and controls.
    pub(crate) fn into_parts(self) -> (Vec<f64>, Vec<[f64; 4]>) {
        (self.knots.into_vec(), self.controls.into_vec())
    }
}

/// The spans [`homogeneous_spans`] extracts, computed with its arithmetic in
/// time linear in the knots and the insertions.
///
/// [`homogeneous_spans`]: cadmpeg_ir::geometry::nurbs::bezier::homogeneous_spans
pub(crate) fn homogeneous_bezier_extraction(
    ctx: &DecodeContext<'_>,
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
) -> Result<Option<Vec<HomogeneousBezierSpan>>, CodecError> {
    let count = controls.len();
    let Some(expected_knots) = count
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
    else {
        return Ok(None);
    };
    if degree >= count
        || knots.len() != expected_knots
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || controls.iter().flatten().any(|value| !value.is_finite())
    {
        return Ok(None);
    }
    let domain = [knots[degree], knots[count]];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    let multiplicity = |value: f64| {
        knots.partition_point(|knot| *knot <= value) - knots.partition_point(|knot| *knot < value)
    };
    // The endpoints come first and the internal knots ascend, as in
    // `homogeneous_spans`, so each insertion sees the same net.
    let mut targets = Vec::new();
    targets.extend(domain.map(|end| (end, degree + 1)));
    for &knot in knots {
        if domain[0] < knot
            && knot < domain[1]
            && targets.last().is_none_or(|(value, _)| *value != knot)
        {
            targets.push((knot, degree));
        }
    }
    let insertions = targets
        .iter()
        .map(|(value, target)| (*target, multiplicity(*value)))
        .filter(|(target, multiplicity)| multiplicity < target)
        .map(|(target, multiplicity)| target - multiplicity)
        .sum();
    let mut net = RefiningNet::new(ctx, knots, controls, insertions)?;
    for (value, target) in targets {
        for _ in multiplicity(value)..target {
            if net.insert(value, degree, BlendCheck::Extraction).is_none() {
                return Ok(None);
            }
        }
    }
    let (knots, controls) = net.into_parts();
    let mut spans = Vec::new();
    for span in degree..controls.len() {
        let interval = [knots[span], knots[span + 1]];
        if interval[0] < interval[1] && interval[0] >= domain[0] && interval[1] <= domain[1] {
            let mut span_controls = Vec::new();
            span_controls.extend_from_slice(&controls[span - degree..=span]);
            spans.push(HomogeneousBezierSpan {
                domain: interval,
                controls: span_controls,
            });
        }
    }
    Ok((!spans.is_empty()).then_some(spans))
}

pub(crate) fn homogeneous_point_is_valid(point: &[f64; 4]) -> bool {
    point.iter().all(|value| value.is_finite()) && point[0] > 0.0
}

pub(crate) fn homogeneous_control_points(curve: &NurbsCurve) -> Option<Vec<[f64; 4]>> {
    let mut homogeneous = Vec::new();
    for index in 0..curve.pole_count() {
        let point = curve.pole_rows().point_at(index)?;
        let weight = if matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }) {
            let weight = curve.pole_rows().weight_at(index)?;
            weight
        } else {
            1.0
        };
        let homogeneous_point = [weight, weight * point.x, weight * point.y, weight * point.z];
        if !homogeneous_point_is_valid(&homogeneous_point) {
            return None;
        }
        homogeneous.push(homogeneous_point);
    }
    Some(homogeneous)
}

pub(crate) struct EuclideanControlNet {
    pub(crate) control_points: Vec<FinitePoint3>,
    pub(crate) weights: Option<Vec<PositiveReal>>,
}

pub(crate) fn euclidean_control_points(
    homogeneous: Vec<[f64; 4]>,
    rational: bool,
) -> Option<EuclideanControlNet> {
    let mut control_points = Vec::new();
    let mut weights = rational.then(Vec::new);
    for [weight, x, y, z] in homogeneous {
        let weight = PositiveReal::new(weight)?;
        let point = Point3::new(x / weight.get(), y / weight.get(), z / weight.get());
        let point = FinitePoint3::new(point)?;
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Some(EuclideanControlNet {
        control_points,
        weights,
    })
}

#[derive(Debug)]
pub(crate) struct InsertedKnotNet {
    pub(crate) control_points: Vec<[f64; 4]>,
    pub(crate) knots: Vec<f64>,
}

pub(crate) fn insert_homogeneous_knot(
    ctx: &DecodeContext<'_>,
    control_points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    value: f64,
) -> Result<Option<InsertedKnotNet>, CodecError> {
    let control_count = control_points.len();
    let Some(last_control) = control_count.checked_sub(1) else {
        return Ok(None);
    };
    let Some(span) = knots.iter().rposition(|knot| *knot <= value) else {
        return Ok(None);
    };
    let span = if degree == 0 {
        span.min(last_control)
    } else {
        span
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let Some(left_end) = span.checked_sub(degree) else {
        return Ok(None);
    };
    let Some(expected_knots) = control_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if control_count <= degree
        || left_end > last_control
        || multiplicity > degree
        || span < degree
        || knots.len() != expected_knots
    {
        return Ok(None);
    }
    let Some(knot_count) = knots.len().checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_knots = ctx.alloc_filled(knot_count, 0.0, "iges composite inserted knots")?;
    inserted_knots.clear();
    let Some(left_knots) = knots.get(..=span) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(left_knots);
    inserted_knots.push(value);
    let Some(next_knot) = span.checked_add(1) else {
        return Ok(None);
    };
    let Some(right_knots) = knots.get(next_knot..) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(right_knots);

    let Some(inserted_count) = control_count.checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_control_points = ctx.alloc_filled(
        inserted_count,
        [0.0; 4],
        "iges composite knot-insertion control points",
    )?;
    let Some(tail_start) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if tail_start > last_control {
        return Ok(None);
    }
    inserted_control_points[..=left_end].copy_from_slice(&control_points[..=left_end]);
    inserted_control_points[tail_start + 1..]
        .copy_from_slice(&control_points[tail_start..control_count]);
    let Some(first_interior) = left_end.checked_add(1) else {
        return Ok(None);
    };
    for index in first_interior..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (value - knots[index]) / denominator;
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Ok(None);
        }
        let previous = control_points[index - 1];
        let current = control_points[index];
        let point = [
            alpha * current[0] + (1.0 - alpha) * previous[0],
            alpha * current[1] + (1.0 - alpha) * previous[1],
            alpha * current[2] + (1.0 - alpha) * previous[2],
            alpha * current[3] + (1.0 - alpha) * previous[3],
        ];
        if !homogeneous_point_is_valid(&point) {
            return Ok(None);
        }
        inserted_control_points[index] = point;
    }
    Ok(Some(InsertedKnotNet {
        control_points: inserted_control_points,
        knots: inserted_knots,
    }))
}

pub(crate) type TrimmedLanes = (Vec<FinitePoint3>, Option<Vec<PositiveReal>>, Vec<f64>);

pub(crate) fn trim_nurbs_lanes(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<TrimmedLanes>, CodecError> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let control_count = curve.pole_count();
    if curve.periodic() {
        return Ok(None);
    }
    let [start, end] = interval;
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Ok(None);
    }
    let (Some(&domain_start), Some(&domain_end)) =
        (curve.knots().get(degree), curve.knots().get(control_count))
    else {
        return Ok(None);
    };
    if !domain_start.is_finite()
        || !domain_end.is_finite()
        || domain_start >= domain_end
        || start < domain_start
        || end > domain_end
    {
        return Ok(None);
    }
    let Some(mut homogeneous) = homogeneous_control_points(curve) else {
        return Ok(None);
    };
    let mut knots = curve.knots().to_vec();
    for value in [start, end] {
        let Some(target_multiplicity) = degree.checked_add(1) else {
            return Ok(None);
        };
        while knots.iter().filter(|knot| **knot == value).count() < target_multiplicity {
            let Some(InsertedKnotNet {
                control_points: new_homogeneous,
                knots: new_knots,
            }) = insert_homogeneous_knot(ctx, &homogeneous, &knots, degree, value)?
            else {
                return Ok(None);
            };
            homogeneous = new_homogeneous;
            knots = new_knots;
        }
    }
    let (Some(start_knot), Some(end_knot)) = (
        knots.iter().position(|knot| *knot == start),
        knots.iter().rposition(|knot| *knot == end),
    ) else {
        return Ok(None);
    };
    let Some(control_end) = end_knot.checked_sub(degree) else {
        return Ok(None);
    };
    if start_knot >= control_end {
        return Ok(None);
    }
    let (Some(homogeneous_slice), Some(knot_slice)) = (
        homogeneous.get(start_knot..control_end),
        knots.get(start_knot..=end_knot),
    ) else {
        return Ok(None);
    };
    let trimmed_homogeneous = homogeneous_slice.to_vec();
    let trimmed_knots = knot_slice.to_vec();
    let Some(expected_knots) = trimmed_homogeneous
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if trimmed_knots.len() != expected_knots {
        return Ok(None);
    }
    let Some(EuclideanControlNet {
        control_points,
        weights,
    }) = euclidean_control_points(
        trimmed_homogeneous,
        matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }),
    )
    else {
        return Ok(None);
    };
    Ok(Some((control_points, weights, trimmed_knots)))
}

/// Restrict the tensor-product basis to its active rectangle without changing
/// its parameter chart or approximating any pole or rational weight.
pub(crate) fn trim_surface(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    ranges: [[f64; 2]; 2],
) -> Result<Option<NurbsSurface>, CodecError> {
    let u_domain = [
        surface.u_knots()[index_from_u32(surface.u_degree())],
        surface.u_knots()[surface.u_count()],
    ];
    let v_domain = [
        surface.v_knots()[index_from_u32(surface.v_degree())],
        surface.v_knots()[surface.v_count()],
    ];
    let mut result = if ranges[0] == u_domain {
        surface.clone()
    } else {
        let Some(trimmed) = trim_surface_u(ctx, surface, ranges[0])? else {
            return Ok(None);
        };
        trimmed
    };
    if ranges[1] != v_domain {
        result.transpose_parameter_axes();
        let Some(trimmed) = trim_surface_u(ctx, &result, ranges[1])? else {
            return Ok(None);
        };
        result = trimmed;
        result.transpose_parameter_axes();
    }
    Ok(Some(result))
}

fn trim_surface_u(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    interval: [f64; 2],
) -> Result<Option<NurbsSurface>, CodecError> {
    let degree = index_from_u32(surface.u_degree());
    let end = interval[1];
    // At a fully repeated interior knot the source selects the right patch.
    // A clamped restriction selects the left patch at its terminal endpoint.
    // Distinct endpoint poles therefore cannot represent the same restriction.
    if end < surface.u_knots()[surface.u_count()]
        && surface
            .u_knots()
            .iter()
            .filter(|knot| **knot == end)
            .count()
            > degree
    {
        let Some(right) = surface
            .u_knots()
            .iter()
            .rposition(|knot| *knot <= end)
            .and_then(|span| span.checked_sub(degree))
        else {
            return Ok(None);
        };
        let Some(left) = right.checked_sub(1) else {
            return Ok(None);
        };
        if (0..surface.v_count()).any(|v| surface.pole(left, v) != surface.pole(right, v)) {
            return Ok(None);
        }
    }
    let mut columns = Vec::new();
    for v in 0..surface.v_count() {
        let points = (0..surface.u_count())
            .map(|u| surface.pole(u, v))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                CodecError::malformed("admitted NURBS surface has an incomplete pole grid")
            })?;
        let weights = if surface.weight(0, v).is_some() {
            Some(
                (0..surface.u_count())
                    .map(|u| surface.weight(u, v))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "admitted NURBS surface has an incomplete weight grid",
                        )
                    })?,
            )
        } else {
            None
        };
        let knots = surface.u_knots().to_vec();
        let curve = NurbsCurve::from_checked_lanes(
            surface.u_degree(),
            knots,
            points,
            weights,
            surface.u_periodic(),
        )
        .map_err(CodecError::malformed)?;
        let Some(column) = trim_nurbs_lanes(ctx, &curve, interval)? else {
            return Ok(None);
        };
        columns.push(column);
    }
    let Some(first) = columns.first() else {
        return Ok(None);
    };
    let count = first.0.len();
    if columns.iter().any(|column| {
        column.0.len() != count
            || column.2 != first.2
            || column.1.is_some() != first.1.is_some()
            || column
                .1
                .as_ref()
                .is_some_and(|weights| weights.len() != count)
    }) {
        return Ok(None);
    }
    let mut points = Vec::new();
    let mut weights = if first.1.is_some() {
        Some(Vec::new())
    } else {
        None
    };
    for u in 0..count {
        let mut row = Vec::new();
        let mut weight_row = if weights.is_some() {
            Some(Vec::new())
        } else {
            None
        };
        for column in &columns {
            row.push(column.0[u]);
            if let (Some(row), Some(column)) = (&mut weight_row, &column.1) {
                row.push(NonZeroReal::from(column[u]));
            }
        }
        points.push(row);
        if let (Some(rows), Some(row)) = (&mut weights, weight_row) {
            rows.push(row);
        }
    }
    let u_knots = first.2.clone();
    let v_knots = surface.v_knots().to_vec();
    NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(surface.u_degree(), u_knots, false),
        NurbsSurfaceAxis::new(surface.v_degree(), v_knots, surface.v_periodic()),
        NurbsSurfaceLanes::new(points, weights),
        surface.normal_reversed(),
    )
    .map(Some)
    .map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests;
