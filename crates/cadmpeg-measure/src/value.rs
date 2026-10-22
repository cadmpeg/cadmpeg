// SPDX-License-Identifier: Apache-2.0
//! The tagged value object every quantity serializes as.
//!
//! A quantity is measured, with its unit, method, error terms and optional
//! provenance, reading and candidates, or unavailable, with a pinned reason
//! and its category. Constructors keep every value finite and every error
//! non-negative; a value that is not finite becomes unavailable with
//! `evaluation.non-finite`.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use serde::Serialize;

use crate::reason::{Category, UnavailableReason};

/// A measured or unavailable quantity.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Quantity<T> {
    /// The quantity was measured.
    Measured(Measured<T>),
    /// The quantity is unavailable.
    Unavailable(Unavailable<T>),
}

impl<T> Quantity<T> {
    /// The measured value, when the quantity was measured.
    #[must_use]
    pub const fn measured(&self) -> Option<&Measured<T>> {
        match self {
            Self::Measured(measured) => Some(measured),
            Self::Unavailable(_) => None,
        }
    }

    /// The unavailable value, when the quantity is unavailable.
    #[must_use]
    pub const fn unavailable(&self) -> Option<&Unavailable<T>> {
        match self {
            Self::Measured(_) => None,
            Self::Unavailable(unavailable) => Some(unavailable),
        }
    }

    /// An unavailable quantity with the reason's own category.
    pub(crate) fn missing(
        reason: UnavailableReason,
        entity: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self::Unavailable(Unavailable::new(reason, entity, message))
    }

    /// The category this quantity counts under, when it is unavailable.
    pub(crate) fn unavailable_category(&self) -> Option<Category> {
        self.unavailable().map(Unavailable::category)
    }
}

/// A measured value. Fields are private: constructors keep values finite,
/// errors non-negative, and candidates in reading order.
#[derive(Debug, Clone, Serialize)]
pub struct Measured<T> {
    #[serde(flatten)]
    value: T,
    unit: Unit,
    method: Method,
    numeric_error: NonNegativeReal,
    model_uncertainty: NonNegativeReal,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    provenance: Vec<SourceField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reading: Option<Reading>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<Candidate<T>>,
}

impl<T> Measured<T> {
    /// The value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The unit of the value and of both error terms.
    #[must_use]
    pub const fn unit(&self) -> Unit {
        self.unit
    }

    /// How the value was obtained.
    #[must_use]
    pub const fn method(&self) -> Method {
        self.method
    }

    /// The a-posteriori estimate of the computation error.
    #[must_use]
    pub const fn numeric_error(&self) -> NonNegativeReal {
        self.numeric_error
    }

    /// The first-order change of the value under the declared tolerances and
    /// the measured deviations.
    #[must_use]
    pub const fn model_uncertainty(&self) -> NonNegativeReal {
        self.model_uncertainty
    }

    /// The source fields the value is read or computed from.
    #[must_use]
    pub fn provenance(&self) -> &[SourceField] {
        &self.provenance
    }

    /// The reading the value is measured under, when several exist.
    #[must_use]
    pub const fn reading(&self) -> Option<Reading> {
        self.reading
    }

    /// The other readings of the value.
    #[must_use]
    pub fn candidates(&self) -> &[Candidate<T>] {
        &self.candidates
    }

    pub(crate) fn with_provenance(mut self, provenance: Vec<SourceField>) -> Self {
        self.provenance = provenance;
        self
    }

    /// The value as a candidate under `reading`.
    pub(crate) fn into_candidate(self, reading: Reading) -> Candidate<T> {
        Candidate {
            reading,
            value: self.value,
            unit: self.unit,
            method: self.method,
            numeric_error: self.numeric_error,
            model_uncertainty: self.model_uncertainty,
        }
    }

    /// The value measured under `reading`, with the other readings as
    /// candidates.
    pub(crate) fn under_reading(mut self, reading: Reading, candidates: Vec<Candidate<T>>) -> Self {
        self.reading = Some(reading);
        self.candidates = candidates;
        self
    }
}

