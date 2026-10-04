// SPDX-License-Identifier: Apache-2.0

use super::super::close_sketch_constraint_parameter_references;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntityId, SketchId,
};

#[test]
fn emitted_parameter_identity_refuses_work_and_preserves_constraint() {
    let parameter_id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#1:7")
        .expect("sketch entity ID");
    let sketch_id = SketchId::mint("creo:model:sketch#1").expect("sketch ID");
    let constraint_id = SketchConstraintId::mint("creo:model:sketch_constraint#1")
        .expect("constraint ID");
    let radius = SketchConstraintDefinitionInput::Radius {
        entity: entity_id,
        parameter: parameter_id.clone(),
    };
    let mut input = CadIr::empty();
    input.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: None,
        ordinal: 0,
        name: "radius".into(),
        expression: "radius".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let second_parameter_id = ParameterId::mint("creo:featdefs:parameter#2")
        .expect("second parameter ID");
    input.model.parameters.push(DesignParameter {
        id: second_parameter_id.clone(),
        owner: None,
        ordinal: 1,
        name: "second".into(),
        expression: "second".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    input.model.sketch_constraints.push(SketchConstraint {
        id: constraint_id,
        sketch: sketch_id,
        definition: SketchConstraintDefinition::try_from(radius.clone())
            .expect("radius definition"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });

    let service = crate::test_support::assert_work_boundaries(
        &["creo emitted parameter identity membership"],
        |ctx| {
            let mut output = input.clone();
            close_sketch_constraint_parameter_references(ctx, &mut output)?;
            Ok(output)
        },
    );
    assert_eq!(service.model.parameters.len(), 2);
    assert_eq!(service.model.parameters[0].id, parameter_id);
    assert_eq!(service.model.parameters[1].id, second_parameter_id);
    assert_eq!(service.model.sketch_constraints.len(), 1);
    assert_eq!(
        *service.model.sketch_constraints[0].definition.kind(),
        radius
    );
}
