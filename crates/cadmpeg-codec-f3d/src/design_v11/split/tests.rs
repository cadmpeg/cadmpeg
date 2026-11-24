// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::design::selection::{OrderedSelection, SetSelection};
use cadmpeg_ir::features::geometry::FeatureDirection3;
use cadmpeg_ir::math::Vector3;

const SCOPE: &str = "f3d:design:scope#silhouette";
const GROUP: &str = "f3d:design:group#bodies";

fn silhouette(target: BodySelection, mode: SilhouetteSplitMode) -> LegacyOperation {
    LegacyOperation::SilhouetteSplit {
        target,
        direction: FeatureDirection3::new(Vector3::new(0.0, 0.0, 2.0)),
        mode,
    }
}

fn native_value<O: cadmpeg_ir::design::selection::MemberOrder>(
    selection: &Selection<O>,
) -> Option<&NativeNameValue> {
    let Picks::Named(picks) = &selection.picks else {
        return None;
    };
    let [pick] = picks.as_slice() else {
        return None;
    };
    Some(&pick.native.as_ref()?.value)
}

/// §6.5: a Fusion silhouette split is a `ByTools` split of its body group,
/// with a `Silhouette` tool whose one `Whole` `Parallel` input picks the
/// unjoined selector and whose `resolved` is the stored vector, and no method.
#[test]
fn a_silhouette_split_is_a_split_along_its_direction() {
    let operation = silhouette(
        BodySelection::Native(GROUP.to_owned()),
        SilhouetteSplitMode::FacesOnly,
    );
    let Some(FeatureOperation::Split(split)) = project(&operation, SCOPE) else {
        panic!("a typed split");
    };
    let SplitMode::ByTools {
        targets,
        tools: SplitTool::Silhouette(direction),
        method,
        kept,
    } = &split.mode
    else {
        panic!("a silhouette tool");
    };
    let targets: &SetSelection = targets;
    assert_eq!(
        native_value(targets),
        Some(&NativeNameValue::Text(NonBlankString::new(GROUP).unwrap()))
    );
    assert_eq!(*method, None);
    assert_eq!(
        *kept,
        Some(Kept::All {
            form: Some(PieceForm::Faces)
        })
    );
    assert_eq!(direction.resolved, Some(UnitVector3::Z_AXIS));
    let [input] = direction.inputs.as_slice() else {
        panic!("one input");
    };
    assert_eq!(
        input.role,
        Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Parallel,
        })
    );
    let Some(InputReference::Picks(picks)) = &input.reference else {
        panic!("a picked reference");
    };
    let picks: &OrderedSelection = picks;
    assert_eq!(
        native_value(picks),
        Some(&NativeNameValue::Text(NonBlankString::new(SCOPE).unwrap()))
    );
    let wire = serde_json::to_value(FeatureOperation::Split(split.clone())).unwrap();
    let back: FeatureOperation = serde_json::from_value(wire).unwrap();
    assert_eq!(back, FeatureOperation::Split(split));
}

/// §6.5: the solid-body and shelled-body modes keep their source names, and
/// an unread mode or target is stated as unread.
#[test]
fn a_silhouette_split_keeps_its_body_modes_native() {
    let kept = |target, mode| {
        let Some(FeatureOperation::Split(split)) = project(&silhouette(target, mode), SCOPE) else {
            panic!("a typed split");
        };
        let SplitMode::ByTools { targets, kept, .. } = split.mode else {
            panic!("by tools");
        };
        (targets.picks, kept)
    };
    let group = || BodySelection::Native(GROUP.to_owned());
    assert_eq!(
        kept(group(), SilhouetteSplitMode::SolidBody).1,
        Some(Kept::Native(
            NonBlankString::new("f3d:silhouette_split_solid_body").unwrap()
        ))
    );
    assert_eq!(
        kept(group(), SilhouetteSplitMode::ShelledBody).1,
        Some(Kept::Native(
            NonBlankString::new("f3d:silhouette_split_shelled_body").unwrap()
        ))
    );
    let (picks, mode) = kept(BodySelection::Unresolved, SilhouetteSplitMode::Unresolved);
    assert_eq!(picks, Picks::Unread);
    assert_eq!(mode, None);
}
