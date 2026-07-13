// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information independent of design history.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, EdgeId, FaceId, OccurrenceId, PmiId, ProductId, VertexId};
use crate::transform::Transform;

/// A model object qualified by an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// Product prototype.
    Product {
        /// Qualified product.
        product: ProductId,
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PmiValue {
    /// Numeric value in millimeters, radians, or unitless ratio as selected by
    /// `quantity`.
    pub value: f64,
    /// Physical quantity and canonical unit of `value`.
    pub quantity: PmiQuantity,
}

/// Physical quantity carried by a PMI value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Semantic geometric-tolerance characteristic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DatumReference {
    /// Referenced datum annotation.
    pub datum: PmiId,
    /// Precedence within the datum system, starting at one.
    pub precedence: u32,
    /// Source-defined material-condition and translation modifiers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// Semantic or presentation PMI payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Geometric tolerance and optional datum system.
    GeometricTolerance {
        /// Tolerance characteristic.
        tolerance: GeometricToleranceKind,
        /// Tolerance-zone magnitude.
        magnitude: PmiValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Referenced datum-system annotation.
        datum_system: Option<PmiId>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PmiAnnotation {
    /// Stable annotation identity.
    pub id: PmiId,
    /// Display or source name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    use crate::units::Units;
    use crate::validate;

    #[test]
    fn datum_system_references_resolve_with_precedence() {
        let datum_id = PmiId("test:model:pmi#datum-a".into());
        let mut ir = CadIr::empty(Units::default());
        ir.model.pmi.push(PmiAnnotation {
            id: datum_id.clone(),
            name: Some("datum A".into()),
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
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem {
                references: vec![DatumReference {
                    datum: datum_id,
                    precedence: 1,
                    modifiers: Vec::new(),
                }],
            },
        });
        ir.finalize();

        assert!(validate(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn unresolved_semantic_reference_is_invalid() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId("test:model:pmi#graphic".into()),
            name: None,
            targets: Vec::new(),
            definition: PmiDefinition::Presentation {
                text: None,
                placement: None,
                semantics: vec![PmiId("test:model:pmi#missing".into())],
            },
        });

        let report = validate(&ir, Vec::new());
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.check == Check::Pmi));
    }
}
