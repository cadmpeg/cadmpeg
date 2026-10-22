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
use crate::region::{LoopShape, Region};
use crate::report::RegionRule;
use crate::value::{measured, Errors, Method, Quantity, Reading, Scalar, Unit};

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
    /// The complement is empty: the declared reading is the region.
    Confirmed,
    /// The declared reading is empty: the complement is the region.
    Conflict,
    /// Both readings bound area; the declared one is reported.
    Unconfirmed,
}

/// A face's area and the verdict of its readings.
pub(super) struct FaceArea {
    pub(super) area: Quantity<Scalar>,
    pub(super) verdict: Verdict,
}

impl AreaInput<'_, '_, '_> {
    fn quantity(&self, area: Area) -> Quantity<Scalar> {
        measured(
            Scalar::new(area.value),
            Unit::SquareMillimeter,
            Method::BoundaryQuadrature,
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
            area: input.quantity(declared),
            verdict: Verdict::Settled,
        });
    }
    let domain = domain_area(eval, input.chart, input.scale)?;
    let complement = Area {
        value: domain.value - declared.value,
        numeric_error: domain.numeric_error + declared.numeric_error,
    };
    let (declared_empty, complement_empty) = (input.empty(declared), input.empty(complement));
    if complement_empty && !declared_empty {
        return Ok(FaceArea {
            area: input.quantity(declared),
            verdict: Verdict::Confirmed,
        });
    }
    if declared_empty && !complement_empty {
        return Ok(FaceArea {
            area: input.quantity(complement),
            verdict: Verdict::Conflict,
        });
    }
    let same = (declared.value - complement.value).abs()
        <= declared.numeric_error + complement.numeric_error + 2.0 * input.boundary_model;
    let area = match (input.quantity(declared), input.quantity(complement)) {
        (Quantity::Measured(declared), Quantity::Measured(complement)) if !same => {
            Quantity::Measured(declared.under_reading(
                Reading::Declared,
                vec![complement.into_candidate(Reading::Complement)],
            ))
        }
        (declared, _) => declared,
    };
    Ok(FaceArea {
        area,
        verdict: Verdict::Unconfirmed,
    })
}
