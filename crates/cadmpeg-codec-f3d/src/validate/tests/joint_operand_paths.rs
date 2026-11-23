// SPDX-License-Identifier: Apache-2.0
//! Joint operand occurrence-path resolution.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocument, JointConnector, JointId, JointOperand, Occurrence,
    PairedJointKind, ProductDefinition, Storage,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::test_support::native_test::f3d_native_mut;

const MESSAGE: &str = "Fusion joint operand occurrence path names no single child occurrence";

fn occurrence_id(id: &str) -> OccurrenceId {
    OccurrenceId::mint(id).unwrap()
}

fn definition_id(id: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(id).unwrap()
}

fn document(path: Vec<String>) -> CadIr {
    let mut ir = CadIr::empty();
    let mut root = ProductDefinition::root(
        definition_id("f3d:model:product#root"),
        ExternalDocument::default(),
    );
    root.occurrences = vec![occurrence_id("f3d:model:occurrence#a")];
    let mut part =
        ProductDefinition::new(definition_id("f3d:model:product#part"), Storage::Embedded);
    part.occurrences = vec![
        occurrence_id("f3d:model:occurrence#a:cb5cf1ed"),
        occurrence_id("f3d:model:occurrence#xref-0-0"),
    ];
    let leaf = ProductDefinition::new(definition_id("f3d:model:product#leaf"), Storage::Embedded);
    ir.model.product_definitions = vec![root, part, leaf];
    ir.model.occurrences = ["a", "a:cb5cf1ed", "xref-0-0"]
        .into_iter()
        .map(|name| {
            let prototype = if name == "a" { "part" } else { "leaf" };
            Occurrence::new(
                occurrence_id(&format!("f3d:model:occurrence#{name}")),
                definition_id(&format!("f3d:model:product#{prototype}")),
                Some(Transform::identity()),
            )
        })
        .collect();
    let connector = |operand| JointConnector {
        operand,
        frame: Transform::identity(),
        detached: false,
    };
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        JointId::mint("f3d:model:joint#bolt").unwrap(),
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            connector(JointOperand::root("root", Vec::new())),
            connector(JointOperand::nested_occurrence(
                occurrence_id("f3d:model:occurrence#a"),
                path,
                "bolt",
                Vec::new(),
            )),
        ],
        None,
    ));
    drop(f3d_native_mut(&mut ir));
    ir
}

fn reports(path: &[&str]) -> bool {
    let ir = document(path.iter().map(|&element| element.to_owned()).collect());
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    })
    .iter()
    .any(|finding| finding.message == MESSAGE)
}

#[test]
fn a_path_element_equal_to_a_child_identity_segment_resolves() {
    assert!(!reports(&[]));
    assert!(!reports(&["cb5cf1ed"]));
    assert!(!reports(&["xref-0-0"]));
    // The occurrence below the path holds no child in the model: the rest of
    // the path lies in its own document.
    assert!(!reports(&["cb5cf1ed", "f1f2f3f4"]));
}

#[test]
fn a_path_element_that_names_no_child_identity_segment_is_an_error() {
    assert!(reports(&["f1f2f3f4"]));
    assert!(reports(&["cb5cf1ed-f1ee-4933-8103-02ea3faf9dd6"]));
}
