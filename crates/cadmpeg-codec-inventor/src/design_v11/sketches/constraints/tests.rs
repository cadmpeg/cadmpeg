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
        &BTreeMap::new(),
    )
    .expect("projection");
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
        project(&source, &BTreeMap::new())
            .expect("projection")
            .relation,
        SketchRelation::Horizontal {
            a: cadmpeg_ir::design::ops::sketch::Operand::Entity(entity_id("line")),
            b: None
        }
    );
}

#[test]
fn native_dimensions_keep_binding_state_flags_and_operands() {
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    use cadmpeg_ir::features::ParameterId;
    let parameter = ParameterId::mint("inventor:design:parameter#native").expect("parameter");
    for (kind, quantity) in [
        ("angle_dimension", Quantity::ANGLE),
        ("axis_distance_dimension", Quantity::LENGTH),
    ] {
        let source: LegacyConstraint = serde_json::from_value(serde_json::json!({
            "id":"inventor:design:constraint#native", "sketch":sketch_id(), "orientation": 7,
            "definition": {"kind":"native", "native_kind":kind, "native_state":5, "native_flags":u64::MAX,
                "native_properties":{"control":""}, "entities":[entity_id("first"),entity_id("second")], "parameter":parameter,
                "operands":[{"native_kind":"first", "object_index":3, "native_ref":"inventor:pmdc:point#first"}]}
        })).expect("native relation");
        let kinds = BTreeMap::from([(parameter.clone(), ValueKind::Quantity(quantity))]);
        let target = project(&source, &kinds).expect("projection");
        let SketchRelation::Native {
            operands, values, ..
        } = &target.relation
        else {
            panic!("native relation");
        };
        assert_eq!(operands.len(), 2);
        let expected = if quantity == Quantity::ANGLE {
            ScalarDimension::Angle(Dimension::Parameter(parameter.clone()))
        } else {
            ScalarDimension::Length(Dimension::Parameter(parameter.clone()))
        };
        assert_eq!(values, &[expected]);
        assert_eq!(
            target.native_options["inventor:flags"],
            NativeOption::Text(Dimension::Literal(
                cadmpeg_core::text::NonBlankString::new(u64::MAX.to_string()).expect("flags")
            ))
        );
        assert!(target
            .native_options
            .contains_key(&cadmpeg_ir::codec_name!("inventor:operands")));
        assert!(loss(&source, &kinds).is_none());
        let fallback = project(&source, &BTreeMap::new()).expect("retained unresolved parameter");
        assert!(fallback
            .native_options
            .contains_key(&cadmpeg_ir::codec_name!("inventor:relation")));
        assert_eq!(
            loss(&source, &BTreeMap::new()).expect("pinned loss").code,
            crate::loss::InventorLossCode::SketchDesignConstraintNative.kind()
        );
        assert_eq!(
            crate::loss::InventorLossCode::SketchDesignConstraintNative.code(),
            "sketch.design-constraint-native"
        );
    }
}
