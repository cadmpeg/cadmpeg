// SPDX-License-Identifier: Apache-2.0
//! Reference target contracts and their diagnostic labels.
use serde::{Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReferenceExpectation {
    Type406Form11OrType212GeneralNote,
    ConstructiveSolidOrType186,
    ConstructiveSolid,
    Type186,
    Type132OrGroup,
    Type132,
    Type312OrType212,
    Type312,
    Type402Form11Or18,
    Type402Form20,

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
    Type180Form0Or1,
    Type212GeneralNote,
    Type212OrType312OrType402,
    Type214Form1Through12,
    Type304Form1Or2,
    Type310Form0,
    Type310Form0FontDefinition,
    Type314Form0,
    Type316OrType322OrType406OrType422,
    Type320OrType420,
    Type322Form0,
    Type302MatchingForm,
    Type306OrType416,
    StructureNotPermitted,
    Type304,
    Type406Form1,
    Type410OrType402Form3419,
    Type124,
    Type402Form5,
    Type314,
    Type { entity_type: i64, forms: Vec<i64> },
}
impl fmt::Display for ReferenceExpectation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type406Form11OrType212GeneralNote => {
                f.write_str("type-406-form-11-or-type-212-general-note")
            }
            Self::ConstructiveSolidOrType186 => f.write_str("constructive-solid-or-type-186"),
            Self::ConstructiveSolid => f.write_str("constructive-solid"),
            Self::Type186 => f.write_str("type-186"),
            Self::Type132OrGroup => f.write_str("type-132-or-group"),
            Self::Type132 => f.write_str("type-132"),
            Self::Type312OrType212 => f.write_str("type-312-or-type-212"),
            Self::Type312 => f.write_str("type-312"),
            Self::Type402Form11Or18 => f.write_str("type-402-form-11-or-18"),
            Self::Type402Form20 => f.write_str("type-402-form-20"),

            Self::ArrayBaseEntity => f.write_str("array-base-entity"),
            Self::CurveEntity => f.write_str("curve-entity"),
            Self::DimensionEntity => f.write_str("dimension-entity"),
            Self::DrawingSpaceAnnotation => f.write_str("drawing-space-annotation"),
            Self::ExistingDirectoryEntry => f.write_str("existing-directory-entry"),
            Self::MatchingFlowAssociativity => f.write_str("matching-flow-associativity"),
            Self::NonAssociativityOrType402Form7 => {
                f.write_str("non-associativity-or-type-402-form-7")
            }
            Self::ParameterizedCurve => f.write_str("parameterized-curve"),
            Self::PointDimensionEnclosure => f.write_str("point-dimension-enclosure"),
            Self::SectionBoundaryEntity => f.write_str("section-boundary-entity"),
            Self::SignalStringGeometry => f.write_str("signal-string-geometry"),
            Self::SubordinateAnnotationGeometry => f.write_str("subordinate-annotation-geometry"),
            Self::Type106Form40OrLeader => f.write_str("type-106-form-40-or-leader"),
            Self::Type124Transformation => f.write_str("type-124-transformation"),
            Self::Type180Form0Or1 => f.write_str("type-180-form-0-or-1"),
            Self::Type212GeneralNote => f.write_str("type-212-general-note"),
            Self::Type212OrType312OrType402 => f.write_str("type-212-or-type-312-or-type-402"),
            Self::Type214Form1Through12 => f.write_str("type-214-form-1-through-12"),
            Self::Type304Form1Or2 => f.write_str("type-304-form-1-or-2"),
            Self::Type310Form0 => f.write_str("type-310-form-0"),
            Self::Type310Form0FontDefinition => f.write_str("type-310-form-0-font-definition"),
            Self::Type314Form0 => f.write_str("type-314-form-0"),
            Self::Type316OrType322OrType406OrType422 => {
                f.write_str("type-316-or-type-322-or-type-406-or-type-422")
            }
            Self::Type320OrType420 => f.write_str("type-320-or-type-420"),
            Self::Type322Form0 => f.write_str("type-322-form-0"),
            Self::Type302MatchingForm => f.write_str("type-302-matching-form"),
            Self::Type306OrType416 => f.write_str("type-306-or-type-416"),
            Self::StructureNotPermitted => f.write_str("structure-not-permitted"),
            Self::Type304 => f.write_str("type-304"),
            Self::Type406Form1 => f.write_str("type-406-form-1"),
            Self::Type410OrType402Form3419 => f.write_str("type-410-or-type-402-form-3-4-19"),
            Self::Type124 => f.write_str("type-124"),
            Self::Type402Form5 => f.write_str("type-402-form-5"),
            Self::Type314 => f.write_str("type-314"),
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
        }
    }
}
impl Serialize for ReferenceExpectation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
