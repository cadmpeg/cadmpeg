// SPDX-License-Identifier: Apache-2.0
//! Pattern operands, admitted transforms, and stage composition.

use super::{
    BodySelection, FaceSelection, FeatureDirection3, FeatureId, FinitePoint3, PathRef,
    SelectionMembers,
};
use crate::ids::OccurrenceId;
use crate::scalar::{Angle, Length, PositiveAngle, PositiveLength, PositiveReal};
use crate::units::UnitVector3;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One geometric selection repeated or reflected by a pattern operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PatternSeed {
    /// Complete result of a preceding construction-history feature.
    Feature(FeatureId),
    /// Selected faces, including faces in an intermediate regenerated result.
    Faces(FaceSelection),
    /// Selected bodies, including bodies in an intermediate regenerated result.
    Bodies(BodySelection),
    /// Selected placed component occurrences.
    Occurrences(
        #[serde(deserialize_with = "deserialize_local_occurrences")] SelectionMembers<OccurrenceId>,
    ),
}

/// The stages of a composite pattern nested inside a composite stage.
///
/// A composite stage applies one transform, never another sequence of stages,
/// so this type has no value and [`PatternTransform::Composite`] is
/// unreachable for a stage transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum NoNestedComposite {}

/// A pattern transform a composite stage may apply.
pub type StagePatternKind = PatternKind<NoNestedComposite>;

/// What a composite pattern arm applies.
///
/// A top-level pattern applies a [`CompositePattern`]; a composite stage
/// applies nothing, so its implementation is over an uninhabited type and its
/// methods are unreachable.
pub trait CompositeStages: Sized {
    /// The stages, in application order.
    fn stages(&self) -> &[PatternStage];

    /// Build the same kind of arm from stages.
    ///
    /// # Errors
    ///
    /// Returns the admission message when the stages do not compose.
    fn rebuild(stages: Vec<PatternStage>) -> Result<Self, &'static str>;

    /// Scale the length-bearing fields of every stage while carrying the
    /// admitted stage count, order, and operand relationships.
    fn try_map_stage_lengths<E>(
        &self,
        edit: &mut impl FnMut(PatternLengthField<'_>) -> Result<(), E>,
    ) -> Result<Self, PatternLengthEditError<E>>;
}

impl CompositeStages for CompositePattern {
    fn stages(&self) -> &[PatternStage] {
        &self.0
    }

    fn rebuild(stages: Vec<PatternStage>) -> Result<Self, &'static str> {
        Self::new(stages)
    }

    fn try_map_stage_lengths<E>(
        &self,
        edit: &mut impl FnMut(PatternLengthField<'_>) -> Result<(), E>,
    ) -> Result<Self, PatternLengthEditError<E>> {
        let mut stages = self.0.clone();
        for stage in &mut stages {
            *stage.pattern = stage.pattern.try_map_lengths(edit)?;
        }
        Ok(Self(stages))
    }
}

impl CompositeStages for NoNestedComposite {
    fn stages(&self) -> &[PatternStage] {
        match *self {}
    }

    fn rebuild(_stages: Vec<PatternStage>) -> Result<Self, &'static str> {
        Err("a composite stage applies no nested sequence of stages")
    }

