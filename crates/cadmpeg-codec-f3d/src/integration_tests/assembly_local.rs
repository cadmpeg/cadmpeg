// SPDX-License-Identifier: Apache-2.0
//! Component geometry remains local while the occurrence graph composes placement.

use cadmpeg_ir::products::{AssemblyGraph, OccurrenceParent};
use cadmpeg_ir::transform::Transform;

use crate::test_support::assembly_test::f3d_without_brep_with_xref_placement;

use super::{decode, f3d_with_smbh, f3z_archive, synthetic_geometry_smbh, XREF_ROLE};

#[test]
fn nested_affine_occurrences_do_not_bake_world_placement_into_component_bodies() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let local = decode(component.clone());
    let outer_rows = [
        [0.0, -2.0, 0.0, 1.0],
        [3.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 4.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let inner_rows = [
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 1.0, 0.0, 5.0],
        [0.0, 0.0, 1.0, 6.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let middle = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "middle.f3d",
        "component.f3d",
        CHILD_ROLE,
        inner_rows,
    );
    let root = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "middle.f3d",
        XREF_ROLE,
        outer_rows,
    );
    let merged = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));
    assert!(
        merged
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code.local_code() == "xref.placement-undecoded" }),
        "the nonuniform source matrix is outside the rigid XREF grammar"
    );
    assert!(merged
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Model,));
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let plan = crate::F3dCodec
        .plan(
            EncodeInput::new(merged.ir(), Some(merged.source_fidelity())),
            TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
        )
        .expect("a supported target synthesizes independently of the malformed source placement");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    // The general affine occurrence is authored in its canonical neutral home.
    // It cannot be admitted as an F3D source proper-rigid placement matrix.
    let expected_outer = Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [3.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 4.0, 30.0],
    ])
    .unwrap();
    let mut authored = merged.ir().clone();
    authored
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .expect("the archive root's component occurrence")
        .transform = expected_outer;
    let model = &authored.model;
    assert_eq!(model.bodies.len(), local.ir().model.bodies.len());
    for (merged_body, local_body) in model.bodies.iter().zip(&local.ir().model.bodies) {
        assert_eq!(merged_body.transform, local_body.transform);
    }
    assert_eq!(model.occurrences.len(), 2);
    let outer = model
        .occurrences
        .iter()
        .find(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .expect("the archive root's component occurrence");
    let inner = model
        .occurrences
        .iter()
        .find(|occurrence| {
            matches!(&occurrence.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &outer.id)
        })
        .expect("the nested component occurrence");
    // Source placement translations are centimetres; the sole canonical
    // occurrence matrix stores millimetres and retains its nonuniform scale.

    let expected_inner = Transform::affine([
        [1.0, 0.0, 0.0, 40.0],
        [0.0, 1.0, 0.0, 50.0],
        [0.0, 0.0, 1.0, 60.0],
    ])
    .unwrap();
    assert_eq!(outer.transform, expected_outer);
    assert_eq!(inner.transform, expected_inner);
    let graph = AssemblyGraph::new(&model.occurrences).expect("resolved occurrence parents");
    assert_eq!(
        graph.effective_transform(&inner.id),
        Some(expected_outer.compose(expected_inner).unwrap())
    );
    assert_ne!(
        expected_outer.compose(expected_inner).unwrap(),
        expected_inner.compose(expected_outer).unwrap()
    );
}

#[test]
fn rigid_source_occurrence_retains_its_actual_matrix_and_translation_scale() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let rows = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let root = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "component.f3d",
        XREF_ROLE,
        rows,
    );
    let merged = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));
    assert!(!merged
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "xref.placement-undecoded" }));
    assert_eq!(merged.ir().model.occurrences.len(), 1);
    let expected = Transform::affine([
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 30.0],
    ])
    .unwrap();
    assert_eq!(merged.ir().model.occurrences[0].transform, expected);
}
