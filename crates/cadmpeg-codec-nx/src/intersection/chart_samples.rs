// SPDX-License-Identifier: Apache-2.0
//! Checked physical chart layouts and paired samples for solved charts.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::FitTolerance;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, Magnification, NonNegativeReal, NonZeroReal, PositiveReal};
use std::convert::Infallible;

/// At least two finite, spatially distinct chart points with native parameters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChartSamples {
    samples: crate::om::nonempty::NonEmpty<(FinitePoint3, FiniteReal)>,
}

impl ChartSamples {
    pub(crate) fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let values = ctx.collect_vec(self.samples.iter().copied(), operation)?;
        let samples = crate::om::nonempty::NonEmpty::from_admitted_vec(values)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 0))?;
        Ok(Self { samples })
    }
    fn from_xyz3_charged(
        ctx: &DecodeContext<'_>,
        points: Vec<FinitePoint3>,
        preamble: ChartPreamble,
    ) -> Result<Option<Self>, CodecError> {
        if points.len() < 2 {
            return Ok(None);
        }
        if !ctx.any_by(
            points.windows(2),
            |pair| Ok(pair[0] != pair[1]),
            "NX chart spatial variation",
        )? {
            return Ok(None);
        }
        let mut storage = ctx.reserve_scoped(0, "NX derived chart sample pairs")?;
        let mut samples = Vec::new();
        let mut parameter = preamble.base_parameter();
        let mut previous = None::<FinitePoint3>;
        let mut points = points.into_iter();
        while let Some(point) =
            ctx.next_charged(&mut points, "form NX derived chart sample pairs")?
        {
            if let Some(before) = previous {
                let chord_m = before.get().distance(point.get()) / 1000.0;
                parameter += chord_m * preamble.base_scale();
            }
            let Some(parameter) = FiniteReal::new(parameter) else {
                return Ok(None);
            };
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut samples,
                    (point, parameter),
                    "NX derived chart sample pairs",
                )
            })?;
            previous = Some(point);
        }
        let Some(samples) = crate::om::nonempty::NonEmpty::from_admitted_vec(samples) else {
            return Ok(None);
        };
        Ok(Some(storage.commit_value(Self { samples })?))
    }
    #[cfg(test)]
    fn new(points: Vec<FinitePoint3>, parameters: Vec<FiniteReal>) -> Result<Self, &'static str> {
        if points.len() != parameters.len() {
            return Err("native_parameters: one value per point required");
        }
        if !points.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err("points: distinct chart points required");
        }
        let samples = crate::om::nonempty::NonEmpty::new(points.into_iter().zip(parameters))
            .filter(|samples| samples.len() >= 2)
            .ok_or("points: at least two points required")?;
        Ok(Self { samples })
    }

    #[cfg(test)]
    pub(crate) fn from_test_values(
        points: Vec<Point3>,
        parameters: Vec<f64>,
    ) -> Result<Self, &'static str> {
        let points = points
            .into_iter()
            .map(|point| FinitePoint3::new(point).ok_or("points: coordinates must be finite"))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(
            points,
            parameters
                .into_iter()
                .map(|value| FiniteReal::new(value).ok_or("native_parameters: must be finite"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }

    #[cfg(test)]
    pub(crate) fn points(&self) -> Vec<Point3> {
        self.samples.iter().map(|sample| sample.0.get()).collect()
    }
    pub(crate) fn points_charged(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Point3>, CodecError> {
        ctx.collect_vec(
            self.samples.iter().map(|sample| sample.0.get()),
            "NX chart points",
        )
    }
    pub(crate) fn iter_points(&self) -> impl DoubleEndedIterator<Item = Point3> + '_ {
        self.samples.iter().map(|sample| sample.0.get())
    }
    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }
    #[cfg(test)]
    pub(crate) fn parameters(&self) -> Vec<f64> {
        self.samples.iter().map(|sample| sample.1.get()).collect()
    }
    pub(crate) fn parameters_charged(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<f64>, CodecError> {
        ctx.collect_vec(
            self.samples.iter().map(|sample| sample.1.get()),
            "NX chart parameters",
        )
    }

    pub(crate) fn endpoints(&self) -> [Point3; 2] {
        [self.samples.first().0.get(), self.samples.last().0.get()]
    }

    pub(crate) fn parameter_range(&self) -> [f64; 2] {
        [self.samples.first().1.get(), self.samples.last().1.get()]
    }

    /// Replace the parameterization when both charts have the same sample count.
    pub(super) fn replace_parameters_from_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
        other: &Self,
    ) -> Result<bool, CodecError> {
        if self.samples.len() != other.samples.len() || self.samples.len() < 2 {
            return Ok(false);
        }
        let replacement = ctx.collect_vec(
            self.samples
                .iter()
                .zip(other.samples.iter())
                .map(|(old, new)| (old.0, new.1)),
            "NX chart parameter replacement",
        )?;
        let Some(samples) = crate::om::nonempty::NonEmpty::from_admitted_vec(replacement) else {
            return Ok(false);
        };
        self.samples = samples;
        Ok(true)
    }
}

