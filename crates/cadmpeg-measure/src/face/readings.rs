// SPDX-License-Identifier: Apache-2.0
//! A face's area under its region rule, and the two readings of a face whose
//! region rests on its declared orientation.
//!
//! The declared reading is the region on the left of the oriented loops; the
//! complement is the rest of the domain. When one reading has zero area
//! within its error, the other is the face's region whatever the
//! orientation: the empty-reading witness confirms the declared reading or
//! puts the face in conflict. Otherwise the area is measured under the
//! declared reading with the complement as a candidate.

use crate::area::{domain_area, region_area, Area};
use crate::boundary::ResolvedLoop;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::Measure;
use crate::reason::UnavailableReason;
use crate::region::{LoopShape, Region};
use crate::report::RegionRule;
use crate::value::{
    measured, BoxValue, Errors, Measured, Method, Quantity, Range, Reading, Scalar, Sweep,
    Unavailable, Unit,
};

/// What the area of one face reads.
pub(super) struct AreaInput<'a, 'b, 'ir> {
    pub(super) id: &'a str,
    pub(super) chart: &'a Chart<'ir>,
    pub(super) region: &'a Region,
    pub(super) shapes: &'a [Option<LoopShape>],
    pub(super) resolved: &'a [Option<&'b ResolvedLoop<'ir>>],
    /// The length quadrature targets are relative to.
    pub(super) scale: f64,
    /// The boundary term of the model uncertainty.
    pub(super) boundary_model: f64,
    /// The placement term of an area.
    pub(super) placement: &'a dyn Fn(f64) -> f64,
}

/// What the readings of a declared-orientation face settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The region rule does not rest on the orientation.
    Settled,
    /// A witness confirms the declared reading.
    Confirmed,
    /// The declared reading is empty: the complement is the region.
    Conflict,
    /// No witness decides; the declared reading is reported.
    Unconfirmed,
    /// A witness contradicts the declared reading; the values that depend
    /// on the reading are unavailable with both readings as candidates.
    Contradicted,
}

/// The parts of a value that two readings compare.
pub(crate) trait Coordinates {
    fn coordinates(&self) -> Vec<f64>;
}

impl Coordinates for Scalar {
    fn coordinates(&self) -> Vec<f64> {
        vec![self.value.get()]
    }
}

impl Coordinates for Range {
    fn coordinates(&self) -> Vec<f64> {
        vec![self.min.get(), self.max.get()]
    }
}

impl Coordinates for Sweep {
    fn coordinates(&self) -> Vec<f64> {
        vec![self.value.get(), if self.full { 1.0 } else { 0.0 }]
    }
}

impl Coordinates for BoxValue {
    fn coordinates(&self) -> Vec<f64> {
        let (low, high) = (self.min.get(), self.max.get());
        vec![low.x, low.y, low.z, high.x, high.y, high.z]
    }
}

/// Whether two readings give the same value within their errors.
fn same<T: Coordinates>(a: &Measured<T>, b: &Measured<T>) -> bool {
    let allowed = a.numeric_error().get()
        + b.numeric_error().get()
        + a.model_uncertainty().get()
        + b.model_uncertainty().get();
    a.value()
        .coordinates()
        .iter()
        .zip(b.value().coordinates())
        .all(|(x, y)| (x - y).abs() <= allowed)
}

/// A face value under the declared reading and, for a face whose region
/// rests on its declared orientation, under the complement reading.
#[derive(Debug, Clone)]
pub(crate) struct Readings<T> {
    pub(crate) declared: Quantity<T>,
    pub(crate) complement: Option<Quantity<T>>,
}

impl<T: Clone + Coordinates> Readings<T> {
    /// A value that does not depend on the reading.
    pub(crate) const fn single(value: Quantity<T>) -> Self {
        Self {
            declared: value,
            complement: None,
        }
    }

    /// The value the face reports under `verdict`. Readings that agree
    /// within their errors give one value with no reading tag.
    pub(crate) fn reported(&self, verdict: Verdict, entity: &str) -> Quantity<T> {
        let Some(complement) = &self.complement else {
            return self.declared.clone();
        };
        match verdict {
            Verdict::Settled | Verdict::Confirmed => self.declared.clone(),
            Verdict::Conflict => complement.clone(),
            Verdict::Unconfirmed => match (&self.declared, complement) {
                (Quantity::Measured(declared), Quantity::Measured(complement))
                    if same(declared, complement) =>
                {
                    self.declared.clone()
                }
                (Quantity::Measured(declared), Quantity::Measured(complement)) => {
                    Quantity::Measured(declared.clone().under_reading(
                        Reading::Declared,
                        vec![complement.clone().into_candidate(Reading::Complement)],
                    ))
                }
                (Quantity::Measured(declared), Quantity::Unavailable(_)) => Quantity::Measured(
                    declared
                        .clone()
                        .under_reading(Reading::Declared, Vec::new()),
                ),
                (Quantity::Unavailable(_), _) => self.declared.clone(),
            },
            Verdict::Contradicted => match (&self.declared, complement) {
                (Quantity::Measured(declared), Quantity::Measured(complement))
                    if !same(declared, complement) =>
                {
                    Quantity::Unavailable(
                        Unavailable::new(
                            UnavailableReason::RegionOrientationConflict,
                            Some(entity),
                            "a witness contradicts the face's declared orientation",
                        )
                        .with_candidates(vec![
                            declared.clone().into_candidate(Reading::Declared),
                            complement.clone().into_candidate(Reading::Complement),
                        ]),
                    )
                }
                _ => self.declared.clone(),
            },
        }
    }
}

/// A face's area under both readings and the verdict of its readings.
pub(super) struct FaceArea {
    pub(super) area: Readings<Scalar>,
    pub(super) verdict: Verdict,
}

impl AreaInput<'_, '_, '_> {
    fn quantity(&self, area: Area) -> Quantity<Scalar> {
        let method =
            if self.region.rule == RegionRule::WholeSurface && self.chart.closed_area().is_some() {
                Method::ClosedForm
            } else {
                Method::BoundaryQuadrature
            };
        measured(
            Scalar::new(area.value),
            Unit::SquareMillimeter,
            method,
            Errors::new(
                area.numeric_error,
                self.boundary_model + (self.placement)(area.value),
            ),
            self.id,
        )
    }

    /// Whether an area is zero within its error.
    fn empty(&self, area: Area) -> bool {
        area.value.abs() <= area.numeric_error + self.boundary_model
    }
}

/// The area of a face under its region rule.
pub(super) fn face_area(eval: &Eval<'_, '_>, input: &AreaInput<'_, '_, '_>) -> Measure<FaceArea> {
    let declared = region_area(
        eval,
        input.chart,
        input.region,
        input.shapes,
        input.resolved,
        input.scale,
    )?;
    if input.region.rule != RegionRule::DeclaredOrientation {
        return Ok(FaceArea {
            area: Readings::single(input.quantity(declared)),
            verdict: Verdict::Settled,
        });
    }
    let domain = domain_area(eval, input.chart, input.scale)?;
    let complement = Area {
        value: domain.value - declared.value,
        numeric_error: domain.numeric_error + declared.numeric_error,
    };
    let (declared_empty, complement_empty) = (input.empty(declared), input.empty(complement));
    let verdict = if complement_empty && !declared_empty {
        Verdict::Confirmed
    } else if declared_empty && !complement_empty {
        Verdict::Conflict
    } else {
        Verdict::Unconfirmed
    };
    Ok(FaceArea {
        area: Readings {
            declared: input.quantity(declared),
            complement: Some(input.quantity(complement)),
        },
        verdict,
    })
}
