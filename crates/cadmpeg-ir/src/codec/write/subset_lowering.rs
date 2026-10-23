// SPDX-License-Identifier: Apache-2.0
//! Subset curves restated on the parameter of the curve they trim.
//!
//! A subset curve runs on the local parameter from its first trim, and its
//! cache and edge ranges are on that parameter. A format whose trimmed-curve
//! record keeps the parameter of its basis cannot state that. The lowering
//! restates a subset on its first source that is not lowered. A NURBS cache
//! moves onto that source's parameter, and any other cache becomes that
//! source's geometry. The subset interval and every interval on the curve are
//! on that source's parameter, and a subset that runs against that source also
//! reverses the edges on it. A chain of lowered subsets composes into one map.

use std::borrow::Cow;
use std::collections::BTreeSet;

use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::curve_payloads::SubsetCurveConstruction;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::scalar::FiniteReal;
use crate::topology::{IncreasingParameterInterval, ParameterInterval, Sense};

/// The map from a subset's local parameter `s` onto its root source's
/// parameter, `offset + s` or `offset - s`.
#[derive(Clone, Copy)]
struct RootMap {
    offset: f64,
    forward: bool,
}

impl RootMap {
    const IDENTITY: Self = Self {
        offset: 0.0,
        forward: true,
    };

    /// This map followed by a subset map from `[first, last]` with `sense`.
    fn then(self, [first, last]: [f64; 2], sense: bool) -> Self {
        if sense {
            Self {
                offset: first + self.offset,
                forward: self.forward,
            }
        } else {
            Self {
                offset: last - self.offset,
                forward: !self.forward,
            }
        }
    }

    /// `cache`, a spline on the local parameter, on the root parameter.
    fn spline(self, cache: &NurbsCurve) -> Option<NurbsCurve> {
        let mut spline = cache.clone();
        if self.forward {
            spline
                .edit_knots(|knots| knots.iter_mut().for_each(|knot| *knot += self.offset))
                .ok()?;
        } else {
            spline.reverse_parameterization_in_range(
                FiniteReal::ZERO,
                FiniteReal::new(self.offset)?,
            )?;
        }
        Some(spline)
    }

    fn interval(self, [start, end]: [f64; 2]) -> Result<ParameterInterval, CodecError> {
        let mapped = if self.forward {
            [self.offset + start, self.offset + end]
        } else {
            [self.offset - end, self.offset - start]
        };
        ParameterInterval::new(mapped).map_err(CodecError::malformed)
    }
}

struct Lowering {
    curve: CurveId,
    construction: ProceduralCurveId,
    geometry: SolvedCurveGeometry,
    span: f64,
    map: RootMap,
    definition: SubsetCurveConstruction,
}

/// `ir` with every subset curve that `lower` selects restated on its first
/// source that `lower` does not select. A subset whose chain ends at a source
/// with no solved geometry is kept.
pub fn lower_subset_curves(
    ir: &CadIr,
    lower: impl Fn(&Curve) -> bool,
) -> Result<Cow<'_, CadIr>, CodecError> {
    let lowerings = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| lowering(ir, procedural, &lower).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    if lowerings.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let mut lowered = ir.clone();
    for lowering in lowerings {
        apply(&mut lowered, lowering)?;
    }
    Ok(Cow::Owned(lowered))
}

fn subset_owner<'ir>(
    ir: &'ir CadIr,
    procedural: &'ir ProceduralCurve,
) -> Option<(&'ir Curve, &'ir SubsetCurveConstruction)> {
    let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
        return None;
    };
    let owner = ir.model.procedural_curve_owner(&procedural.id)?;
    Some((
        ir.model.curves.iter().find(|curve| curve.id == *owner)?,
        subset,
    ))
}

