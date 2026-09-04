// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information independent of design history.

use std::num::NonZeroU32;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, OccurrenceId, PmiId, PointId, ProductDefinitionId, VertexId,
};
use crate::transform::Transform;

/// A model object qualified by an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
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
        source_id: String,
    },
}

/// Numeric semantic-PMI quantity in canonical units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PmiValue {
    /// Numeric value in millimeters, radians, or unitless ratio as selected by
    /// `quantity`.
    pub value: f64,
    /// Physical quantity and canonical unit of `value`.
    pub quantity: PmiQuantity,
}

/// Physical quantity carried by a PMI value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub enum DimensionKind {
    /// Size of one shape aspect.
    Size,
    /// Relative location of two shape aspects.
    Location,
    /// Angular size or location.
    Angular,
    /// Diameter.
    Diameter,
    /// Radius.
    Radius,
    /// Source-defined dimensional subtype.
    Other(String),
}

/// Geometric form of a datum target feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DatumTargetForm {
    /// Point target.
    Point,
    /// Line target.
    Line,
    /// Rectangular target.
    Rectangle,
    /// Circular target.
    Circle,
    /// Circular-curve target.
    CircularCurve,
    /// Source-defined or invalid target form.
    Other(String),
}

/// Semantic geometric-tolerance characteristic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GeometricToleranceKind {
    /// Straightness.
    Straightness,
    /// Flatness.
    Flatness,
    /// Roundness or circularity.
    Roundness,
    /// Cylindricity.
    Cylindricity,
    /// Coaxiality.
    Coaxiality,
    /// Profile of a line.
    LineProfile,
    /// Profile of a surface.
    SurfaceProfile,
    /// Angularity.
    Angularity,
    /// Perpendicularity.
    Perpendicularity,
    /// Parallelism.
    Parallelism,
    /// Position.
    Position,
    /// Concentricity.
    Concentricity,
    /// Symmetry.
    Symmetry,
    /// Circular runout.
    CircularRunout,
    /// Total runout.
    TotalRunout,
    /// Source-defined tolerance subtype.
    Other(String),
}

/// One datum in an ordered datum system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DatumReference {
    /// Referenced datum annotation.
    pub datum: PmiId,
    /// Precedence within the datum system, starting at one.
    #[serde(deserialize_with = "deserialize_datum_precedence")]
    pub precedence: NonZeroU32,
    /// Identity of a common-datum group within this datum system. References
    /// with the same precedence and group form one simultaneous compartment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_group: Option<u32>,
    /// Source-defined material-condition and translation modifiers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

fn deserialize_datum_precedence<'de, D>(deserializer: D) -> Result<NonZeroU32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    NonZeroU32::new(value)
        .ok_or_else(|| D::Error::custom("DatumReference.precedence must start at one"))
}

/// ISO limits-and-fits tolerance class attached to a dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LimitsAndFits {
    /// Form-variance designation.
    pub form_variance: String,
    /// Zone-variance designation.
    pub zone_variance: String,
    /// Tolerance grade.
    pub grade: String,
    /// Source standard or authority text.
    pub source: String,
}

/// Semantic or presentation PMI payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PmiDefinition {
    /// Datum identification attached to a datum feature.
    Datum {
        /// Datum identifier shown in the feature-control frame.
        identification: String,
    },
    /// Ordered collection of datum references.
    DatumSystem {
        /// Ordered datum references.
        references: Vec<DatumReference>,
    },
    /// Datum target feature and its geometric form.
    DatumTarget {
        /// Geometric form of the target feature.
        form: DatumTargetForm,
        /// Target identifier shown with the datum target.
        identification: String,
        /// Shape aspects that provide the datum-target basis.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        basis: Vec<PmiTarget>,
    },
    /// Geometric tolerance, zone units, modifiers, and optional datum system.
    GeometricTolerance {
        /// Tolerance characteristic.
        tolerance: GeometricToleranceKind,
        /// Tolerance-zone magnitude.
        magnitude: PmiValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Explicit tolerance-zone unit size.
        defined_unit: Option<PmiValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Explicit area-unit shape for the tolerance zone.
        defined_area_unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Second unit for rectangular, cylindrical, or spherical zones.
        defined_area_second_unit: Option<PmiValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Referenced datum-system annotation.
        datum_system: Option<PmiId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        /// Source-defined geometric-tolerance modifiers.
        modifiers: Vec<String>,
    },
    /// Size or location dimension with optional plus/minus limits.
    Dimension {
        /// Dimensional characteristic.
        dimension: DimensionKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Nominal value.
        nominal: Option<PmiValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Signed lower deviation from nominal.
        lower_deviation: Option<PmiValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Signed upper deviation from nominal.
        upper_deviation: Option<PmiValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Limits-and-fits tolerance class.
        limits_and_fits: Option<LimitsAndFits>,
    },
    /// Graphical annotation retained independently of semantic PMI.
    Presentation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Decoded annotation text.
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Model-space graphical placement.
        placement: Option<Transform>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        /// Semantic annotations depicted by this presentation.
        semantics: Vec<PmiId>,
    },
}

