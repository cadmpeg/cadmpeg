// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, OperandContainer};

use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::feature::assembly::{
    DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
    DesignAssemblyOperandPath, DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier,
    DesignJointFrameSense, TYPED_OCCURRENCE_PATH_TYPE_GUID,
};
use crate::records::feature::assembly_features::DesignComponentInsertConstruction;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayload, DesignScopePayloadMut,
};
use crate::records::identity::{Located, ReferenceRun};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const INSERT_KEY: &str = "bc7c305e-42cc-4c2d-a213-063506349b08";
const NESTED_KEY: &str = "d7dd44c5-9574-4a6d-9dd2-dc82af142924";
const ASSEMBLE_RECORD: u32 = 3;

fn insert_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#100"),
        DesignFeatureKind::ComponentInsert,
        9,
    );
    if let DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
        slot.construction = Some(DesignComponentInsertConstruction {
            relation_record_index: 8,
            carrier_record_index: 7,
            occurrence_identity: Some(17),
            neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
            neutron_role_offset: 259,
            occurrence_key: Some(Located {
                value: INSERT_KEY.to_ascii_uppercase().try_into().unwrap(),
                offset: 40,
            }),
            placement: None,
        });
    }
    scope
}

fn operand_path(record_index: u32) -> DesignAssemblyOperandPath {
    let link = DesignAssemblyOperandPathLink {
        locator_reference_offset: 0,
        locator_record_index: record_index - 1,
        locator_class_tag: "337".to_owned().try_into().unwrap(),
        locator_byte_offset: 0,
        locator_scope_reference_offset: 0,
        wrapper_record_index: record_index + 1,
        wrapper_reference_offset: 0,
        wrapper_class_tag: "341".to_owned().try_into().unwrap(),
        wrapper_byte_offset: 0,
        path_reference_offset: 0,
    };
    DesignAssemblyOperandPath::try_new_typed(
        link,
        record_index,
        "384".to_owned().try_into().unwrap(),
        Some(
            TYPED_OCCURRENCE_PATH_TYPE_GUID
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        100,
        [INSERT_KEY, NESTED_KEY]
            .into_iter()
            .zip([200, 240])
            .map(|(key, offset)| Located {
                value: key.to_owned().try_into().unwrap(),
                offset,
            })
            .collect(),
        (0..4)
            .map(|ordinal| Located {
                value: "22222222-2222-2222-2222-22222222222b"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                offset: 300 + ordinal,
            })
            .collect(),
    )
    .unwrap()
}

fn assemble_scope() -> DesignParameterScope {
    let frames = [10, 20].map(|index| DesignAssemblyOperandFrame {
        reference_record_index: index,
        reference_offset: 0,
        transform: cadmpeg_ir::transform::Transform::identity()
            .rows()
            .try_into()
            .unwrap(),
        transform_offset: 0,
    });
    let qualifiers = [11, 21].map(|index| DesignAssemblyOperandQualifier::OccurrencePath {
        path: operand_path(index),
    });
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{ASSEMBLE_RECORD}"),
        DesignFeatureKind::Assemble,
        ASSEMBLE_RECORD,
    );
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Assemble(Some(
                DesignAssemblyAlignment::try_new(
                    0.0,
                    [0.0; 3],
                    Vec::new(),
                    Some(DesignAssemblyAlignmentForm::qualified(frames, qualifiers)),
                )
                .unwrap()
                .with_frame_sense(Some(DesignJointFrameSense::Aligned)),
            ));
        })
        .unwrap();
    scope
}

fn assemble_type() -> SegmentType {
    SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#0".to_owned(),
        SegmentTypeData {
            byte_offset: 0,
            type_guid: crate::design::decode::scopes::assembly_v3::TYPE_GUID
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Absent,
            version: 3,
            version_offset: 0,
            module: "Fusion".to_owned(),
            entities: ReferenceRun::unlocated(vec![u64::from(ASSEMBLE_RECORD)]),
        },
    )
    .unwrap()
}

fn insert_occurrence() -> Occurrence {
    Occurrence {
        id: OccurrenceId::mint("f3d:model:occurrence#insert").unwrap(),
        prototype: cadmpeg_ir::products::PrototypeReference::Unresolved {},
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: Some(format!("{STREAM}:design-parameter-scope#100")),
    }
}

#[test]
fn writer_accepts_a_standalone_joint_nested_below_a_component_insert_key() {
    let native = crate::native::F3dNative {
        design_parameter_scopes: vec![insert_scope(), assemble_scope()],
        design_types: vec![assemble_type()],
        ..Default::default()
    };
    let occurrence = insert_occurrence();
    let mut target = CadIr::empty();
    target.model.occurrences = vec![occurrence.clone()];
    let projected = crate::design::assembly::project_assembly_joints_with_paths(
        &native.design_parameter_scopes,
        &[],
        &crate::design::components::ComponentInserts::decide(
            &native.design_parameter_scopes,
            &[],
            &[],
        ),
        &[],
        &crate::design::components::paths::LocalOccurrencePaths::new(
            &[],
            &[],
            &target.model.occurrences,
            &native.design_parameter_scopes,
        ),
    );
    assert_eq!(projected.joints.len(), 1);
    let mut nested = projected.joints.clone();
    for connector in nested[0].connectors_mut() {
        assert_eq!(connector.operand.subelements, [NESTED_KEY]);
        connector.operand.container = OperandContainer::Occurrence {
            occurrence: occurrence.id.clone(),
            occurrence_path: std::mem::take(&mut connector.operand.subelements),
        };
    }

    target.model.assembly_joints = nested;
    crate::writer::primitives::validate_assembly_projection(&target, Some(&native))
        .expect("the writer re-projects the nested occurrence path");

    target.model.assembly_joints = projected.joints;
    assert!(
        crate::writer::primitives::validate_assembly_projection(&target, Some(&native)).is_err()
    );
}
