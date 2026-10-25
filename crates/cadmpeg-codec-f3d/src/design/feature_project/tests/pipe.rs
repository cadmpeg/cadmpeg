// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::{
    feature::path_features::DesignPathFeatureConstruction,
    topology::extrude_selection::DesignOperandRole,
};

#[test]
fn pipe_dispatcher_requires_shape_evidence_beyond_opaque_bytes() {
    use crate::records::topology::construction::DesignConstructionOperandGroupFrame;

    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:pipe-scope#1",
        crate::records::feature::scope::DesignFeatureKind::Pipe,
        1,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("405".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![30, 33, 36, 39, 20, 21, 22]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::path_features::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: Some(26),
                is_hollow: false,
                is_hollow_offset: Some(25),
                opaque_post_operation: [1, 1].into(),
                opaque_post_operation_offset: Some(30),
                values: crate::test_support::reals([1.0, 1.0, 0.6, 0.15]),
                record_indexes: [30, 33, 36, 39],
                value_offsets: Some([40, 151, 262, 373]),
            },
        ));
        scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }

    let parameter = |record_index: u32,
                     source_kind: &str,
                     unit: Option<&str>,
                     evaluated_value: f64| {
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("f3d:Design/BulkStream.dat:pipe-parameter#{record_index}"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                record_index,
                source_ordinal: record_index,
                source: crate::records::parameters::DesignParameterSource::new(
                    source_kind.into(),
                    Some(0),
                    None,
                )
                .unwrap(),
                expression: evaluated_value.to_string(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: unit.map(|value| crate::records::identity::RecordedValue {
                    value: value.to_owned(),
                    offset: 70,
                }),
                name: source_kind.into(),
                name_offset: 80,
                evaluated_value,
                evaluated_value_offset: 90,
            },
        )
        .unwrap()
    };
    let parameters = [
        parameter(31, "AlongDistance", None, 1.0),
        parameter(34, "AgainstDistance", None, 1.0),
        parameter(37, "SectionSize", Some("cm"), 0.6),
        parameter(40, "SectionThickness", Some("cm"), 0.15),
    ];
    let owners = parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| {
            crate::records::parameters::DesignParameterOwner::try_from(
                crate::records::parameters::DesignParameterOwnerWire {
                    id: format!(
                        "f3d:Design/BulkStream.dat:parameter-owner#{}",
                        parameter.record_index - 1
                    ),
                    byte_offset: 0,
                    frame_length: 103,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "342".to_owned(),
                    )
                    .unwrap(),
                    record_index: parameter.record_index - 1,
                    scope_record_index: 1,
                    local_ordinal: u32::try_from(ordinal).unwrap(),
                    evaluated_value: parameter.evaluated_value().get(),
                    evaluated_value_offset: 40,
                    parameter_record_index: parameter.record_index,
                    owned_ordinal: u32::try_from(ordinal).unwrap(),
                    variant: None,
                    companion_record_index: parameter.record_index + 1,
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let path_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:pipe-group#20".into(),
            scope_record_index: 1,
            scope_reference_ordinal: 4,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("312".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();

    for opaque in [[1, 1], [0, 1], [0, 255]] {
        for is_hollow in [false, true] {
            if let crate::records::feature::scope::DesignScopePayloadMut::Pipe(Some(construction)) =
                scope.payload_mut()
            {
                construction.opaque_post_operation = opaque.into();
                construction.is_hollow = is_hollow;
            }
            let scopes = [scope.clone()];
            let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
            let projected = crate::test_support::with_decode_context(|ctx| {
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    ctx,
                    &crate::design::feature_project::ProjectInputs {
                        native: &parameters,
                        owners: &owners,
                        scopes: &scopes,
                        timelines: &timelines,
                        construction_groups: std::slice::from_ref(&path_group),
                        ..Default::default()
                    },
                )
                .unwrap()
            });
            assert_eq!(projected.features.len(), 1);
            assert!(matches!(projected.features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
                if kind.as_str() == "Pipe" && parameters.len() == 4));
        }
    }
}
