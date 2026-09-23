// SPDX-License-Identifier: Apache-2.0
//! Checked direction owners and byte-extent overlap for native record invariants.

use std::marker::PhantomData;

use cadmpeg_ir::math::Vector3;
use serde::{Deserialize, Serialize};

/// A tolerance on a direction's measured length deviating from one. Each
/// implementing type names one tolerance, so a direction admits only within a
/// tolerance it names. A tolerance is a copyable marker, so the direction it
/// selects stays `Copy`. Each constructor states which measurement of the
/// direction the tolerance is applied to.
pub(crate) trait DeviationTolerance: Copy {
    /// The largest admitted deviation of the measured length from one.
    const TOLERANCE: f64;
}

/// Admits a measured length within `1e-9` of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RelaxedDeviation;

impl DeviationTolerance for RelaxedDeviation {
    const TOLERANCE: f64 = 1.0e-9;
}

/// Admits a measured length within `1e-12` of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExactDeviation;

impl DeviationTolerance for ExactDeviation {
    const TOLERANCE: f64 = 1.0e-12;
}

/// A measurement of a direction's length. Each implementing type names one
/// measurement, so a direction admits only by a measurement it names. A
/// measurement is a copyable marker, so the direction it selects stays `Copy`.
pub(crate) trait LengthMeasurement: Copy {
    /// The direction's measured length.
    fn length(value: [f64; 3]) -> f64;
}

/// Measures a direction's length as the sum of its squared components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SquaredLength;

impl LengthMeasurement for SquaredLength {
    fn length(value: [f64; 3]) -> f64 {
        value[0] * value[0] + value[1] * value[1] + value[2] * value[2]
    }
}

/// Measures a direction's length as the square root of the sum of its squared
/// components, the way `Vector3::norm` does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NormLength;

impl LengthMeasurement for NormLength {
    fn length(value: [f64; 3]) -> f64 {
        SquaredLength::length(value).sqrt()
    }
}

/// Measures a direction's length as a chain of `hypot` calls, the way the
/// records that store a planar pair do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HypotLength;

impl LengthMeasurement for HypotLength {
    fn length(value: [f64; 3]) -> f64 {
        value[0].hypot(value[1]).hypot(value[2])
    }
}

/// A finite direction admitted by `Measurement` of its length deviating from
/// one by at most `Tolerance`. Both the measurement and the tolerance are the
/// record grammar's, so every route into the type — literal construction,
/// derivation and serde — admits exactly the same directions.
///
/// Every measurement and tolerance admits a subset of the IR
/// [`cadmpeg_ir::units::UnitVector3`] set, whose admission is the `hypot`
/// length within `1e-9` of one. A squared length within `1e-9` of one puts
/// the norm within about `5e-10` of one, and the square-root and `hypot`
/// measurements differ from each other by a few units in the last place. The
/// type holds the admitted IR direction, so the conversion into it is total.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub(crate) struct UnitVector3<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>(
    cadmpeg_ir::units::UnitVector3,
    PhantomData<(Tolerance, Measurement)>,
);

/// A direction whose squared length is one to `1e-12`.
pub(crate) type ExactUnitVector3 = UnitVector3<ExactDeviation, SquaredLength>;

/// A direction whose norm is one to `1e-12`.
pub(crate) type ExactNormUnitVector3 = UnitVector3<ExactDeviation, NormLength>;

/// A direction whose `hypot` length is one to `1e-12`.
pub(crate) type ExactHypotUnitVector3 = UnitVector3<ExactDeviation, HypotLength>;

/// A direction whose squared length is one to `1e-9`.
pub(crate) type RelaxedUnitVector3 = UnitVector3<RelaxedDeviation, SquaredLength>;

/// A direction whose `hypot` length is one to `1e-9`.
pub(crate) type RelaxedHypotUnitVector3 = UnitVector3<RelaxedDeviation, HypotLength>;

/// A finite planar direction whose `hypot` length is one within `Tolerance`.
///
/// Every tolerance is at most the `1e-9` of the IR
/// [`cadmpeg_ir::units::UnitVector2`] admission, which measures the same
/// `hypot` length. The type holds the admitted IR direction, so its quarter
/// turns and the frames placed from it are the IR's length-preserving routes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub(crate) struct UnitVector2<Tolerance: DeviationTolerance>(
    cadmpeg_ir::units::UnitVector2,
    PhantomData<Tolerance>,
);

