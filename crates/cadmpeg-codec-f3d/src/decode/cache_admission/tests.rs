// SPDX-License-Identifier: Apache-2.0

use super::omit_face_parts;
use cadmpeg_ir::topology::BodyKind;
use std::collections::BTreeSet;

#[test]
fn omitting_a_cube_boundary_reclassifies_only_the_affected_solid() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("closed cube");
    let before = cadmpeg_ir::validate_neutral(&ir, vec![]).unwrap();
    assert!(before.is_ok(), "{before:#?}");
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
    // An unrelated meaningful empty body must retain its existing kind.
    let mut sibling = ir.model.bodies[0].clone();
    sibling.id =
        cadmpeg_ir::ids::BodyId::mint("synthetic:cache-control:body#unaffected-empty").unwrap();
    sibling.regions.clear();
    ir.model.bodies.push(sibling.clone());
    let original_points = ir.model.points.clone();
    let original_edges = ir.model.edges.clone();
    let face = ir.model.faces[0].id.as_str().to_owned();
    let removed = omit_face_parts(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir.model,
        &BTreeSet::from([face.clone()]),
    )
    .unwrap();
    assert!(removed.contains(&face));
    let affected = ir
        .model
        .bodies
        .iter()
        .find(|body| body.id != sibling.id)
        .unwrap();
    assert_eq!(affected.kind, BodyKind::Sheet);
    assert_eq!(
        ir.model.bodies.iter().find(|body| body.id == sibling.id),
        Some(&sibling)
    );
    assert_eq!(ir.model.faces.len(), 5);
    assert_eq!(ir.model.loops.len(), 5);
    assert_eq!(ir.model.coedges.len(), 20);
    // All four formerly shared edges and their endpoints still have owners.
    assert_eq!(ir.model.points, original_points);
    assert_eq!(ir.model.edges, original_edges);
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .filter(|row| row.radial_next == row.id)
            .count(),
        4
    );
    ir.finalize();
    assert_eq!(ir.model.surfaces.len(), 5);
    let after = cadmpeg_ir::validate_neutral(&ir, vec![]).unwrap();
    assert!(after.is_ok(), "{after:#?}");
}
