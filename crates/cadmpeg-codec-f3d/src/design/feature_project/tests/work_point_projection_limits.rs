// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::work_geometry::{
    DesignVertexRecipe, DesignVertexRecipeDraft, DesignVertexResolution,
    DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
    DesignWorkPointPlaneSelection, DesignWorkPointPlaneSelectionDraft, DesignWorkPointRule,
    DesignWorkPointRuleForm,
};
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId;
use std::collections::HashMap;

fn scope() -> DesignParameterScope {
    DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#20",
        DesignFeatureKind::WorkPoint,
        20,
    )
}

fn construction(form: DesignWorkPointRuleForm) -> DesignWorkPointConstruction {
    DesignWorkPointConstruction {
        point_record_index: 21,
        point_record_byte_offset: 0,
        position: crate::test_support::reals([0.0, 0.0, 0.0]),
        position_offset: 0,
        rule: DesignWorkPointRule::try_from(form).unwrap(),
        reference_type_offset: 0,
    }
}

fn input(record_index: u32, carrier: DesignWorkPointInputCarrier) -> DesignWorkPointInput {
    DesignWorkPointInput::try_new(record_index, 0, Some(Box::new(carrier))).unwrap()
}

fn vertex_construction(historical: bool) -> DesignWorkPointConstruction {
    let recipe = DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
        record_index: 22,
        byte_offset: 0,
        class_tag: "369".to_owned().try_into().unwrap(),
        paired_byte_offset: 16,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        recipe_record_index: 25,
        recipe_record_byte_offset: 32,
        recipe_id: "f3d:native/BulkStream.dat:construction-recipe#vertex".to_owned(),
        recipe_prefix_offset: 43,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 4,
        recipe_program: vec![0],
        resolution: historical.then(|| DesignVertexResolution::new(4, 43).unwrap()),
        next_record_index: 27,
        next_byte_offset: 200,
    })
    .unwrap();
    construction(DesignWorkPointRuleForm::Vertex {
        input: input(22, DesignWorkPointInputCarrier::VertexRecipe { recipe }),
    })
}

fn plane_construction() -> DesignWorkPointConstruction {
    let plane = |record_index, work_plane_scope_record_index| {
        let selection = DesignWorkPointPlaneSelection::try_new(
            record_index,
            DesignWorkPointPlaneSelectionDraft {
                class_tag: "267".to_owned().try_into().unwrap(),
                asset_id: "00000000-0000-0000-0000-000000000001"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 1,
                context_id: "00000000-0000-0000-0000-000000000002"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 2,
                identity_record_index: record_index + 3,
                identity_record_offset: 3,
                primary_identity: u64::from(work_plane_scope_record_index - 1),
                primary_identity_offset: 24,
                work_plane_scope_record_index,
                next_record_index: record_index + 4,
                next_byte_offset: 32,
            },
        )
        .unwrap();
        input(
            record_index,
            DesignWorkPointInputCarrier::WorkPlane { selection },
        )
    };
    construction(DesignWorkPointRuleForm::ThreePlaneIntersection {
        inputs: [plane(42, 10), plane(46, 20), plane(50, 30)],
    })
}

fn edge_operand(historical: bool) -> DesignEdgeOperand {
    let mut operand: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:native/BulkStream.dat:design-edge-operand#42",
        "scope_record_index": 20,
        "scope_reference_ordinal": 0,
        "record_index": 42,
        "byte_offset": 0,
        "class_tag": "376",
        "paired_byte_offset": 16,
        "paired_class_tag": "260",
        "recipe_record_index": 45,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:native/BulkStream.dat:construction-recipe#edge",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [],
        "next_record_index": 46,
        "next_byte_offset": 160
    }))
    .unwrap();
    if historical {
        operand.recipe_state_id = Some(4);
        operand.resolved_edge_slot = Some(7);
    }
    operand
}

fn assert_retained_refusal(
    construction: &DesignWorkPointConstruction,
    edge_operands: &[DesignEdgeOperand],
    scope_ids: &HashMap<(&str, u32), FeatureId>,
    operation: &'static str,
    required_bytes: usize,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let preceding = match operation {
        "f3d WorkPoint historical edge operand id"
        | "f3d WorkPoint historical vertex recipe id" => {
            let feature = crate::ids::neutral_feature_id(&scope());
            let prefix = crate::ids::history_input_prefix(&feature.key(), 4);
            let state = crate::ids::feature_input_topology_id(&feature, 4);
            let entity_len = if operation == "f3d WorkPoint historical edge operand id" {
                crate::ids::history_input_edge_id(&prefix, 7).as_str().len()
            } else {
                crate::ids::history_input_vertex_id(&prefix, 43)
                    .as_str()
                    .len()
            };
            feature.as_str().len() + prefix.as_str().len() + state.as_str().len() + entity_len
        }
        _ => 0,
    };
    policy.limits.max_retained_bytes = u64::try_from(preceding + required_bytes - 1).unwrap();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::project_work_point_construction(
        &ctx,
        &scope(),
        construction,
        &[],
        edge_operands,
        scope_ids,
    );
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(ref failure))
        if failure.operation == operation && failure.dimension == ResourceDimension::RetainedBytes),
        "expected {operation} refusal, got {result:?}"
    );
}

