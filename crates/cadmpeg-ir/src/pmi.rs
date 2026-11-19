// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information independent of design history.

use std::num::NonZeroU32;

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::dimension::Dimension;
use crate::design::options::NativeOptions;
use crate::design::parameter::Tolerance;
use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, OccurrenceId, PmiId, PointId, ProductDefinitionId, VertexId,
};
use crate::references::InstanceTarget;
use crate::scalar::{Length, PositiveLength};
use crate::transform::Transform;

pub mod zones;

use zones::ToleranceZone;

/// A model object qualified by an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PmiTarget {
    /// Entire shape body.
    Body {
        /// Qualified body.
        body: BodyId,
    },
    /// Topological face.
    Face {
        /// Qualified face.
        face: FaceId,
    },
    /// Topological edge.
    Edge {
        /// Qualified edge.
        edge: EdgeId,
    },
    /// Topological vertex.
    Vertex {
        /// Qualified vertex.
        vertex: VertexId,
    },
    /// Geometric point.
    Point {
        /// Qualified point.
        point: PointId,
    },
    /// Geometric curve carrier.
    Curve {
        /// Qualified curve.
        curve: CurveId,
    },
    /// Product prototype.
    Product {
        /// Qualified product.
        product: ProductDefinitionId,
    },
    /// Placed product occurrence.
    Occurrence {
        /// Qualified occurrence.
        occurrence: OccurrenceId,
    },
    /// Source shape-aspect identity whose geometric target is not resolved.
    ShapeAspect {
        /// Stable source identity of the unresolved aspect.
        #[serde(deserialize_with = "deserialize_source_id")]
        source_id: cadmpeg_core::text::NonBlankString,
    },
}

/// Numeric semantic-PMI quantity in canonical units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PmiValue {
    /// Numeric value in millimeters, radians, or unitless ratio as selected by
    /// `quantity`.
    #[serde(deserialize_with = "deserialize_pmi_value")]
    pub value: crate::scalar::FiniteReal,
    /// Physical quantity and canonical unit of `value`.
    pub quantity: PmiQuantity,
}

crate::units::named_field!(deserialize_pmi_value, crate::scalar::FiniteReal, "value");

impl PmiValue {
    /// Build a semantic quantity from an admitted finite value.
    #[must_use]
    pub const fn from_parts(value: crate::scalar::FiniteReal, quantity: PmiQuantity) -> Self {
        Self { value, quantity }
    }

    /// Construct a finite semantic quantity.
    pub fn new(value: f64, quantity: PmiQuantity) -> Option<Self> {
        Some(Self::from_parts(
            crate::scalar::FiniteReal::new(value)?,
            quantity,
        ))
    }
}

/// A nonnegative finite geometric-tolerance magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PmiMagnitude(PmiValue);

impl PmiMagnitude {
    /// Build a tolerance from an admitted nonnegative value and its quantity.
    #[must_use]
    pub fn from_parts(value: crate::scalar::NonNegativeReal, quantity: PmiQuantity) -> Self {
        Self(PmiValue::from_parts(value.into(), quantity))
    }

    /// Construct a nonnegative tolerance magnitude.
    pub fn new(value: PmiValue) -> Option<Self> {
        crate::scalar::NonNegativeReal::try_from(value.value)
            .ok()
            .map(|magnitude| Self::from_parts(magnitude, value.quantity))
    }

    /// Return the finite semantic quantity.
    pub const fn get(self) -> PmiValue {
        self.0
    }
}

impl<'de> Deserialize<'de> for PmiMagnitude {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(crate::units::deserialize_named(deserializer, "magnitude")?)
            .ok_or_else(|| serde::de::Error::custom("magnitude must be nonnegative"))
    }
}

/// Physical quantity carried by a PMI value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PmiQuantity {
    /// Length in millimeters.
    Length,
    /// Angle in radians.
    Angle,
    /// Dimensionless ratio.
    Ratio,
}