/// A planar direction stored loosely, to a deviation tolerance of `1e-9`.
pub(crate) type RelaxedUnitVector2 = UnitVector2<RelaxedDeviation>;

impl<Tolerance: DeviationTolerance> UnitVector2<Tolerance> {
    /// Constructs a planar direction whose `hypot` length is one within the
    /// tolerance.
    pub(crate) fn from_hypot(value: [f64; 2]) -> Option<Self> {
        if !(value.iter().all(|component| component.is_finite())
            && (value[0].hypot(value[1]) - 1.0).abs() <= Tolerance::TOLERANCE)
        {
            return None;
        }
        cadmpeg_ir::units::UnitVector2::new(value).map(|direction| Self(direction, PhantomData))
    }

    /// Returns the direction components.
    #[cfg(test)]
    pub(crate) fn get(self) -> [f64; 2] {
        self.0.get()
    }

    /// Turns the direction a quarter turn, to `[-second, first]`.
    #[cfg(test)]
    pub(crate) fn quarter_turn(self) -> Self {
        Self(self.0.quarter_turn(), PhantomData)
    }

    /// Turns the direction a quarter turn the other way, to `[second, -first]`.
    pub(crate) fn reverse_quarter_turn(self) -> Self {
        Self(self.0.reverse_quarter_turn(), PhantomData)
    }
}

impl<Tolerance: DeviationTolerance> TryFrom<[f64; 2]> for UnitVector2<Tolerance> {
    type Error = String;

    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::from_hypot(value)
            .ok_or_else(|| "planar direction is not a finite unit vector".to_owned())
    }
}

