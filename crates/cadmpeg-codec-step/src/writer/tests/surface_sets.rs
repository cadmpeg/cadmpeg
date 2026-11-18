// SPDX-License-Identifier: Apache-2.0
//! Sheets whose faces are whole bounded surfaces, written as geometric sets.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// A sheet whose one face is the whole unit-square patch, with no loops.
fn whole_patch_sheet() -> CadIr {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, FaceLoops, Region, Sense, Shell};
    let id = |kind: &str| format!("test:model:{kind}#sheet");
    let mut ir = CadIr::empty();
    ir.geometry.surfaces.push(Surface {
        id: SurfaceId::mint(id("surface")).unwrap(),
        geometry: SurfaceGeometry::Solved(crate::test_support::curve_bounded::unit_square_patch()),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: FaceId::mint(id("face")).unwrap(),
        shell: ShellId::mint(id("shell")).unwrap(),
        surface: SurfaceId::mint(id("surface")).unwrap(),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::new(
        ShellId::mint(id("shell")).unwrap(),
        RegionId::mint(id("region")).unwrap(),
    ));
    ir.geometry.regions.push(Region {
        id: RegionId::mint(id("region")).unwrap(),
        body: BodyId::mint(id("body")).unwrap(),
        shells: vec![ShellId::mint(id("shell")).unwrap()],
        native_ref: None,
    });
    ir.geometry.bodies.push(Body {
        id: BodyId::mint(id("body")).unwrap(),
        kind: BodyKind::Sheet,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.finalize();
    ir
}

/// The writer states a face that is a whole bounded surface as a member of
/// a geometric set, which decodes as the same one-face sheet.
#[test]
fn a_sheet_of_whole_bounded_surfaces_round_trips_as_a_geometric_set() {
    let mut output = Vec::new();
    let report = write_step(
        &whole_patch_sheet(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    let text = String::from_utf8(output).unwrap();
    assert_eq!(
        text.matches("= GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION(")
            .count(),
        1
    );
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1);
    assert_eq!(text.matches("= ADVANCED_FACE(").count(), 0);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(
        geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    let [face] = geometry.faces.as_slice() else {
        panic!("one face: {:#?}", geometry.faces);
    };
    assert!(face.loops.is_empty());
}
