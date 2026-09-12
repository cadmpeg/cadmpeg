// SPDX-License-Identifier: Apache-2.0
use super::{FaceSelection, FeatureDirection3, FinitePoint3, PathRef};
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, Length};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The stages of a composite pattern nested inside a composite stage.
///
/// A composite stage applies one transform, never another sequence of stages,
/// so this type has no value and [`PatternTransform::Composite`] is
/// unreachable for a stage transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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

    /// Rebuilds the same kind of arm from edited stages.
    ///
    /// # Errors
    ///
    /// Returns the admission message when the stages do not compose.
    fn rebuild(stages: Vec<PatternStage>) -> Result<Self, &'static str>;
}

impl CompositeStages for CompositePattern {
    fn stages(&self) -> &[PatternStage] {
        &self.0
    }

    fn rebuild(stages: Vec<PatternStage>) -> Result<Self, &'static str> {
        Self::new(stages)
    }
}

impl CompositeStages for NoNestedComposite {
    fn stages(&self) -> &[PatternStage] {
        match *self {}
    }

    fn rebuild(_stages: Vec<PatternStage>) -> Result<Self, &'static str> {
        Err("a composite stage applies no nested sequence of stages")
    }
}

/// An admitted pattern with valid geometry, repetition counts, and stage composition.
///
/// `C` names the stages a composite arm applies.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternKind<C = CompositePattern>(PatternTransform<C>);

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
            PatternTransform::Linear {
                direction,
                spacing,
                count,
                second,
            } => {
                require(
                    direction.is_none_or(|direction| FeatureDirection3::new(direction).is_some()),
                    "pattern direction must have a finite nonzero norm",
                )?;
                require(spacing.get() > 0.0, "pattern spacing must be positive")?;
                require(*count > 0, "pattern count must be positive")?;
                if let Some(second) = second {
                    require(
                        FeatureDirection3::new(second.direction).is_some(),
                        "pattern second.direction must have a finite nonzero norm",
                    )?;
                    require(
                        second.spacing.get() > 0.0,
                        "pattern second.spacing must be positive",
                    )?;
                    require(second.count > 0, "pattern second.count must be positive")?;
                }
            }
            PatternTransform::LinearOffsets { direction, offsets } => {
                require(
                    direction.is_none_or(|direction| FeatureDirection3::new(direction).is_some()),
                    "pattern direction must have a finite nonzero norm",
                )?;
                require(
                    valid_increasing_locations(offsets.iter().map(|offset| offset.get())),
                    "pattern offsets must start at zero and strictly increase",
                )?;
            }
            PatternTransform::Circular {
                axis_origin,
                axis_dir,
                angle,
                count,
            } => {
                require(
                    FinitePoint3::new(*axis_origin).is_some(),
                    "pattern axis_origin must be finite",
                )?;
                require(
                    FeatureDirection3::new(*axis_dir).is_some(),
                    "pattern axis_dir must have a finite nonzero norm",
                )?;
                require(angle.get() > 0.0, "pattern angle must be positive")?;
                require(*count > 0, "pattern count must be positive")?;
            }
            PatternTransform::CircularAngles {
                axis_origin,
                axis_dir,
                angles,
            } => {
                require(
                    FinitePoint3::new(*axis_origin).is_some(),
                    "pattern axis_origin must be finite",
                )?;
                require(
                    FeatureDirection3::new(*axis_dir).is_some(),
                    "pattern axis_dir must have a finite nonzero norm",
                )?;
                require(
                    valid_increasing_locations(angles.iter().map(|angle| angle.get())),
                    "pattern angles must start at zero and strictly increase",
                )?;
            }
            PatternTransform::CurveDriven { spacing, count, .. } => {
                require(spacing.get() > 0.0, "pattern spacing must be positive")?;
                require(*count > 0, "pattern count must be positive")?;
            }
            PatternTransform::Mirror {
                plane_origin,
                plane_normal,
            } => {
                require(
                    FinitePoint3::new(*plane_origin).is_some(),
                    "pattern plane_origin must be finite",
                )?;
                require(
                    FeatureDirection3::new(*plane_normal).is_some(),
                    "pattern plane_normal must have a finite nonzero norm",
                )?;
            }
            PatternTransform::MirrorReference {
                plane: FaceSelection::Native(reference),
            } => {
                require(
                    !reference.is_empty(),
                    "pattern plane native reference must be nonempty",
                )?;
            }
            PatternTransform::MirrorReference { .. } => {}
            PatternTransform::Scale {
                center,
                final_factor,
                count,
            } => {
                if let PatternScaleCenter::Point(point) = center {
                    require(
                        FinitePoint3::new(*point).is_some(),
                        "pattern center point must be finite",
                    )?;
                }
                require(
                    final_factor.is_finite() && *final_factor > 0.0,
                    "pattern final_factor must be positive and finite",
                )?;
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

    /// Edits the transform and re-admits it, preserving the pattern on refusal.
    ///
    /// # Errors
    ///
    /// Returns the admission message when the edited transform is inadmissible.
    pub fn try_edit(&mut self, edit: impl FnOnce(&mut PatternTransform<C>)) -> Result<(), &'static str>
    where
        C: Clone,
    {
        let mut candidate = self.0.clone();
        edit(&mut candidate);
        *self = Self::new(candidate)?;
        Ok(())
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<PatternForm>,
    },
    /// Repeats seeds evenly along a straight direction.
    Linear {
        /// Repetition direction, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Distance between consecutive instances.
        spacing: Length,
        /// Total number of instances, including the original.
        count: u32,
        /// Optional complete second translation direction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<LinearPatternDirection>,
    },
    /// Repeats seeds at explicitly located distances along a straight direction.
    LinearOffsets {
        /// Repetition direction, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Cumulative distances from the original instance, beginning with zero.
        offsets: Vec<Length>,
    },
    /// Repeats seeds evenly around an axis.
    Circular {
        /// A point on the pattern axis.
        axis_origin: Point3,
        /// Unit direction of the pattern axis.
        axis_dir: Vector3,
        /// Angular span covered by the pattern.
        angle: Angle,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Repeats seeds at explicitly located angles around an axis.
    CircularAngles {
        /// A point on the pattern axis.
        axis_origin: Point3,
        /// Unit direction of the pattern axis.
        axis_dir: Vector3,
        /// Cumulative angles from the original instance, beginning with zero.
        angles: Vec<Angle>,
    },
    /// Repeats seeds at fixed arc-length spacing along a curve.
    CurveDriven {
        /// Pattern path, when its native reference is available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PathRef>,
        /// Arc-length spacing between consecutive instances.
        spacing: Length,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Reflects seeds across a plane.
    Mirror {
        /// A point on the mirror plane.
        plane_origin: Point3,
        /// Unit normal of the mirror plane.
        plane_normal: Vector3,
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
        final_factor: f64,
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

    /// The rule that combines the stage at `index` with the stages before it.
    ///
    /// The rule is the stage's position and its transform: the first stage
    /// establishes the sequence, a progressive scale slices the sequence it
    /// follows, and every other transform multiplies it.
    #[must_use]
    pub fn combination(&self, index: usize) -> Option<PatternStageCombination> {
        self.0
            .get(index)
            .map(|stage| stage_combination(index, stage))
    }

    /// Each stage paired with the rule that combines it with the stages
    /// before it.
    pub fn combinations(
        &self,
    ) -> impl Iterator<Item = (&PatternStage, PatternStageCombination)> + '_ {
        self.0
            .iter()
            .enumerate()
            .map(|(index, stage)| (stage, stage_combination(index, stage)))
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

fn composite_composition_is_valid(stages: &[crate::features::PatternStage]) -> bool {
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
    Point(Point3),
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
    /// Unit translation direction.
    pub direction: Vector3,
    /// Distance between consecutive instances.
    pub spacing: Length,
    /// Total number of instances, including the original.
    pub count: u32,
}
