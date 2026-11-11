// SPDX-License-Identifier: Apache-2.0

use super::{assert_valid, decode};
use crate::test_support::container::{make_block, outer_header};
use crate::test_support::parasolid::mesh::mesh_body_sequence;
use crate::test_support::parasolid::sequence::topology_sequence;

#[test]
fn mesh_body_loss_survives_metadata_fallback() {
    let mut source = outer_header();
    source.extend(make_block(
        0x41,
        "Contents/Config-0-Partition",
        &mesh_body_sequence(2, 2),
    ));
    let result = decode(source);
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.namespace() == "sldprt"
                && loss.code.local_code() == "geometry.mesh-body-not-transferred"
        })
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.starts_with("2 mesh body/bodies"));
    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.faces.is_empty());
    assert_valid(&result);
}

#[test]
fn analytic_topology_has_no_mesh_body_loss() {
    let (payload, _) = topology_sequence();
    let mut source = outer_header();
    source.extend(make_block(0x41, "Contents/Config-0-Partition", &payload));
    let result = decode(source);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| { loss.code.local_code() != "geometry.mesh-body-not-transferred" }));
}

#[test]
fn mesh_body_loss_survives_geometry_transfer_from_another_site() {
    use crate::test_support::container::sldprt_with_body;
    use crate::test_support::parasolid::owned_triangle;

    let mut source = sldprt_with_body(&owned_triangle(0, 700, 0.0));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-Partition",
        &mesh_body_sequence(1, 1),
    ));
    let result = decode(source);
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "geometry.mesh-body-not-transferred"
            && loss.message.starts_with("1 mesh body/bodies")
    }));
}