fn lowering(
    ir: &CadIr,
    procedural: &ProceduralCurve,
    lower: &impl Fn(&Curve) -> bool,
) -> Result<Option<Lowering>, CodecError> {
    let Some((curve, subset)) = subset_owner(ir, procedural) else {
        return Ok(None);
    };
    if !lower(curve) {
        return Ok(None);
    }
    let [first, last] = subset.parameter_range().endpoints();
    let mut map = RootMap::IDENTITY.then([first, last], *subset.sense());
    let mut visited = BTreeSet::from([&curve.id]);
    let mut source = subset.source();
    let root = loop {
        let Some(candidate) = ir.model.curves.iter().find(|item| item.id == *source) else {
            return Ok(None);
        };
        if !visited.insert(&candidate.id) {
            return Err(CodecError::malformed(format_args!(
                "subset curve {} has a cyclic source chain",
                curve.id
            )));
        }
        let inner = candidate
            .geometry
            .procedural_construction()
            .and_then(|construction| {
                ir.model
                    .procedural_curves
                    .iter()
                    .find(|item| item.id == *construction)
            })
            .and_then(|inner| subset_owner(ir, inner))
            .filter(|(inner_curve, _)| lower(inner_curve));
        let Some((_, inner)) = inner else {
            break candidate;
        };
        map = map.then(inner.parameter_range().endpoints(), *inner.sense());
        source = inner.source();
    };
    let spline = match curve.geometry.solved() {
        Some(SolvedCurveGeometry::Nurbs(cache)) => map.spline(cache),
        _ => None,
    };
    let Some(geometry) = spline
        .map(SolvedCurveGeometry::Nurbs)
        .or_else(|| root.geometry.solved().cloned())
    else {
        return Ok(None);
    };
    let span = last - first;
    let definition = SubsetCurveConstruction::try_new(
        root.id.clone(),
        map.interval([0.0, span])?.endpoints(),
        map.forward,
        subset.legacy_cache(),
    )
    .map_err(|error| CodecError::malformed(format_args!("subset curve {}: {error}", curve.id)))?;
    Ok(Some(Lowering {
        curve: curve.id.clone(),
        construction: procedural.id.clone(),
        geometry,
        span,
        map,
        definition,
    }))
}

fn apply(ir: &mut CadIr, lowering: Lowering) -> Result<(), CodecError> {
    let Lowering {
        curve,
        construction,
        geometry,
        span,
        map,
        definition,
    } = lowering;
    let carrier = ir
        .model
        .curves
        .iter_mut()
        .find(|carrier| carrier.id == curve)
        .ok_or_else(|| CodecError::malformed(format_args!("subset curve {curve} is missing")))?;
    if let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry {
        *cache = Some(geometry);
    }
    let range = carrier
        .parameter_range
        .map_or([0.0, span], IncreasingParameterInterval::endpoints);
    carrier.parameter_range = IncreasingParameterInterval::new(map.interval(range)?.endpoints());
    for procedural in &mut ir.model.procedural_curves {
        if procedural.id == construction {
            *procedural = ProceduralCurve::new(
                construction.clone(),
                ProceduralCurveDefinition::Subset(definition.clone()),
            );
        }
    }
    let mut reversed = BTreeSet::new();
    for edge in &mut ir.model.edges {
        if edge.curve() != Some(&curve) {
            continue;
        }
        if let Some(range) = edge.param_range() {
            edge.set_param_range(Some(map.interval(range.get())?));
        }
        if !map.forward {
            std::mem::swap(&mut edge.start, &mut edge.end);
            reversed.insert(edge.id.clone());
        }
    }
    for coedge in &mut ir.model.coedges {
        if reversed.contains(&coedge.edge) {
            coedge.sense = match coedge.sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            };
        }
        if let Some(use_curve) = coedge
            .use_curve
            .as_mut()
            .filter(|use_curve| use_curve.curve == curve)
        {
            use_curve.parameter_range = map.interval(use_curve.parameter_range.endpoints())?;
        }
    }
    if !map.forward {
        for carrier in &mut ir.model.curves {
            let (CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. })
            | CurveGeometry::Procedural {
                cache: Some(SolvedCurveGeometry::Composite { segments, .. }),
                ..
            }) = &mut carrier.geometry
            else {
                continue;
            };
            for segment in segments.iter_mut().filter(|segment| segment.curve == curve) {
                segment.same_sense = !segment.same_sense;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
