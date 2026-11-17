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
            repeats_body_suffixes: true,
        });
    }
    scope
}

fn project(
    scope: DesignParameterScope,
) -> crate::design::feature_project::ProjectedParameterDesign {
    project_with_bindings(scope, &[])
}

fn project_with_bindings(
    scope: DesignParameterScope,
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
) -> crate::design::feature_project::ProjectedParameterDesign {
    let scopes = [scope];
    crate::test_support::with_decode_context(|ctx| {
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                body_bindings,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    })
}

fn null_flag_losses(
    projection: &crate::design::feature_project::ProjectedParameterDesign,
) -> usize {
    losses_of(projection, F3dLossCode::FeatureBaseFeatureNullFlagAssumed)
}

fn losses_of(
    projection: &crate::design::feature_project::ProjectedParameterDesign,
    code: F3dLossCode,
) -> usize {
    projection
        .projection_losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

#[test]
fn a_flagged_suffix_result_body_frame_projects_its_bodies_with_the_loss() {
    let mut scope = base_feature(None);
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(DesignBaseFeatureConstruction::SuffixResultBodies {
            bodies: vec![DesignBaseFeatureEntry {
                value: 218,
                offset: 25,
                field: [0; 6],
            }],
            repeated_fields: vec![[0; 6].into()],
            results: vec![DesignBaseFeatureEntry {
                value: 229,
                offset: 86,
                field: [0; 6],
            }],
            metadata_record: 215,
            metadata_record_offset: 67,
            metadata_field: vec![0; 6].into(),
            null_reference_flag_offset: Some(48),
        });
    }
    let projection = project(scope);
    assert!(matches!(
        projection.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
    ));
    assert_eq!(null_flag_losses(&projection), 1);
    assert_eq!(
        losses_of(
            &projection,
            F3dLossCode::FeatureBaseFeatureSuffixRunOrderAssumed
        ),
        0
    );
}

#[test]
fn a_suffix_result_body_frame_resolves_its_body_through_the_body_map() {
    let (mut scope, binding) = super::body_selection::fixture();
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(DesignBaseFeatureConstruction::SuffixResultBodies {
            bodies: vec![DesignBaseFeatureEntry {
                value: binding.entity_suffix,
                offset: 25,
                field: [0; 6],
            }],
            repeated_fields: vec![[0; 6].into()],
            results: vec![DesignBaseFeatureEntry {
                value: 229,
                offset: 86,
                field: [0; 6],
            }],
            metadata_record: 215,
            metadata_record_offset: 67,
            metadata_field: vec![0; 6].into(),
            null_reference_flag_offset: None,
        });
    }
    let projection = project_with_bindings(scope, std::slice::from_ref(&binding));
    assert!(matches!(
        projection.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Resolved { bodies, .. },
        }) if bodies.as_slice() == [binding.body.clone().unwrap()]
    ));
}

#[test]
fn a_two_body_suffix_result_body_frame_reports_its_run_order() {
    let body = |value, offset| DesignBaseFeatureEntry::<u64> {
        value,
        offset,
        field: [0; 6],
    };
    let result = |value, offset| DesignBaseFeatureEntry::<u32> {
        value,
        offset,
        field: [0; 6],
    };
    let mut scope = base_feature(None);
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(DesignBaseFeatureConstruction::SuffixResultBodies {
            bodies: vec![body(218, 25), body(219, 40)],
            repeated_fields: vec![[0; 6].into(); 2],
            results: vec![result(229, 112), result(230, 123)],
            metadata_record: 215,
            metadata_record_offset: 93,
            metadata_field: vec![0; 6].into(),
            null_reference_flag_offset: None,
        });
    }
    let projection = project(scope);
    assert_eq!(
        losses_of(
            &projection,
            F3dLossCode::FeatureBaseFeatureSuffixRunOrderAssumed
        ),
        1
    );
    assert_eq!(null_flag_losses(&projection), 0);
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