    fn try_map_stage_lengths<E>(
        &self,
        _edit: &mut impl FnMut(PatternLengthField<'_>) -> Result<(), E>,
    ) -> Result<Self, PatternLengthEditError<E>> {
        match *self {}
    }
}

/// An admitted pattern with valid geometry, repetition counts, and stage composition.
///
/// `C` names the stages a composite arm applies.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternKind<C = CompositePattern>(PatternTransform<C>);

/// A pattern field whose value changes under a length-unit conversion.
pub enum PatternLengthField<'a> {
    /// A finite distance, including a cumulative linear offset.
    Length(&'a mut Length),
    /// A positive distance between instances.
    PositiveLength(&'a mut PositiveLength),
    /// A point in model coordinates.
    Point(&'a mut FinitePoint3),
}

/// A failed length edit or a collapsed ordered offset sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternLengthEditError<E> {
    /// The field conversion failed.
    Field(E),
    /// The converted offsets are no longer strictly increasing.
    Offsets(&'static str),
}

impl<C: CompositeStages + Clone> PatternKind<C> {
    /// Edit length-bearing geometry while retaining admitted unscaled operands.
    ///
    /// # Errors
    ///
    /// Returns a field error or the offset-order refusal when rounding merges offsets.
    pub fn try_map_lengths<E>(
        &self,
        edit: &mut impl FnMut(PatternLengthField<'_>) -> Result<(), E>,
    ) -> Result<Self, PatternLengthEditError<E>> {
        let mut transform = self.0.clone();
        match &mut transform {
            PatternTransform::Linear {
                spacing, second, ..
            } => {
                edit(PatternLengthField::PositiveLength(spacing))
                    .map_err(PatternLengthEditError::Field)?;
                if let Some(second) = second {
                    edit(PatternLengthField::PositiveLength(&mut second.spacing))
                        .map_err(PatternLengthEditError::Field)?;
                }
            }
            PatternTransform::LinearOffsets { offsets, .. } => {
                for offset in offsets.iter_mut() {
                    edit(PatternLengthField::Length(offset))
                        .map_err(PatternLengthEditError::Field)?;
                }
                if !valid_increasing_locations(offsets.iter().map(|offset| offset.get())) {
                    return Err(PatternLengthEditError::Offsets(
                        "pattern offsets must start at zero and strictly increase",
                    ));
                }
            }
            PatternTransform::CurveDriven { spacing, .. } => {
                edit(PatternLengthField::PositiveLength(spacing))
                    .map_err(PatternLengthEditError::Field)?;
            }
            PatternTransform::Circular { axis_origin, .. }
            | PatternTransform::CircularAngles { axis_origin, .. } => {
                edit(PatternLengthField::Point(axis_origin))
                    .map_err(PatternLengthEditError::Field)?;
            }
            PatternTransform::Mirror { plane_origin, .. } => {
                edit(PatternLengthField::Point(plane_origin))
                    .map_err(PatternLengthEditError::Field)?;
            }
            PatternTransform::Composite { stages } => {
                *stages = stages.try_map_stage_lengths(edit)?;
            }
            PatternTransform::Scale { center, .. } => {
                if let PatternScaleCenter::Point(point) = center {
                    edit(PatternLengthField::Point(point))
                        .map_err(PatternLengthEditError::Field)?;
                }
            }
            PatternTransform::Unresolved { .. } | PatternTransform::MirrorReference { .. } => {}
        }
        Ok(Self(transform))
    }
}

impl<C> PatternKind<C> {
    /// An unresolved pattern with no identified form.
    pub const UNRESOLVED: Self = Self(PatternTransform::Unresolved { form: None });

    /// An unresolved linear pattern.
    pub const UNRESOLVED_LINEAR: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::Linear),
    });

    /// An unresolved circular pattern.
    pub const UNRESOLVED_CIRCULAR: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::Circular),
    });

    /// An unresolved curve-driven pattern.
    pub const UNRESOLVED_CURVE_DRIVEN: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::CurveDriven),
    });

    /// An unresolved mirror pattern.
    pub const UNRESOLVED_MIRROR: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::Mirror),
    });

    /// An unresolved scale pattern.
    pub const UNRESOLVED_SCALE: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::Scale),
    });

    /// An unresolved composite pattern.
    pub const UNRESOLVED_COMPOSITE: Self = Self(PatternTransform::Unresolved {
        form: Some(PatternForm::Composite),
    });

    /// Admits pattern geometry, repetition counts, and ordered stage composition.
    pub fn new(transform: PatternTransform<C>) -> Result<Self, &'static str> {
        let require =
            |condition: bool, message: &'static str| condition.then_some(()).ok_or(message);
        match &transform {
            PatternTransform::Unresolved { .. } => {}
            PatternTransform::Linear { count, second, .. } => {
                require(*count > 0, "pattern count must be positive")?;
                if let Some(second) = second {
                    require(second.count > 0, "pattern second.count must be positive")?;
                }
            }
            PatternTransform::LinearOffsets { offsets, .. } => {
                require(
                    valid_increasing_locations(offsets.iter().map(|offset| offset.get())),
                    "pattern offsets must start at zero and strictly increase",
                )?;
            }
            PatternTransform::Circular { count, .. } => {
                require(*count > 0, "pattern count must be positive")?;
            }
            PatternTransform::CircularAngles { angles, .. } => {
                require(
                    valid_increasing_locations(angles.iter().map(|angle| angle.get())),
                    "pattern angles must start at zero and strictly increase",
                )?;
            }
            PatternTransform::CurveDriven { count, .. } => {
                require(*count > 0, "pattern count must be positive")?;
            }
            PatternTransform::Mirror { .. } => {}
            PatternTransform::MirrorReference {
                plane: FaceSelection::Native(reference),
            } => {
                require(
                    !reference.is_empty(),
                    "pattern plane native reference must be nonempty",
                )?;
            }
            PatternTransform::MirrorReference { .. } => {}
            PatternTransform::Scale { count, .. } => {
                require(*count >= 2, "scale pattern count must be at least two")?;
            }
            PatternTransform::Composite { .. } => {}
        }
        Ok(Self(transform))
    }

    /// Returns the admitted transform definition.
    pub fn definition(&self) -> &PatternTransform<C> {
        &self.0
    }

    /// Returns the curve path slot without exposing repetition geometry.
    pub fn curve_path_mut(&mut self) -> Option<&mut Option<PathRef>> {
        match &mut self.0 {
            PatternTransform::CurveDriven { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Returns whether the pattern form lacks its required operands.
    pub const fn is_unresolved(&self) -> bool {
        matches!(self.0, PatternTransform::Unresolved { .. })
    }
}

impl PatternTransform<NoNestedComposite> {
    /// The same transform under an arm that may apply a sequence of stages.
    #[must_use]
    pub fn widen<C>(self) -> PatternTransform<C> {
        match self {
            Self::Unresolved { form } => PatternTransform::Unresolved { form },
            Self::Linear {
                direction,
                spacing,
                count,
                second,
            } => PatternTransform::Linear {
                direction,
                spacing,
                count,
                second,
            },
            Self::LinearOffsets { direction, offsets } => {
                PatternTransform::LinearOffsets { direction, offsets }
            }
            Self::Circular {
                axis_origin,
                axis_dir,
                angle,
                count,
            } => PatternTransform::Circular {
                axis_origin,
                axis_dir,
                angle,
                count,
            },
            Self::CircularAngles {
                axis_origin,
                axis_dir,
                angles,
            } => PatternTransform::CircularAngles {
                axis_origin,
                axis_dir,
                angles,
            },
            Self::CurveDriven {
                path,
                spacing,
                count,
            } => PatternTransform::CurveDriven {
                path,
                spacing,
                count,
            },
            Self::Mirror {
                plane_origin,
                plane_normal,
            } => PatternTransform::Mirror {
                plane_origin,
                plane_normal,
            },
            Self::MirrorReference { plane } => PatternTransform::MirrorReference { plane },
            Self::Scale {
                center,
                final_factor,
                count,
            } => PatternTransform::Scale {
                center,
                final_factor,
                count,
            },
            Self::Composite { stages } => match stages {},
        }
    }
}

impl StagePatternKind {
    /// The same admitted pattern under an arm that may apply a sequence.
    #[must_use]
    pub fn widen<C>(self) -> PatternKind<C> {
        PatternKind(self.0.widen())
    }
}

/// Identified pattern form for a pattern that is not yet resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PatternForm {
    /// Repeats seeds along a straight direction.
    Linear,
    /// Repeats seeds around an axis.
    Circular,
    /// Repeats seeds along a curve.
    CurveDriven,
    /// Reflects seeds across a plane.
    Mirror,
    /// Repeats seeds using progressive uniform scales.
    Scale,
    /// Applies an ordered sequence of pattern stages.
    Composite,
}

/// Spatial transform used to repeat or reflect seed features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternTransform<C = CompositePattern> {
    /// Pattern construction that is not resolved; `form` names it when the
    /// source identified one.
    Unresolved {
        /// Identified pattern form, when the source established one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<PatternForm>,
    },
    /// Repeats seeds evenly along a straight direction.
    Linear {
        /// Repetition direction, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<FeatureDirection3>,
        /// Distance between consecutive instances.
        spacing: PositiveLength,
        /// Total number of instances, including the original.
        count: u32,
        /// Optional complete second translation direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_second"
        )]
        second: Option<LinearPatternDirection>,
    },
    /// Repeats seeds at explicitly located distances along a straight direction.
    LinearOffsets {
        /// Repetition direction, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<FeatureDirection3>,
        /// Cumulative distances from the original instance, beginning with zero.
        offsets: Vec<Length>,
    },
    /// Repeats seeds evenly around an axis.
    Circular {
        /// A point on the pattern axis.
        axis_origin: FinitePoint3,
        /// Direction of the pattern axis with a finite nonzero norm.
        axis_dir: FeatureDirection3,
        /// Angular span covered by the pattern.
        angle: PositiveAngle,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Repeats seeds at explicitly located angles around an axis.
    CircularAngles {
        /// A point on the pattern axis.
        axis_origin: FinitePoint3,
        /// Unit direction of the pattern axis.
        axis_dir: UnitVector3,
        /// Cumulative angles from the original instance, beginning with zero.
        angles: Vec<Angle>,
    },
    /// Repeats seeds at fixed arc-length spacing along a curve.
    CurveDriven {
        /// Pattern path, when its native reference is available.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path"
        )]
        path: Option<PathRef>,
        /// Arc-length spacing between consecutive instances.
        spacing: PositiveLength,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Reflects seeds across a plane.
    Mirror {
        /// A point on the mirror plane.
        plane_origin: FinitePoint3,
        /// Normal of the mirror plane with a finite nonzero norm.
        plane_normal: FeatureDirection3,
    },
    /// Reflects seeds across a source-native plane selection whose frame is not resolved.
    MirrorReference {
        /// Plane or planar face that defines the reflection.
        plane: FaceSelection,
    },
    /// Repeats seeds using progressive uniform scales.
    Scale {
        /// Fixed locus used by every scale transform.
        center: PatternScaleCenter,
        /// Scale factor of the final instance relative to the original.
        final_factor: PositiveReal,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Applies an ordered sequence of pattern stages.
    Composite {
        /// Stages in application order.
        stages: C,
    },
}

