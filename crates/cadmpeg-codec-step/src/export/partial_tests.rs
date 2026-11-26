// SPDX-License-Identifier: Apache-2.0
//! A region that is marked, or that holds a partial face, is written as a
//! shell-based surface model of its stated shells, never as a solid, with
//! each partial face left out (§3.4, Content the target cannot hold).

use cadmpeg_ir::topology::{FaceLoops, RegionUnstated};

fn write(ir: &cadmpeg_ir::CadIr) -> (String, Vec<String>) {
    let mut bytes = Vec::new();
    let report = super::write_step(
        ir,
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .unwrap();
    let messages = report
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    (String::from_utf8(bytes).unwrap(), messages)
}

fn count(text: &str, entity: &str) -> usize {
    text.matches(&format!("{entity}(")).count()
}

#[test]
fn a_solid_with_a_partial_face_is_written_as_an_open_shell_of_its_complete_faces() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let face = &mut ir.geometry.faces[0];
    let loops = face.loops.stated().cloned().collect();
    face.loops = FaceLoops::PartialUnspecified { loops };
    let partial = face.id.clone();

    let (text, losses) = write(&ir);
    assert_eq!(count(&text, "MANIFOLD_SOLID_BREP"), 0, "{text}");
    assert_eq!(count(&text, "SHELL_BASED_SURFACE_MODEL"), 1, "{text}");
    assert_eq!(count(&text, "OPEN_SHELL"), 1, "{text}");
    assert_eq!(count(&text, "CLOSED_SHELL"), 0, "{text}");
    assert_eq!(count(&text, "ADVANCED_FACE"), 5, "{text}");
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("STEP writes no partial face")
                && loss.contains(partial.as_str())),
        "{losses:?}"
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.starts_with("STEP writes no solid for"))
            .count(),
        1,
        "{losses:?}"
    );
    assert!(
        !losses
            .iter()
            .any(|loss| loss.contains("has no writable topology")
                || loss.contains("written as a surface model because")),
        "{losses:?}"
    );
}

#[test]
fn a_region_whose_voids_are_unstated_keeps_its_closed_exterior_as_a_shell() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.regions[0].unstated = Some(RegionUnstated::Voids);

    let (text, losses) = write(&ir);
    assert_eq!(count(&text, "MANIFOLD_SOLID_BREP"), 0, "{text}");
    assert_eq!(count(&text, "SHELL_BASED_SURFACE_MODEL"), 1, "{text}");
    assert_eq!(count(&text, "CLOSED_SHELL"), 1, "{text}");
    assert_eq!(count(&text, "ADVANCED_FACE"), 6, "{text}");
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("STEP writes no solid for")),
        "{losses:?}"
    );
}

#[test]
fn a_complete_solid_is_still_a_manifold_solid() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let (text, losses) = write(&ir);
    assert_eq!(count(&text, "MANIFOLD_SOLID_BREP"), 1, "{text}");
    assert_eq!(count(&text, "SHELL_BASED_SURFACE_MODEL"), 0, "{text}");
    assert!(
        !losses
            .iter()
            .any(|loss| loss.contains("partial") || loss.contains("writes no solid")),
        "{losses:?}"
    );
}
