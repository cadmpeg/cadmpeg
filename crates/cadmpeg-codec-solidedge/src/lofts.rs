// SPDX-License-Identifier: Apache-2.0
//! Lofted protrusions (`docs/formats/solidedge.md` §5.3): a
//! `JMLoftedProtrusionFunction` feature names its section profiles in loft
//! order, stated as a loft through the section sketches.
//!
//! A feature with fewer than two stated sections stays unresolved and adds
//! `feature.extent-unread`.

use std::collections::BTreeMap;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{Loft, LoftShape, Section, SectionForm};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::extents::{output, profile_selection, PART_CLASS};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};
use crate::sketches::{loft_section_words, stated_sketch, LOFT_FUNCTION};

/// The fewest sections a loft has.
const MIN_SECTIONS: usize = 2;

fn loft(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    ir: &CadIr,
    owner: &cadmpeg_ir::features::FeatureId,
    object: &Object<'_>,
    end: usize,
) -> Result<FeatureOperation, String> {
    let mut sections = Vec::new();
    for word in loft_section_words(cluster, types, object, end) {
        let profile = word.map_err(|()| "the feature block holds no section slot".to_owned())?;
        let sketch = stated_sketch(ir, owner, profile, "section")?;
        sections.push(Section {
            form: SectionForm::Profile(profile_selection(&sketch)?),
            flip: None,
            seam: None,
            condition: None,
            trim_support: None,
            spine_point: None,
        });
    }
    if sections.len() < MIN_SECTIONS {
        return Err(format!(
            "the feature names {} section profile(s)",
            sections.len()
        ));
    }
    Ok(FeatureOperation::Loft(Box::new(Loft {
        start: None,
        sections,
        end: None,
        cross: Vec::new(),
        spine: None,
        guides: Vec::new(),
        correspondence: None,
        shape: LoftShape {
            style: None,
            max_degree: None,
            rebuild: None,
            ruled: None,
            closed: None,
            relimit: None,
            merge_tangent_faces: None,
            smoothing: None,
            section_law: None,
        },
        output: output(false),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

/// State the loft of each lofted protrusion whose sections are stated
/// sketches. A feature that does not read keeps its unresolved operation and
/// adds `feature.extent-unread`.
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
        if function != LOFT_FUNCTION {
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
        match loft(cluster, types, ir, &id, &object, end) {
            Ok(operation) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = operation;
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeatureExtentUnread.note(format!(
                "the loft of feature object {:#x} ({function}) does not read: {reason}; the \
                 feature stays unresolved",
                entry.object
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
