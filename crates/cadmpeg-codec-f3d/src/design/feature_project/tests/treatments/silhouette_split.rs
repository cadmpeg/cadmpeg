// SPDX-License-Identifier: Apache-2.0

use super::{localized_fillet_group_with_role, localized_fillet_scope_without_owners};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::feature::silhouette_split::{DesignSilhouetteDirection, DesignSilhouetteSplit};
use crate::records::identity::ReferenceRun;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::surfaces::SilhouetteSplitMode;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::FiniteReal;

const EPS_DIRECTION: f64 = 1.0e-12;

#[test]
fn silhouette_split_projects_the_saved_mode_direction_and_body_group() {
    let mut scope = localized_fillet_scope_without_owners();
    scope.source_kind = None;
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::SilhouetteSplit(Some(DesignSilhouetteSplit {
                mode_word: 1,
                carrier: Some(DesignSilhouetteDirection {
                    position: [FiniteReal::ZERO; 3],
                    direction: [2.5, 0.0, 0.0].map(|value| FiniteReal::try_from(value).unwrap()),
                    reference_type: 7,
                }),
            }));
            draft.reference_members = ReferenceRun::unlocated(vec![200, 201, 300, 301]);
            draft.layout_fixture_references();
            let kind_bytes =
                cadmpeg_core::decode::u64_from_index("SilhouetteSplit".encode_utf16().count()) * 2;
            draft.paired_byte_offset = draft.kind_offset + kind_bytes + 72;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = localized_fillet_group_with_role(300, 2, vec![301], DesignOperandRole::BODIES_A);
    let mut losses = Vec::new();
    let projected = crate::design::feature_project::silhouette_split::project(
        &scope,
        std::slice::from_ref(&group),
        &mut losses,
    )
    .unwrap();
    assert!(losses.is_empty());
    let FeatureDefinition::Operation(FeatureOperation::SilhouetteSplit {
        target: BodySelection::Native(ref native),
        direction: Some(direction),
        mode: SilhouetteSplitMode::ShelledBody,
    }) = projected
    else {
        panic!("expected a shelled Silhouette Split");
    };
    assert_eq!(native, &group.id);
    assert!((direction.get().x - 2.5).abs() < EPS_DIRECTION);
    let wire = crate::json_value(&projected).unwrap();
    assert_eq!(wire["value"]["kind"], "silhouette_split");
    assert_eq!(wire["value"]["value"]["mode"], "shelled_body");
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        projected
    );
    let projected = crate::design::feature_project::silhouette_split::project(
        &scope,
        &[group.clone(), group],
        &mut losses,
    );
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Operation(
            FeatureOperation::SilhouetteSplit {
                target: BodySelection::Unresolved,
                mode: SilhouetteSplitMode::ShelledBody,
                ..
            }
        ))
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureSilhouetteSplitTargetUnresolved.kind()
    );
}