#[test]
fn work_point_native_vertex_recipe_id_refuses_retained_limit() {
    let construction = vertex_construction(false);
    let len = "f3d:native/BulkStream.dat:construction-recipe#vertex".len();
    assert_retained_refusal(
        &construction,
        &[],
        &HashMap::new(),
        "f3d WorkPoint native vertex recipe id",
        len,
    );
}

#[test]
fn work_point_historical_vertex_recipe_id_refuses_retained_limit() {
    let construction = vertex_construction(true);
    let len = "f3d:native/BulkStream.dat:construction-recipe#vertex".len();
    assert_retained_refusal(
        &construction,
        &[],
        &HashMap::new(),
        "f3d WorkPoint historical vertex recipe id",
        len,
    );
}

#[test]
fn work_point_plane_feature_id_refuses_retained_limit() {
    let construction = plane_construction();
    let id = FeatureId::mint("synthetic:test:id#f3d:work-plane:10").unwrap();
    let scope_ids = HashMap::from([(("f3d:native/BulkStream.dat", 10), id.clone())]);
    assert_retained_refusal(
        &construction,
        &[],
        &scope_ids,
        "f3d WorkPoint plane feature id",
        id.as_str().len(),
    );
}

#[test]
fn work_point_native_edge_operand_id_refuses_retained_limit() {
    let operand = edge_operand(false);
    let construction = construction(DesignWorkPointRuleForm::CircleCenter {
        input: input(
            42,
            DesignWorkPointInputCarrier::EdgeRecipe {
                operand_id: operand.id.clone(),
            },
        ),
    });
    assert_retained_refusal(
        &construction,
        std::slice::from_ref(&operand),
        &HashMap::new(),
        "f3d WorkPoint native edge operand id",
        operand.id.len(),
    );
}

#[test]
fn work_point_historical_edge_operand_id_refuses_retained_limit() {
    let operand = edge_operand(true);
    let construction = construction(DesignWorkPointRuleForm::CircleCenter {
        input: input(
            42,
            DesignWorkPointInputCarrier::EdgeRecipe {
                operand_id: operand.id.clone(),
            },
        ),
    });
    assert_retained_refusal(
        &construction,
        std::slice::from_ref(&operand),
        &HashMap::new(),
        "f3d WorkPoint historical edge operand id",
        operand.id.len(),
    );
}

#[test]
fn work_point_without_recipe_state_skips_terminal_edge_proof() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_ir::features::{DatumPointConstruction, EdgeSelection};

    let side = |scalars: [i32; 2]| {
        serde_json::from_value(serde_json::json!({
            "field_count": 3, "header_value": 0, "scalars": scalars,
            "payload_prefix": [0], "payload_entry_count": 0, "entries": []
        })).unwrap()
    };
    let mut operand = edge_operand(false);
    operand.recipe_structure = Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2, sides: vec![side([2, 1]), side([1, 3])],
    });
    operand.terminal_reference_edge_slots = vec![vec![25], vec![25], vec![35]];
    operand.terminal_boundary_edge_slots = vec![25];
    let construction = construction(DesignWorkPointRuleForm::CircleCenter {
        input: input(42, DesignWorkPointInputCarrier::EdgeRecipe { operand_id: operand.id.clone() }),
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for operation in ["scan F3D terminal reference edges", "find F3D terminal shared reference edge"] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let scope = scope();
            let project = |operand: &DesignEdgeOperand| {
                super::super::project_work_point_construction(ctx, &scope, &construction, &[], std::slice::from_ref(operand), &HashMap::new())
            };
            operand.recipe_state_id = Some(4);
            let Some(DatumPointConstruction::CircleCenter { edge: EdgeSelection::Historical { state, edges, native } }) = project(&operand).unwrap() else { panic!("historical circle center"); };
            let feature = crate::ids::neutral_feature_id(&scope);
            let prefix = crate::ids::history_input_prefix(&feature.key(), 4);
            assert_eq!(state, crate::ids::feature_input_topology_id(&feature, 4));
            assert_eq!(edges.as_slice(), &[crate::ids::history_input_edge_id(&prefix, 25)]);
            assert_eq!(native.as_str(), operand.id);

            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            operand.recipe_state_id = None;
            assert!(matches!(project(&operand).unwrap(), Some(DatumPointConstruction::CircleCenter { edge: EdgeSelection::Native(native) }) if native == operand.id));
            operand.recipe_state_id = Some(4);
            let CodecError::ResourceLimit(original) = project(&operand).unwrap_err() else { panic!("terminal proof refusal"); };
            assert_eq!(original.operation, operation);
            operand.recipe_state_id = None;
            let CodecError::ResourceLimit(sticky) = project(&operand).unwrap_err() else { panic!("sticky projection refusal"); };
            assert_eq!(sticky, original);
        });
    }
}