/// An unavailable value. `category` is set by the constructors: the reason's
/// fixed category, or the cited member's category for an aggregate.
#[derive(Debug, Clone, Serialize)]
pub struct Unavailable<T> {
    reason: UnavailableReason,
    category: Category,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<String>,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<Candidate<T>>,
}

impl<T> Unavailable<T> {
    /// The pinned reason.
    #[must_use]
    pub const fn reason(&self) -> UnavailableReason {
        self.reason
    }

    /// The closed category the reason counts under.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    /// The entity the reason names, when it names one.
    #[must_use]
    pub fn entity(&self) -> Option<&str> {
        self.entity.as_deref()
    }

    /// Human text. Not a contract.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The readings that remain available.
    #[must_use]
    pub fn candidates(&self) -> &[Candidate<T>] {
        &self.candidates
    }

    /// An unavailable value with the reason's own category. An aggregate
    /// reason has no category of its own and is built with
    /// [`Self::aggregate`]; given here, it counts as `input`.
    pub(crate) fn new(
        reason: UnavailableReason,
        entity: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            reason,
            category: reason.category().unwrap_or(Category::Input),
            entity: entity.map(str::to_owned),
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    /// An aggregate that cites an unavailable member of `category`.
    pub(crate) fn aggregate(category: Category, entity: &str, message: impl Into<String>) -> Self {
        Self {
            reason: UnavailableReason::AggregateMemberUnavailable,
            category,
            entity: Some(entity.to_owned()),
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    /// The same reason with a value of another shape.
    pub(crate) fn recast<U>(&self) -> Unavailable<U> {
        Unavailable {
            reason: self.reason,
            category: self.category,
            entity: self.entity.clone(),
            message: self.message.clone(),
            candidates: Vec::new(),
        }
    }
}

/// One reading of a value: no provenance, no nested candidates.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate<T> {
    reading: Reading,
    #[serde(flatten)]
    value: T,
    unit: Unit,
    method: Method,
    numeric_error: NonNegativeReal,
    model_uncertainty: NonNegativeReal,
}

impl<T> Candidate<T> {
    /// The reading this candidate states.
    #[must_use]
    pub const fn reading(&self) -> Reading {
        self.reading
    }

    /// The value under the reading.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The computation error estimate.
    #[must_use]
    pub const fn numeric_error(&self) -> NonNegativeReal {
        self.numeric_error
    }

    /// The model uncertainty.
    #[must_use]
    pub const fn model_uncertainty(&self) -> NonNegativeReal {
        self.model_uncertainty
    }
}

/// A reading of an orientation fact the measurement cannot confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    /// Every face's oriented normal from its sense.
    FaceSense,
    /// Faces with a determinable loop reading use it.
    LoopWinding,
    /// A declared-orientation face as the document states it.
    Declared,
    /// The other region bounded by the same loops, with the opposite normal.
    Complement,
}

/// A finite scalar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Scalar {
    /// The value.
    pub value: FiniteReal,
}

/// A finite point or vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    /// The value.
    pub value: FinitePoint3,
}

/// A finite closed interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Range {
    /// Lower end.
    pub min: FiniteReal,
    /// Upper end.
    pub max: FiniteReal,
}

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BoxValue {
    /// Lower corner.
    pub min: FinitePoint3,
    /// Upper corner.
    pub max: FinitePoint3,
    /// Whether the box is the tight box or an enclosing box.
    pub kind: BoxKind,
    /// The box of the boundary pieces, a lower bound of the tight box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary: Option<BoundaryBox>,
}

/// The box of the boundary pieces of a face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BoundaryBox {
    /// Lower corner.
    pub min: FinitePoint3,
    /// Upper corner.
    pub max: FinitePoint3,
}

