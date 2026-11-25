// SPDX-License-Identifier: Apache-2.0
//! `ON_SubD` payloads read back through the decoder to the cage they state.

use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::subd::{SubdCage, SubdEdge};

use super::{payload, placed, Unheld, SUBD_CLASS};
use crate::chunks::ArchiveVersion;
use crate::settings::MillimeterScale;
use crate::test_support::two_quad_cage;
use crate::RhinoArchiveVersion;

fn read_back(cage: &SubdCage, version: RhinoArchiveVersion) -> (SubdCage, Vec<Unheld>) {
    let written = payload(cage, version).expect("the payload is written");
    assert_eq!(written.direct, [1]);
    let decoded = crate::subd::decode(
        &written.body,
        0..written.body.len(),
        ArchiveVersion::from_word(version.value()),
        MillimeterScale::IDENTITY,
        SubdId::mint("test:rhino:subd#written").expect("identity grammar"),
    )
    .expect("the payload decodes")
    .expect("the payload holds a SubDimple");
    assert_eq!(decoded.warnings.messages().count(), 0);
    assert!(!decoded.neutral_metadata);
    (decoded.surface.cage, written.unheld)
}

#[test]
fn the_class_id_is_the_on_subd_class() {
    assert_eq!(SUBD_CLASS, crate::subd::ON_SUBD.to_wire());
}

#[test]
fn an_archive_80_payload_reads_back_as_its_cage() {
    let cage = two_quad_cage();
    assert_eq!(
        read_back(&cage, RhinoArchiveVersion::V8),
        (cage, Vec::new())
    );
}

#[test]
fn an_earlier_payload_holds_the_start_sharpness_at_both_ends() {
    let cage = two_quad_cage();
    let mut expected = cage.clone();
    expected
        .edit_edges(|edges| {
            for edge in edges.iter_mut() {
                let [start, _] = edge
                    .sharpness()
                    .map(cadmpeg_ir::scalar::NonNegativeReal::get);
                *edge = SubdEdge::new(
                    edge.vertices(),
                    [start, start],
                    edge.tag,
                    None,
                    edge.sector_coefficients()
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                )?;
            }
            Ok(())
        })
        .expect("the edges stay valid");
    for version in [RhinoArchiveVersion::V6, RhinoArchiveVersion::V7] {
        assert_eq!(
            read_back(&cage, version),
            (expected.clone(), vec![Unheld::EndSharpness]),
            "{version:?}"
        );
    }
}

#[test]
fn a_cage_of_no_vertex_reads_back_empty() {
    let cage = SubdCage::default();
    assert_eq!(
        read_back(&cage, RhinoArchiveVersion::V8),
        (cage, Vec::new())
    );
}

#[test]
fn a_placed_cage_moves_its_vertices() {
    let cage = two_quad_cage();
    let shift = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, -2.0],
    ])
    .expect("an affine placement");
    let moved = placed(&cage, Some(shift)).expect("the placement is finite");
    for (before, after) in cage.vertices().iter().zip(moved.vertices()) {
        let [before, after] = [before.point().get(), after.point().get()];
        assert_eq!(
            [after.x, after.y, after.z],
            [before.x + 10.0, before.y, before.z - 2.0]
        );
    }
    assert_eq!(moved.edges(), cage.edges());
}