/// Serialized value used by both missing-parameter error slots.
pub(crate) const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;

/// Finite chart preamble with a nonzero scale and positive chordal error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChartPreamble {
    base_parameter: FiniteReal,
    base_scale: NonZeroReal,
    chordal_error: PositiveReal,
    angular_error: FiniteReal,
}
impl ChartPreamble {
    pub(crate) fn new(
        base_parameter: f64,
        base_scale: f64,
        chordal_error: f64,
        angular_error: f64,
    ) -> Result<Self, &'static str> {
        let base_parameter =
            FiniteReal::new(base_parameter).ok_or("base_parameter: must be finite")?;
        let base_scale =
            NonZeroReal::new(base_scale).ok_or("base_scale: must be finite and nonzero")?;
        let chordal_error =
            PositiveReal::new(chordal_error).ok_or("chordal_error: must be finite and positive")?;
        let angular_error =
            FiniteReal::new(angular_error).ok_or("angular_error: must be finite")?;
        Ok(Self {
            base_parameter,
            base_scale,
            chordal_error,
            angular_error,
        })
    }
    pub(crate) fn base_parameter(self) -> f64 {
        self.base_parameter.get()
    }
    pub(crate) fn base_scale(self) -> f64 {
        self.base_scale.get()
    }
    pub(crate) fn chordal_error(self) -> PositiveReal {
        self.chordal_error
    }
    /// The metre chordal error in millimetres, as the fit tolerance of the
    /// solved chart. A positive error stays non-negative after the
    /// conversion, so only an error that overflows has no tolerance.
    pub(crate) fn fit_tolerance(self) -> Option<FitTolerance> {
        FitTolerance::from(NonNegativeReal::from(self.chordal_error))
            .scaled(PositiveReal::from(Magnification::MILLIMETERS_PER_METER))
    }
    pub(crate) fn angular_error(self) -> f64 {
        self.angular_error.get()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SourceEncoding {
    Xyz3 {
        points: Vec<FinitePoint3>,
    },
    Ext11 {
        samples: crate::om::nonempty::NonEmpty<(FinitePoint3, FiniteReal)>,
        support_uv: super::SupportUv,
    },
}

/// A nonempty finite source point lane with its Hvec layout fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceChartData {
    encoding: SourceEncoding,
    count: u32,
}
impl SourceChartData {
    fn point_count(points: &[Point3]) -> Result<u32, &'static str> {
        if points.is_empty() {
            return Err("points: at least one point required");
        }
        u32::try_from(points.len()).map_err(|_| "points: count exceeds u32")
    }

    fn xyz3_with_storage<E>(
        points: &[Point3],
        mut checked: Vec<FinitePoint3>,
        mut next: impl FnMut(&mut std::slice::Iter<'_, Point3>) -> Result<Option<Point3>, E>,
        mut push: impl FnMut(&mut Vec<FinitePoint3>, FinitePoint3) -> Result<(), E>,
    ) -> Result<Result<Self, &'static str>, E> {
        let count = match Self::point_count(points) {
            Ok(count) => count,
            Err(error) => return Ok(Err(error)),
        };
        let mut points = points.iter();
        while let Some(point) = next(&mut points)? {
            let Some(point) = FinitePoint3::new(point) else {
                return Ok(Err("points: coordinates must be finite"));
            };
            push(&mut checked, point)?;
        }
        Ok(Ok(Self {
            count,
            encoding: SourceEncoding::Xyz3 { points: checked },
        }))
    }

    fn ext11_with_storage<E>(
        points: &[Point3],
        parameters: &[f64],
        support_uv: super::SupportUv,
        mut samples: Vec<(FinitePoint3, FiniteReal)>,
        mut next: impl FnMut(
            &mut std::iter::Zip<std::slice::Iter<'_, Point3>, std::slice::Iter<'_, f64>>,
        ) -> Result<Option<(Point3, f64)>, E>,
        mut push: impl FnMut(
            &mut Vec<(FinitePoint3, FiniteReal)>,
            (FinitePoint3, FiniteReal),
        ) -> Result<(), E>,
    ) -> Result<Result<Self, &'static str>, E> {
        let count = match Self::point_count(points) {
            Ok(count) => count,
            Err(error) => return Ok(Err(error)),
        };
        if parameters.len() != points.len() {
            return Ok(Err("native_parameters: one value per point required"));
        }
        if support_uv
            .iter()
            .flatten()
            .any(|lane| lane.as_slice().len() != points.len())
        {
            return Ok(Err("ext_support_uv: one pair per point required"));
        }
        let mut pairs = points.iter().zip(parameters);
        while let Some((point, parameter)) = next(&mut pairs)? {
            let Some(point) = FinitePoint3::new(point) else {
                return Ok(Err("points: coordinates must be finite"));
            };
            let Some(parameter) = FiniteReal::new(parameter) else {
                return Ok(Err(
                    "native_parameters: finite strictly increasing values required",
                ));
            };
            if samples
                .last()
                .is_some_and(|previous| parameter <= previous.1)
            {
                return Ok(Err(
                    "native_parameters: finite strictly increasing values required",
                ));
            }
            push(&mut samples, (point, parameter))?;
        }
        let Some(samples) = crate::om::nonempty::NonEmpty::from_admitted_vec(samples) else {
            return Ok(Err("points: at least one point required"));
        };
        Ok(Ok(Self {
            count,
            encoding: SourceEncoding::Ext11 {
                samples,
                support_uv,
            },
        }))
    }

    pub(crate) fn xyz3_charged(
        ctx: &DecodeContext<'_>,
        points: &[Point3],
    ) -> Result<Option<Self>, CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "NX finite chart points")?;
        let data = Self::xyz3_with_storage(
            points,
            Vec::new(),
            |points| Ok(ctx.next_charged(points, "admit NX xyz3 chart")?.copied()),
            |checked, point| {
                reservation.with_storage(|| ctx.push_vec(checked, point, "NX finite chart points"))
            },
        )?
        .ok();
        data.map(|value| reservation.commit_value(value))
            .transpose()
    }

    pub(crate) fn xyz3(points: &[Point3]) -> Result<Self, &'static str> {
        let checked = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(points.len()).map(|()| storage)
        }
        .map_err(|_| "points: storage allocation failed")?;
        match Self::xyz3_with_storage(
            points,
            checked,
            |points| Ok::<_, Infallible>(points.next().copied()),
            |checked, point| {
                checked.push(point);
                Ok(())
            },
        ) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }

    pub(crate) fn ext11_charged(
        ctx: &DecodeContext<'_>,
        points: &[Point3],
        parameters: &[f64],
        support_uv: [Option<Vec<[f64; 2]>>; 2],
    ) -> Result<Option<Self>, CodecError> {
        let [first, second] = support_uv;
        let (first_reservation, first) = match first {
            Some(values) => match super::SupportUvLane::from_present_values_scoped(ctx, &values)? {
                Some((lane, reservation)) => (Some(reservation), Some(lane)),
                None => return Ok(None),
            },
            None => (None, None),
        };
        let (second_reservation, second) = match second {
            Some(values) => match super::SupportUvLane::from_present_values_scoped(ctx, &values)? {
                Some((lane, reservation)) => (Some(reservation), Some(lane)),
                None => return Ok(None),
            },
            None => (None, None),
        };
        let mut reservation = ctx.reserve_scoped(0, "NX chart sample pairs")?;
        let data = Self::ext11_with_storage(
            points,
            parameters,
            [first, second],
            Vec::new(),
            |pairs| {
                Ok(ctx
                    .next_charged(pairs, "admit NX ext11 chart")?
                    .map(|(point, parameter)| (*point, *parameter)))
            },
            |samples, pair| {
                reservation.with_storage(|| ctx.push_vec(samples, pair, "NX chart sample pairs"))
            },
        )?
        .ok();
        let Some(data) = data else {
            return Ok(None);
        };
        let mut data = reservation.commit_value(data)?;
        for lane_reservation in [first_reservation, second_reservation]
            .into_iter()
            .flatten()
        {
            data = lane_reservation.commit_value(data)?;
        }
        Ok(Some(data))
    }

    pub(crate) fn ext11(
        points: &[Point3],
        parameters: &[f64],
        support_uv: [Option<Vec<[f64; 2]>>; 2],
    ) -> Result<Self, &'static str> {
        let [first, second] = support_uv.map(|lane| {
            lane.map(|values| {
                super::SupportUvLane::from_present_values(&values)
                    .ok_or("ext_support_uv: finite present parameter values required")
            })
            .transpose()
        });
        let samples = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(points.len()).map(|()| storage)
        }
        .map_err(|_| "points: storage allocation failed")?;
        match Self::ext11_with_storage(
            points,
            parameters,
            [first?, second?],
            samples,
            |pairs| {
                Ok::<_, Infallible>(pairs.next().map(|(point, parameter)| (*point, *parameter)))
            },
            |samples, pair| {
                samples.push(pair);
                Ok(())
            },
        ) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }

    #[cfg(test)]
    pub(crate) fn points(&self) -> Vec<Point3> {
        match &self.encoding {
            SourceEncoding::Xyz3 { points } => points.iter().map(|point| point.get()).collect(),
            SourceEncoding::Ext11 { samples, .. } => {
                samples.iter().map(|sample| sample.0.get()).collect()
            }
        }
    }
    pub(crate) fn point_at(&self, index: usize) -> Option<Point3> {
        match &self.encoding {
            SourceEncoding::Xyz3 { points } => points.get(index).map(|point| point.get()),
            SourceEncoding::Ext11 { samples, .. } => {
                samples.get(index).map(|sample| sample.0.get())
            }
        }
    }

    pub(crate) fn native_parameter_at(&self, index: usize) -> Option<f64> {
        match &self.encoding {
            SourceEncoding::Xyz3 { .. } => None,
            SourceEncoding::Ext11 { samples, .. } => {
                samples.get(index).map(|sample| sample.1.get())
            }
        }
    }

    pub(crate) fn support_uv_ref(&self) -> [Option<&super::SupportUvLane>; 2] {
        match &self.encoding {
            SourceEncoding::Xyz3 { .. } => [None, None],
            SourceEncoding::Ext11 { support_uv, .. } => support_uv.each_ref().map(Option::as_ref),
        }
    }
    pub(crate) fn count(&self) -> u32 {
        self.count
    }
    pub(crate) fn point_layout(&self) -> super::ChartPointLayout {
        match self.encoding {
            SourceEncoding::Xyz3 { .. } => super::ChartPointLayout::Xyz3,
            SourceEncoding::Ext11 { .. } => super::ChartPointLayout::Ext11,
        }
    }
    #[cfg(test)]
    pub(crate) fn native_parameters(&self) -> Option<Vec<f64>> {
        match &self.encoding {
            SourceEncoding::Xyz3 { .. } => None,
            SourceEncoding::Ext11 { samples, .. } => {
                Some(samples.iter().map(|sample| sample.1.get()).collect())
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn support_uv(&self) -> super::SupportUv {
        match &self.encoding {
            SourceEncoding::Xyz3 { .. } => [None, None],
            SourceEncoding::Ext11 { support_uv, .. } => support_uv.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn into_samples(
        self,
        preamble: ChartPreamble,
    ) -> Option<(ChartSamples, super::SupportUv)> {
        match self.encoding {
            SourceEncoding::Xyz3 { points } => {
                let mut parameter = preamble.base_parameter();
                let parameters = std::iter::once(parameter)
                    .chain(points.windows(2).map(|pair| {
                        let chord_m = pair[0].get().distance(pair[1].get()) / 1000.0;
                        parameter += chord_m * preamble.base_scale();
                        parameter
                    }))
                    .map(FiniteReal::new)
                    .collect::<Option<Vec<_>>>()?;
                Some((ChartSamples::new(points, parameters).ok()?, [None, None]))
            }
            SourceEncoding::Ext11 {
                samples,
                support_uv,
            } => {
                if samples.len() < 2
                    || !samples
                        .iter()
                        .zip(samples.iter().skip(1))
                        .any(|(a, b)| a.0 != b.0)
                {
                    return None;
                }
                Some((ChartSamples { samples }, support_uv))
            }
        }
    }

    pub(super) fn into_samples_charged(
        self,
        ctx: &DecodeContext<'_>,
        preamble: ChartPreamble,
    ) -> Result<Option<(ChartSamples, super::SupportUv)>, CodecError> {
        match self.encoding {
            SourceEncoding::Xyz3 { points } => {
                Ok(ChartSamples::from_xyz3_charged(ctx, points, preamble)?
                    .map(|samples| (samples, [None, None])))
            }
            SourceEncoding::Ext11 {
                samples,
                support_uv,
            } => {
                if samples.len() < 2 {
                    return Ok(None);
                }
                if !ctx.any_by(
                    samples.iter().zip(samples.iter().skip(1)),
                    |(first, second)| Ok(first.0 != second.0),
                    "admit NX chart carrier",
                )? {
                    return Ok(None);
                }
                Ok(Some((ChartSamples { samples }, support_uv)))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{ChartPreamble, ChartSamples, SourceChartData, MISSING_PARAMETER};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn chart_point_projection_refuses_collection_limit() {
        let samples = ChartSamples::from_test_values(
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            vec![0.0, 1.0],
        )
        .unwrap();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 1;
            },
            |ctx| {
                assert!(matches!(
                    samples.points_charged(ctx),
                    Err(CodecError::ResourceLimit(_))
                ));
            },
        );
    }

    #[test]
    fn chart_parameter_projection_refuses_retained_limit() {
        let samples = ChartSamples::from_test_values(
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            vec![0.0, 1.0],
        )
        .unwrap();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 15;
            },
            |ctx| {
                assert!(matches!(
                    samples.parameters_charged(ctx),
                    Err(CodecError::ResourceLimit(_))
                ));
            },
        );
    }

    #[test]
    fn chart_preamble_retains_finite_parameter_scale_and_angle() {
        let preamble = ChartPreamble::new(-0.0, -2.0, 0.25, 0.125).unwrap();
        assert_eq!(preamble.base_parameter().to_bits(), (-0.0_f64).to_bits());
        assert_eq!(preamble.base_scale(), -2.0);
        assert_eq!(preamble.angular_error(), 0.125);
        assert_eq!(
            ChartPreamble::new(f64::NAN, 1.0, 0.25, 0.0),
            Err("base_parameter: must be finite")
        );
        assert_eq!(
            ChartPreamble::new(0.0, 0.0, 0.25, 0.0),
            Err("base_scale: must be finite and nonzero")
        );
        assert_eq!(
            ChartPreamble::new(0.0, 1.0, 0.25, f64::INFINITY),
            Err("angular_error: must be finite")
        );
    }

    /// The chordal error is admitted positive once; its millimetre fit
    /// tolerance is refused only when the conversion overflows.
    #[test]
    fn a_chart_fit_tolerance_is_the_millimetre_chordal_error() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                ChartPreamble::new(0.0, 1.0, value, 0.0),
                Err("chordal_error: must be finite and positive")
            );
        }
        let preamble = ChartPreamble::new(0.0, 1.0, 1.0e-5, 0.0).unwrap();
        assert_eq!(preamble.chordal_error().get(), 1.0e-5);
        assert_eq!(
            preamble
                .fit_tolerance()
                .map(cadmpeg_ir::geometry::FitTolerance::get),
            Some(1.0e-5 * 1000.0)
        );
        let tiny = ChartPreamble::new(0.0, 1.0, f64::from_bits(1), 0.0).unwrap();
        assert!(tiny
            .fit_tolerance()
            .is_some_and(|tolerance| tolerance.get() > 0.0));
        let huge = ChartPreamble::new(0.0, 1.0, f64::MAX, 0.0).unwrap();
        assert_eq!(huge.fit_tolerance(), None);
    }

    #[test]
    fn chart_sample_constructor_rejects_truncation_in_both_directions() {
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ];
        assert_eq!(
            ChartSamples::from_test_values(points.clone(), vec![0.0, 1.0]),
            Err("native_parameters: one value per point required")
        );
        assert_eq!(
            ChartSamples::from_test_values(points[..2].to_vec(), vec![0.0, 1.0, 2.0]),
            Err("native_parameters: one value per point required")
        );
    }

    #[test]
    fn chart_samples_require_paired_values_and_two_endpoints() {
        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let make_samples = |points: &[Point3], parameters: &[f64]| {
            SourceChartData::ext11(points, parameters, [None, None]).and_then(|data| {
                data.into_samples(ChartPreamble::new(0.0, 1.0, 0.01, 0.0).unwrap())
                    .map(|pair| pair.0)
                    .ok_or("points: carrier requires two distinct points")
            })
        };
        assert!(make_samples(&[], &[]).is_err());
        assert!(make_samples(&[points[0]], &[0.0]).is_err());
        assert!(make_samples(&points, &[0.0]).is_err());
        assert!(make_samples(&points, &[0.0, 1.0, 2.0]).is_err());
        let samples = make_samples(&points, &[2.0, 5.0]).unwrap();
        assert_eq!(samples.points(), points);
        assert_eq!(samples.parameters(), [2.0, 5.0]);
        assert_eq!(samples.endpoints(), [points[0], points[1]]);
        assert_eq!(samples.parameter_range(), [2.0, 5.0]);
    }
    #[test]
    fn source_layouts_own_parameter_and_uv_constraints() {
        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let coincident = SourceChartData::xyz3(&[points[0]; 2]).unwrap();
        assert!(coincident
            .into_samples(ChartPreamble::new(0.0, 1.0, 0.01, 0.0).unwrap())
            .is_none());
        assert!(SourceChartData::xyz3(&[points[0], Point3::new(f64::INFINITY, 0.0, 0.0)]).is_err());
        assert!(SourceChartData::ext11(&points, &[2.0, 1.0], [None, None]).is_err());
        assert!(
            SourceChartData::ext11(&points, &[1.0, 2.0], [Some(vec![[0.0; 2]]), None]).is_err()
        );
        assert!(SourceChartData::ext11(
            &points,
            &[1.0, 2.0],
            [Some(vec![[MISSING_PARAMETER; 2]; 2]), None]
        )
        .is_err());
        let xyz = SourceChartData::xyz3(&points).unwrap();
        let (samples, uv) = xyz
            .into_samples(ChartPreamble::new(2.0, 1000.0, 0.01, 0.0).unwrap())
            .unwrap();
        assert_eq!(samples.points(), points);
        assert_eq!(samples.parameters(), [2.0, 3.0]);
        assert_eq!(uv, [None, None]);
    }
}

