// SPDX-License-Identifier: Apache-2.0
//! Fills.
//!
//! The v10 projection writes a fill's continuity and merge flag as
//! constants it does not read; they are unstated here.

use std::collections::BTreeMap;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::surface::{Patch, PatchBounds};
use cadmpeg_ir::design::options::SheetOutput;
use cadmpeg_ir::design::selection::PickTarget;
use cadmpeg_ir::features::{self as old, FeatureOperation as LegacyOperation};

use super::{selections, Stated};

pub(super) fn operation(value: &LegacyOperation, stated: &Stated<'_>) -> Option<FeatureOperation> {
    match value {
        LegacyOperation::FilledSurface {
            boundary: old::SurfaceBoundary::Path(old::PathRef::Sketch(sketch)),
            interior_constraints,
            reverse_normal: None,
            ..
        } if interior_constraints.is_empty() && stated.sketches.contains_key(sketch) => {
            Some(FeatureOperation::Patch(Box::new(Patch {
                bounds: PatchBounds::Profile(selections::named(vec![selections::picked(
                    PickTarget::Sketch(sketch.clone()),
                )])?),
                passing: None,
                planar: None,
                simplified: None,
                output: SheetOutput {
                    solid: None,
                    boolean: None,
                },
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })))
        }
        _ => None,
    }
}
