// SPDX-License-Identifier: Apache-2.0
//! Fills and merges.
//!
//! The v10 projection writes a fill's continuity and merge flag, and a
//! merge's flags, as constants it does not read; they are unstated here.

use std::collections::BTreeMap;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::surface::{Knit, Patch, PatchBounds};
use cadmpeg_ir::design::options::SheetOutput;
use cadmpeg_ir::design::selection::PickTarget;
use cadmpeg_ir::features::{self as old, FeatureOperation as LegacyOperation};

use super::{selections, Stated};

fn output() -> SheetOutput {
    SheetOutput {
        solid: None,
        boolean: None,
    }
}

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
                output: output(),
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })))
        }
        LegacyOperation::KnitSurface {
            faces,
            gap_tolerance: None,
            ..
        } => {
            let native = match faces {
                old::FaceSelection::Native(native) => native.as_str(),
                old::FaceSelection::Generated { native, .. } => native.as_str(),
                _ => return None,
            };
            Some(FeatureOperation::Knit(Box::new(Knit {
                elements: selections::unread(native)?,
                tolerance: None,
                simplify: None,
                healing: None,
                federation: None,
                output: output(),
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })))
        }
        _ => None,
    }
}