/// An angular sweep.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sweep {
    /// The swept angle.
    pub value: FiniteReal,
    /// Whether the sweep covers the full circle.
    pub full: bool,
    /// The swept intervals `[start, end]`, start in `[0, 2π)`, sorted by start.
    pub intervals: Vec<[FiniteReal; 2]>,
}

/// Whether a box is tight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxKind {
    /// The smallest axis-aligned box of the entity.
    Tight,
    /// A box that contains the entity.
    Conservative,
}

/// The unit of a value and its error terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Unit {
    /// Millimetre.
    #[serde(rename = "mm")]
    Millimeter,
    /// Square millimetre.
    #[serde(rename = "mm2")]
    SquareMillimeter,
    /// Cubic millimetre.
    #[serde(rename = "mm3")]
    CubicMillimeter,
    /// Radian.
    #[serde(rename = "rad")]
    Radian,
    /// A dimensionless ratio or direction.
    #[serde(rename = "1")]
    One,
}

/// How a value was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// A stored value, exactly as stored.
    Source,
    /// Exact arithmetic on stored values.
    ClosedForm,
    /// Adaptive Gauss–Kronrod quadrature along boundary pieces.
    BoundaryQuadrature,
    /// Extremum search along boundary pieces only.
    BoundaryExtremum,
    /// Boundary extrema and the carrier's interior critical points in the
    /// region.
    CriticalPoints,
    /// A NURBS Bézier control-net bound.
    ControlHull,
    /// Volume, centroid and closure from face fluxes.
    DivergenceQuadrature,
    /// The sum of member values.
    Sum,
}

/// A source field a value is read or computed from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SourceField {
    entity: String,
    field: String,
    exactness: SourceExactness,
}

impl SourceField {
    pub(crate) fn new(
        entity: impl Into<String>,
        field: impl Into<String>,
        exactness: SourceExactness,
    ) -> Self {
        Self {
            entity: entity.into(),
            field: field.into(),
            exactness,
        }
    }

    /// The entity that stores the field.
    #[must_use]
    pub fn entity(&self) -> &str {
        &self.entity
    }

    /// The serialized field path.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// How the source established the field.
    #[must_use]
    pub const fn exactness(&self) -> SourceExactness {
        self.exactness
    }
}

/// How the source established a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceExactness {
    /// Read verbatim from the source.
    ByteExact,
    /// Computed deterministically from byte-exact inputs.
    Derived,
    /// Filled in from context or convention.
    Inferred,
    /// Origin could not be established.
    Unknown,
    /// The document carries no decode sidecar.
    NotRecorded,
}

/// The two error terms of a value, before admission.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Errors {
    pub(crate) numeric: f64,
    pub(crate) model: f64,
}

impl Errors {
    pub(crate) const ZERO: Self = Self {
        numeric: 0.0,
        model: 0.0,
    };

    pub(crate) const fn new(numeric: f64, model: f64) -> Self {
        Self { numeric, model }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            numeric: self.numeric + other.numeric,
            model: self.model + other.model,
        }
    }

    fn admitted(self) -> Option<(NonNegativeReal, NonNegativeReal)> {
        Some((
            NonNegativeReal::new(self.numeric)?,
            NonNegativeReal::new(self.model)?,
        ))
    }
}

/// A measured quantity, or `evaluation.non-finite` when the value or an
/// error term is not finite.
pub(crate) fn measured<T>(
    value: Option<T>,
    unit: Unit,
    method: Method,
    errors: Errors,
    entity: &str,
) -> Quantity<T> {
    match (value, errors.admitted()) {
        (Some(value), Some((numeric_error, model_uncertainty))) => Quantity::Measured(Measured {
            value,
            unit,
            method,
            numeric_error,
            model_uncertainty,
            provenance: Vec::new(),
            reading: None,
            candidates: Vec::new(),
        }),
        _ => Quantity::missing(
            UnavailableReason::EvaluationNonFinite,
            Some(entity),
            "the value or its error estimate left the finite range",
        ),
    }
}

