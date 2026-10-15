// SPDX-License-Identifier: Apache-2.0

#[test]
fn face_operand_rejects_overflowed_node_offset() {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        references::DesignClassTag,
        topology::face::{DesignFaceOperand, DesignFaceOperandDraft},
    };

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let stream = "f3d:Design/BulkStream.dat";
    let recipe_id = format!("{stream}:construction-recipe#0");
    let program = vec![0, -1];
    let next_byte_offset = 1_063 + u64::try_from(program.len()).unwrap() * 4;
    let mut operand = DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("{stream}:design-face-operand#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 2,
        group: None,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
        paired_byte_offset: 1_016,
        paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
        recipe_record_index: 103,
        recipe_record_byte_offset: 1_032,
        recipe_id,
        recipe_prefix_offset: 1_043,
        recipe_references: Vec::new(),
        recipe_prefix_bytes: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1_063,
        recipe_program: program,
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 105,
        next_byte_offset,
    })
    .unwrap();
    operand.recipe_program_offset = u64::MAX;
    operand.recipe_program = vec![0, -1, -1, 2];
    let native = crate::native::F3dNative {
        design_face_operands: vec![operand],
        ..Default::default()
    };
    let error = crate::test_support::with_decode_context(|service_ctx| {
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_face_operands(&ctx, &mut Vec::new(), &[]).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(ref message)
        if message == "F3D face recipe node offset overflows")
    );
}
