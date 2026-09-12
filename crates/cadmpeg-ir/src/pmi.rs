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
        source_id: crate::products::NonEmptyString,
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
    pub value: crate::units::FiniteScalar,
    /// Physical quantity and canonical unit of `value`.
    pub quantity: PmiQuantity,
}

crate::units::named_field!(deserialize_pmi_value, crate::units::FiniteScalar, "value");

impl PmiValue {
    /// Construct a finite semantic quantity.
    pub fn new(value: f64, quantity: PmiQuantity) -> Option<Self> {
        Some(Self {
            value: crate::units::FiniteScalar::new(value)?,
            quantity,
        })
    }
}

/// A nonnegative finite geometric-tolerance magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PmiMagnitude(PmiValue);

impl PmiMagnitude {
    /// Construct a nonnegative tolerance magnitude.
    pub fn new(value: PmiValue) -> Option<Self> {
        crate::units::NonNegativeScalar::new(value.value.get()).map(|_| Self(value))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Ordered datum references with consistent precedence compartments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vec<DatumReference>", into = "Vec<DatumReference>")]
pub struct DatumReferences(Vec<DatumReference>);

impl DatumReferences {
    /// Return the ordered datum references.
    #[must_use]
    pub fn as_slice(&self) -> &[DatumReference] {
        &self.0
    }

    /// Replace the references after checking their precedence compartments.
    pub fn replace(&mut self, references: Vec<DatumReference>) -> Result<(), String> {
        let replacement = Self::try_from(references)?;
        *self = replacement;
        Ok(())
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
#[serde(deny_unknown_fields)]
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

/// Tolerance carried by a semantic dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionTolerance {
    /// Signed lower and upper deviations from the nominal value.
    PlusMinus {
        /// Signed lower deviation from nominal.
        lower: PmiValue,
        /// Signed upper deviation from nominal.
        upper: PmiValue,
    },
    /// ISO limits-and-fits tolerance class.
    Fit {
        /// ISO limits-and-fits tolerance class.
        fit: LimitsAndFits,
    },
    /// Signed deviations qualified by an ISO limits-and-fits tolerance class.
    PlusMinusFit {
        /// Signed lower deviation from nominal.
        lower: PmiValue,
        /// Signed upper deviation from nominal.
        upper: PmiValue,
        /// ISO limits-and-fits tolerance class.
        fit: LimitsAndFits,
    },
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        basis: Vec<PmiTarget>,
    },
    /// Geometric tolerance, zone units, modifiers, and optional datum system.
    GeometricTolerance {
        /// Tolerance characteristic.
        tolerance: GeometricToleranceKind,
        /// Tolerance-zone magnitude.
        magnitude: PmiMagnitude,
        /// Explicit tolerance-zone unit size.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defined_unit: Option<PmiValue>,
        /// Explicit area-unit shape for the tolerance zone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defined_area_unit: Option<String>,
        /// Second unit for rectangular, cylindrical, or spherical zones.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defined_area_second_unit: Option<PmiValue>,
        /// Referenced datum-system annotation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        datum_system: Option<PmiId>,
        /// Source-defined geometric-tolerance modifiers.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modifiers: Vec<String>,
    },
    /// Size or location dimension with optional plus/minus limits.
    Dimension {
        /// Dimensional characteristic.
        dimension: DimensionKind,
        /// Nominal value, absent when the source carries none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nominal: Option<PmiValue>,
        /// Optional plus/minus or limits-and-fits tolerance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tolerance: Option<DimensionTolerance>,
    },
    /// Graphical annotation retained independently of semantic PMI.
    Presentation {
        /// Decoded annotation text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// Model-space graphical placement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<Transform>,
        /// Semantic annotations depicted by this presentation.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

crate::units::named_field!(
    deserialize_source_id,
    crate::products::NonEmptyString,
    "source_id"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadIr;
    use crate::report::Check;
    use crate::validate::validate_neutral;

    #[test]
    fn datum_system_references_resolve_with_precedence() {
        let datum_id = PmiId::mint("test:model:pmi#datum-a").expect("valid identity");
        let mut ir = CadIr::empty();
        ir.model.pmi.push(PmiAnnotation {
            id: datum_id.clone(),
            name: Some("datum A".into()),
            visible: None,
            targets: vec![PmiTarget::ShapeAspect {
                source_id: crate::products::NonEmptyString::new("#10")
                    .expect("nonempty source identity"),
            }],
            definition: PmiDefinition::Datum {
                identification: "A".into(),
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId::mint("test:model:pmi#system").expect("valid identity"),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem {
                references: vec![DatumReference {
                    datum: datum_id,
                    precedence: NonZeroU32::MIN,
                    common_group: None,
                    modifiers: Vec::new(),
                }]
                .try_into()
                .expect("valid datum compartments"),
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
    fn dimension_wire_nests_its_tolerance_and_refuses_the_deleted_flat_keys() {
        let definition = PmiDefinition::Dimension {
            dimension: DimensionKind::Size,
            nominal: Some(PmiValue::new(12.0, PmiQuantity::Length).expect("finite value")),
            tolerance: Some(DimensionTolerance::PlusMinus {
                lower: PmiValue::new(-0.1, PmiQuantity::Length).expect("finite value"),
                upper: PmiValue::new(0.2, PmiQuantity::Length).expect("finite value"),
            }),
        };

        let value = serde_json::to_value(&definition).unwrap();
        assert_eq!(value["kind"], "dimension");
        assert_eq!(value["nominal"]["value"], 12.0);
        assert_eq!(value["tolerance"]["form"], "plus_minus");
        assert_eq!(value["tolerance"]["lower"]["value"], -0.1);
        assert_eq!(value["tolerance"]["upper"]["value"], 0.2);
        assert!(value.get("lower_deviation").is_none());
        assert!(value.get("upper_deviation").is_none());
        assert!(value.get("limits_and_fits").is_none());
        assert_eq!(
            serde_json::from_value::<PmiDefinition>(value).unwrap(),
            definition
        );

        let error = serde_json::from_value::<PmiDefinition>(serde_json::json!({
            "kind": "dimension",
            "dimension": "size",
            "lower_deviation": {"value": -0.1, "quantity": "length"}
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `lower_deviation`"), "{error}");

        let error = serde_json::from_value::<DimensionTolerance>(serde_json::json!({
            "form": "fit",
            "fit": {
                "form_variance": "H",
                "zone_variance": "",
                "grade": "7",
                "source": "ISO 286"
            },
            "lower": {"value": -0.1, "quantity": "length"}
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `lower`"), "{error}");
    }

    #[test]
    fn dimension_wire_carries_an_absent_nominal_and_a_combined_tolerance() {
        let value = serde_json::json!({
            "kind": "dimension",
            "dimension": "diameter",
            "tolerance": {
                "form": "plus_minus",
                "lower": {"value": -0.1, "quantity": "length"},
                "upper": {"value": 0.2, "quantity": "length"}
            }
        });
        let definition =
            serde_json::from_value::<PmiDefinition>(value.clone()).expect("absent nominal");
        assert!(matches!(
            definition,
            PmiDefinition::Dimension {
                dimension: DimensionKind::Diameter,
                nominal: None,
                tolerance: Some(DimensionTolerance::PlusMinus { .. }),
            }
        ));
        assert_eq!(serde_json::to_value(&definition).expect("wire"), value);

        let value = serde_json::json!({
            "kind": "dimension",
            "dimension": "size",
            "nominal": {"value": 12.0, "quantity": "length"},
            "tolerance": {
                "form": "plus_minus_fit",
                "lower": {"value": -0.1, "quantity": "length"},
                "upper": {"value": 0.2, "quantity": "length"},
                "fit": {
                    "form_variance": "H",
                    "zone_variance": "",
                    "grade": "7",
                    "source": "ISO 286"
                }
            }
        });
        let definition =
            serde_json::from_value::<PmiDefinition>(value.clone()).expect("combined tolerance");
        assert!(matches!(
            definition,
            PmiDefinition::Dimension {
                tolerance: Some(DimensionTolerance::PlusMinusFit { .. }),
                ..
            }
        ));
        assert_eq!(serde_json::to_value(&definition).expect("wire"), value);
    }

    #[test]
    fn curve_target_resolves_against_the_curve_arena() {
        let mut ir = crate::examples::unit_cube();
        let curve = ir.model.curves[0].id.clone();
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId::mint("synthetic:model:pmi#curve-target").expect("valid identity"),
            name: Some("curve target".into()),
            visible: None,
            targets: vec![PmiTarget::Curve { curve }],
            definition: PmiDefinition::Dimension {
                dimension: DimensionKind::Size,
                nominal: Some(PmiValue::new(1.0, PmiQuantity::Length).expect("finite value")),
                tolerance: None,
            },
        });
        ir.finalize();

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn unresolved_semantic_reference_is_invalid() {
        let mut ir = CadIr::empty();
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId::mint("test:model:pmi#graphic").expect("valid identity"),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Presentation {
                text: None,
                placement: None,
                semantics: vec![PmiId::mint("test:model:pmi#missing").expect("valid identity")],
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
        let dimension_id = PmiId::mint("test:model:pmi#dimension").expect("valid identity");
        ir.model.pmi.push(PmiAnnotation {
            id: dimension_id.clone(),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Dimension {
                dimension: DimensionKind::Size,
                nominal: Some(PmiValue::new(1.0, PmiQuantity::Length).expect("finite value")),
                tolerance: None,
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId::mint("test:model:pmi#system").expect("valid identity"),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem {
                references: vec![DatumReference {
                    datum: dimension_id.clone(),
                    precedence: NonZeroU32::MIN,
                    common_group: None,
                    modifiers: Vec::new(),
                }]
                .try_into()
                .expect("valid datum compartments"),
            },
        });
        ir.model.pmi.push(PmiAnnotation {
            id: PmiId::mint("test:model:pmi#tolerance").expect("valid identity"),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::GeometricTolerance {
                tolerance: GeometricToleranceKind::Position,
                magnitude: PmiMagnitude::new(
                    PmiValue::new(0.1, PmiQuantity::Length).expect("finite value"),
                )
                .expect("nonnegative magnitude"),
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
    fn pmi_magnitude_admission_keeps_signed_dimensions_and_zero_angles() {
        let negative = PmiValue::new(-1.0, PmiQuantity::Length).expect("signed dimension");
        assert!(PmiMagnitude::new(negative).is_none());
        assert!(PmiValue::new(f64::INFINITY, PmiQuantity::Length).is_none());
        let zero = PmiMagnitude::new(PmiValue::new(0.0, PmiQuantity::Angle).expect("zero angle"))
            .expect("zero magnitude");
        let wire = serde_json::to_string(&zero).expect("serialize");
        assert_eq!(wire, r#"{"value":0.0,"quantity":"angle"}"#);
        assert_eq!(
            serde_json::from_str::<PmiMagnitude>(&wire).expect("deserialize"),
            zero
        );
        let error = serde_json::from_str::<PmiMagnitude>(r#"{"value":-1.0,"quantity":"length"}"#)
            .expect_err("negative magnitude");
        assert!(error.to_string().contains("magnitude"));
    }

    #[test]
    fn source_identity_admission_preserves_nonempty_wire() {
        let wire = serde_json::json!({"kind": "shape_aspect", "source_id": "#42"});
        let value: PmiTarget = serde_json::from_value(wire.clone()).expect("nonempty source_id");
        assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        let error = serde_json::from_value::<PmiTarget>(
            serde_json::json!({"kind": "shape_aspect", "source_id": ""}),
        )
        .expect_err("empty source_id");
        assert!(error.to_string().contains("source_id"));
        assert!(crate::products::NonEmptyString::new("").is_none());
    }

    #[test]
    fn datum_compartments_are_checked_at_construction_wire_and_replacement() {
        let reference = |id: &str, precedence, common_group| DatumReference {
            datum: PmiId::mint(format!("test:model:pmi#{id}")).expect("valid identity"),
            precedence: NonZeroU32::new(precedence).expect("positive precedence"),
            common_group,
            modifiers: Vec::new(),
        };
        let valid = vec![
            reference("c", 2, None),
            reference("a", 1, Some(0)),
            reference("b", 1, Some(0)),
        ];
        let mut admitted = DatumReferences::try_from(valid.clone()).expect("valid compartments");
        assert_eq!(admitted.as_slice(), valid);
        let wire = serde_json::json!({"kind": "datum_system", "references": valid});
        let definition: PmiDefinition = serde_json::from_value(wire.clone()).expect("valid wire");
        assert_eq!(serde_json::to_value(definition).expect("serialize"), wire);
        assert!(DatumReferences::try_from(Vec::new())
            .expect("empty system")
            .as_slice()
            .is_empty());
        for invalid in [
            vec![reference("a", 1, Some(7))],
            vec![reference("a", 1, None), reference("b", 1, None)],
            vec![reference("a", 1, Some(7)), reference("b", 1, None)],
            vec![reference("a", 1, Some(7)), reference("b", 1, Some(8))],
            vec![
                reference("a", 1, Some(7)),
                reference("b", 1, Some(7)),
                reference("c", 2, Some(7)),
                reference("d", 2, Some(7)),
            ],
        ] {
            assert!(DatumReferences::try_from(invalid.clone()).is_err());
            let error = serde_json::from_value::<PmiDefinition>(
                serde_json::json!({"kind": "datum_system", "references": invalid}),
            )
            .expect_err("invalid compartments");
            assert!(error.to_string().contains("references"));
            assert!(serde_json::from_value::<DatumReferences>(
                serde_json::to_value(&invalid).expect("serialize")
            )
            .is_err());
            assert!(admitted.replace(invalid).is_err());
            assert_eq!(admitted.as_slice(), valid);
        }
        let replacement = vec![reference("z", 3, None)];
        admitted
            .replace(replacement.clone())
            .expect("valid replacement");
        assert_eq!(admitted.as_slice(), replacement);
    }
}