/// Ordered composite-pattern stages whose combination rules and occurrence
/// counts compose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vec<PatternStage>", into = "Vec<PatternStage>")]
pub struct CompositePattern(Vec<PatternStage>);

impl CompositePattern {
    /// Admits ordered stages whose occurrence counts compose.
    pub fn new(stages: Vec<PatternStage>) -> Result<Self, &'static str> {
        if stages.is_empty() {
            return Err("pattern stages must be nonempty");
        }
        if !composite_composition_is_valid(&stages) {
            return Err("pattern stage counts must not overflow and must divide aligned slices");
        }
        Ok(Self(stages))
    }
}

/// The rule that combines the stage at `index` with the stages before it.
fn stage_combination(index: usize, stage: &PatternStage) -> PatternStageCombination {
    if index == 0 {
        PatternStageCombination::Initialize
    } else if matches!(stage.pattern.definition(), PatternTransform::Scale { .. }) {
        PatternStageCombination::AlignedSlices
    } else {
        PatternStageCombination::CartesianProduct
    }
}

impl std::ops::Deref for CompositePattern {
    type Target = [PatternStage];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<Vec<PatternStage>> for CompositePattern {
    type Error = &'static str;

    fn try_from(stages: Vec<PatternStage>) -> Result<Self, Self::Error> {
        Self::new(stages)
    }
}

impl From<CompositePattern> for Vec<PatternStage> {
    fn from(pattern: CompositePattern) -> Self {
        pattern.0
    }
}

impl<'a> IntoIterator for &'a CompositePattern {
    type Item = &'a PatternStage;
    type IntoIter = std::slice::Iter<'a, PatternStage>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<C: Serialize> Serialize for PatternKind<C> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de, C: Deserialize<'de>> Deserialize<'de> for PatternKind<C> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(PatternTransform::<C>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl<C: JsonSchema> JsonSchema for PatternKind<C> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PatternKind".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        PatternTransform::<C>::json_schema(generator)
    }
}

