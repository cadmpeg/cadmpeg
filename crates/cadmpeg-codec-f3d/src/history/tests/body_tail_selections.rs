// SPDX-License-Identifier: Apache-2.0
//! Body selections whose group members are whole-body reference tails.

use super::bind_feature_body_selections;
use super::body_selection::body_operand_group;
use crate::history::body_selections::FeatureBodySelectionInputs;
use crate::records::body_reference_tail::{
    BodyTailReference, DesignBodyReferenceTail, DesignBodyReferenceTailWire,
};
use crate::records::feature::combine::DesignCombineBodyNode;
use crate::records::identity::Located;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

const STREAM: &str = "f3d:Design/BulkStream.dat";

/// The tail record 21, a member of group 20, whose one second reference names
/// a body browser node for body entity `entity`.
fn tail(entity: u32) -> DesignBodyReferenceTail {
    let mut reference = vec![1];
    reference.extend_from_slice(&40u64.to_le_bytes());
    reference.extend_from_slice(&[0, 0]);
    DesignBodyReferenceTail::try_from(DesignBodyReferenceTailWire {
        id: format!("{STREAM}:design-body-reference-tail#10"),
        record_index: 21,
        byte_offset: 10,
        serialization_form: Located {
            value: 0,
            offset: 30,
        },
        first_count_offset: 34,
        first_references: Vec::new(),
        second_count_offset: 38,
        second_references: vec![BodyTailReference::new(42, reference).expect("local reference")],
        second_body_nodes: vec![DesignCombineBodyNode {
            record_index: 40,
            body_entity_suffix: entity,
            creating_scope_record_index: 3,
        }],
        extra_zero: None,
        trailing_form: Located {
            value: 8,
            offset: 53,
        },
    })
    .expect("tail")
}

fn binding(
    entity: u64,
    body: Option<cadmpeg_ir::ids::BodyId>,
) -> crate::records::bodies::DesignBodyBinding {
    crate::records::bodies::DesignBodyBinding::try_from(
        crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: format!("{STREAM}:design-body-binding#0"),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 5,
            asm_body_key_offset: 0,
            entity_suffix: entity,
            entity_suffix_offset: 8,
            blob_name: "BREP.a.smbh".into(),
            blob_name_offset: 16,
            body,
        },
    )
    .expect("binding")
}

fn remove_body(group_id: &str, scope_id: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#remove").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native(group_id.into()),
                mode: cadmpeg_ir::features::operation_settings::BodyRetentionMode::DeleteSelected,
            }),
        ),
        native_ref: Some(scope_id.into()),
    }
}

fn bind(
    tail: DesignBodyReferenceTail,
    binding: crate::records::bodies::DesignBodyBinding,
    bodies: &[cadmpeg_ir::topology::Body],
) -> (Feature, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::RemoveBody,
        10,
    );
    let group_id = format!("{STREAM}:design-construction-operand-group#20");
    let group = body_operand_group(&group_id, 10);
    let mut features = [remove_body(&group_id, &scope.id)];
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        scope_histories: &std::collections::HashMap::new(),
        bodies,
        regions: &[],
        shells: &[],
        faces: &[],
        body_reference_tails: std::slice::from_ref(&tail),
        body_bindings: std::slice::from_ref(&binding),
    };
    let losses = bind_feature_body_selections(&mut features, &inputs);
    let [feature] = features;
    (feature, losses)
}

/// A model body with identity `id` and the given visibility.
fn model_body(id: &str, visible: Option<bool>) -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint(id).unwrap();
    body.visible = visible;
    body
}

#[test]
fn a_stateless_remove_body_selects_the_hidden_solved_body_its_tail_node_names() {
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#4").unwrap();
    let hidden = model_body("f3d:brep:entity#4", Some(false));
    let (feature, losses) = bind(
        tail(7),
        binding(7, Some(body.clone())),
        std::slice::from_ref(&hidden),
    );
    assert!(losses.is_empty());
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Resolved { bodies, .. },
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("the selection stays native");
    };
    assert_eq!(bodies.as_slice(), [body]);
}

#[test]
fn a_tail_node_whose_body_pair_is_unsolved_leaves_the_selection_native_with_a_loss() {
    let (feature, losses) = bind(tail(7), binding(7, None), &[]);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(_),
            ..
        })
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureBodyTailUnbound.kind()
    );
}

#[test]
fn a_tail_node_naming_another_entity_leaves_the_selection_native() {
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#4").unwrap();
    let hidden = model_body("f3d:brep:entity#4", Some(false));
    let (feature, _) = bind(
        tail(7),
        binding(8, Some(body)),
        std::slice::from_ref(&hidden),
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(_),
            ..
        })
    ));
}

#[test]
fn a_visible_solved_body_is_not_shown_to_be_the_removed_body() {
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#4").unwrap();
    for visible in [None, Some(true)] {
        let shown = model_body("f3d:brep:entity#4", visible);
        let (feature, losses) = bind(
            tail(7),
            binding(7, Some(body.clone())),
            std::slice::from_ref(&shown),
        );
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native(_),
                ..
            })
        ));
        assert_eq!(losses.len(), 1);
    }
}
