// SPDX-License-Identifier: Apache-2.0
//! Write a subset curve whose local cache IGES cannot state on its source.
//!
//! A subset curve runs on the local parameter from its first trim. An ellipse
//! subset whose angle 0 is off the ellipse axes caches the unit circle under an
//! affine map, and a subset of a parabola, hyperbola, polyline, or composite has
//! no local cache. IGES has no entity for either. Such a subset is written as
//! its source carrier, with every interval on it moved onto the source
//! parameter. A subset that runs against its source also reverses the edges on
//! it.

use std::borrow::Cow;
use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::topology::{IncreasingParameterInterval, ParameterInterval, Sense};
use cadmpeg_ir::CadIr;

use super::flatten_curve;

struct Lowering {
    curve: CurveId,
    source: SolvedCurveGeometry,
    range: [f64; 2],
    sense: bool,
}

/// `ir` with each subset curve that IGES cannot write moved onto its
/// writable source carrier.
pub(super) fn lower_subset_curves(ir: &CadIr) -> Result<Cow<'_, CadIr>, CodecError> {
    let mut lowered: Option<CadIr> = None;
    // A subset of a lowered subset has a writable source on the next pass.
    for _ in 0..ir.model.curves.len() {
        let pending = pending_lowerings(lowered.as_ref().unwrap_or(ir));
        if pending.is_empty() {
            break;
        }
        let model = lowered.get_or_insert_with(|| ir.clone());
        for lowering in pending {
            apply(model, lowering)?;
        }
    }
    Ok(lowered.map_or(Cow::Borrowed(ir), Cow::Owned))
}

fn writable(geometry: &CurveGeometry) -> bool {
    geometry
        .solved()
        .is_some_and(|solved| flatten_curve(solved).is_ok())
}

fn pending_lowerings(ir: &CadIr) -> Vec<Lowering> {
    ir.model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
                return None;
            };
            let owner = ir.model.procedural_curve_owner(&procedural.id)?;
            let curve = ir.model.curves.iter().find(|curve| curve.id == *owner)?;
            if writable(&curve.geometry) {
                return None;
            }
            let source = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *subset.source())?;
            if !writable(&source.geometry) {
                return None;
            }
            Some(Lowering {
                curve: owner.clone(),
                source: source.geometry.solved()?.clone(),
                range: subset.parameter_range().endpoints(),
                sense: *subset.sense(),
            })
        })
        .collect()
}

fn apply(ir: &mut CadIr, lowering: Lowering) -> Result<(), CodecError> {
    let Lowering {
        curve,
        source,
        range: [first, last],
        sense,
    } = lowering;
    let to_source = |[start, end]: [f64; 2]| {
        let mapped = if sense {
            [first + start, first + end]
        } else {
            [last - end, last - start]
        };
        ParameterInterval::new(mapped).map_err(CodecError::malformed)
    };
    let carrier = ir
        .model
        .curves
        .iter_mut()
        .find(|carrier| carrier.id == curve)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("IGES subset curve {curve} is missing"))
        })?;
    if let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry {
        *cache = Some(source);
    }
    carrier.parameter_range = match carrier.parameter_range {
        Some(range) => IncreasingParameterInterval::new(to_source(range.endpoints())?.endpoints()),
        None => IncreasingParameterInterval::new([first, last]),
    };
    let mut reversed = BTreeSet::new();
    for edge in &mut ir.model.edges {
        if edge.curve() != Some(&curve) {
            continue;
        }
        if let Some(range) = edge.param_range() {
            edge.set_param_range(Some(to_source(range.get())?));
        }
        if !sense {
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
            use_curve.parameter_range = to_source(use_curve.parameter_range.endpoints())?;
        }
    }
    if !sense {
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