fn composite_composition_is_valid(stages: &[crate::features::patterns::PatternStage]) -> bool {
    let mut occurrences = None;
    stages.iter().enumerate().all(|(index, stage)| {
        let Some(stage_count) = pattern_occurrence_count(stage.pattern.definition()) else {
            return true;
        };
        if index == 0 {
            occurrences = Some(stage_count);
            return true;
        }
        match stage_combination(index, stage) {
            PatternStageCombination::CartesianProduct => {
                if let Some(count) = occurrences {
                    occurrences = count.checked_mul(stage_count);
                    occurrences.is_some()
                } else {
                    true
                }
            }
            PatternStageCombination::AlignedSlices => {
                occurrences.is_none_or(|count| count % stage_count == 0)
            }
            PatternStageCombination::Initialize => false,
        }
    })
}

fn pattern_occurrence_count<C>(pattern: &PatternTransform<C>) -> Option<usize> {
    match pattern {
        PatternTransform::Linear { count, .. }
        | PatternTransform::Circular { count, .. }
        | PatternTransform::CurveDriven { count, .. }
        | PatternTransform::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternTransform::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternTransform::CircularAngles { angles, .. } => Some(angles.len()),
        PatternTransform::Mirror { .. } | PatternTransform::MirrorReference { .. } => Some(2),
        PatternTransform::Unresolved { .. } | PatternTransform::Composite { .. } => None,
    }
}

