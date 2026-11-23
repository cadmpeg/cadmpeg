// SPDX-License-Identifier: Apache-2.0

use super::project_surface_extend;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

const SCOPE_RECORD: u32 = 12;
const INPUT_STATE: i64 = 3;

fn scope(edge_record_indices: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#surface-extend",
        crate::records::feature::scope::DesignFeatureKind::SurfaceExtend,
        SCOPE_RECORD,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignSurfaceExtendOperation {
            distance: crate::test_support::real(0.5),
            distance_offset: 40,
            distance_record_index: 400,
            boundary_record_index: 500,
            boundary_reference_record_index: 900,
            boundary_reference_offset: 106,
            edge_record_indices,
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(f64::EPSILON)
                .expect("checked fixture value"),
            tolerance_offset: 139,
            method_code: Some(1),
            method_code_offset: Some(29),
        });
    }
    scope
}

fn operand(
    scope_record: u32,
    record: u32,
    state: Option<i64>,
    preceding_faces: &[&str],
) -> DesignEdgeOperand {
    let mut value = serde_json::json!({
        "id": format!("f3d:native/BulkStream.dat:design-edge-operand#{record}"),
        "scope_record_index": scope_record,
        "scope_reference_ordinal": 2,
        "record_index": record,
        "byte_offset": 0,
        "class_tag": "322",
        "paired_byte_offset": 16,
        "paired_class_tag": "265",
        "recipe_record_index": record + 3,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [-1, -1, 2],
        "preceding_candidate_faces": preceding_faces,
        "next_record_index": record + 4,
        "next_byte_offset": 160
    });
    if let Some(state) = state {
        value["recipe_state_id"] = state.into();
    }
    serde_json::from_value(value).expect("edge operand fixture")
}

fn project(
    scope: &DesignParameterScope,
    operands: &[DesignEdgeOperand],
) -> (FaceSelection, Vec<String>) {
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::ExtendSurface { faces, .. })) =
        project_surface_extend(scope, operands, &mut losses)
    else {
        panic!("SurfaceExtend did not project as a typed feature");
    };
    let codes = losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    (faces, codes)
}

fn historical_slots(scope: &DesignParameterScope, faces: &FaceSelection) -> Vec<String> {
    let FaceSelection::Historical { state, faces, .. } = faces else {
        panic!("faces are not a historical selection: {faces:?}");
    };
    let feature = crate::ids::neutral_feature_id(scope);
    assert_eq!(
        *state,
        crate::ids::feature_input_topology_id(&feature, INPUT_STATE)
    );
    faces.as_slice().iter().map(ToString::to_string).collect()
}

fn input_face(scope: &DesignParameterScope, slot: i64) -> String {
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature, INPUT_STATE);
    crate::ids::history_input_face_id(&prefix, slot).to_string()
}

#[test]
fn one_boundary_edge_states_the_input_face_it_bounds_and_charges_the_loss() {
    let scope = scope(vec![503]);
    let operands = [operand(
        SCOPE_RECORD,
        503,
        Some(INPUT_STATE),
        &["f3d:brep:entity#8"],
    )];
    let (faces, codes) = project(&scope, &operands);
    assert_eq!(historical_slots(&scope, &faces), [input_face(&scope, 8)]);
    assert_eq!(codes, ["feature.surface-extend-edges-as-faces"]);
}

#[test]
fn two_boundary_edges_of_one_face_state_that_face_once() {
    let scope = scope(vec![503, 507]);
    let operands = [
        operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
        operand(SCOPE_RECORD, 507, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
    ];
    let (faces, codes) = project(&scope, &operands);
    assert_eq!(historical_slots(&scope, &faces), [input_face(&scope, 8)]);
    assert_eq!(codes, ["feature.surface-extend-edges-as-faces"]);
}

#[test]
fn boundary_edges_of_two_faces_state_both_faces_in_edge_order() {
    let scope = scope(vec![503, 507]);
    let operands = [
        operand(SCOPE_RECORD, 507, Some(INPUT_STATE), &["f3d:brep:entity#6"]),
        operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
    ];
    let (faces, _) = project(&scope, &operands);
    assert_eq!(
        historical_slots(&scope, &faces),
        [input_face(&scope, 8), input_face(&scope, 6)]
    );
}

fn assert_native(scope: &DesignParameterScope, operands: &[DesignEdgeOperand]) {
    let (faces, codes) = project(scope, operands);
    let FaceSelection::Native(native) = faces else {
        panic!("faces are not native: {faces:?}");
    };
    assert!(native.ends_with(":design-record#500"));
    assert!(codes.is_empty());
}

#[test]
fn an_edge_without_an_operand_keeps_the_native_boundary() {
    let scope = scope(vec![503, 507]);
    assert_native(
        &scope,
        &[operand(
            SCOPE_RECORD,
            503,
            Some(INPUT_STATE),
            &["f3d:brep:entity#8"],
        )],
    );
}

#[test]
fn an_operand_of_another_scope_keeps_the_native_boundary() {
    let scope = scope(vec![503]);
    assert_native(
        &scope,
        &[operand(
            SCOPE_RECORD + 1,
            503,
            Some(INPUT_STATE),
            &["f3d:brep:entity#8"],
        )],
    );
}

#[test]
fn two_operands_for_one_edge_keep_the_native_boundary() {
    let scope = scope(vec![503]);
    assert_native(
        &scope,
        &[
            operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
            operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
        ],
    );
}

#[test]
fn an_edge_with_two_candidate_faces_keeps_the_native_boundary() {
    let scope = scope(vec![503]);
    assert_native(
        &scope,
        &[operand(
            SCOPE_RECORD,
            503,
            Some(INPUT_STATE),
            &["f3d:brep:entity#8", "f3d:brep:entity#6"],
        )],
    );
}

#[test]
fn an_edge_without_a_candidate_face_keeps_the_native_boundary() {
    let scope = scope(vec![503]);
    assert_native(
        &scope,
        &[operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &[])],
    );
}

#[test]
fn an_operand_without_an_input_state_keeps_the_native_boundary() {
    let scope = scope(vec![503]);
    assert_native(
        &scope,
        &[operand(SCOPE_RECORD, 503, None, &["f3d:brep:entity#8"])],
    );
}

#[test]
fn operands_of_two_input_states_keep_the_native_boundary() {
    let scope = scope(vec![503, 507]);
    assert_native(
        &scope,
        &[
            operand(SCOPE_RECORD, 503, Some(INPUT_STATE), &["f3d:brep:entity#8"]),
            operand(
                SCOPE_RECORD,
                507,
                Some(INPUT_STATE + 1),
                &["f3d:brep:entity#8"],
            ),
        ],
    );
}
