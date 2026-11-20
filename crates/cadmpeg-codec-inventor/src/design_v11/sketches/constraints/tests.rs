// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::sketches::SketchId;

fn sketch_id() -> SketchId {
    SketchId::mint("inventor:design:sketch#test").expect("id")
}

fn entity_id(name: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("inventor:design:entity#{name}")).expect("id")
}

#[test]
fn established_constraints_keep_parameter_bindings() {
    use cadmpeg_ir::features::ParameterId;
    let parameter = ParameterId::mint("inventor:design:parameter#radius").expect("parameter");
    let relation = project(
        &serde_json::from_value(serde_json::json!({
            "id":"inventor:design:constraint#radius", "sketch":sketch_id(),
            "definition": {"kind":"radius", "entity":entity_id("circle"), "parameter":parameter}
        }))
        .expect("constraint"),
    );
    assert_eq!(
        relation.relation,
        SketchRelation::Radial {
            of: entity_id("circle"),
            measure: RadialMeasure::Radius,
            value: cadmpeg_ir::design::dimension::Dimension::Parameter(parameter)
        }
    );
    let definition: cadmpeg_ir::sketches::SketchConstraintDefinition = Kind::Horizontal {
        entity: entity_id("line"),
    }
    .try_into()
    .expect("definition");
    let source: cadmpeg_ir::sketches::SketchConstraint = serde_json::from_value(serde_json::json!({
        "id":"inventor:design:constraint#horizontal", "sketch":sketch_id(), "definition":definition
    })).expect("constraint");
    assert_eq!(
        project(&source).relation,
        SketchRelation::Horizontal {
            a: cadmpeg_ir::design::ops::sketch::Operand::Entity(entity_id("line")),
            b: None
        }
    );
}