impl<Tolerance: DeviationTolerance> From<UnitVector2<Tolerance>> for [f64; 2] {
    fn from(value: UnitVector2<Tolerance>) -> Self {
        value.0.get()
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    UnitVector3<Tolerance, Measurement>
{
    /// Constructs a unit direction whose measured length is one within the
    /// tolerance.
    pub(crate) fn new(value: [f64; 3]) -> Option<Self> {
        if !(value.iter().all(|component| component.is_finite())
            && (Measurement::length(value) - 1.0).abs() <= Tolerance::TOLERANCE)
        {
            return None;
        }
        cadmpeg_ir::units::UnitVector3::new(Vector3::from(value))
            .map(|direction| Self(direction, PhantomData))
    }

    /// Constructs a unit direction from a `scale`-scaled stored vector whose
    /// `hypot` length is `scale` within the tolerance. The divided components
    /// must also pass this type's unit-length admission.
    pub(crate) fn from_scaled(stored: [f64; 3], scale: f64) -> Option<Self> {
        let length = stored[0].hypot(stored[1]).hypot(stored[2]);
        if !(length.is_finite() && ((length / scale) - 1.0).abs() <= Tolerance::TOLERANCE) {
            return None;
        }
        Self::new(stored.map(|component| component / scale))
    }

    /// Returns the direction components.
    pub(crate) fn get(self) -> [f64; 3] {
        (*self.0.as_raw()).into()
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    From<UnitVector3<Tolerance, Measurement>> for cadmpeg_ir::units::UnitVector3
{
    fn from(value: UnitVector3<Tolerance, Measurement>) -> Self {
        value.0
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement> TryFrom<[f64; 3]>
    for UnitVector3<Tolerance, Measurement>
{
    type Error = String;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "direction is not a finite unit vector".to_owned())
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    From<UnitVector3<Tolerance, Measurement>> for [f64; 3]
{
    fn from(value: UnitVector3<Tolerance, Measurement>) -> Self {
        value.get()
    }
}

/// Two perpendicular directions, each admitted by `Measurement` within
/// `Tolerance`, held as the admitted IR frame.
///
/// Every route stores directions that carry this type's direction admission:
/// the constant is exact, [`Self::right_handed`] stores two admitted
/// directions unchanged, and the planar routes place an admitted planar
/// direction with its `hypot` length bit for bit. The accessors therefore
/// return admitted directions without a second test, and the conversion into
/// the IR frame is total.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnitFrame3<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>(
    cadmpeg_ir::units::OrthonormalFrame3,
    PhantomData<(Tolerance, Measurement)>,
);

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    UnitFrame3<Tolerance, Measurement>
{
    /// The frame with first direction +X and second direction +Y. Every
    /// measurement gives these directions a length of exactly one.
    pub(crate) const X_AXIS_Y_REFERENCE: Self = Self(
        cadmpeg_ir::units::OrthonormalFrame3::X_AXIS_Y_REFERENCE,
        PhantomData,
    );

    /// Admits the frame of `axis` and `reference` when the three admitted
    /// directions form a right-handed orthonormal frame by the IR
    /// right-handed frame route: `reference · binormal` is zero and
    /// `reference × binormal` equals `axis`, each within `1e-12`.
    pub(crate) fn right_handed(
        axis: UnitVector3<Tolerance, Measurement>,
        reference: UnitVector3<Tolerance, Measurement>,
        binormal: UnitVector3<Tolerance, Measurement>,
    ) -> Option<Self> {
        cadmpeg_ir::units::OrthonormalFrame3::right_handed(axis.0, reference.0, binormal.0)
            .map(|frame| Self(frame, PhantomData))
    }

    /// Returns the first direction.
    pub(crate) fn axis(self) -> UnitVector3<Tolerance, Measurement> {
        UnitVector3(*self.0.axis(), PhantomData)
    }

    /// Returns the second direction.
    pub(crate) fn reference(self) -> UnitVector3<Tolerance, Measurement> {
        UnitVector3(*self.0.reference(), PhantomData)
    }
}

impl<Tolerance: DeviationTolerance> UnitFrame3<Tolerance, HypotLength> {
    /// Places a planar direction in the XY plane as the first direction, with
    /// its quarter turn `[-second, first, 0]` as the second direction.
    pub(crate) fn in_xy_plane(axis: UnitVector2<Tolerance>) -> Self {
        Self(
            cadmpeg_ir::units::OrthonormalFrame3::in_xy_plane(axis.0),
            PhantomData,
        )
    }

    /// Takes +Y as the first direction and places a planar direction in the
    /// XZ plane, as `[first, 0, second]`, as the second direction.
    pub(crate) fn about_y_axis(reference: UnitVector2<Tolerance>) -> Self {
        Self(
            cadmpeg_ir::units::OrthonormalFrame3::about_y_axis(reference.0),
            PhantomData,
        )
    }
}

impl<Tolerance: DeviationTolerance, Measurement: LengthMeasurement>
    From<UnitFrame3<Tolerance, Measurement>> for cadmpeg_ir::units::OrthonormalFrame3
{
    fn from(value: UnitFrame3<Tolerance, Measurement>) -> Self {
        value.0
    }
}

/// A byte-extent width that reports its own overflow on addition.
pub(crate) trait ByteExtent: Copy + Ord {
    /// The sum, or `None` when the sum leaves the width.
    fn checked_sum(self, other: Self) -> Option<Self>;
}

impl ByteExtent for u64 {
    fn checked_sum(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }
}

impl ByteExtent for usize {
    fn checked_sum(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }
}

/// Whether two byte extents share at least one byte. An extent whose end
/// leaves the width overlaps nothing.
pub(crate) fn extents_overlap<Extent: ByteExtent>(
    first_start: Extent,
    first_len: Extent,
    second_start: Extent,
    second_len: Extent,
) -> bool {
    first_start
        .checked_sum(first_len)
        .zip(second_start.checked_sum(second_len))
        .is_some_and(|(first_end, second_end)| first_start < second_end && second_start < first_end)
}

#[cfg(test)]
mod tests {
    use super::{
        DeviationTolerance, ExactDeviation, ExactHypotUnitVector3, ExactNormUnitVector3,
        ExactUnitVector3, HypotLength, RelaxedDeviation, RelaxedHypotUnitVector3,
        RelaxedUnitVector2, RelaxedUnitVector3, UnitFrame3,
    };

    type RelaxedHypotFrame = UnitFrame3<RelaxedDeviation, HypotLength>;

    /// The pair placed in the XY and the XZ plane, through the frames that
    /// place it.
    fn placements(pair: RelaxedUnitVector2) -> [RelaxedHypotUnitVector3; 2] {
        [
            RelaxedHypotFrame::in_xy_plane(pair).axis(),
            RelaxedHypotFrame::about_y_axis(pair).reference(),
        ]
    }

    #[test]
    fn exact_directions_reject_the_relaxed_tolerance_band() {
        let value = [0.0, 0.0, (1.0_f64 + 5.0e-10).sqrt()];
        assert!(RelaxedUnitVector3::new(value).is_some());
        assert!(ExactUnitVector3::new(value).is_none());
        assert!(serde_json::from_value::<ExactUnitVector3>(serde_json::json!(value)).is_err());
        assert!(serde_json::from_value::<RelaxedUnitVector3>(serde_json::json!(value)).is_ok());
    }

    #[test]
    fn norm_measured_directions_match_the_record_norm_predicate() {
        for component in [
            1.0_f64,
            1.0 + 9.0e-13,
            1.0 - 9.0e-13,
            1.0 + 2.0e-12,
            1.0 - 2.0e-12,
        ] {
            let value = [0.0, component, 0.0];
            let norm = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
            let admitted = (norm - 1.0).abs() <= ExactDeviation::TOLERANCE;
            assert_eq!(ExactNormUnitVector3::new(value).is_some(), admitted);
        }
    }

    #[test]
    fn scaled_directions_reject_a_rounded_length_that_hides_a_nonunit_result() {
        let smallest = f64::from_bits(1);
        assert!(ExactHypotUnitVector3::from_scaled([smallest, smallest, 0.0], smallest).is_none());
        assert_eq!(
            ExactHypotUnitVector3::from_scaled([smallest, 0.0, 0.0], smallest)
                .map(ExactHypotUnitVector3::get),
            Some([1.0, 0.0, 0.0])
        );
        for scale in [0.0, f64::NAN, f64::INFINITY, -1.0] {
            assert!(ExactHypotUnitVector3::from_scaled([1.0, 0.0, 0.0], scale).is_none());
        }
    }

    #[test]
    fn planar_placements_deserialize_wherever_the_pair_was_admitted() {
        let component = 1.0 + 6.0e-10;
        let pair = RelaxedUnitVector2::from_hypot([component, 0.0]).expect("pair inside the band");
        for placed in placements(pair) {
            let wire = serde_json::to_value(placed).expect("serialize placed direction");
            assert_eq!(
                serde_json::from_value::<RelaxedHypotUnitVector3>(wire)
                    .expect("a placed direction deserializes wherever its pair was admitted"),
                placed
            );
        }
        assert!(RelaxedUnitVector3::new([component, 0.0, 0.0]).is_none());
    }

    #[test]
    fn admitted_directions_convert_to_the_ir_direction_with_the_same_components() {
        fn same_ir_direction(components: [f64; 3], converted: cadmpeg_ir::units::UnitVector3) {
            let raw = *converted.as_raw();
            assert_eq!(
                [raw.x, raw.y, raw.z].map(f64::to_bits),
                components.map(f64::to_bits)
            );
            assert_eq!(
                cadmpeg_ir::units::UnitVector3::new(raw),
                Some(converted),
                "the IR admission accepts every value the codec admission accepts"
            );
        }

        let relaxed_edge = [0.0, 0.0, (1.0_f64 + 9.9e-10).sqrt()];
        let relaxed = RelaxedUnitVector3::new(relaxed_edge).expect("squared length in the band");
        same_ir_direction(relaxed.get(), relaxed.into());
        let exact_edge = [0.0, (1.0_f64 + 9.9e-13).sqrt(), 0.0];
        let exact = ExactUnitVector3::new(exact_edge).expect("squared length in the band");
        same_ir_direction(exact.get(), exact.into());
        let norm = ExactNormUnitVector3::new([0.6, 0.8 + 9.0e-13, 0.0]).expect("norm in the band");
        same_ir_direction(norm.get(), norm.into());
        let hypot = RelaxedHypotUnitVector3::new([1.0 + 9.9e-10, 0.0, 0.0]).expect("hypot band");
        same_ir_direction(hypot.get(), hypot.into());
        let exact_hypot =
            ExactHypotUnitVector3::new([0.0, 0.0, -1.0 - 9.0e-13]).expect("hypot band");
        same_ir_direction(exact_hypot.get(), exact_hypot.into());
        for constant in [
            RelaxedHypotFrame::X_AXIS_Y_REFERENCE.axis(),
            RelaxedHypotFrame::X_AXIS_Y_REFERENCE.reference(),
        ] {
            same_ir_direction(constant.get(), constant.into());
        }

        let pair = RelaxedUnitVector2::from_hypot([0.6 * (1.0 + 9.9e-10), 0.8 * (1.0 + 9.9e-10)])
            .expect("pair inside the band");
        for turned in [pair, pair.quarter_turn(), pair.reverse_quarter_turn()] {
            for placed in placements(turned) {
                assert_eq!(RelaxedHypotUnitVector3::new(placed.get()), Some(placed));
                same_ir_direction(placed.get(), placed.into());
            }
        }
        let [first, second] = pair.get();
        assert_eq!(pair.quarter_turn().get(), [-second, first]);
        assert_eq!(pair.reverse_quarter_turn().get(), [second, -first]);
        assert_eq!(
            placements(pair)[1].get().map(f64::to_bits),
            [first, 0.0, second].map(f64::to_bits)
        );
    }

    #[test]
    fn frames_return_the_directions_they_admitted_and_convert_to_the_same_ir_frame() {
        let pair = RelaxedUnitVector2::from_hypot([0.6 * (1.0 + 9.9e-10), -0.8])
            .expect("pair inside the band");
        let [first, second] = pair.get();
        let bits = |direction: RelaxedHypotUnitVector3| direction.get().map(f64::to_bits);
        let xy = RelaxedHypotFrame::in_xy_plane(pair);
        assert_eq!(bits(xy.axis()), [first, second, 0.0].map(f64::to_bits));
        assert_eq!(
            bits(xy.reference()),
            [-second, first, 0.0].map(f64::to_bits)
        );
        let about_y = RelaxedHypotFrame::about_y_axis(pair);
        assert_eq!(bits(about_y.axis()), [0.0, 1.0, 0.0].map(f64::to_bits));
        assert_eq!(
            bits(about_y.reference()),
            [first, 0.0, second].map(f64::to_bits)
        );
        let constant = RelaxedHypotFrame::X_AXIS_Y_REFERENCE;
        assert_eq!(bits(constant.axis()), [1.0, 0.0, 0.0].map(f64::to_bits));
        assert_eq!(
            bits(constant.reference()),
            [0.0, 1.0, 0.0].map(f64::to_bits)
        );
        for frame in [xy, about_y, constant] {
            for direction in [frame.axis(), frame.reference()] {
                assert_eq!(
                    RelaxedHypotUnitVector3::new(direction.get()),
                    Some(direction)
                );
            }
            assert_eq!(
                cadmpeg_ir::units::OrthonormalFrame3::from(frame),
                cadmpeg_ir::units::OrthonormalFrame3::from_units(
                    frame.axis().into(),
                    frame.reference().into()
                )
                .expect("perpendicular")
            );
        }
    }

    #[test]
    fn right_handed_frames_hold_the_directions_they_admitted() {
        let axis = ExactUnitVector3::new([0.0, 0.0, 1.0]).expect("unit");
        let reference = ExactUnitVector3::new([1.0, 0.0, 0.0]).expect("unit");
        let binormal = ExactUnitVector3::new([0.0, 1.0, 0.0]).expect("unit");
        let reversed = ExactUnitVector3::new([0.0, -1.0, 0.0]).expect("unit");
        let tilted = ExactUnitVector3::new([1.0, 0.0, 2.0e-9]).expect("unit");
        let skewed = ExactUnitVector3::new([2.0e-9, 1.0, 0.0]).expect("unit");
        let frame = UnitFrame3::right_handed(axis, reference, binormal).expect("right-handed");
        assert_eq!((frame.axis(), frame.reference()), (axis, reference));
        assert_eq!(
            cadmpeg_ir::units::OrthonormalFrame3::from(frame),
            cadmpeg_ir::units::OrthonormalFrame3::right_handed(
                axis.into(),
                reference.into(),
                binormal.into(),
            )
            .expect("right-handed")
        );
        assert!(UnitFrame3::right_handed(axis, reference, reversed).is_none());
        assert!(UnitFrame3::right_handed(axis, tilted, binormal).is_none());
        assert!(UnitFrame3::right_handed(axis, reference, skewed).is_none());
    }

    #[test]
    fn scaled_directions_accept_exactly_the_stored_length_ratio() {
        let radius = 5.0_f64;
        for factor in [1.0, 1.0 + 9.0e-13, 1.0 - 9.0e-13, 1.0 + 2.0e-12] {
            let stored = [0.0, 0.0, radius * factor];
            let length = stored[0].hypot(stored[1]).hypot(stored[2]);
            let admitted = ((length / radius) - 1.0).abs() <= ExactDeviation::TOLERANCE;
            assert_eq!(
                ExactHypotUnitVector3::from_scaled(stored, radius).is_some(),
                admitted
            );
        }
    }
}