#[cfg(test)]
mod admission_tests {
    use super::{ChartPreamble, ChartSamples, SourceChartData};
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;

    fn points() -> Vec<FinitePoint3> {
        [0.0, 1000.0, 2000.0]
            .into_iter()
            .map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap())
            .collect()
    }

    #[test]
    fn xyz_chart_rejects_overflowed_derived_parameters() {
        crate::test_support::with_decode_context(|ctx| {
            let preamble = ChartPreamble::new(0.0, f64::MAX, 0.25, 0.0).unwrap();
            assert!(ChartSamples::from_xyz3_charged(ctx, points(), preamble)
                .unwrap()
                .is_none());
            let preamble = ChartPreamble::new(0.0, 2.0, 0.25, 0.0).unwrap();
            assert_eq!(
                ChartSamples::from_xyz3_charged(ctx, points(), preamble)
                    .unwrap()
                    .unwrap()
                    .parameters(),
                [0.0, 2.0, 4.0]
            );
        });
    }

    #[test]
    fn chart_pairing_refuses_work_before_source_pairing() {
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "admit NX ext11 chart",
            |ctx| {
                SourceChartData::ext11_charged(
                    ctx,
                    &points()
                        .into_iter()
                        .map(FinitePoint3::get)
                        .collect::<Vec<_>>(),
                    &[0.0; 3],
                    [None, None],
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "admit NX ext11 chart")
        );
    }

    #[test]
    fn chart_pairing_refuses_work_before_parameter_derivation() {
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "form NX derived chart sample pairs",
            |ctx| {
                ChartSamples::from_xyz3_charged(
                    ctx,
                    points(),
                    ChartPreamble::new(0.0, f64::MAX, 0.25, 0.0).unwrap(),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "form NX derived chart sample pairs")
        );
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::SourceChartData;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn chart_decode_and_wire_constructors_share_invariants() {
        let valid = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        for points in [
            valid.clone(),
            vec![valid[0]; 2],
            vec![valid[0]],
            vec![valid[0], Point3::new(f64::INFINITY, 0.0, 0.0)],
        ] {
            crate::test_support::with_decode_context(|ctx| {
                assert_eq!(
                    SourceChartData::xyz3_charged(ctx, &points).unwrap(),
                    SourceChartData::xyz3(&points).ok()
                );
            });
        }
        for parameters in [
            vec![0.0, 1.0],
            vec![1.0, 0.0],
            vec![0.0],
            vec![0.0, f64::INFINITY],
        ] {
            for lane in [
                None,
                Some(vec![[0.0; 2]; 2]),
                Some(vec![[super::MISSING_PARAMETER; 2]; 2]),
                Some(vec![[0.0; 2]]),
            ] {
                crate::test_support::with_decode_context(|ctx| {
                    assert_eq!(
                        SourceChartData::ext11_charged(
                            ctx,
                            &valid,
                            &parameters,
                            [lane.clone(), None]
                        )
                        .unwrap(),
                        SourceChartData::ext11(&valid, &parameters, [lane.clone(), None]).ok()
                    );
                });
            }
        }
    }

    #[test]
    fn chart_invariant_admission_preserves_storage_refusal() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_materialized_bytes = 0,
            |ctx| {
                let result = SourceChartData::xyz3_charged(
                    ctx,
                    &[Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                );
                assert!(
                    matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
                );
            },
        );
    }
}

