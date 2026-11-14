// SPDX-License-Identifier: Apache-2.0

use crate::loss::F3dLossCode;
use crate::records::feature::base_feature::{
    DesignBaseFeatureConstruction, DesignBaseFeatureEntry, DesignBaseFeatureResultBody,
    DesignBaseFeatureResults,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

const STREAM: &str = "f3d:native/BulkStream.dat";

fn base_feature(null_reference_flag_offset: Option<u64>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#202"),
        DesignFeatureKind::BaseFeature,
        202,
    );
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: DesignBaseFeatureResults::WithoutRepeatedFields(vec![
                DesignBaseFeatureResultBody {
                    entity: DesignBaseFeatureEntry {
                        value: 209,
                        offset: 25,
                        field: [0; 6],
                    },
                    reference: DesignBaseFeatureEntry {
                        value: 215,
                        offset: 40,
                        field: [0; 6],
                    },
                    result: DesignBaseFeatureEntry {
                        value: 220,
                        offset: 101,
                        field: [0; 6],
                    },
                },
            ]),
            metadata_record: 206,
            metadata_record_offset: 82,
            metadata_field: vec![0; 6].into(),
            null_reference_flag_offset,
        });
    }
    scope
}

fn project(
    scope: DesignParameterScope,
) -> crate::design::feature_project::ProjectedParameterDesign {
    let scopes = [scope];
    crate::test_support::with_decode_context(|ctx| {
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    })
}

fn null_flag_losses(
    projection: &crate::design::feature_project::ProjectedParameterDesign,
) -> usize {
    projection
        .projection_losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::FeatureBaseFeatureNullFlagAssumed.kind())
        .count()
}

#[test]
fn a_flagged_null_reference_projects_the_result_bodies_with_its_loss() {
    for (flag_offset, losses) in [(None, 0), (Some(63), 1)] {
        let projection = project(base_feature(flag_offset));
        assert!(
            matches!(
                projection.features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
            ),
            "{flag_offset:?}"
        );
        assert_eq!(null_flag_losses(&projection), losses, "{flag_offset:?}");
    }
}
