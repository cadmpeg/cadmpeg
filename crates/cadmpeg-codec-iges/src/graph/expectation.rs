// SPDX-License-Identifier: Apache-2.0
//! Reference target contracts and their diagnostic labels.
use serde::{Serialize, Serializer};
use std::fmt;

/// A reference target contract with its stable diagnostic form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReferenceExpectation {
    Type {
        entity_type: i64,
        forms: Vec<i64>,
    },
    AnyOf {
        first: i64,
        second: i64,
        rest: Vec<i64>,
    },
    Named(ExpectationLabel),
}

/// A semantic or annotated reference target label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectationLabel {
    Type406Form11OrType212GeneralNote,
    ConstructiveSolidOrType186,
    ConstructiveSolid,
    Type132OrGroup,
    ArrayBaseEntity,
    CurveEntity,
    DimensionEntity,
    DrawingSpaceAnnotation,
    ExistingDirectoryEntry,
    MatchingFlowAssociativity,
    NonAssociativityOrType402Form7,
    ParameterizedCurve,
    PointDimensionEnclosure,
    SectionBoundaryEntity,
    SignalStringGeometry,
    SubordinateAnnotationGeometry,
    Type106Form40OrLeader,
    Type124Transformation,
    Type212GeneralNote,
    Type214Form1Through12,
    Type310Form0FontDefinition,
    Type302MatchingForm,
    StructureNotPermitted,
    Type410OrType402Form3419,
}

impl fmt::Display for ExpectationLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Type406Form11OrType212GeneralNote => "type-406-form-11-or-type-212-general-note",
            Self::ConstructiveSolidOrType186 => "constructive-solid-or-type-186",
            Self::ConstructiveSolid => "constructive-solid",
            Self::Type132OrGroup => "type-132-or-group",
            Self::ArrayBaseEntity => "array-base-entity",
            Self::CurveEntity => "curve-entity",
            Self::DimensionEntity => "dimension-entity",
            Self::DrawingSpaceAnnotation => "drawing-space-annotation",
            Self::ExistingDirectoryEntry => "existing-directory-entry",
            Self::MatchingFlowAssociativity => "matching-flow-associativity",
            Self::NonAssociativityOrType402Form7 => "non-associativity-or-type-402-form-7",
            Self::ParameterizedCurve => "parameterized-curve",
            Self::PointDimensionEnclosure => "point-dimension-enclosure",
            Self::SectionBoundaryEntity => "section-boundary-entity",
            Self::SignalStringGeometry => "signal-string-geometry",
            Self::SubordinateAnnotationGeometry => "subordinate-annotation-geometry",
            Self::Type106Form40OrLeader => "type-106-form-40-or-leader",
            Self::Type124Transformation => "type-124-transformation",
            Self::Type212GeneralNote => "type-212-general-note",
            Self::Type214Form1Through12 => "type-214-form-1-through-12",
            Self::Type310Form0FontDefinition => "type-310-form-0-font-definition",
            Self::Type302MatchingForm => "type-302-matching-form",
            Self::StructureNotPermitted => "structure-not-permitted",
            Self::Type410OrType402Form3419 => "type-410-or-type-402-form-3-4-19",
        })
    }
}

impl fmt::Display for ReferenceExpectation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type { entity_type, forms } => {
                write!(f, "type-{entity_type}")?;
                if let Some((first, rest)) = forms.split_first() {
                    write!(f, "-form-{first}")?;
                    for form in rest {
                        write!(f, "-or-{form}")?;
                    }
                }
                Ok(())
            }
            Self::AnyOf {
                first,
                second,
                rest,
            } => {
                write!(f, "type-{first}-or-type-{second}")?;
                for entity_type in rest {
                    write!(f, "-or-type-{entity_type}")?;
                }
                Ok(())
            }
            Self::Named(label) => label.fmt(f),
        }
    }
}

impl Serialize for ReferenceExpectation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
