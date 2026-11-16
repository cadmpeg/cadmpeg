// SPDX-License-Identifier: Apache-2.0

use crate::ids::{native_stream, neutral_sketch_id, neutral_spatial_sketch_id};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::report::loss::LossNote;
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
) -> Vec<LossNote> {
    let mut composed = 0usize;
    let mut owners = HashMap::new();
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == DesignFeatureKind::Sketch)
    {
        let Some(placement) = exact_scope_placement(scope, placements) else {
            continue;
        };
        if placement.display_name.is_some() {
            continue;
        }
        let mut matching = features
            .iter()
            .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        let name = matching.next().and_then(|feature| feature.name.as_deref());
        let name = name.filter(|name| {
            !name.is_empty()
                && matching.next().is_none()
                && scope.label.as_ref().is_none_or(|l| l.value != *name)
        });
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
            composed += 1;
        }
        for sketch in spatial_sketches
            .iter_mut()
            .filter(|sketch| sketch.id == spatial)
        {
            sketch.name = Some(name.to_owned());
            composed += 1;
        }
    }
    if composed == 0 {
        Vec::new()
    } else {
        vec![F3dLossCode::SketchNameComposedFromFeatureOrdinal.note(format!(
            "{composed} sketch name(s) use the projected Feature name composed from the stored scope kind spelling and feature ordinal; this assumes the default timeline name."
        ))]
    }
}

#[cfg(test)]
mod tests;
