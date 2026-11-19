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

/// A geometric set of two curve-bounded surfaces on one plane: a ring of
/// outer radius 5 with a hole of radius 2, and a disc of radius 1.
fn ring_and_disc_set() -> String {
    let mut source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .unwrap()
    .replace(
        "#10=(BOUNDARY_CURVE() ",
        "#10=(BOUNDARY_CURVE() OUTER_BOUNDARY_CURVE() ",
    )
    .replace(
        "#11=CURVE_BOUNDED_SURFACE('disc',#8,(#10),.F.);",
        "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10,#16),.F.);",
    )
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#20));",
    );
    let end = source.rfind("ENDSEC;").unwrap();
    source.insert_str(
        end,
        "#14=CIRCLE('',#6,2.);\n\
         #15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);\n\
         #16=BOUNDARY_CURVE('hole',(#15),.F.);\n\
         #17=CIRCLE('',#6,1.);\n\
         #18=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#17);\n\
         #19=OUTER_BOUNDARY_CURVE('',(#18),.F.);\n\
         #20=CURVE_BOUNDED_SURFACE('disc',#8,(#19),.F.);\n",
    );
    source
}

/// The loop counts of the faces of a one-body sheet, ascending.
fn sheet_loop_counts(ir: &CadIr) -> Vec<usize> {
    let geometry = &ir.geometry;
    assert_eq!(geometry.bodies.len(), 1, "{:#?}", geometry.bodies);
    assert_eq!(
        geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    let mut counts: Vec<usize> = geometry.faces.iter().map(|face| face.loops.len()).collect();
    counts.sort_unstable();
    counts
}

/// The writer states the curve-bounded faces of a sheet as the members of
/// one geometric set, each bounded by its boundary curves, so the sheet
/// decodes as one body with the same faces and loops.
#[test]
fn a_sheet_of_curve_bounded_surfaces_round_trips_as_one_geometric_set() {
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(ring_and_disc_set()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(sheet_loop_counts(source.ir()), [1, 2]);
    let mut output = Vec::new();
    write_step(
        source.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1, "{text}");
    assert_eq!(text.matches("= CURVE_BOUNDED_SURFACE(").count(), 2);
    assert_eq!(text.matches("= OUTER_BOUNDARY_CURVE(").count(), 2);
    assert_eq!(text.matches("= BOUNDARY_CURVE(").count(), 1);
    assert_eq!(text.matches("= SHELL_BASED_SURFACE_MODEL(").count(), 0);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sheet_loop_counts(decoded.ir()), [1, 2]);
}

/// A surface profile tolerance on `target`.
fn profile_on(target: cadmpeg_ir::pmi::PmiTarget) -> cadmpeg_ir::pmi::PmiAnnotation {
    use cadmpeg_ir::design::options::NativeOptions;
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{
        GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiMagnitude, PmiQuantity, PmiValue,
    };
    PmiAnnotation {
        id: PmiId::mint("test:surface-set:pmi#profile").unwrap(),
        name: Some("profile".into()),
        visible: None,
        targets: vec![target.into()],
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: PmiMagnitude::new(PmiValue::new(0.5, PmiQuantity::Length).unwrap()).unwrap(),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: Vec::new(),
            displacement: None,
            maximum_upper: None,
            zones: Vec::new(),
        },
        native_ref: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

/// A PMI annotation on a curve-bounded face, on an edge of its loops, or
/// on a vertex of those edges needs that face, edge, or vertex as a written
/// item. The sheet is then written as a shell of faces, and the annotation
/// names its target through a `GEOMETRIC_ITEM_SPECIFIC_USAGE`.
#[test]
fn a_pmi_target_on_a_curve_bounded_face_keeps_the_sheet_a_shell() {
    use cadmpeg_ir::pmi::PmiTarget;
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(ring_and_disc_set()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ring = source
        .ir()
        .geometry
        .faces
        .iter()
        .find(|face| face.loops.len() == 2)
        .unwrap();
    let edge = source
        .ir()
        .geometry
        .loops
        .iter()
        .find(|face_loop| ring.loops.contains(&face_loop.id))
        .and_then(|face_loop| face_loop.coedges().first())
        .and_then(|coedge| {
            source
                .ir()
                .geometry
                .coedges
                .iter()
                .find(|candidate| &candidate.id == coedge)
        })
        .map(|coedge| coedge.edge.clone())
        .unwrap();
    let mut targets = vec![
        PmiTarget::Face {
            face: ring.id.clone(),
        },
        PmiTarget::Edge { edge: edge.clone() },
    ];
    if let Some(vertex) = source
        .ir()
        .geometry
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.vertices().next())
    {
        targets.push(PmiTarget::Vertex {
            vertex: vertex.clone(),
        });
    }
    for target in targets {
        let mut ir = source.ir().clone();
        ir.model.pmi = vec![profile_on(target.clone())];
        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{target:?}");
        assert_eq!(
            text.matches("= SHELL_BASED_SURFACE_MODEL(").count(),
            1,
            "{target:?}"
        );
        assert!(
            text.contains("= GEOMETRIC_ITEM_SPECIFIC_USAGE("),
            "{target:?}\n{text}"
        );
    }
}