/// Semantic dimensional characteristic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PmiDimensionKind {
    /// Size of one shape aspect.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Size,
    /// Relative location of two shape aspects.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Location,
    /// A location measured from an origin to a target: the annotation's
    /// first target is the origin and its second the target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    DirectedLocation,
    /// Angular size or location.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Angular,
    /// Diameter.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Diameter,
    /// Radius.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Radius,
    /// Diameter of a spherical feature.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SphericalDiameter,
    /// Radius of a spherical feature.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SphericalRadius,
    /// Length along the picked curve between its ends.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CurveLength,
    /// Source-defined dimensional subtype.
    Other(String),
}

impl PmiDimensionKind {
    /// The quantity of a dimension of this kind: `None` for `Other`, whose
    /// quantity is its nominal's.
    #[must_use]
    pub fn quantity(&self) -> Option<PmiQuantity> {
        match self {
            Self::Angular => Some(PmiQuantity::Angle),
            Self::Size
            | Self::Location
            | Self::DirectedLocation
            | Self::Diameter
            | Self::Radius
            | Self::SphericalDiameter
            | Self::SphericalRadius
            | Self::CurveLength => Some(PmiQuantity::Length),
            Self::Other(_) => None,
        }
    }
}

/// A qualifier of a dimension, as ASME Y14.5 defines it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DimensionModifier {
    /// A dimension given for information only.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Reference,
    /// A tolerance applied by statistical process control.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Statistical,
    /// A radius with no flats or reversals within its tolerance zone.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ControlledRadius,
    /// The source's text of another modifier.
    Other(String),
}

/// Geometric form of a datum target feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DatumTargetForm {
    /// Point target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Point,
    /// Line target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Line,
    /// Rectangular target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Rectangle,
    /// Circular target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Circle,
    /// Circular-curve target.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CircularCurve,
    /// A region given explicitly by the faces `basis` names.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Area,
    /// A region given explicitly by the curves `basis` names.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Curve,
    /// Source-defined or invalid target form.
    Other(String),
}

/// Semantic geometric-tolerance characteristic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum GeometricToleranceKind {
    /// Straightness.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Straightness,
    /// Flatness.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Flatness,
    /// Roundness or circularity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Roundness,
    /// Cylindricity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Cylindricity,
    /// Coaxiality.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Coaxiality,
    /// Profile of a line.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    LineProfile,
    /// Profile of a surface.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SurfaceProfile,
    /// Angularity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Angularity,
    /// Perpendicularity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Perpendicularity,
    /// Parallelism.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Parallelism,
    /// Position.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Position,
    /// Concentricity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Concentricity,
    /// Symmetry.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Symmetry,
    /// Circular runout.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CircularRunout,
    /// Total runout.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    TotalRunout,
    /// Source-defined tolerance subtype.
    Other(String),
}

/// One datum in an ordered datum system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatumReference {
    /// Referenced datum annotation.
    pub datum: PmiId,
    /// Precedence within the datum system, starting at one.
    pub precedence: NonZeroU32,
    /// Identity of a common-datum group within this datum system. References
    /// with the same precedence and group form one simultaneous compartment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_common_group"
    )]
    pub common_group: Option<u32>,
    /// Source-defined material-condition and translation modifiers.
    pub modifiers: Vec<String>,
}

/// Ordered datum references with consistent precedence compartments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vec<DatumReference>", into = "Vec<DatumReference>")]
pub struct DatumReferences(Vec<DatumReference>);

impl DatumReferences {
    /// Return the ordered datum references.
    #[must_use]
    pub fn as_slice(&self) -> &[DatumReference] {
        &self.0
    }
}

impl From<DatumReferences> for Vec<DatumReference> {
    fn from(value: DatumReferences) -> Self {
        value.0
    }
}

impl TryFrom<Vec<DatumReference>> for DatumReferences {
    type Error = String;