#[cfg(test)]
mod physical_lane_tests {
    use super::{ChartPreamble, SourceChartData};
    use cadmpeg_ir::math::Point3;

    #[test]
    fn physical_single_point_charts_survive_without_a_carrier() {
        for ext in [false, true] {
            crate::test_support::with_decode_context(|ctx| {
                let points = vec![Point3::new(1.0, 2.0, 3.0)];
                let data = if ext {
                    SourceChartData::ext11_charged(ctx, &points, &[4.0], [None, None])
                } else {
                    SourceChartData::xyz3_charged(ctx, &points)
                }
                .unwrap()
                .unwrap();
                assert_eq!(data.count(), 1);
                assert_eq!(data.points(), points);
                assert!(data
                    .into_samples_charged(ctx, ChartPreamble::new(0.0, 1.0, 0.01, 0.0).unwrap())
                    .unwrap()
                    .is_none());
            });
        }
    }

    #[test]
    fn coincident_physical_charts_survive_without_a_carrier() {
        for ext in [false, true] {
            crate::test_support::with_decode_context(|ctx| {
                let points = vec![Point3::new(1.0, 2.0, 3.0); 2];
                let data = if ext {
                    SourceChartData::ext11_charged(ctx, &points, &[4.0, 5.0], [None, None])
                } else {
                    SourceChartData::xyz3_charged(ctx, &points)
                }
                .unwrap()
                .unwrap();
                assert_eq!(data.count(), 2);
                assert_eq!(data.points(), points);
                assert!(data
                    .into_samples_charged(ctx, ChartPreamble::new(0.0, 1.0, 0.01, 0.0).unwrap())
                    .unwrap()
                    .is_none());
            });
        }
    }
    #[test]
    fn xyz_chart_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "admit NX xyz3 chart",
            |ctx| {
                super::SourceChartData::xyz3_charged(
                    ctx,
                    &[cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)],
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "admit NX xyz3 chart"));
    }

    #[test]
    fn ext_chart_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "admit NX ext11 chart",
            |ctx| {
                super::SourceChartData::ext11_charged(
                    ctx,
                    &[cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)],
                    &[0.0],
                    [None, None],
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "admit NX ext11 chart"));
    }
}

#[cfg(test)]
mod validation_budget_tests {
    use super::SourceChartData;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn chart_validation_does_not_visit_an_invalid_points_suffix() {
        let mut points = vec![Point3::new(0.0, 0.0, 0.0); 4096];
        points[0] = Point3::new(f64::INFINITY, 0.0, 0.0);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 1;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                assert!(SourceChartData::xyz3_charged(ctx, &points)
                    .unwrap()
                    .is_none());
            },
        );
    }
}