impl Scalar {
    /// A finite scalar, absent otherwise.
    pub(crate) fn new(value: f64) -> Option<Self> {
        FiniteReal::new(value).map(|value| Self { value })
    }
}

impl Range {
    /// A finite range, absent otherwise.
    pub(crate) fn new(min: f64, max: f64) -> Option<Self> {
        Some(Self {
            min: FiniteReal::new(min)?,
            max: FiniteReal::new(max)?,
        })
    }
}

impl Sweep {
    /// A finite sweep, absent otherwise.
    pub(crate) fn new(value: f64, full: bool, intervals: &[[f64; 2]]) -> Option<Self> {
        let mut finite = Vec::with_capacity(intervals.len());
        for [start, end] in intervals {
            finite.push([FiniteReal::new(*start)?, FiniteReal::new(*end)?]);
        }
        Some(Self {
            value: FiniteReal::new(value)?,
            full,
            intervals: finite,
        })
    }
}

impl Point {
    /// A finite point, absent otherwise.
    pub(crate) fn new(value: Point3) -> Option<Self> {
        FinitePoint3::new(value).map(|value| Self { value })
    }
}

#[cfg(test)]
mod tests {
    use super::{measured, BoxKind, Errors, Method, Quantity, Scalar, Unit};
    use crate::reason::{Category, UnavailableReason};

    #[test]
    fn constructors_keep_values_finite_and_errors_non_negative() {
        let finite = measured(
            Scalar::new(2.0),
            Unit::Millimeter,
            Method::ClosedForm,
            Errors::new(0.0, 1.0e-7),
            "test:model:edge#a",
        );
        let measured_value = finite.measured().expect("finite value is measured");
        assert_eq!(measured_value.value().value.get(), 2.0);
        assert_eq!(measured_value.model_uncertainty().get(), 1.0e-7);

        for (value, errors) in [
            (f64::NAN, Errors::ZERO),
            (f64::INFINITY, Errors::ZERO),
            (1.0, Errors::new(f64::NAN, 0.0)),
            (1.0, Errors::new(-1.0, 0.0)),
        ] {
            let quantity = measured(
                Scalar::new(value),
                Unit::Millimeter,
                Method::ClosedForm,
                errors,
                "test:model:edge#a",
            );
            let unavailable = quantity.unavailable().expect("non-finite is unavailable");
            assert_eq!(unavailable.reason(), UnavailableReason::EvaluationNonFinite);
            assert_eq!(unavailable.category(), Category::Input);
            assert_eq!(unavailable.entity(), Some("test:model:edge#a"));
        }
    }

    #[test]
    fn an_aggregate_unavailable_takes_its_members_category() {
        let unavailable = super::Unavailable::<Scalar>::aggregate(
            Category::Budget,
            "test:model:face#a",
            "member face area is unavailable",
        );
        assert_eq!(
            unavailable.reason(),
            UnavailableReason::AggregateMemberUnavailable
        );
        assert_eq!(unavailable.category(), Category::Budget);
        let value = serde_json::to_value(Quantity::Unavailable(unavailable)).unwrap();
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["reason"], "aggregate.member-unavailable");
        assert_eq!(value["category"], "budget");
    }

    #[test]
    fn value_objects_serialize_in_declaration_order() {
        let quantity = measured(
            Scalar::new(1.5),
            Unit::SquareMillimeter,
            Method::BoundaryQuadrature,
            Errors::new(1.0e-13, 2.0e-6),
            "test:model:face#a",
        );
        let text = serde_json::to_string(&quantity).unwrap();
        assert_eq!(
            text,
            "{\"status\":\"measured\",\"value\":1.5,\"unit\":\"mm2\",\"method\":\"boundary_quadrature\",\"numeric_error\":1e-13,\"model_uncertainty\":2e-6}"
        );
        assert_eq!(
            serde_json::to_value(BoxKind::Conservative).unwrap(),
            "conservative"
        );
    }
}