    fn try_from(references: Vec<DatumReference>) -> Result<Self, Self::Error> {
        let mut compartments = std::collections::BTreeMap::<_, (usize, Option<u32>)>::new();
        let mut common_groups = std::collections::BTreeMap::new();
        for reference in &references {
            let (count, group) = compartments
                .entry(reference.precedence)
                .or_insert((0, reference.common_group));
            if *group != reference.common_group {
                return Err(
                    "references: a precedence compartment must use one common_group".into(),
                );
            }
            *count += 1;
            if let Some(group) = reference.common_group {
                if common_groups
                    .insert(group, reference.precedence)
                    .is_some_and(|precedence| precedence != reference.precedence)
                {
                    return Err("references: common_group spans precedence compartments".into());
                }
            }
        }
        if compartments
            .values()
            .any(|(count, group)| *count == 1 && group.is_some() || *count > 1 && group.is_none())
        {
            return Err("references: one datum must have no common_group and multiple datums must share a common_group".into());
        }
        Ok(Self(references))
    }
}

/// A limits-and-fits designation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LimitsAndFits {
    /// The fundamental deviation's letter, stored in capitals.
    pub form_variance: String,
    /// The feature the designation applies to.
    pub zone: FitZone,
    /// The tolerance grade.
    pub grade: String,
    /// The standard whose table defines the designation, where the source
    /// names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fit_source"
    )]
    pub source: Option<FitStandard>,
}

/// The feature a limits-and-fits designation applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FitZone {
    /// A hole.
    Hole,
    /// A shaft.
    Shaft,
}

/// The standard whose table defines a limits-and-fits designation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FitStandard {
    /// ISO 286.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Iso286,
    /// ANSI B4.1.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AnsiB41,
    /// Any other standard's name, as the source spells it.
    Native(cadmpeg_core::text::NonBlankString),
}

/// A semantic dimension: its kind, its nominal, its tolerance, the key a
/// general tolerance by decimal places looks it up by, and its modifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PmiDimension {
    /// The dimensional characteristic, which gives the dimension's quantity.
    pub dimension: PmiDimensionKind,
    /// A literal nominal, or the design parameter that drives the
    /// annotation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nominal"
    )]
    pub nominal: Option<Dimension<PmiValue>>,
    /// The tolerance, in the canonical unit of the dimension's quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<Tolerance>,
    /// The key a general tolerance by decimal places looks the dimension up
    /// by; not the places it displays.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_decimals"
    )]
    pub decimals: Option<u8>,
    /// Qualifiers in source order, each once.
    pub modifiers: Vec<DimensionModifier>,
}

impl PmiDimension {
    /// A dimension of `dimension` with no nominal, tolerance, decimals, or
    /// modifier.
    #[must_use]
    pub const fn new(dimension: PmiDimensionKind) -> Self {
        Self {
            dimension,
            nominal: None,
            tolerance: None,
            decimals: None,
            modifiers: Vec::new(),
        }
    }

    /// The literal nominal, where the nominal is stated and binds no
    /// parameter.
    #[must_use]
    pub fn literal_nominal(&self) -> Option<PmiValue> {
        match &self.nominal {
            Some(Dimension::Literal(value)) => Some(*value),
            _ => None,
        }
    }

    /// The dimension's quantity: its kind's, or for `Other` its literal
    /// nominal's.
    #[must_use]
    pub fn quantity(&self) -> Option<PmiQuantity> {
        self.dimension
            .quantity()
            .or_else(|| self.literal_nominal().map(|value| value.quantity))
    }

    /// Whether a literal nominal's quantity disagrees with the kind: an
    /// `Angular` dimension takes an angle, every other kind but `Other` a
    /// length.
    #[must_use]
    pub fn nominal_disagrees_with_kind(&self) -> bool {
        match (self.dimension.quantity(), self.literal_nominal()) {
            (Some(expected), Some(nominal)) => nominal.quantity != expected,
            _ => false,
        }
    }
}

