// SPDX-License-Identifier: Apache-2.0
//! Swept protrusions (`docs/formats/solidedge.md` §5.3): a
//! `JMMultiSweepProtrusionFunction` feature names its path profile and its
//! section profile, stated as a sweep of the section along the path.
//!
//! A feature whose profiles are not stated sketches stays unresolved and adds
//! `feature.extent-unread`.

use std::collections::BTreeMap;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{SpineLimits, Sweep, SweepControls, SweepSection};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchId;

use crate::extents::{output, profile_selection, slot, PART_CLASS};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};
use crate::sketches::{sketch_id, PROFILE_AFTER_NAME, SWEEP_SECTION_AFTER_NAME};

/// The function of a swept protrusion.
const SWEEP_FUNCTION: &str = "JMMultiSweepProtrusionFunction";

/// The sketch the profile `after` bytes past the function name ending at
/// `end` names, where the feature `owner` owns it.
fn sketch_at(
    ir: &CadIr,
    owner: &cadmpeg_ir::features::FeatureId,
    object: &Object<'_>,
    end: usize,
    after: usize,
    what: &str,
) -> Result<SketchId, String> {
    let id = sketch_id(slot(object, end, after)?);
    if ir
        .design
        .sketches
        .iter()
        .any(|sketch| sketch.id == id && &sketch.owner == owner)
    {
        Ok(id)
    } else {
        Err(format!("the {what} profile is not a stated sketch"))
    }
}

fn sweep(
    ir: &CadIr,
    owner: &cadmpeg_ir::features::FeatureId,
    object: &Object<'_>,
    end: usize,
) -> Result<FeatureOperation, String> {
    let path = sketch_at(ir, owner, object, end, PROFILE_AFTER_NAME, "path")?;
    let section = sketch_at(ir, owner, object, end, SWEEP_SECTION_AFTER_NAME, "section")?;
    Ok(FeatureOperation::Sweep(Box::new(Sweep {
        section: SweepSection::Profile(profile_selection(&section)?),
        path: profile_selection(&path)?,
        controls: SweepControls {
            orientation: None,
            twist: None,
            scaling: None,
        },
        guides: Vec::new(),
        extent: None,
        placement: None,
        limits: SpineLimits {
            start: None,
            end: None,
        },
        transition: None,
        output: output(false),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

/// State the sweep of each swept protrusion whose path and section are
/// stated sketches. A feature that does not read keeps its unresolved
/// operation and adds `feature.extent-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        if function != SWEEP_FUNCTION {
            continue;
        }
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some((_, end)) = function_span(&object) else {
            continue;
        };
        let id = feature_id(entry.object);
        match sweep(ir, &id, &object, end) {
            Ok(operation) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = operation;
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeatureExtentUnread.note(format!(
                "the sweep of feature object {:#x} ({function}) does not read: {reason}; the \
                 feature stays unresolved",
                entry.object
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
