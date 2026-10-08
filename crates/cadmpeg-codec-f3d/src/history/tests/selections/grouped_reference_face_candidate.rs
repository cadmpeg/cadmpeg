use crate::history::grouped_reference_face_candidate;
use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::face::DesignFaceOperand;
use std::collections::HashSet;

fn operand() -> DesignFaceOperand {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    for (token, references) in [
        ("1", [10u32, 20].as_slice()),
        ("2", [30u32].as_slice()),
        ("3", [40u32].as_slice()),
        ("4", [50u32].as_slice()),
        ("5", [60u32].as_slice()),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-face-operand#1",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 0,
        "class_tag": "277",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#5",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "next_record_index": 6,
        "next_byte_offset": 160
    }))
    .expect("grouped face operand");
    operand.recipe_prefix_bytes = prefix;
    operand.recipe_references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &operand.recipe_prefix_bytes,
            0,
        )
        .expect("recipe references")
    });
    operand
}

fn topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10, 20],
        ..AsmHistoricalTopology::default()
    }
}

fn refusal(operation: &'static str, changed: HashSet<i64>) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| grouped_reference_face_candidate(ctx, &operand(), &topology(), &changed),
    )
}

#[test]
fn grouped_reference_face_scan_refuses_work_limit() {
    let operation = "scan F3D grouped recipe face references";
    assert!(matches!(
        refusal(operation, HashSet::from([10])),
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn grouped_reference_topology_membership_refuses_work_limit() {
    let operation = "check F3D grouped topology face membership";
    assert!(matches!(
        refusal(operation, HashSet::from([10])),
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