/// One document-level PMI annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PmiAnnotation {
    /// Stable annotation identity.
    pub id: PmiId,
    /// Display or source name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether the source explicitly displays this annotation occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Qualified model objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<PmiTarget>,
    /// Semantic or graphical payload.
    pub definition: PmiDefinition,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadIr;
    use crate::report::Check;
    use crate::validate::validate_neutral;

    #[test]
    fn datum_system_references_resolve_with_precedence() {
        let datum_id = PmiId("test:model:pmi#datum-a".into());
        let mut ir = CadIr::empty();
        ir.model.pmi.push(PmiAnnotation {
            id: datum_id.clone(),
            name: Some("datum A".into()),
            visible: None,
            targets: vec![PmiTarget::ShapeAspect {
                source_id: "#10".into(),
            }],
            definition: PmiDefinition::Datum {
                identification: "A".into(),
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#system".into()),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem {
                references: vec![DatumReference {
                    datum: datum_id,
                    precedence: NonZeroU32::MIN,
                    common_group: None,
                    modifiers: Vec::new(),
                }],
            },
        });
        ir.finalize();

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn datum_reference_wire_rejects_zero_precedence() {
        let error = serde_json::from_value::<DatumReference>(serde_json::json!({
            "datum": "test:model:pmi#datum-a",
            "precedence": 0
        }))
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("DatumReference.precedence must start at one"));
    }

    #[test]
    fn curve_target_resolves_against_the_curve_arena() {
        let mut ir = crate::examples::unit_cube();
        let curve = ir.model.curves[0].id.clone();
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("synthetic:model:pmi#curve-target".into()),
            name: Some("curve target".into()),
            visible: None,
            targets: vec![PmiTarget::Curve { curve }],
            definition: PmiDefinition::Dimension {
                dimension: DimensionKind::Size,
                nominal: None,
                lower_deviation: None,
                upper_deviation: None,
                limits_and_fits: None,
            },
        });
        ir.finalize();

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn unresolved_semantic_reference_is_invalid() {
        let mut ir = CadIr::empty();
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#graphic".into()),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Presentation {
                text: None,
                placement: None,
                semantics: vec![PmiId("test:model:pmi#missing".into())],
            },
        });

        let report = validate_neutral(&ir, Vec::new());
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.check == Check::Pmi));
    }

    #[test]
    fn datum_references_are_type_checked_and_common_groups_are_explicit() {
        let mut ir = CadIr::empty();
        let dimension_id = PmiId("test:model:pmi#dimension".into());
        ir.model.pmi.push(PmiAnnotation {
            id: dimension_id.clone(),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Dimension {
                dimension: DimensionKind::Size,
                nominal: None,
                lower_deviation: None,
                upper_deviation: None,
                limits_and_fits: None,
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#system".into()),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem {
                references: vec![DatumReference {
                    datum: dimension_id.clone(),
                    precedence: NonZeroU32::MIN,
                    common_group: None,
                    modifiers: Vec::new(),
                }],
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#tolerance".into()),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::GeometricTolerance {
                tolerance: GeometricToleranceKind::Position,
                magnitude: PmiValue {
                    value: 0.1,
                    quantity: PmiQuantity::Length,
                },
                defined_unit: None,
                defined_area_unit: None,
                defined_area_second_unit: None,
                datum_system: Some(dimension_id),
                modifiers: Vec::new(),
            },
        });

        let findings = validate_neutral(&ir, Vec::new()).findings;
        assert!(
            findings
                .iter()
                .filter(|finding| finding.check == Check::Pmi)
                .count()
                >= 2
        );
    }

    #[test]
    fn non_finite_presentation_placement_is_invalid() {
        let mut ir = CadIr::empty();
        let mut placement = Transform::identity();
        placement.rows[0][3] = f64::INFINITY;
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#graphic".into()),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Presentation {
                text: None,
                placement: Some(placement),
                semantics: Vec::new(),
            },
        });
        assert!(validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == Check::Pmi && finding.message.contains("non-finite")));
    }
}
