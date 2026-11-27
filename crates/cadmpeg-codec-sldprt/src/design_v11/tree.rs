// SPDX-License-Identifier: Apache-2.0
//! The feature tree of a part as one ordered timeline (§5.2).
//!
//! Each feature node of the Keywords tree is a member, in regeneration order:
//! a node's nested nodes (an absorbed sketch) precede it. A `FeatureManager`
//! folder is a `Folder` over the run of its members. The other tree nodes
//! (annotations, lights, body folders, the equations folder) have no design
//! meaning, so neither they nor the nodes under them are members; the origin
//! is a datum feature (§6.2). A feature node under such a node is listed, so
//! that a loss names it.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::Folder;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::features::tree::FeatureTreeNodeRole;
use cadmpeg_ir::features::FeatureId;

use crate::history::classify::{feature_tree_node_role, HistoryMetadata};
use crate::history::project::neutral_feature_id;
use crate::records::{self, FeatureHistory, FeatureSource};

/// The members of a part's timeline and what the parameter projection needs
/// to know of the tree.
#[derive(Default)]
pub(super) struct Timeline {
    /// The features, in member order.
    pub(super) features: Vec<Feature>,
    /// The folders over runs of the members.
    pub(super) folders: Vec<Folder>,
    /// The feature each member node's native record became.
    pub(super) features_by_record: HashMap<String, FeatureId>,
    /// The native records of the equations folders, whose dimensions are the
    /// part's global variables.
    pub(super) equation_records: HashSet<String>,
    /// The feature nodes under a tree node that is no member, by native
    /// record, with the feature each would have become.
    pub(super) skipped: Vec<(String, FeatureId)>,
}

impl Timeline {
    pub(super) fn members(&self) -> Vec<Member> {
        self.features
            .iter()
            .map(|feature| Member::Feature(feature.id.clone()))
            .collect()
    }
}

/// One step of the depth-first walk over a history's tree.
enum Step<'a> {
    /// Visit a node and the nodes under it.
    Enter(&'a records::Feature),
    /// Emit a node after the nodes under it.
    Emit(&'a records::Feature, bool),
    /// Close the folder whose first member is at `first`.
    CloseFolder { first: usize, name: &'a str },
}

/// The timeline of every history, in history order.
pub(super) fn timeline(histories: &[FeatureHistory]) -> Result<Timeline, CodecError> {
    let mut timeline = Timeline::default();
    let mut emitted = HashSet::new();
    for history in histories {
        let metadata = HistoryMetadata::new(&history.features);
        let by_source = history
            .features
            .iter()
            .filter_map(|feature| Some((feature.source_id?, feature)))
            .collect::<HashMap<FeatureSource, &records::Feature>>();
        let mut children = HashMap::<&str, Vec<&records::Feature>>::new();
        let mut roots = Vec::new();
        for feature in &history.features {
            match feature.tree_parent_record_id() {
                Some(parent) => children.entry(parent).or_default().push(feature),
                None => roots.push(feature),
            }
        }
        let mut stack = roots
            .iter()
            .rev()
            .map(|root| Step::Enter(root))
            .collect::<Vec<_>>();
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter(feature) => {
                    let nested = children.get(feature.id.as_str());
                    let nested = nested.map(Vec::as_slice).unwrap_or_default();
                    if metadata.contains(feature) {
                        skip(nested, &children, &metadata, history, &mut timeline)?;
                        continue;
                    }
                    match feature_tree_node_role(feature, &history.features) {
                        Some(FeatureTreeNodeRole::FeatureFolder) => {
                            stack.push(Step::CloseFolder {
                                first: timeline.features.len(),
                                name: &feature.name,
                            });
                        }
                        Some(FeatureTreeNodeRole::ModelOrigin) => {
                            stack.push(Step::Emit(feature, true));
                        }
                        Some(FeatureTreeNodeRole::Equations) => {
                            timeline.equation_records.insert(feature.id.clone());
                            continue;
                        }
                        Some(_) => {
                            skip(nested, &children, &metadata, history, &mut timeline)?;
                            continue;
                        }
                        None => stack.push(Step::Emit(feature, false)),
                    }
                    stack.extend(nested.iter().rev().map(|child| Step::Enter(child)));
                }
                Step::Emit(feature, origin) => {
                    if !emitted.insert(feature.id.as_str()) {
                        continue;
                    }
                    let operation = if origin {
                        super::operation::origin()
                    } else {
                        super::operation::operation(feature, &history.features, &by_source)
                    };
                    let id = neutral_feature_id(&feature.id)?;
                    timeline
                        .features_by_record
                        .insert(feature.id.clone(), id.clone());
                    timeline.features.push(Feature {
                        id,
                        name: (!feature.name.is_empty()).then(|| feature.name.clone()),
                        suppressed: Some(Dimension::Literal(feature.suppressed)),
                        definition: FeatureDefinition {
                            operation,
                            assembly: None,
                            refine: None,
                            fuzzy_tolerance: None,
                        },
                        placement: None,
                        component: None,
                        driven_by: None,
                        native_ref: Some(feature.id.clone()),
                    });
                }
                Step::CloseFolder { first, name } => {
                    let Some(last) = timeline.features.len().checked_sub(1) else {
                        continue;
                    };
                    if last < first {
                        continue;
                    }
                    let index = |at: usize| {
                        u32::try_from(at).map_err(|_| {
                            CodecError::malformed("SLDPRT timeline member index exceeds u32")
                        })
                    };
                    timeline.folders.push(Folder {
                        name: (!name.is_empty()).then(|| name.to_owned()),
                        first: index(first)?,
                        last: index(last)?,
                    });
                }
            }
        }
    }
    Ok(timeline)
}

/// Lists in `timeline.skipped` the feature nodes among `nested`, the nodes
/// under a tree node that is no member, and the nodes under them.
fn skip<'a>(
    nested: &[&'a records::Feature],
    children: &HashMap<&str, Vec<&'a records::Feature>>,
    metadata: &HistoryMetadata<'_>,
    history: &FeatureHistory,
    timeline: &mut Timeline,
) -> Result<(), CodecError> {
    let mut stack = nested.iter().rev().copied().collect::<Vec<_>>();
    while let Some(feature) = stack.pop() {
        if !metadata.contains(feature)
            && feature_tree_node_role(feature, &history.features).is_none()
        {
            timeline
                .skipped
                .push((feature.id.clone(), neutral_feature_id(&feature.id)?));
        }
        if let Some(nested) = children.get(feature.id.as_str()) {
            stack.extend(nested.iter().rev().copied());
        }
    }
    Ok(())
}
