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

    fn interval(self, [start, end]: [f64; 2]) -> Option<ParameterInterval> {
        let mapped = if self.forward {
            [self.offset + start, self.offset + end]
        } else {
            [self.offset - end, self.offset - start]
        };
        ParameterInterval::new(mapped).ok()
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
/// source that `lower` does not select. A subset whose chain cannot be lowered
/// is kept for the writer's admission pass.
pub fn lower_subset_curves(
    ir: &CadIr,
    lower: impl Fn(&Curve) -> bool,
) -> Result<Cow<'_, CadIr>, CodecError> {
    let mut lowerings = Vec::new();
    for procedural in &ir.model.procedural_curves {
        let Some((curve, subset)) = subset_owner(ir, procedural) else {
            continue;
        };
        if !lower(curve) {
            continue;
        }
        if ir
            .model
            .curves
            .iter()
            .filter(|item| item.id == curve.id)
            .count()
            != 1
        {
            continue;
        }
        if let Some(lowering) = lowering(ir, procedural, curve, subset, &lower)
            .filter(|lowering| ranges_fit(ir, lowering))
        {
            lowerings.push(lowering);
        }
    }
    let planned: BTreeSet<_> = lowerings.iter().map(|item| &item.curve).collect();
    let mut retained = BTreeSet::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
            continue;
        };
        if ir
            .model
            .procedural_curve_owner(&procedural.id)
            .is_some_and(|owner| planned.contains(owner))
        {
            continue;
        }
        let mut source = subset.source();
        while planned.contains(source) && retained.insert(source.clone()) {
            let Some((_, parent)) = ir
                .model
                .procedural_curves
                .iter()
                .filter_map(|record| subset_owner(ir, record))
                .find(|(owner, _)| &owner.id == source)
            else {
                break;
            };
            source = parent.source();
        }
    }
    drop(planned);
    lowerings.retain(|item| !retained.contains(&item.curve));
    if lowerings.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let mut lowered = ir.clone();
    for lowering in lowerings {
        if !apply(&mut lowered, lowering) {
            return Ok(Cow::Borrowed(ir));
        }
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
    curve: &Curve,
    subset: &SubsetCurveConstruction,
    lower: &impl Fn(&Curve) -> bool,
) -> Option<Lowering> {
    let [first, last] = subset.parameter_range().endpoints();
    let mut map = RootMap::IDENTITY.then([first, last], *subset.sense());
    let mut visited = BTreeSet::from([&curve.id]);
    let mut source = subset.source();
    let root = loop {
        let candidate = ir.model.curves.iter().find(|item| item.id == *source)?;
        if !visited.insert(&candidate.id) {
            return None;
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
    let geometry = spline
        .map(SolvedCurveGeometry::Nurbs)
        .or_else(|| root.geometry.solved().cloned())?;
    let span = last - first;
    let root_interval = map.interval([0.0, span])?;
    let definition = SubsetCurveConstruction::try_new(
        root.id.clone(),
        root_interval.endpoints(),
        map.forward,
        subset.legacy_cache(),
    )
    .ok()?;
    Some(Lowering {
        curve: curve.id.clone(),
        construction: procedural.id.clone(),
        geometry,
        span,
        map,
        definition,
    })
}

fn ranges_fit(ir: &CadIr, lowering: &Lowering) -> bool {
    let Some(carrier) = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == lowering.curve)
    else {
        return false;
    };
    let range = carrier
        .parameter_range
        .map_or([0.0, lowering.span], IncreasingParameterInterval::endpoints);
    if lowering
        .map
        .interval(range)
        .and_then(|range| IncreasingParameterInterval::new(range.endpoints()))
        .is_none()
    {
        return false;
    }
    ir.model
        .edges
        .iter()
        .filter(|edge| edge.curve() == Some(&lowering.curve))
        .filter_map(crate::topology::Edge::param_range)
        .all(|range| lowering.map.interval(range.get()).is_some())
        && ir
            .model
            .coedges
            .iter()
            .filter_map(|coedge| coedge.use_curve.as_ref())
            .filter(|value| value.curve == lowering.curve)
            .all(|value| {
                lowering
                    .map
                    .interval(value.parameter_range.endpoints())
                    .is_some()
            })
}

fn apply(ir: &mut CadIr, lowering: Lowering) -> bool {
    if !ranges_fit(ir, &lowering) {
        return false;
    }
    let Lowering {
        curve,
        construction,
        geometry,
        span,
        map,
        definition,
    } = lowering;
    let carrier = ir.model.curves.iter().find(|carrier| carrier.id == curve);
    debug_assert!(
        carrier.is_some(),
        "lowered curves remain in the cloned document"
    );
    let Some(carrier) = carrier else {
        return false;
    };
    let range = carrier
        .parameter_range
        .map_or([0.0, span], IncreasingParameterInterval::endpoints);
    let Some(carrier_range) = map.interval(range) else {
        return false;
    };
    let carrier = ir
        .model
        .curves
        .iter_mut()
        .find(|carrier| carrier.id == curve);
    debug_assert!(
        carrier.is_some(),
        "lowered curves remain in the cloned document"
    );
    let Some(carrier) = carrier else {
        return false;
    };
    if let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry {
        *cache = Some(geometry);
    }
    carrier.parameter_range = IncreasingParameterInterval::new(carrier_range.endpoints());
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
            let mapped = map.interval(range.get());
            debug_assert!(mapped.is_some(), "each edge range was preflighted");
            if let Some(mapped) = mapped {
                edge.set_param_range(Some(mapped));
            }
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
            let mapped = map.interval(use_curve.parameter_range.endpoints());
            debug_assert!(mapped.is_some(), "each coedge range was preflighted");
            if let Some(mapped) = mapped {
                use_curve.parameter_range = mapped;
            }
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
    true
}

#[cfg(test)]
mod tests;
