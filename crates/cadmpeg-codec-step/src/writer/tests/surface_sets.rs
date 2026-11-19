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

/// The `#n = TYPE(...)` records of a written file, by instance number:
/// simple records only, which is every record the walk below follows.
fn records(text: &str) -> std::collections::HashMap<u64, (&str, &str)> {
    text.lines()
        .filter_map(|line| {
            let (id, rest) = line.strip_prefix('#')?.split_once(" = ")?;
            let (name, arguments) = rest.split_once('(')?;
            Some((id.parse().ok()?, (name, arguments)))
        })
        .collect()
}

/// The instance references in a record's arguments, in order.
fn references(arguments: &str) -> Vec<u64> {
    arguments
        .split('#')
        .skip(1)
        .filter_map(|tail| {
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// The `GEOMETRIC_SET`s a consumer reaches from `representation` by
/// following only its items and, from a `MAPPED_ITEM`, its map's mapped
/// representation: no `SHAPE_REPRESENTATION_RELATIONSHIP`, in either
/// direction.
fn directed_sets(
    records: &std::collections::HashMap<u64, (&str, &str)>,
    representation: u64,
) -> Vec<u64> {
    let mut found = Vec::new();
    let mut pending = vec![representation];
    while let Some(representation) = pending.pop() {
        let (_, arguments) = records[&representation];
        // The items are the representation's one list argument; the context
        // reference follows it.
        let items = arguments
            .split_once('(')
            .and_then(|(_, tail)| tail.split_once(')'))
            .map(|(items, _)| references(items))
            .unwrap_or_default();
        for item in items {
            match records[&item] {
                ("GEOMETRIC_SET", _) => found.push(item),
                ("MAPPED_ITEM", arguments) => {
                    let map = references(arguments)[0];
                    let (name, map_arguments) = records[&map];
                    assert_eq!(name, "REPRESENTATION_MAP");
                    pending.push(references(map_arguments)[1]);
                }
                _ => {}
            }
        }
    }
    found
}

fn placed_patch_sheet() -> CadIr {
    let mut ir = whole_patch_sheet();
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
        ])
        .unwrap(),
    );
    ir
}

/// A placed sheet's body-local representation would hold nothing but its
/// set, and `representation.items` is SET [1:?]: the map names the
/// bounded-surface representation itself, and a reader that follows the
/// product's representation, its mapped item and the map's representation,
/// and no relationship, reaches the set. The decoded body keeps its
/// placement, and so does a second write of it.
#[test]
fn a_placed_sheet_maps_its_bounded_surface_representation() {
    let ir = placed_patch_sheet();
    let mut text = String::new();
    let mut decoded_ir = ir.clone();
    for pass in 0..2 {
        let mut output = Vec::new();
        write_step(
            &decoded_ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .unwrap();
        text = String::from_utf8(output).unwrap();
        let records = records(&text);
        for (id, (name, arguments)) in &records {
            assert!(
                !(name.ends_with("REPRESENTATION") && arguments.contains(",(),")),
                "pass {pass}: #{id} {name} has no items:\n{text}"
            );
        }
        let (_, definition) = records
            .values()
            .find(|(name, _)| *name == "SHAPE_DEFINITION_REPRESENTATION")
            .unwrap();
        let product_representation = references(definition)[1];
        assert_eq!(
            directed_sets(&records, product_representation).len(),
            1,
            "pass {pass}:\n{text}"
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(text.clone()), &DecodeOptions::default())
            .unwrap();
        assert!(
            !decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("GEOMETRICALLY_BOUNDED")),
            "pass {pass}: {:#?}",
            decoded.report().losses
        );
        let geometry = &decoded.ir().geometry;
        let [body] = geometry.bodies.as_slice() else {
            panic!("pass {pass}: one body: {:#?}", geometry.bodies);
        };
        assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Sheet);
        assert_eq!(body.transform, ir.geometry.bodies[0].transform);
        let [face] = geometry.faces.as_slice() else {
            panic!("pass {pass}: one face: {:#?}", geometry.faces);
        };
        assert!(face.loops.is_empty());
        decoded_ir = decoded.ir().clone();
    }
    assert_eq!(text.matches("= MAPPED_ITEM(").count(), 1);
}