fn valid_increasing_locations(locations: impl Iterator<Item = f64>) -> bool {
    let mut locations = locations;
    let Some(first) = locations.next() else {
        return false;
    };
    first == 0.0
        && locations
            .try_fold(first, |previous, location| {
                (location.is_finite() && location > previous).then_some(location)
            })
            .is_some()
}

/// Fixed locus for a progressive pattern scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PatternScaleCenter {
    /// Volume centroid of the first seed feature.
    FirstSeedCentroid,
    /// Explicit model-space point.
    Point(FinitePoint3),
    /// Format-native center reference.
    Native(String),
}

/// One stage of an ordered composite pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PatternStage {
    /// Pattern transform contributed by this stage; never a nested sequence.
    pub pattern: Box<StagePatternKind>,
}

/// Combination rule for a composite-pattern stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PatternStageCombination {
    /// Establishes the initial transform sequence.
    Initialize,
    /// Applies each new transform to every preceding transform.
    CartesianProduct,
    /// Aligns transforms with equally sized slices of preceding occurrences.
    AlignedSlices,
}

/// Complete secondary direction of a two-direction linear pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LinearPatternDirection {
    /// Translation direction with a finite nonzero norm.
    pub direction: FeatureDirection3,
    /// Distance between consecutive instances.
    pub spacing: PositiveLength,
    /// Total number of instances, including the original.
    pub count: u32,
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_form, PatternForm, "form");
cadmpeg_core::named_optional_field!(deserialize_direction, FeatureDirection3, "direction");
cadmpeg_core::named_optional_field!(deserialize_second, LinearPatternDirection, "second");
cadmpeg_core::named_optional_field!(deserialize_path, PathRef, "path");

selection_field_deserializer!(deserialize_local_occurrences, "occurrences");

#[cfg(test)]
mod tests;
