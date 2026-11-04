// SPDX-License-Identifier: Apache-2.0

use super::{history_in_source, native_base_feature};
use crate::history::bind_feature_outputs;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::{BodySelection, DistinctMembers, FeatureDefinition, FeatureOperation};

#[test]
fn feature_outputs_keep_complete_stored_body_selection() {
    for bound in [false, true] {
        let (mut feature, scope, _, body) = native_base_feature();
        if bound {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::BaseFeature {
                        bodies: BodySelection::Resolved {
                            bodies: DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
                            native: scope.id.clone(),
                        },
                    },
                ));
        }
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history_in_source("selected.smbh", &[1, 2])],
            std::slice::from_ref(&body),
            &Default::default(),
            Some("BREP.selected.smbh"),
        )
        .unwrap();
        assert_eq!(feature.evaluation.outputs().as_slice(), [body.id]);
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::FeatureOutputBodyUnbound.kind())
                .count(),
            usize::from(!bound)
        );
        let stored = crate::history::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].code,
            if bound {
                F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
            } else {
                F3dLossCode::FeatureStoredBodiesUnbound.kind()
            }
        );
    }
}
