// SPDX-License-Identifier: Apache-2.0
//! Silhouette splits in the v11 design layer (§6.5).

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::body::{Kept, PieceForm, Split, SplitMode, SplitTool};
use cadmpeg_ir::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::surfaces::SilhouetteSplitMode;
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::units::UnitVector3;

/// A Fusion silhouette split as a `ByTools` split with a `Silhouette` tool.
/// The target is the body group the scope names, and the direction has one
/// `Whole` `Parallel` input whose pick is the scope's unjoined selector, with
/// the stored model-space vector as `resolved`. The solid-body and
/// shelled-body result modes keep their source names as `Kept::Native`.
pub(super) fn project(operation: &LegacyOperation, scope: &str) -> Option<FeatureOperation> {
    let LegacyOperation::SilhouetteSplit {
        target,
        direction,
        mode,
    } = operation
    else {
        return None;
    };
    let targets = match target {
        BodySelection::Native(group) => native_selection(group.as_str())?,
        BodySelection::Unresolved => Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        },
        _ => return None,
    };
    let resolved = match direction {
        Some(direction) => Some(UnitVector3::normalized(direction.get())?),
        None => None,
    };
    let kept = match mode {
        SilhouetteSplitMode::FacesOnly => Some(Kept::All {
            form: Some(PieceForm::Faces),
        }),
        SilhouetteSplitMode::ShelledBody => Some(Kept::Native(cadmpeg_core::nonblank_literal!(
            "f3d:silhouette_split_shelled_body"
        ))),
        SilhouetteSplitMode::SolidBody => Some(Kept::Native(cadmpeg_core::nonblank_literal!(
            "f3d:silhouette_split_solid_body"
        ))),
        SilhouetteSplitMode::Unresolved => None,
    };
    Some(FeatureOperation::Split(Box::new(Split {
        mode: SplitMode::ByTools {
            targets,
            tools: SplitTool::Silhouette(Box::new(Construction {
                inputs: vec![ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation: Relation::Parallel,
                    }),
                    reference: Some(InputReference::Picks(native_selection(scope)?)),
                    value: None,
                    reversed: None,
                }],
                resolved,
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: BTreeMap::new(),
            })),
            method: None,
            kept,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

fn native_selection<O: cadmpeg_ir::design::selection::MemberOrder>(
    source: &str,
) -> Option<Selection<O>> {
    Some(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target: PickTarget::Native { within: None },
                context: None,
                native: Some(NativeName {
                    codec: cadmpeg_core::nonblank_literal!("f3d").into(),
                    value: NativeNameValue::Text(NonBlankString::new(source)?),
                }),
                domain: None,
                witness: None,
                resolved: None,
            }])
            .ok()?,
        ),
        native: None,
    })
}

#[cfg(test)]
mod tests;
