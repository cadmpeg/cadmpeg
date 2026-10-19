// SPDX-License-Identifier: Apache-2.0
//! Boundable authored domains, separate from evaluated geometry intervals.

use super::Dimension;
use crate::scalar::{Angle, FiniteReal};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Directed, nonzero authored angular interval of a feature curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureAngularRangeWire", into = "FeatureAngularRangeWire")]
pub struct FeatureAngularRange {
    start: Dimension<Angle>,
    end: Dimension<Angle>,
}
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureAngularRangeWire {
    /// Canonical start angle.
    start: Dimension<Angle>,
    /// Canonical end angle in traversal order.
    end: Dimension<Angle>,
}
impl FeatureAngularRange {
    /// Admit distinct literal endpoints, deferring bound relations to document validation.
    pub fn new(start: Dimension<Angle>, end: Dimension<Angle>) -> Option<Self> {
        start
            .literal()
            .zip(end.literal())
            .is_none_or(|(start, end)| start != end)
            .then_some(Self { start, end })
    }
    /// Canonical start and end controls, in traversal order.
    pub const fn endpoints(&self) -> [&Dimension<Angle>; 2] {
        [&self.start, &self.end]
    }
    /// Consume the range and return its canonical endpoint controls.
    pub fn into_endpoints(self) -> [Dimension<Angle>; 2] {
        [self.start, self.end]
    }
    /// Resolve the current directed interval without storing another evaluation.
    pub fn resolved(&self, ir: &crate::CadIr) -> Option<crate::geometry::DirectedParameterRange> {
        crate::geometry::DirectedParameterRange::from_angle_endpoints([
            ir.dimension_value(&self.start)?,
            ir.dimension_value(&self.end)?,
        ])
        .ok()
    }
}
impl TryFrom<FeatureAngularRangeWire> for FeatureAngularRange {
    type Error = &'static str;
    fn try_from(value: FeatureAngularRangeWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end).ok_or("angular range endpoints must be distinct")
    }
}
impl From<FeatureAngularRange> for FeatureAngularRangeWire {
    fn from(value: FeatureAngularRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}
impl TryFrom<crate::geometry::DirectedParameterRange> for FeatureAngularRange {
    type Error = &'static str;
    fn try_from(value: crate::geometry::DirectedParameterRange) -> Result<Self, Self::Error> {
        let [start, end] = value.endpoints().map(Angle::new);
        Self::new(
            Dimension::Literal(start.ok_or("non-finite start angle")?),
            Dimension::Literal(end.ok_or("non-finite end angle")?),
        )
        .ok_or("angular range endpoints must be distinct")
    }
}
rewrite_record!(FeatureAngularRange,[];{start,end});

/// Increasing authored scalar parameter domain of an equation-curve feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureScalarRangeWire", into = "FeatureScalarRangeWire")]
pub struct FeatureScalarRange {
    start: Dimension<FiniteReal>,
    end: Dimension<FiniteReal>,
}
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureScalarRangeWire {
    /// Canonical inclusive lower scalar bound.
    start: Dimension<FiniteReal>,
    /// Canonical inclusive upper scalar bound.
    end: Dimension<FiniteReal>,
}
impl FeatureScalarRange {
    /// Admit increasing literal endpoints, deferring bound relations to document validation.
    pub fn new(start: Dimension<FiniteReal>, end: Dimension<FiniteReal>) -> Option<Self> {
        start
            .literal()
            .zip(end.literal())
            .is_none_or(|(start, end)| start.get() < end.get())
            .then_some(Self { start, end })
    }
    /// Canonical start and end controls.
    pub const fn endpoints(&self) -> [&Dimension<FiniteReal>; 2] {
        [&self.start, &self.end]
    }
    /// Consume the domain and return its canonical endpoint controls.
    pub fn into_endpoints(self) -> [Dimension<FiniteReal>; 2] {
        [self.start, self.end]
    }
    /// Resolve the current increasing domain without storing another evaluation.
    pub fn resolved(
        &self,
        ir: &crate::CadIr,
    ) -> Option<crate::topology::IncreasingParameterInterval> {
        crate::topology::IncreasingParameterInterval::new([
            ir.dimension_value(&self.start)?.get(),
            ir.dimension_value(&self.end)?.get(),
        ])
    }
}
impl TryFrom<FeatureScalarRangeWire> for FeatureScalarRange {
    type Error = &'static str;
    fn try_from(value: FeatureScalarRangeWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end).ok_or("scalar domain endpoints must be increasing")
    }
}
impl From<FeatureScalarRange> for FeatureScalarRangeWire {
    fn from(value: FeatureScalarRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}
rewrite_record!(FeatureScalarRange,[];{start,end});