/// Semantic or presentation PMI payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PmiDefinition {
    /// Datum identification attached to a datum feature.
    Datum {
        /// Datum identifier shown in the feature-control frame.
        identification: String,
    },
    /// Ordered collection of datum references.
    DatumSystem {
        /// Ordered datum references.
        references: DatumReferences,
    },
    /// Datum target feature and its geometric form.
    DatumTarget {
        /// Geometric form of the target feature.
        form: DatumTargetForm,
        /// Target identifier shown with the datum target.
        identification: String,
        /// Shape aspects that provide the datum-target basis.
        basis: Vec<InstanceTarget<PmiTarget>>,
    },
    /// Geometric tolerance, zone units, modifiers, and optional datum system.
    GeometricTolerance {
        /// Tolerance characteristic.
        tolerance: GeometricToleranceKind,
        /// Tolerance-zone magnitude.
        magnitude: PmiMagnitude,
        /// Explicit tolerance-zone unit size.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_defined_unit"
        )]
        defined_unit: Option<PmiValue>,
        /// Explicit area-unit shape for the tolerance zone.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_defined_area_unit"
        )]
        defined_area_unit: Option<String>,
        /// Second unit for rectangular, cylindrical, or spherical zones.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_defined_area_second_unit"
        )]
        defined_area_second_unit: Option<PmiValue>,
        /// Referenced datum-system annotation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_datum_system"
        )]
        datum_system: Option<PmiId>,
        /// Source-defined geometric-tolerance modifiers.
        modifiers: Vec<String>,
        /// The offset of an unequally disposed zone from the true profile,
        /// positive toward the outside of the material.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_displacement"
        )]
        displacement: Option<Length>,
        /// The largest value a tolerance with a material-condition modifier
        /// may take.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_maximum_upper"
        )]
        maximum_upper: Option<PositiveLength>,
        /// The forms of the tolerance zone, in source order.
        zones: Vec<ToleranceZone>,
    },
    /// Size or location dimension with optional plus/minus limits.
    Dimension(PmiDimension),
    /// Graphical annotation retained independently of semantic PMI.
    Presentation {
        /// Decoded annotation text.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_text"
        )]
        text: Option<String>,
        /// Model-space graphical placement.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<Transform>,
        /// Semantic annotations depicted by this presentation.
        semantics: Vec<PmiId>,
    },
}

/// One document-level PMI annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PmiAnnotation {
    /// Stable annotation identity.
    pub id: PmiId,
    /// Display or source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Whether the source explicitly displays this annotation occurrence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Qualified model objects, each in the placed instance and the state
    /// that hold it.
    pub targets: Vec<InstanceTarget<PmiTarget>>,
    /// Semantic or graphical payload.
    pub definition: PmiDefinition,
    /// The native record that supplies the annotation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
    /// Source facts whose meaning is not established, and typed facts whose
    /// carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source facts with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

impl PmiAnnotation {
    /// An annotation with no name, visibility, native record, or native
    /// values.
    #[must_use]
    pub fn new(
        id: PmiId,
        targets: Vec<InstanceTarget<PmiTarget>>,
        definition: PmiDefinition,
    ) -> Self {
        Self {
            id,
            name: None,
            visible: None,
            targets,
            definition,
            native_ref: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }
    }
}

crate::units::named_field!(
    deserialize_source_id,
    cadmpeg_core::text::NonBlankString,
    "source_id"
);

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_common_group, u32, "common_group");
cadmpeg_core::named_optional_field!(deserialize_defined_unit, PmiValue, "defined_unit");
cadmpeg_core::named_optional_field!(deserialize_defined_area_unit, String, "defined_area_unit");
cadmpeg_core::named_optional_field!(
    deserialize_defined_area_second_unit,
    PmiValue,
    "defined_area_second_unit"
);
cadmpeg_core::named_optional_field!(deserialize_datum_system, PmiId, "datum_system");
cadmpeg_core::named_optional_field!(deserialize_displacement, Length, "displacement");
cadmpeg_core::named_optional_field!(deserialize_maximum_upper, PositiveLength, "maximum_upper");
cadmpeg_core::named_optional_field!(deserialize_nominal, Dimension<PmiValue>, "nominal");
cadmpeg_core::named_optional_field!(deserialize_tolerance, Tolerance, "tolerance");
cadmpeg_core::named_optional_field!(deserialize_decimals, u8, "decimals");
cadmpeg_core::named_optional_field!(deserialize_fit_source, FitStandard, "source");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_text, String, "text");
cadmpeg_core::named_optional_field!(deserialize_placement, Transform, "placement");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");

mod identity_rewrite;

mod serialization;
