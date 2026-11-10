// SPDX-License-Identifier: Apache-2.0

use crate::ids::{native_stream, neutral_sketch_id, neutral_spatial_sketch_id};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::sketches::{Sketch, SpatialSketch};
use std::collections::HashMap;

pub(crate) fn exact_scope_placement<'a>(
    scope: &DesignParameterScope,
    placements: &'a [DesignSketchPlacement],
) -> Option<&'a DesignSketchPlacement> {
    let stream = native_stream(&scope.id)?;
    let mut matching = placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.scope_record_index == Some(scope.record_index)
    });
    let placement = matching.next()?;
    matching.next().is_none().then_some(placement)
}

pub(crate) fn bind_sketch_names(
    features: &[Feature],
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    sketches: &mut [Sketch],
    spatial_sketches: &mut [SpatialSketch],
) {
    let mut owners = HashMap::new();
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == DesignFeatureKind::Sketch)
    {
        let Some(placement) = exact_scope_placement(scope, placements) else {
            continue;
        };
        let mut matching = features
            .iter()
            .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        let name = matching.next().and_then(|feature| feature.name.as_deref());
        let name = name.filter(|name| !name.is_empty() && matching.next().is_none());
        owners
            .entry(neutral_sketch_id(placement))
            .and_modify(|owner| *owner = None)
            .or_insert(Some((placement, name)));
    }
    for (placement, name) in owners.into_values().flatten() {
        let Some(name) = name else { continue };
        let planar = neutral_sketch_id(placement);
        let spatial = neutral_spatial_sketch_id(placement);
        for sketch in sketches.iter_mut().filter(|sketch| sketch.id == planar) {
            sketch.name = Some(name.to_owned());
        }
        for sketch in spatial_sketches
            .iter_mut()
            .filter(|sketch| sketch.id == spatial)
        {
            sketch.name = Some(name.to_owned());
        }
    }
}

#[cfg(test)]
mod tests;
